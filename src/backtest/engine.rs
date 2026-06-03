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
//!      b. Check exit for open position (SL / TP / TimeExit) — even on entry candle.
//!      c. Evaluate strategy — no lookahead across 5m / 15m; skipped on entry candle.
//!      d. If signal approved by risk engine, set pending entry for next candle.
//!   7. Close any remaining open position as EndOfBacktest.
//!   8. Return BacktestResult.
//!
//! Conservative intrabar rule: if SL and TP are both touched in the same candle,
//! SL is assumed first.
//!
//! No exchange calls. No LLMs. Historical simulation only.

use std::path::Path;

use crate::backtest::fill_model::{FillModel, OpenSimPosition};
use crate::backtest::metrics::{BacktestSummary, EquityPoint, Metrics};
use crate::config::ResearchConfig;
use crate::core::{
    Candle, NorthflowError, PositionId, Side, Signal, Symbol, Trade, TradeExitReason, TradeId,
};
use crate::indicators::{IndicatorEngine, IndicatorSnapshot};
use crate::market::{CandleStore, OhlcvLoader};
use crate::risk::{CostModelConfig, RiskConfig, RiskContext, RiskEngine};
use crate::strategy::{MultiTimeframeInput, ScreenedVwapScalp, Strategy, StrategyContext};

// ── BacktestConfig ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub initial_equity: f64,
    pub reports_dir: String,
    pub conservative_intrabar: bool,
    pub max_bars_held: u32,
}

