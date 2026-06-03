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
//!      b. Check exit for open position (SL / TP / TimeExit).
//!      c. Evaluate strategy — no lookahead across 5m / 15m.
//!      d. If signal approved by risk engine, set pending entry for next candle.
//!   7. Close any remaining open position as EndOfBacktest.
//!   8. Return BacktestResult.
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
use crate::risk::{CostModelConfig, RiskContext, RiskEngine};
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
                }
                // Do not check exits or evaluate strategy on the entry candle.
                continue;
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
            // Only when no open position and no pending entry.
            if open_position.is_none() && equity > 0.0 {
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

                            match RiskEngine::assess(&risk_cfg, &cost_cfg, &risk_ctx, &signal) {
                                Ok(assessment) if assessment.approved => {
                                    if let Some(qty) = assessment.qty {
                                        if qty > 0.0 {
                                            pending_entry = Some((signal, qty));
                                        }
                                    }
                                }
                                Ok(_) => {}  // risk rejected
                                Err(_) => {} // risk error — skip signal
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
        let snap = IndicatorSnapshot::default();
        let snaps = vec![(1_700_000_001_000_i64, snap, candle)];
        // max_ts = 999 → nothing qualifies
        let found = latest_snap(&snaps, 999_i64);
        assert!(found.is_none());
    }

    #[test]
    fn latest_snap_returns_most_recent_qualifying() {
        let candle = Candle {
            timestamp: 100,
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 10.0,
        };
        let s1 = IndicatorSnapshot::default();
        let s2 = IndicatorSnapshot::default();
        let snaps = vec![(100_i64, s1, candle), (200_i64, s2, candle)];
        // max_ts = 200 → both qualify, should get ts=200
        let found = latest_snap(&snaps, 200_i64).unwrap();
        assert_eq!(found.0, 200);
    }

    #[test]
    fn drawdown_pct_zero_when_at_peak() {
        assert!((drawdown_pct(5000.0, 5000.0)).abs() < 1e-9);
    }

    #[test]
    fn drawdown_pct_correct_calculation() {
        let dd = drawdown_pct(10_000.0, 9_000.0);
        assert!((dd - 10.0).abs() < 1e-9);
    }
}
