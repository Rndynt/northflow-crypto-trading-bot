//! Backtest engine — deterministic 1m replay with no lookahead.
//!
//! Flow:
//!   1. Load 1m CSV.
//!   2. Reject if data quality errors.
//!   3. Build CandleStore (1m, 5m, 15m).
//!   4. Precompute indicator snapshots for 5m and 15m.
//!   5. Replay 1m candles chronologically.
//!   6. For each candle:
//!      a. Handle pending entry (enter at candle open).
//!         - Compute actual adverse entry price.
//!         - Re-risk at actual price; soft-reject if invalid geometry or risk
//!           guards reject; record RiskRejection row(s).
//!      b. Check exit for open position (SL / TP / TimeExit) — even on entry candle.
//!      c. Evaluate strategy — no lookahead across 5m / 15m; skipped on entry candle.
//!         - Initial risk assessment at signal-close price; soft-reject and record
//!           RiskRejection row(s) if rejected.
//!      d. If signal pre-approved by initial risk check, set pending entry.
//!   7. Close any remaining open position as EndOfBacktest.
//!   8. Finalise SignalFlowSummary.
//!   9. Return BacktestResult.
//!
//! Conservative intrabar rule: if SL and TP are both touched in the same candle,
//! SL is assumed first.
//!
//! No exchange calls. No LLMs. Historical simulation only.

use std::path::Path;

use crate::backtest::fill_model::{FillModel, OpenSimPosition};
use crate::backtest::geometry::{EntryGeometryMode, adjusted_signal_for_actual_entry};
use crate::backtest::metrics::{BacktestSummary, EquityPoint, Metrics};
use crate::backtest::risk_trace::{RiskRejection, SignalFlowSummary};
use crate::config::ResearchConfig;
use crate::core::{
    Candle, NorthflowError, PositionId, Side, Signal, Symbol, Trade, TradeExitReason, TradeId,
};
use crate::indicators::{IndicatorEngine, IndicatorSnapshot};
use crate::market::{CandleStore, OhlcvLoader};
use crate::risk::{CostModelConfig, RiskContext, RiskEngine};
use crate::strategy::{
    EmaTrendPullbackV1, MultiTimeframeInput, ScreenedVwapScalp, ScreenedVwapScalpV2, Strategy,
    StrategyContext,
};

// ── ActiveStrategy ────────────────────────────────────────────────────────────

enum ActiveStrategy {
    V1(ScreenedVwapScalp),
    V2(ScreenedVwapScalpV2),
    Etp(EmaTrendPullbackV1),
}

impl ActiveStrategy {
    fn evaluate(
        &self,
        ctx: &StrategyContext,
        input: &MultiTimeframeInput,
    ) -> Result<Option<crate::core::Signal>, crate::core::NorthflowError> {
        match self {
            Self::V1(s) => s.evaluate(ctx, input),
            Self::V2(s) => s.evaluate(ctx, input),
            Self::Etp(s) => s.evaluate(ctx, input),
        }
    }
}

// ── BacktestConfig ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub initial_equity: f64,
    pub reports_dir: String,
    pub conservative_intrabar: bool,
    pub max_bars_held: u32,
    pub entry_geometry_mode: EntryGeometryMode,
}

// ── BacktestResult ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<EquityPoint>,
    pub summary: BacktestSummary,
    pub risk_rejections: Vec<RiskRejection>,
    pub signal_flow: SignalFlowSummary,
}

// ── BacktestEngine ────────────────────────────────────────────────────────────

pub struct BacktestEngine;