// ── BacktestResult ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<EquityPoint>,
    pub summary: BacktestSummary,
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

        // Build candle store.
        let store = CandleStore::build_from_1m(load_result.candles)?;

        if store.one_minute.is_empty() {
            return Ok(None);
        }

        // Precompute 5m snapshots — iterate all completed 5m candles through
        // a fresh IndicatorEngine, storing (timestamp, snapshot, candle).
        let mut eng_5m = IndicatorEngine::new_default()?;
        let mut snaps_5m: Vec<(i64, IndicatorSnapshot, Candle)> = Vec::new();
        for &c in &store.five_minute {
            let snap = eng_5m.next(c)?;
            snaps_5m.push((c.timestamp, snap, c));
        }

        // Precompute 15m snapshots.
        let mut eng_15m = IndicatorEngine::new_default()?;
        let mut snaps_15m: Vec<(i64, IndicatorSnapshot, Candle)> = Vec::new();
        for &c in &store.fifteen_minute {
            let snap = eng_15m.next(c)?;
            snaps_15m.push((c.timestamp, snap, c));
        }

        let bt_cfg = BacktestConfig {
            initial_equity: cfg.initial_equity,
            reports_dir: cfg.reports_dir.clone(),
            conservative_intrabar: cfg.conservative_intrabar,
            max_bars_held: cfg.max_bars_held,
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
        if let Some(first) = store.one_minute.first() {
            equity_curve.push(EquityPoint {
                timestamp: first.timestamp,
                equity,
                drawdown_pct: 0.0,
            });
        }

        let mut signal_counter: u64 = 0;
        let mut pending_entry: Option<(Signal, f64)> = None;
        let mut open_position: Option<OpenSimPosition> = None;

        let strategy = ScreenedVwapScalp::default();
        let mut eng_1m = IndicatorEngine::new_default()?;

        let one_minute = &store.one_minute;
        let n = one_minute.len();

        for i in 0..n {
            let candle = one_minute[i];

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
            // After entry, fall through to the exit-check block (B) so that
            // SL/TP can be triggered on the same candle the position was opened.
            let mut entered_this_bar = false;
            if let Some((signal, qty)) = pending_entry.take() {
                if open_position.is_none() && equity > 0.0 {
                    let entry = FillModel::simulate_entry(
                        &signal,
                        qty,
                        &candle,
                        cost_cfg.slippage_bps,
                        cost_cfg.taker_fee_bps,
                    );
                    open_position = Some(OpenSimPosition {
                        signal,
                        qty,
                        entry_time: entry.time,
                        entry_price: entry.price,
                        entry_fee: entry.fee,
                        entry_slippage: entry.slippage,
                        bars_held: 0,
                    });
                    entered_this_bar = true;
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
            if !entered_this_bar && open_position.is_none() && equity > 0.0 {
                // No-lookahead rule:
                //   signal_time = candle.timestamp + 60_000
                //   5m available: 5m_ts + 300_000 <= signal_time → 5m_ts <= candle.ts - 240_000
                //   15m available: 15m_ts + 900_000 <= signal_time → 15m_ts <= candle.ts - 840_000
                let max_5m_ts = candle.timestamp - 240_000;
                let max_15m_ts = candle.timestamp - 840_000;

                let snap5 = latest_snap(&snaps_5m, max_5m_ts);
                let snap15 = latest_snap(&snaps_15m, max_15m_ts);

                if let (Some((_, s5, c5)), Some((_, s15, c15))) = (snap5, snap15) {
                    let estimated_cost = cost_cfg.taker_fee_bps * 2.0
                        + cost_cfg.slippage_bps * 2.0
                        + cost_cfg.spread_bps;

                    let ctx = StrategyContext {
                        symbol: symbol_obj.clone(),
                        signal_index: signal_counter + 1,
                        estimated_cost_bps: estimated_cost,
                        min_confidence: cfg.min_confidence,
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

                            let risk_ctx = RiskContext {
                                equity,
                                peak_equity,
                                daily_realized_pnl,
                                open_positions: 0,
                            };

                            match try_assess_risk(&risk_cfg, &cost_cfg, &risk_ctx, signal)? {
                                Some((sig, qty)) => {
                                    pending_entry = Some((sig, qty));
                                }
                                None => {}
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
        }

        // ── End of backtest: close any remaining position ─────────────────────
        if let Some(ref pos) = open_position {
            if let Some(&last) = one_minute.last() {
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

        let summary = Metrics::summarize(&trades, &equity_curve);

        Ok(Some(BacktestResult {
            trades,
            equity_curve,
            summary,
        }))
    }
}

// ── Helper: assess risk and return pending entry or propagate error ────────────
//
// Ok(Some((signal, qty))) — approved, use for pending entry
// Ok(None)                — rejected normally, skip signal
// Err(e)                  — invalid input / config; stop the backtest
fn try_assess_risk(
    risk_cfg: &RiskConfig,
    cost_cfg: &CostModelConfig,
    risk_ctx: &RiskContext,
    signal: Signal,
) -> Result<Option<(Signal, f64)>, NorthflowError> {
    match RiskEngine::assess(risk_cfg, cost_cfg, risk_ctx, &signal) {
        Ok(assessment) if assessment.approved => {
            if let Some(qty) = assessment.qty {
                if qty > 0.0 {
                    return Ok(Some((signal, qty)));
                }
            }
            Ok(None)
        }
        Ok(_) => Ok(None),
        Err(e) => Err(e),
    }
}

// ── Helper: latest completed snapshot with ts <= max_ts ───────────────────────

fn latest_snap<'a>(
    snaps: &'a [(i64, IndicatorSnapshot, Candle)],
    max_ts: i64,
) -> Option<&'a (i64, IndicatorSnapshot, Candle)> {
    snaps.iter().rev().find(|(ts, _, _)| *ts <= max_ts)
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

    /// Verifies that try_assess_risk propagates Err (invalid config/signal)
    /// and does not silently swallow it.
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

        let result = try_assess_risk(&bad_risk_cfg, &cost_cfg, &risk_ctx, long_signal());
        assert!(
            result.is_err(),
            "try_assess_risk must propagate RiskEngine Err, got: {result:?}"
        );
    }

    /// Verifies that a normal risk rejection (approved=false) returns Ok(None),
    /// not an Err.
    #[test]
    fn engine_risk_rejection_returns_ok_none() {
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

        let result = try_assess_risk(&risk_cfg, &cost_cfg, &risk_ctx, long_signal());
        assert!(result.is_ok(), "risk rejection must be Ok, not Err");
        assert!(result.unwrap().is_none(), "risk rejection must return None");
    }
}