impl BacktestEngine {
    /// Run the backtest for one symbol.
    ///
    /// Returns `Ok(None)` if the historical CSV does not exist.
    /// Returns `Err` if data quality has errors or processing fails.
    pub fn run(
        cfg: &ResearchConfig,
        symbol: &str,
    ) -> Result<Option<BacktestResult>, NorthflowError> {
        // Validate strategy config before any data loading so unknown strategy_id
        // always returns Err, regardless of CSV presence.
        cfg.validate_strategy_config()?;

        let csv_path = Path::new(&cfg.data_dir).join(format!("{symbol}.csv"));

        if !csv_path.exists() {
            return Ok(None);
        }

        // Load and validate data.
        let load_result = OhlcvLoader::load_file(&csv_path)
            .map_err(|e| NorthflowError::DataError(e.to_string()))?;

        let quality = &load_result.quality;
        if quality.error_count() > 0 {
            return Err(NorthflowError::DataError(format!(
                "data quality errors in {}: {} error(s) must be fixed before backtest",
                csv_path.display(),
                quality.error_count()
            )));
        }

        if load_result.candles.is_empty() {
            return Ok(None);
        }

        // Parse timeframe roles from config.
        let entry_tf = crate::core::Timeframe::from_str(&cfg.entry_timeframe)
            .map_err(|e| NorthflowError::ConfigError(format!("entry_timeframe: {e}")))?;
        let confirmation_tf = crate::core::Timeframe::from_str(&cfg.confirmation_timeframe)
            .map_err(|e| NorthflowError::ConfigError(format!("confirmation_timeframe: {e}")))?;
        let screening_tf = crate::core::Timeframe::from_str(&cfg.screening_timeframe)
            .map_err(|e| NorthflowError::ConfigError(format!("screening_timeframe: {e}")))?;

        // Build candle store with configured TF roles.
        let store = CandleStore::build(load_result.candles, entry_tf, confirmation_tf, screening_tf)?;

        if store.entry_candles.is_empty() {
            return Ok(None);
        }

        // Precompute confirmation-TF snapshots (no-lookahead lookup at eval time).
        let mut eng_conf = IndicatorEngine::new_default()?;
        let mut snaps_conf: Vec<(i64, IndicatorSnapshot, Candle)> = Vec::new();
        for &c in &store.confirmation_candles {
            let snap = eng_conf.next(c)?;
            snaps_conf.push((c.timestamp, snap, c));
        }

        // Precompute screening-TF snapshots.
        let mut eng_screen = IndicatorEngine::new_default()?;
        let mut snaps_screen: Vec<(i64, IndicatorSnapshot, Candle)> = Vec::new();
        for &c in &store.screening_candles {
            let snap = eng_screen.next(c)?;
            snaps_screen.push((c.timestamp, snap, c));
        }

        let geometry_mode = EntryGeometryMode::parse(&cfg.entry_geometry_mode)?;
        let bt_cfg = BacktestConfig {
            initial_equity: cfg.initial_equity,
            reports_dir: cfg.reports_dir.clone(),
            conservative_intrabar: cfg.conservative_intrabar,
            max_bars_held: cfg.max_bars_held,
            entry_geometry_mode: geometry_mode,
        };
        let risk_cfg = cfg.risk_config();
        let cost_cfg = cfg.cost_model_config();
        let symbol_obj = Symbol::new(symbol)
            .map_err(|e| NorthflowError::DataError(format!("invalid symbol '{symbol}': {e}")))?;

        // ── Main replay loop ──────────────────────────────────────────────────

        let mut equity = bt_cfg.initial_equity;
        let mut peak_equity = equity;
        let mut daily_realized_pnl = 0.0_f64;
        let mut current_day = -1_i64;

        let mut trades: Vec<Trade> = Vec::new();
        let mut equity_curve: Vec<EquityPoint> = Vec::new();

        // Initial equity point.
        if let Some(first) = store.entry_candles.first() {
            equity_curve.push(EquityPoint {
                timestamp: first.timestamp,
                equity,
                drawdown_pct: 0.0,
            });
        }

        let mut signal_counter: u64 = 0;
        let mut pending_entry: Option<Signal> = None;
        let mut open_position: Option<OpenSimPosition> = None;
        let mut risk_rejections: Vec<RiskRejection> = Vec::new();
        let mut signal_flow = SignalFlowSummary::default();

        // Build strategy from config.
        let strategy = match cfg.strategy_id.as_str() {
            "screened_vwap_scalp" => ActiveStrategy::V1(ScreenedVwapScalp::default()),
            "screened_vwap_scalp_v2" => {
                ActiveStrategy::V2(ScreenedVwapScalpV2::new(cfg.v2_config()))
            }
            "ema_trend_pullback_v1" => {
                ActiveStrategy::Etp(EmaTrendPullbackV1::new(cfg.etp_config()))
            }
            other => {
                return Err(NorthflowError::ConfigError(format!(
                    "unknown strategy_id: '{other}'"
                )));
            }
        };
        let cooldown_bars = cfg.cooldown_bars_for_strategy(&cfg.strategy_id) as usize;
        let mut last_signal_bar: Option<usize> = None;
        let mut eng_1m = IndicatorEngine::new_default()?;

        let entry_candles = &store.entry_candles;
        let n = entry_candles.len();

        for i in 0..n {
            let candle = entry_candles[i];

            if i > 0 && i % 50_000 == 0 {
                println!(
                    "  Backtest progress: {}/{} 1m candles ({:.1}%)",
                    i,
                    n,
                    i as f64 / n as f64 * 100.0
                );
            }

            // Update 1m indicator engine.
            let snap_1m = eng_1m.next(candle)?;

            // Day boundary — reset daily PnL.
            let day = candle.timestamp / 86_400_000;
            if day != current_day {
                current_day = day;
                daily_realized_pnl = 0.0;
            }

            // ── A. Handle pending entry ───────────────────────────────────────
            // A signal from the previous candle is entered at THIS candle's open.
            // Actual entry price is computed with adverse slippage at candle open.
            // Risk is re-assessed at the actual price; invalid geometry or risk
            // rejection are recorded as soft rejections (not fatal).
            // After this block, fall through to B so exit checks run on the entry candle.
            let mut entered_this_bar = false;
            if let Some(signal) = pending_entry.take() {
                if open_position.is_none() && equity > 0.0 {
                    let actual_price = FillModel::adverse_entry_price(
                        signal.side,
                        candle.open,
                        cost_cfg.slippage_bps,
                    );
                    let adjusted = adjusted_signal_for_actual_entry(
                        &signal,
                        actual_price,
                        bt_cfg.entry_geometry_mode,
                    );

                    if !adjusted.valid_geometry() {
                        // Adverse slippage made the trade geometry invalid — soft reject.
                        signal_flow.signals_rejected_actual_entry += 1;
                        risk_rejections.push(build_rejection(
                            &adjusted,
                            "actual_entry",
                            bt_cfg.entry_geometry_mode.as_str(),
                            candle.timestamp,
                            equity,
                            peak_equity,
                            daily_realized_pnl,
                            "actual_entry_invalid_geometry",
                            adjusted.expected_reward_bps,
                            adjusted.estimated_cost_bps,
                            adjusted.expected_net_edge_bps,
                        ));
                    } else {
                        let risk_ctx = RiskContext {
                            equity,
                            peak_equity,
                            daily_realized_pnl,
                            open_positions: 0,
                        };
                        match RiskEngine::assess(&risk_cfg, &cost_cfg, &risk_ctx, &adjusted) {
                            Err(NorthflowError::InvalidSignal(_)) => {
                                // Geometry looked valid but signal failed deeper validation —
                                // treat as soft actual-entry rejection.
                                signal_flow.signals_rejected_actual_entry += 1;
                                risk_rejections.push(build_rejection(
                                    &adjusted,
                                    "actual_entry",
                                    bt_cfg.entry_geometry_mode.as_str(),
                                    candle.timestamp,
                                    equity,
                                    peak_equity,
                                    daily_realized_pnl,
                                    "actual_entry_risk_error",
                                    adjusted.expected_reward_bps,
                                    adjusted.estimated_cost_bps,
                                    adjusted.expected_net_edge_bps,
                                ));
                            }
                            Err(e) => return Err(e),
                            Ok(assessment) if !assessment.approved => {
                                signal_flow.signals_rejected_actual_entry += 1;
                                for reason in &assessment.failed {
                                    risk_rejections.push(build_rejection(
                                        &adjusted,
                                        "actual_entry",
                                        bt_cfg.entry_geometry_mode.as_str(),
                                        candle.timestamp,
                                        equity,
                                        peak_equity,
                                        daily_realized_pnl,
                                        reason,
                                        assessment.expected_reward_bps,
                                        assessment.expected_cost_bps,
                                        assessment.expected_net_edge_bps,
                                    ));
                                }
                            }
                            Ok(assessment) => {
                                if let Some(qty) = assessment.qty {
                                    if qty > 0.0 {
                                        let entry = FillModel::simulate_entry(
                                            &adjusted,
                                            qty,
                                            &candle,
                                            cost_cfg.slippage_bps,
                                            cost_cfg.taker_fee_bps,
                                        );
                                        open_position = Some(OpenSimPosition {
                                            signal: adjusted,
                                            qty,
                                            entry_time: entry.time,
                                            entry_price: entry.price,
                                            entry_fee: entry.fee,
                                            entry_slippage: entry.slippage,
                                            bars_held: 0,
                                        });
                                        signal_flow.trades_opened += 1;
                                        entered_this_bar = true;
                                    }
                                }
                            }
                        }
                    }
                }
                // Do NOT continue — fall through to B so exit checks run on the
                // entry candle.  Strategy evaluation (C) is skipped via the flag.
            }

            // ── B. Check exits for open position ─────────────────────────────
            let mut closed_this_bar = false;
            if let Some(ref mut pos) = open_position {
                pos.bars_held += 1;

                let exit_fill = FillModel::check_exit(
                    pos,
                    &candle,
                    bt_cfg.conservative_intrabar,
                    cost_cfg.slippage_bps,
                    cost_cfg.taker_fee_bps,
                    bt_cfg.max_bars_held,
                );

                if let Some(exit) = exit_fill {
                    let trade = build_trade(pos, &exit, symbol_obj.clone(), &cost_cfg);
                    equity += trade.net_pnl;
                    daily_realized_pnl += trade.net_pnl;
                    peak_equity = peak_equity.max(equity);
                    let dd = drawdown_pct(peak_equity, equity);

                    equity_curve.push(EquityPoint {
                        timestamp: candle.timestamp,
                        equity,
                        drawdown_pct: dd,
                    });
                    trades.push(trade);
                    closed_this_bar = true;
                }
            }

            if closed_this_bar {
                open_position = None;
                if equity <= 0.0 {
                    break;
                }
            }

            // ── C. Evaluate strategy — no lookahead ───────────────────────────
            // Skipped on the candle where an entry was just opened to avoid
            // evaluating a new signal before the just-opened trade has had a
            // chance to develop.
            // Cooldown: if v2_cooldown_bars > 0, skip strategy evaluation for
            // that many bars after the last signal was preapproved.
            let in_cooldown = cooldown_bars > 0
                && last_signal_bar.map_or(false, |last| i.saturating_sub(last) <= cooldown_bars);
            if !entered_this_bar && open_position.is_none() && equity > 0.0 && !in_cooldown {
                // No-lookahead rule (generic):
                //   signal_time = candle.timestamp + entry_tf.to_millis()
                //   conf available: conf_ts + conf_tf.to_millis() <= signal_time
                //     → conf_ts <= candle.ts + entry_tf.to_millis() - conf_tf.to_millis()
                //   screen available: screen_ts + screen_tf.to_millis() <= signal_time
                //     → screen_ts <= candle.ts + entry_tf.to_millis() - screen_tf.to_millis()
                let max_conf_ts   = candle.timestamp + entry_tf.to_millis() - confirmation_tf.to_millis();
                let max_screen_ts = candle.timestamp + entry_tf.to_millis() - screening_tf.to_millis();

                let snap5 = latest_snap(&snaps_conf, max_conf_ts);
                let snap15 = latest_snap(&snaps_screen, max_screen_ts);

                if let (Some((_, s5, c5)), Some((_, s15, c15))) = (snap5, snap15) {
                    let estimated_cost = cost_cfg.taker_fee_bps * 2.0
                        + cost_cfg.slippage_bps * 2.0
                        + cost_cfg.spread_bps;

                    let ctx = StrategyContext {
                        symbol: symbol_obj.clone(),
                        signal_index: signal_counter + 1,
                        estimated_cost_bps: estimated_cost,
                        min_confidence: cfg.min_confidence,
                        entry_timeframe: entry_tf,
                        confirmation_timeframe: confirmation_tf,
                        screening_timeframe: screening_tf,
                    };

                    let input = MultiTimeframeInput {
                        entry_candle: candle,
                        confirmation_candle: *c5,
                        screening_candle: *c15,
                        entry_indicators: snap_1m.clone(),
                        confirmation_indicators: s5.clone(),
                        screening_indicators: s15.clone(),
                    };

                    match strategy.evaluate(&ctx, &input) {
                        Ok(None) => {}
                        Ok(Some(signal)) => {
                            signal_counter += 1;
                            signal_flow.signals_generated += 1;

                            let risk_ctx = RiskContext {
                                equity,
                                peak_equity,
                                daily_realized_pnl,
                                open_positions: 0,
                            };

                            match RiskEngine::assess(&risk_cfg, &cost_cfg, &risk_ctx, &signal) {
                                Err(e) => return Err(e),
                                Ok(assessment) if !assessment.approved => {
                                    signal_flow.signals_rejected_initial_risk += 1;
                                    for reason in &assessment.failed {
                                        risk_rejections.push(build_rejection(
                                            &signal,
                                            "initial_risk",
                                            bt_cfg.entry_geometry_mode.as_str(),
                                            candle.timestamp,
                                            equity,
                                            peak_equity,
                                            daily_realized_pnl,
                                            reason,
                                            assessment.expected_reward_bps,
                                            assessment.expected_cost_bps,
                                            assessment.expected_net_edge_bps,
                                        ));
                                    }
                                }
                                Ok(_) => {
                                    signal_flow.signals_preapproved += 1;
                                    last_signal_bar = Some(i);
                                    pending_entry = Some(signal);
                                }
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
        }

        // ── End of backtest: close any remaining position ─────────────────────
        if let Some(ref pos) = open_position {
            if let Some(&last) = entry_candles.last() {
                let exit = FillModel::end_of_backtest_exit(
                    pos,
                    &last,
                    cost_cfg.slippage_bps,
                    cost_cfg.taker_fee_bps,
                );
                let trade = build_trade(pos, &exit, symbol_obj.clone(), &cost_cfg);
                equity += trade.net_pnl;
                peak_equity = peak_equity.max(equity);
                let dd = drawdown_pct(peak_equity, equity);
                equity_curve.push(EquityPoint {
                    timestamp: last.timestamp,
                    equity,
                    drawdown_pct: dd,
                });
                trades.push(trade);
            }
        }

        // Finalise signal flow counters.
        signal_flow.entry_geometry_mode = bt_cfg.entry_geometry_mode.as_str().to_string();
        signal_flow.finalise(&risk_rejections, trades.len());

        println!(
            "  Backtest complete: {} trades, final equity {:.2}",
            trades.len(),
            equity
        );

        let summary = Metrics::summarize(&trades, &equity_curve);

        Ok(Some(BacktestResult {
            trades,
            equity_curve,
            summary,
            risk_rejections,
            signal_flow,
        }))
    }
}

// ── Helper: latest completed snapshot with ts <= max_ts ───────────────────────
//
// Uses partition_point (binary search) for O(log n) lookup.
// The slice is always sorted chronologically because snapshots are precomputed
// from sorted candles.  No-lookahead semantics are unchanged: only snapshots
// with ts <= max_ts are eligible.

fn latest_snap<'a>(
    snaps: &'a [(i64, IndicatorSnapshot, Candle)],
    max_ts: i64,
) -> Option<&'a (i64, IndicatorSnapshot, Candle)> {
    let idx = snaps.partition_point(|(ts, _, _)| *ts <= max_ts);

    if idx == 0 {
        None
    } else {
        Some(&snaps[idx - 1])
    }
}

// ── Helper: drawdown percentage ───────────────────────────────────────────────

fn drawdown_pct(peak: f64, equity: f64) -> f64 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((peak - equity) / peak * 100.0).max(0.0)
}

// ── Helper: build a Trade from a closed position ──────────────────────────────

fn build_trade(
    pos: &OpenSimPosition,
    exit: &crate::backtest::fill_model::ExitFill,
    symbol: Symbol,
    cost_cfg: &CostModelConfig,
) -> Trade {
    let entry_notional = pos.entry_price * pos.qty;
    let spread_cost = entry_notional * cost_cfg.spread_bps / 10_000.0;
    let market_impact_cost = entry_notional * cost_cfg.market_impact_bps / 10_000.0;
    let stop_slippage_cost = if exit.reason == TradeExitReason::StopLoss {
        entry_notional * cost_cfg.stop_slippage_bps / 10_000.0
    } else {
        0.0
    };

    let fee = pos.entry_fee + exit.fee;
    let slippage =
        pos.entry_slippage + exit.slippage + spread_cost + market_impact_cost + stop_slippage_cost;

    let gross_pnl = match pos.signal.side {
        Side::Long => (exit.price - pos.entry_price) * pos.qty,
        Side::Short => (pos.entry_price - exit.price) * pos.qty,
    };
    let net_pnl = gross_pnl - fee - slippage;

    let actual_edge_bps = if entry_notional > 0.0 {
        net_pnl / entry_notional * 10_000.0
    } else {
        0.0
    };

    let risk = (pos.entry_price - pos.signal.stop_loss).abs();
    let reward = (pos.signal.take_profit - pos.entry_price).abs();
    let reward_risk = if risk > 0.0 { reward / risk } else { 0.0 };

    let sig_id = pos.signal.signal_id.as_str();
    let position_id = PositionId::new(format!("POS-{sig_id}"));
    let trade_id = TradeId::new(format!("TRD-{sig_id}"));

    Trade {
        trade_id,
        signal_id: pos.signal.signal_id.clone(),
        position_id,
        symbol,
        strategy_id: pos.signal.strategy_id.clone(),
        regime: pos.signal.regime.clone(),
        side: pos.signal.side,
        entry_time: pos.entry_time,
        exit_time: exit.time,
        entry_price: pos.entry_price,
        exit_price: exit.price,
        stop_loss: pos.signal.stop_loss,
        take_profit: pos.signal.take_profit,
        quantity: pos.qty,
        gross_pnl,
        fee,
        slippage,
        net_pnl,
        reward_risk,
        bars_held: exit.bars_held,
        exit_reason: exit.reason,
        entry_reason: pos.signal.entry_reason.clone(),
        filters_passed: pos.signal.filters_passed.clone(),
        filters_failed: pos.signal.filters_failed.clone(),
        expected_edge_bps: pos.signal.expected_net_edge_bps,
        actual_edge_bps,
    }
}

// ── Helper: build a RiskRejection record ──────────────────────────────────────

fn build_rejection(
    signal: &Signal,
    stage: &str,
    entry_geometry_mode: &str,
    timestamp: i64,
    equity: f64,
    peak_equity: f64,
    daily_realized_pnl: f64,
    reason: &str,
    expected_reward_bps: f64,
    expected_cost_bps: f64,
    expected_net_edge_bps: f64,
) -> RiskRejection {
    let drawdown_pct = if peak_equity > 0.0 && equity < peak_equity {
        (peak_equity - equity) / peak_equity * 100.0
    } else {
        0.0
    };
    RiskRejection {
        signal_id: signal.signal_id.as_str().to_string(),
        stage: stage.to_string(),
        entry_geometry_mode: entry_geometry_mode.to_string(),
        timestamp,
        side: signal.side.as_str().to_string(),
        regime: signal.regime.clone(),
        reason: reason.to_string(),
        equity,
        peak_equity,
        drawdown_pct,
        daily_realized_pnl,
        expected_reward_bps,
        expected_cost_bps,
        expected_net_edge_bps,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{SignalId, StrategyId, Symbol, Timeframe};
    use crate::risk::{CostModelConfig, RiskConfig, RiskContext};
    use std::io::Write;

    fn default_cfg() -> ResearchConfig {
        ResearchConfig::default()
    }

    /// Write a flat-price 1m CSV to a temp path and return the path.
    fn write_test_csv(path: &str, n: usize, start_ts_ms: i64) {
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(f, "timestamp,open,high,low,close,volume").unwrap();
        for i in 0..n {
            let ts = start_ts_ms + (i as i64) * 60_000;
            // Flat market — unlikely to trigger a signal.
            writeln!(f, "{},30000,30100,29900,30000,1000", ts).unwrap();
        }
    }

    fn write_dupe_csv(path: &str) {
        let mut f = std::fs::File::create(path).unwrap();
        writeln!(f, "timestamp,open,high,low,close,volume").unwrap();
        writeln!(f, "1700000000000,100,110,90,105,1000").unwrap();
        writeln!(f, "1700000000000,101,111,91,106,1000").unwrap(); // duplicate
    }

    fn pid_prefix() -> String {
        format!("/tmp/nf_eng_test_{}", std::process::id())
    }

    fn make_candle(ts: i64, open: f64, high: f64, low: f64, close: f64) -> Candle {
        Candle {
            timestamp: ts,
            open,
            high,
            low,
            close,
            volume: 1000.0,
        }
    }

    fn long_signal() -> Signal {
        Signal {
            signal_id: SignalId::new("SIG-BT-00000001"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            side: Side::Long,
            entry_timeframe: Timeframe::OneMinute,
            screening_timeframe: Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time: 1_700_000_000_000,
            entry_price: 30_000.0,
            stop_loss: 29_700.0,
            take_profit: 30_600.0,
            confidence: 75,
            regime: "bullish".to_string(),
            entry_reason: "ema_cross".to_string(),
            filters_passed: vec![],
            filters_failed: vec![],
            expected_reward_bps: 200.0,
            estimated_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    fn default_cost_cfg() -> CostModelConfig {
        CostModelConfig {
            taker_fee_bps: 4.0,
            slippage_bps: 2.0,
            spread_bps: 1.0,
            market_impact_bps: 1.0,
            stop_slippage_bps: 5.0,
        }
    }

    fn default_bt_cfg() -> BacktestConfig {
        BacktestConfig {
            initial_equity: 10_000.0,
            reports_dir: "/tmp".to_string(),
            conservative_intrabar: true,
            max_bars_held: 60,
            entry_geometry_mode: EntryGeometryMode::PreserveSignalLevels,
        }
    }

    // ── Structural tests ──────────────────────────────────────────────────────

    #[test]
    fn engine_returns_none_when_csv_missing() {
        let cfg = default_cfg();
        let result = BacktestEngine::run(&cfg, "NONEXISTENT_SYMBOL_XYZ");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn unknown_strategy_id_returns_config_error() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "bad_strategy_xyz".to_string();
        // validate_strategy_config runs before CSV check, so non-existent CSV is fine.
        let result = BacktestEngine::run(&cfg, "NONEXISTENT_SYMBOL_XYZ");
        assert!(result.is_err(), "expected Err for unknown strategy_id");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("bad_strategy_xyz") || msg.contains("unknown"),
            "error must mention the bad id: {msg}"
        );
    }

    #[test]
    fn backtest_selects_v1_from_config() {
        let dir = "/tmp";
        let sym = format!("nf_v1sel_{}", std::process::id());
        let path = format!("{dir}/{sym}.csv");
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();
        cfg.strategy_id = "screened_vwap_scalp".to_string();

        let result = BacktestEngine::run(&cfg, &sym);
        assert!(result.is_ok(), "v1 strategy must not return Err");
        assert!(result.unwrap().is_some(), "expected Some for valid CSV");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn backtest_selects_v2_from_config() {
        let dir = "/tmp";
        let sym = format!("nf_v2sel_{}", std::process::id());
        let path = format!("{dir}/{sym}.csv");
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();
        cfg.strategy_id = "screened_vwap_scalp_v2".to_string();

        let result = BacktestEngine::run(&cfg, &sym);
        assert!(
            result.is_ok(),
            "v2 strategy must not return Err: {result:?}"
        );
        assert!(result.unwrap().is_some(), "expected Some for valid CSV");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn v2_trade_reports_strategy_id() {
        let dir = "/tmp";
        let sym = format!("nf_v2sid_{}", std::process::id());
        let path = format!("{dir}/{sym}.csv");
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();
        cfg.strategy_id = "screened_vwap_scalp_v2".to_string();

        let result = BacktestEngine::run(&cfg, &sym)
            .expect("v2 must not error")
            .expect("must return Some for valid CSV");

        // Any trades generated must have the v2 strategy_id.
        for trade in &result.trades {
            assert_eq!(
                trade.strategy_id.as_str(),
                "screened_vwap_scalp_v2",
                "trade strategy_id must match active strategy"
            );
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn engine_rejects_data_quality_errors() {
        let path = format!("{}_dupe.csv", pid_prefix());
        let sym = format!("{}_dupe", pid_prefix().replace('/', "_").replace('-', "_"));
        // Use a path that maps to data_dir + symbol.csv
        let dir = "/tmp";
        let sym_clean = format!("nf_dupe_{}", std::process::id());
        let full = format!("{}/{}.csv", dir, sym_clean);
        write_dupe_csv(&full);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym_clean);
        assert!(
            result.is_err(),
            "expected Err for duplicate timestamps, got: {result:?}"
        );
        std::fs::remove_file(&full).ok();
        let _ = path;
        let _ = sym;
    }

    #[test]
    fn engine_produces_result_for_valid_csv() {
        let dir = "/tmp";
        let sym = format!("nf_valid_{}", std::process::id());
        let path = format!("{}/{}.csv", dir, sym);
        // 250 candles — enough for indicator warmup
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym).expect("expected Ok");
        assert!(result.is_some(), "expected Some for valid CSV");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn engine_writes_no_fake_trades_when_no_signal() {
        let dir = "/tmp";
        let sym = format!("nf_nosig_{}", std::process::id());
        let path = format!("{}/{}.csv", dir, sym);
        // Flat price — strategy unlikely to emit a signal
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym).expect("ok").expect("some");

        // Verify all trades have deterministic IDs
        for trade in &result.trades {
            let tid = trade.trade_id.as_str();
            let sid = trade.signal_id.as_str();
            assert!(tid.starts_with("TRD-SIG-BT-"), "bad trade_id: {tid}");
            assert!(sid.starts_with("SIG-BT-"), "bad signal_id: {sid}");
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn engine_generates_deterministic_signal_ids() {
        let dir = "/tmp";
        let sym = format!("nf_sigid_{}", std::process::id());
        let path = format!("{}/{}.csv", dir, sym);
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym).expect("ok").expect("some");

        for trade in &result.trades {
            let sid = trade.signal_id.as_str();
            assert!(
                sid.starts_with("SIG-BT-"),
                "signal_id must start with SIG-BT-: {sid}"
            );
            // Must be exactly 8 hex/decimal digits after the prefix
            let suffix = &sid["SIG-BT-".len()..];
            assert_eq!(suffix.len(), 8, "signal_id suffix must be 8 chars: {sid}");
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn engine_does_not_use_incomplete_5m_or_15m_candles() {
        let dir = "/tmp";
        let sym = format!("nf_incomplete_{}", std::process::id());
        let path = format!("{}/{}.csv", dir, sym);
        // Only 3 1m candles — no complete 5m or 15m → no signals, no crash
        write_test_csv(&path, 3, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym).expect("ok").expect("some");
        assert_eq!(result.trades.len(), 0, "no trades without complete 5m/15m");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn engine_updates_equity_after_closed_trade() {
        let dir = "/tmp";
        let sym = format!("nf_equity_{}", std::process::id());
        let path = format!("{}/{}.csv", dir, sym);
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym).expect("ok").expect("some");

        // Equity curve always has at least one point (initial).
        assert!(
            !result.equity_curve.is_empty(),
            "equity curve must not be empty"
        );

        // If trades occurred, verify equity curve has more than initial point.
        if !result.trades.is_empty() {
            assert!(
                result.equity_curve.len() > 1,
                "equity curve must grow with each trade"
            );
        }

        // All equity values must be finite.
        for ep in &result.equity_curve {
            assert!(
                ep.equity.is_finite(),
                "equity must be finite: {}",
                ep.equity
            );
            assert!(ep.drawdown_pct.is_finite(), "drawdown must be finite");
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn engine_closes_open_trade_at_end_of_backtest() {
        // The end-of-backtest close is tested by the fill model test.
        // Here we verify engine returns a result for the minimal case.
        let dir = "/tmp";
        let sym = format!("nf_eob_{}", std::process::id());
        let path = format!("{}/{}.csv", dir, sym);
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym).expect("ok").expect("some");

        // After engine completes, open_position is always None (closed or none opened).
        // The result should be consistent: equity_curve has initial + 1 point per trade.
        let trade_count = result.trades.len();
        assert!(
            result.equity_curve.len() >= 1,
            "at minimum, initial equity point present"
        );
        assert_eq!(
            result.summary.total_trades, trade_count,
            "summary and trades must agree"
        );

        std::fs::remove_file(&path).ok();
    }

    // ── No-lookahead helper tests ─────────────────────────────────────────────

    #[test]
    fn latest_snap_returns_none_when_all_too_recent() {
        let candle = Candle {
            timestamp: 1_700_000_000_000,
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 10.0,
        };
        let eng = &mut IndicatorEngine::new_default().unwrap();
        let snap = eng.next(candle).unwrap();
        let snaps = vec![(candle.timestamp, snap, candle)];
        // max_ts is before all snaps → must return None
        let result = latest_snap(&snaps, candle.timestamp - 1);
        assert!(result.is_none());
    }

    #[test]
    fn latest_snap_returns_most_recent_eligible() {
        let mut eng = IndicatorEngine::new_default().unwrap();
        let mut snaps = Vec::new();
        for i in 0..5_i64 {
            let c = Candle {
                timestamp: 1_700_000_000_000 + i * 300_000,
                open: 100.0,
                high: 110.0,
                low: 90.0,
                close: 105.0,
                volume: 10.0,
            };
            let snap = eng.next(c).unwrap();
            snaps.push((c.timestamp, snap, c));
        }
        // max_ts = ts of 3rd entry (index 2)
        let max = snaps[2].0;
        let result = latest_snap(&snaps, max);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, max);
    }

    // ── Binary-search latest_snap tests ──────────────────────────────────────

    fn make_snaps(timestamps: &[i64]) -> Vec<(i64, IndicatorSnapshot, Candle)> {
        let mut eng = IndicatorEngine::new_default().unwrap();
        timestamps
            .iter()
            .map(|&ts| {
                let c = make_candle(ts, 100.0, 110.0, 90.0, 105.0);
                let snap = eng.next(c).unwrap();
                (ts, snap, c)
            })
            .collect()
    }

    #[test]
    fn latest_snap_uses_exact_match() {
        let timestamps = [1_000_000, 2_000_000, 3_000_000];
        let snaps = make_snaps(&timestamps);
        let result = latest_snap(&snaps, 2_000_000);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, 2_000_000);
    }

    #[test]
    fn latest_snap_returns_previous_when_between_timestamps() {
        let timestamps = [1_000_000, 2_000_000, 3_000_000];
        let snaps = make_snaps(&timestamps);
        // 1_500_000 is between index 0 and 1 → must return index 0
        let result = latest_snap(&snaps, 1_500_000);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, 1_000_000);
    }

    #[test]
    fn latest_snap_returns_none_before_first_timestamp() {
        let timestamps = [1_000_000, 2_000_000, 3_000_000];
        let snaps = make_snaps(&timestamps);
        let result = latest_snap(&snaps, 999_999);
        assert!(result.is_none());
    }

    #[test]
    fn latest_snap_returns_last_when_after_last_timestamp() {
        let timestamps = [1_000_000, 2_000_000, 3_000_000];
        let snaps = make_snaps(&timestamps);
        let result = latest_snap(&snaps, 9_999_999);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, 3_000_000);
    }

    #[test]
    fn latest_snap_never_returns_snapshot_with_ts_greater_than_max() {
        let timestamps = [1_000_000, 2_000_000, 3_000_000];
        let snaps = make_snaps(&timestamps);
        for &max_ts in &[
            500_000_i64,
            1_000_000,
            1_500_000,
            2_000_000,
            2_500_000,
            3_000_000,
        ] {
            if let Some(snap) = latest_snap(&snaps, max_ts) {
                assert!(
                    snap.0 <= max_ts,
                    "no-lookahead violated: snap.ts={} > max_ts={}",
                    snap.0,
                    max_ts
                );
            }
        }
    }

    // ── Same-candle exit after entry ─────────────────────────────────────────

    /// Verifies that after entering at candle open, SL/TP can fire on the same
    /// candle.  We simulate the engine's A→B flow directly: create the position,
    /// increment bars_held (as the loop does), then call check_exit.
    #[test]
    fn engine_does_not_skip_exit_check_on_entry_candle() {
        let signal = long_signal();
        let cost_cfg = default_cost_cfg();
        let bt_cfg = default_bt_cfg();
        let symbol = Symbol::new("BTCUSDT").unwrap();

        // Entry candle: open near entry price, low touches SL (29700), high well clear
        let entry_candle = make_candle(
            1_700_000_060_000,
            30_000.0, // open — entry price
            30_050.0, // high — does not touch TP (30600)
            29_650.0, // low  — touches SL (29700)
            29_800.0,
        );

        // Simulate entry (as engine section A does)
        let entry_fill = FillModel::simulate_entry(
            &signal,
            0.1,
            &entry_candle,
            cost_cfg.slippage_bps,
            cost_cfg.taker_fee_bps,
        );
        let mut pos = OpenSimPosition {
            signal: signal.clone(),
            qty: 0.1,
            entry_time: entry_fill.time,
            entry_price: entry_fill.price,
            entry_fee: entry_fill.fee,
            entry_slippage: entry_fill.slippage,
            bars_held: 0,
        };

        // Engine section B: increment bars_held, then check exit
        pos.bars_held += 1;
        let exit_fill = FillModel::check_exit(
            &pos,
            &entry_candle,
            bt_cfg.conservative_intrabar,
            cost_cfg.slippage_bps,
            cost_cfg.taker_fee_bps,
            bt_cfg.max_bars_held,
        );

        assert!(
            exit_fill.is_some(),
            "exit must fire on entry candle when low touches SL"
        );
        let exit = exit_fill.unwrap();
        assert_eq!(
            exit.reason,
            TradeExitReason::StopLoss,
            "SL must be the exit reason"
        );

        // Build a trade — must not panic and net_pnl must be computed
        let trade = build_trade(&pos, &exit, symbol, &cost_cfg);
        assert!(trade.net_pnl.is_finite(), "net_pnl must be finite");
        assert!(trade.net_pnl < 0.0, "stop-loss trade must be a loss");
    }

    // ── Risk error propagation ────────────────────────────────────────────────

    /// Verifies that RiskEngine::assess returns Err for invalid config (not a
    /// soft rejection), so the backtest engine propagates it as a fatal error.
    #[test]
    fn engine_propagates_risk_engine_error() {
        // Invalid risk config: risk_per_trade_pct = 0 → RiskEngine::assess returns Err
        let bad_risk_cfg = RiskConfig {
            risk_per_trade_pct: 0.0, // invalid — must be > 0
            max_open_positions: 3,
            max_leverage: 3.0,
            min_reward_risk: 1.5,
            max_daily_loss_pct: 2.0,
            max_drawdown_pct: 5.0,
        };
        let cost_cfg = default_cost_cfg();
        let risk_ctx = RiskContext {
            equity: 10_000.0,
            peak_equity: 10_000.0,
            daily_realized_pnl: 0.0,
            open_positions: 0,
        };

        let result = RiskEngine::assess(&bad_risk_cfg, &cost_cfg, &risk_ctx, &long_signal());
        assert!(
            result.is_err(),
            "RiskEngine::assess must return Err for invalid config, got: {result:?}"
        );
    }

    /// Verifies that a normal risk rejection returns Ok with approved=false,
    /// not an Err — so the engine records it and continues, not fatal.
    #[test]
    fn engine_risk_rejection_returns_ok_approved_false() {
        // Context with too many open positions → rejected, not errored
        let risk_cfg = RiskConfig {
            risk_per_trade_pct: 1.0,
            max_open_positions: 1,
            max_leverage: 3.0,
            min_reward_risk: 1.5,
            max_daily_loss_pct: 2.0,
            max_drawdown_pct: 5.0,
        };
        let cost_cfg = default_cost_cfg();
        let risk_ctx = RiskContext {
            equity: 10_000.0,
            peak_equity: 10_000.0,
            daily_realized_pnl: 0.0,
            open_positions: 1, // >= max_open_positions(1) → rejected
        };

        let result = RiskEngine::assess(&risk_cfg, &cost_cfg, &risk_ctx, &long_signal());
        assert!(result.is_ok(), "risk rejection must be Ok, not Err");
        assert!(
            !result.unwrap().approved,
            "normal risk rejection must have approved=false"
        );
    }

    // ── adjusted_signal_for_actual_entry helper ───────────────────────────────

    #[test]
    fn adjusted_signal_for_actual_entry_long_recalculates_expected_reward() {
        let signal = long_signal(); // entry=30000, TP=30600, SL=29700
        // Adverse price slightly higher than original entry
        let actual_price = 30_006.0;
        let adjusted = adjusted_signal_for_actual_entry(
            &signal,
            actual_price,
            EntryGeometryMode::PreserveSignalLevels,
        );

        assert_eq!(adjusted.entry_price, actual_price);
        // expected_reward_bps = (30600 - 30006) / 30006 * 10000 ≈ 197.99
        let expected = (30_600.0 - actual_price) / actual_price * 10_000.0;
        assert!(
            (adjusted.expected_reward_bps - expected).abs() < 1e-6,
            "expected_reward_bps mismatch: got {}, expected {}",
            adjusted.expected_reward_bps,
            expected
        );
        // net edge = reward - cost
        let expected_net = expected - signal.estimated_cost_bps;
        assert!(
            (adjusted.expected_net_edge_bps - expected_net).abs() < 1e-6,
            "expected_net_edge_bps mismatch"
        );
        // Other fields unchanged
        assert_eq!(adjusted.stop_loss, signal.stop_loss);
        assert_eq!(adjusted.take_profit, signal.take_profit);
        assert_eq!(adjusted.signal_id, signal.signal_id);
    }

    #[test]
    fn adjusted_signal_for_actual_entry_short_recalculates_expected_reward() {
        use crate::core::Timeframe;
        let signal = Signal {
            signal_id: SignalId::new("SIG-BT-00000002"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            side: Side::Short,
            entry_timeframe: Timeframe::OneMinute,
            screening_timeframe: Timeframe::FifteenMinute,
            confirmation_timeframe: Timeframe::FiveMinute,
            entry_time: 1_700_000_000_000,
            entry_price: 30_000.0,
            stop_loss: 30_300.0,
            take_profit: 29_400.0,
            confidence: 75,
            regime: "bearish".to_string(),
            entry_reason: "ema_cross_down".to_string(),
            filters_passed: vec![],
            filters_failed: vec![],
            expected_reward_bps: 200.0,
            estimated_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        };
        // Adverse price slightly lower than original entry (short fills below open)
        let actual_price = 29_994.0;
        let adjusted = adjusted_signal_for_actual_entry(
            &signal,
            actual_price,
            EntryGeometryMode::PreserveSignalLevels,
        );

        assert_eq!(adjusted.entry_price, actual_price);
        // expected_reward_bps = (29994 - 29400) / 29994 * 10000
        let expected = (actual_price - 29_400.0) / actual_price * 10_000.0;
        assert!(
            (adjusted.expected_reward_bps - expected).abs() < 1e-6,
            "expected_reward_bps mismatch: got {}, expected {}",
            adjusted.expected_reward_bps,
            expected
        );
    }

    #[test]
    fn actual_entry_invalid_long_geometry_is_rejected_not_fatal() {
        // Long signal: TP=30600, SL=29700. If actual entry >= TP, geometry invalid.
        let signal = long_signal(); // entry=30000, TP=30600, SL=29700
        let actual_price = 30_600.0; // == TP → invalid geometry for long (need entry < TP)
        let adjusted = adjusted_signal_for_actual_entry(
            &signal,
            actual_price,
            EntryGeometryMode::PreserveSignalLevels,
        );
        assert!(
            !adjusted.valid_geometry(),
            "entry == take_profit must be invalid geometry for long"
        );
    }

    #[test]
    fn actual_entry_invalid_long_geometry_above_tp_is_rejected_not_fatal() {
        let signal = long_signal();
        let actual_price = 30_700.0; // > TP → definitely invalid
        let adjusted = adjusted_signal_for_actual_entry(
            &signal,
            actual_price,
            EntryGeometryMode::PreserveSignalLevels,
        );
        assert!(
            !adjusted.valid_geometry(),
            "entry > take_profit must be invalid geometry for long"
        );
    }

    #[test]
    fn actual_entry_valid_long_geometry_passes() {
        let signal = long_signal(); // entry=30000, TP=30600, SL=29700
        // Slippage of 2 bps on 30000 → 30006 < TP=30600 and > SL=29700 → valid
        let actual_price = FillModel::adverse_entry_price(Side::Long, 30_000.0, 2.0);
        let adjusted = adjusted_signal_for_actual_entry(
            &signal,
            actual_price,
            EntryGeometryMode::PreserveSignalLevels,
        );
        assert!(
            adjusted.valid_geometry(),
            "normal slippage must not invalidate geometry: actual_price={actual_price}"
        );
    }

    // ── BacktestResult new fields ─────────────────────────────────────────────

    #[test]
    fn signal_flow_summary_present_in_backtest_result() {
        let dir = "/tmp";
        let sym = format!("nf_flow_{}", std::process::id());
        let path = format!("{}/{}.csv", dir, sym);
        write_test_csv(&path, 250, 1_700_000_000_000);

        let mut cfg = default_cfg();
        cfg.data_dir = dir.to_string();

        let result = BacktestEngine::run(&cfg, &sym).expect("ok").expect("some");

        // New fields must be present and internally consistent.
        let flow = &result.signal_flow;
        assert_eq!(
            flow.trades_closed,
            result.trades.len(),
            "signal_flow.trades_closed must equal trades.len()"
        );
        assert_eq!(
            flow.risk_rejections,
            result.risk_rejections.len(),
            "signal_flow.risk_rejections must equal risk_rejections.len()"
        );
        assert!(
            flow.signals_preapproved >= flow.trades_opened,
            "trades_opened must not exceed preapproved signals"
        );
        assert!(
            flow.signals_generated >= flow.signals_preapproved + flow.signals_rejected_initial_risk,
            "generated must be >= preapproved + initial_rejected"
        );

        std::fs::remove_file(&path).ok();
    }

    // ── stage / assessment-field tests ────────────────────────────────────────

    #[test]
    fn initial_risk_rejection_stage_is_initial_risk() {
        let signal = long_signal();
        let rej = build_rejection(
            &signal,
            "initial_risk",
            "preserve_signal_levels",
            signal.entry_time,
            10_000.0,
            10_000.0,
            0.0,
            "reward_risk_below_minimum",
            signal.expected_reward_bps,
            signal.estimated_cost_bps,
            signal.expected_net_edge_bps,
        );
        assert_eq!(rej.stage, "initial_risk");
    }

    #[test]
    fn actual_entry_rejection_stage_is_actual_entry() {
        let signal = long_signal();
        let rej = build_rejection(
            &signal,
            "actual_entry",
            "preserve_signal_levels",
            signal.entry_time,
            10_000.0,
            10_000.0,
            0.0,
            "expected_net_edge_not_positive",
            signal.expected_reward_bps,
            signal.estimated_cost_bps,
            signal.expected_net_edge_bps,
        );
        assert_eq!(rej.stage, "actual_entry");
    }

    #[test]
    fn initial_risk_rejection_uses_assessment_cost_fields() {
        let signal = long_signal();
        let risk_cfg = RiskConfig {
            risk_per_trade_pct: 1.0,
            max_open_positions: 1,
            max_leverage: 5.0,
            min_reward_risk: 1.5,
            max_daily_loss_pct: 2.0,
            max_drawdown_pct: 10.0,
        };
        let cost_cfg = default_cost_cfg();
        // Reject via max_open_positions so we get a clean assessment with cost fields.
        let risk_ctx = RiskContext {
            equity: 10_000.0,
            peak_equity: 10_000.0,
            daily_realized_pnl: 0.0,
            open_positions: 1, // at max — will reject
        };
        let assessment = RiskEngine::assess(&risk_cfg, &cost_cfg, &risk_ctx, &signal).unwrap();
        assert!(
            !assessment.approved,
            "expected rejection for max_open_positions"
        );

        // assessment.expected_cost_bps comes from the cost model (total_adverse_cost_bps),
        // which sums taker fees, spread, slippage, market impact, and stop slippage.
        // With default_cost_cfg() this is > signal.estimated_cost_bps (8.0).
        assert!(
            (assessment.expected_cost_bps - signal.estimated_cost_bps).abs() > 1e-9,
            "assessment cost should differ from stale signal cost so the test is meaningful"
        );

        // The rejection built with assessment fields must use assessment values, not signal values.
        let rej = build_rejection(
            &signal,
            "initial_risk",
            "preserve_signal_levels",
            signal.entry_time,
            risk_ctx.equity,
            risk_ctx.peak_equity,
            risk_ctx.daily_realized_pnl,
            "max_open_positions",
            assessment.expected_reward_bps,
            assessment.expected_cost_bps,
            assessment.expected_net_edge_bps,
        );
        assert!(
            (rej.expected_cost_bps - assessment.expected_cost_bps).abs() < 1e-9,
            "rejection must carry assessment.expected_cost_bps={}, got {}",
            assessment.expected_cost_bps,
            rej.expected_cost_bps
        );
        assert!(
            (rej.expected_net_edge_bps - assessment.expected_net_edge_bps).abs() < 1e-9,
            "rejection must carry assessment.expected_net_edge_bps={}, got {}",
            assessment.expected_net_edge_bps,
            rej.expected_net_edge_bps
        );
    }

    #[test]
    fn actual_entry_risk_rejection_uses_assessment_cost_fields() {
        let signal = long_signal();
        let risk_cfg = RiskConfig {
            risk_per_trade_pct: 1.0,
            max_open_positions: 1,
            max_leverage: 5.0,
            min_reward_risk: 1.5,
            max_daily_loss_pct: 2.0,
            max_drawdown_pct: 10.0,
        };
        let cost_cfg = default_cost_cfg();
        let risk_ctx = RiskContext {
            equity: 10_000.0,
            peak_equity: 10_000.0,
            daily_realized_pnl: 0.0,
            open_positions: 1, // at max — will reject
        };
        let assessment = RiskEngine::assess(&risk_cfg, &cost_cfg, &risk_ctx, &signal).unwrap();
        assert!(!assessment.approved);

        let rej = build_rejection(
            &signal,
            "actual_entry",
            "preserve_signal_levels",
            signal.entry_time,
            risk_ctx.equity,
            risk_ctx.peak_equity,
            risk_ctx.daily_realized_pnl,
            "max_open_positions",
            assessment.expected_reward_bps,
            assessment.expected_cost_bps,
            assessment.expected_net_edge_bps,
        );
        assert_eq!(rej.stage, "actual_entry");
        assert!(
            (rej.expected_cost_bps - assessment.expected_cost_bps).abs() < 1e-9,
            "actual_entry rejection must carry assessment cost, not stale signal cost"
        );
    }
}
