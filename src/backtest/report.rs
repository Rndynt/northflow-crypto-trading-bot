//! Report writer — writes backtest results to reports/ directory.
//!
//! No external dependencies.  Uses std::fs only.
//! Creates the reports directory if missing.

use std::fs;
use std::path::Path;

use crate::backtest::metrics::{BacktestSummary, EquityPoint};
use crate::backtest::risk_trace::{RiskRejection, SignalFlowSummary};
use crate::core::{NorthflowError, Trade};

// ── ReportWriter ──────────────────────────────────────────────────────────────

pub struct ReportWriter;

impl ReportWriter {
    pub fn write_all(
        reports_dir: &str,
        summary: &BacktestSummary,
        trades: &[Trade],
        equity_curve: &[EquityPoint],
        risk_rejections: &[RiskRejection],
        signal_flow: &SignalFlowSummary,
    ) -> Result<(), NorthflowError> {
        let dir = Path::new(reports_dir);
        fs::create_dir_all(dir).map_err(|e| {
            NorthflowError::DataError(format!("cannot create reports dir '{}': {e}", reports_dir))
        })?;

        Self::write_summary_json(dir, summary)?;
        Self::write_trades_csv(dir, trades)?;
        Self::write_equity_csv(dir, equity_curve)?;
        Self::write_risk_rejections_csv(dir, risk_rejections)?;
        Self::write_signal_flow_summary_json(dir, signal_flow)?;

        Ok(())
    }

    // ── Summary JSON ──────────────────────────────────────────────────────────

    fn write_summary_json(dir: &Path, s: &BacktestSummary) -> Result<(), NorthflowError> {
        let path = dir.join("backtest_summary.json");
        let pf = if s.profit_factor.is_infinite() {
            "\"inf\"".to_string()
        } else if s.profit_factor.is_nan() {
            "0".to_string()
        } else {
            format!("{:.6}", s.profit_factor)
        };

        let json = format!(
            "{{\n\
              \"total_trades\": {},\n\
              \"win_rate\": {:.6},\n\
              \"net_pnl\": {:.6},\n\
              \"gross_pnl\": {:.6},\n\
              \"total_fee\": {:.6},\n\
              \"total_slippage\": {:.6},\n\
              \"profit_factor\": {},\n\
              \"expectancy\": {:.6},\n\
              \"avg_win\": {:.6},\n\
              \"avg_loss\": {:.6},\n\
              \"max_drawdown\": {:.6},\n\
              \"max_consecutive_losses\": {},\n\
              \"avg_trade_duration\": {:.6}\n\
            }}",
            s.total_trades,
            s.win_rate,
            s.net_pnl,
            s.gross_pnl,
            s.total_fee,
            s.total_slippage,
            pf,
            s.expectancy,
            s.avg_win,
            s.avg_loss,
            s.max_drawdown,
            s.max_consecutive_losses,
            s.avg_trade_duration,
        );

        fs::write(&path, json)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── Trades CSV ────────────────────────────────────────────────────────────

    fn write_trades_csv(dir: &Path, trades: &[Trade]) -> Result<(), NorthflowError> {
        let path = dir.join("trades.csv");
        let mut rows: Vec<String> = Vec::with_capacity(trades.len() + 1);
        rows.push(
            "trade_id,signal_id,symbol,strategy_id,regime,side,\
             entry_time,exit_time,entry_price,exit_price,stop_loss,take_profit,qty,\
             gross_pnl,fee,slippage,net_pnl,reward_risk,bars_held,exit_reason,\
             entry_reason,filters_passed,filters_failed,expected_edge_bps,actual_edge_bps"
                .to_string(),
        );

        for t in trades {
            let filters_passed = t.filters_passed.join("|");
            let filters_failed = t.filters_failed.join("|");
            let row = format!(
                "{},{},{},{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.8},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{},{},{},{:.6},{:.6}",
                csv_escape(t.trade_id.as_str()),
                csv_escape(t.signal_id.as_str()),
                csv_escape(t.symbol.as_str()),
                csv_escape(t.strategy_id.as_str()),
                csv_escape(&t.regime),
                csv_escape(t.side.as_str()),
                t.entry_time,
                t.exit_time,
                t.entry_price,
                t.exit_price,
                t.stop_loss,
                t.take_profit,
                t.quantity,
                t.gross_pnl,
                t.fee,
                t.slippage,
                t.net_pnl,
                t.reward_risk,
                t.bars_held,
                csv_escape(t.exit_reason.as_str()),
                csv_escape(&t.entry_reason),
                csv_escape(&filters_passed),
                csv_escape(&filters_failed),
                t.expected_edge_bps,
                t.actual_edge_bps,
            );
            rows.push(row);
        }

        let content = rows.join("\n") + "\n";
        fs::write(&path, content)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── Equity CSV ────────────────────────────────────────────────────────────

    fn write_equity_csv(dir: &Path, curve: &[EquityPoint]) -> Result<(), NorthflowError> {
        let path = dir.join("equity_curve.csv");
        let mut rows: Vec<String> = Vec::with_capacity(curve.len() + 1);
        rows.push("timestamp,equity,drawdown_pct".to_string());

        for p in curve {
            rows.push(format!(
                "{},{:.6},{:.6}",
                p.timestamp, p.equity, p.drawdown_pct
            ));
        }

        let content = rows.join("\n") + "\n";
        fs::write(&path, content)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── Risk Rejections CSV ───────────────────────────────────────────────────

    fn write_risk_rejections_csv(
        dir: &Path,
        rejections: &[RiskRejection],
    ) -> Result<(), NorthflowError> {
        let path = dir.join("risk_rejections.csv");
        let mut rows: Vec<String> = Vec::with_capacity(rejections.len() + 1);
        rows.push(
            "signal_id,stage,entry_geometry_mode,timestamp,side,regime,reason,equity,peak_equity,\
             drawdown_pct,daily_realized_pnl,expected_reward_bps,\
             expected_cost_bps,expected_net_edge_bps"
                .to_string(),
        );
        for r in rejections {
            rows.push(format!(
                "{},{},{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
                csv_escape(&r.signal_id),
                csv_escape(&r.stage),
                csv_escape(&r.entry_geometry_mode),
                r.timestamp,
                csv_escape(&r.side),
                csv_escape(&r.regime),
                csv_escape(&r.reason),
                r.equity,
                r.peak_equity,
                r.drawdown_pct,
                r.daily_realized_pnl,
                r.expected_reward_bps,
                r.expected_cost_bps,
                r.expected_net_edge_bps,
            ));
        }
        let content = rows.join("\n") + "\n";
        fs::write(&path, content)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── Signal Flow Summary JSON ──────────────────────────────────────────────

    fn write_signal_flow_summary_json(
        dir: &Path,
        flow: &SignalFlowSummary,
    ) -> Result<(), NorthflowError> {
        let path = dir.join("signal_flow_summary.json");
        let json = format!(
            "{{\n\
              \"entry_geometry_mode\": \"{}\",\n\
              \"signals_generated\": {},\n\
              \"signals_preapproved\": {},\n\
              \"signals_rejected_initial_risk\": {},\n\
              \"signals_rejected_actual_entry\": {},\n\
              \"trades_opened\": {},\n\
              \"trades_closed\": {},\n\
              \"risk_rejections\": {},\n\
              \"rejections_max_drawdown\": {},\n\
              \"rejections_daily_loss\": {},\n\
              \"rejections_reward_risk\": {},\n\
              \"rejections_expected_net_edge\": {},\n\
              \"rejections_other\": {}\n\
            }}",
            flow.entry_geometry_mode,
            flow.signals_generated,
            flow.signals_preapproved,
            flow.signals_rejected_initial_risk,
            flow.signals_rejected_actual_entry,
            flow.trades_opened,
            flow.trades_closed,
            flow.risk_rejections,
            flow.rejections_max_drawdown,
            flow.rejections_daily_loss,
            flow.rejections_reward_risk,
            flow.rejections_expected_net_edge,
            flow.rejections_other,
        );
        fs::write(&path, json)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }
}

// ── CSV helpers ───────────────────────────────────────────────────────────────

/// RFC-4180 minimal CSV escaping.
/// Wraps the field in double quotes if it contains comma, quote, or newline.
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::{BacktestSummary, EquityPoint};
    use crate::backtest::risk_trace::{RiskRejection, SignalFlowSummary};
    use crate::core::{
        PositionId, Side, SignalId, StrategyId, Symbol, Trade, TradeExitReason, TradeId,
    };

    fn test_summary() -> BacktestSummary {
        BacktestSummary {
            total_trades: 2,
            win_rate: 50.0,
            net_pnl: 30.0,
            gross_pnl: 45.0,
            total_fee: 10.0,
            total_slippage: 5.0,
            profit_factor: 2.0,
            expectancy: 15.0,
            avg_win: 50.0,
            avg_loss: -20.0,
            max_drawdown: 3.5,
            max_consecutive_losses: 1,
            avg_trade_duration: 600.0,
        }
    }

    fn test_trade() -> Trade {
        Trade {
            trade_id: TradeId::new("TRD-SIG-BT-00000001"),
            signal_id: SignalId::new("SIG-BT-00000001"),
            position_id: PositionId::new("POS-SIG-BT-00000001"),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            regime: "bullish".to_string(),
            side: Side::Long,
            entry_time: 1_700_000_000_000,
            exit_time: 1_700_000_600_000,
            entry_price: 30_000.0,
            exit_price: 30_600.0,
            stop_loss: 29_700.0,
            take_profit: 30_600.0,
            quantity: 0.1,
            gross_pnl: 60.0,
            fee: 5.0,
            slippage: 3.0,
            net_pnl: 52.0,
            reward_risk: 2.0,
            bars_held: 10,
            exit_reason: TradeExitReason::TakeProfit,
            entry_reason: "ema_cross_above_vwap".to_string(),
            filters_passed: vec!["vwap_filter".to_string()],
            filters_failed: vec![],
            expected_edge_bps: 192.0,
            actual_edge_bps: 173.3,
        }
    }

    fn test_equity() -> Vec<EquityPoint> {
        vec![
            EquityPoint {
                timestamp: 1_700_000_000_000,
                equity: 5000.0,
                drawdown_pct: 0.0,
            },
            EquityPoint {
                timestamp: 1_700_000_600_000,
                equity: 5052.0,
                drawdown_pct: 0.0,
            },
        ]
    }

    fn test_rejection() -> RiskRejection {
        RiskRejection {
            signal_id: "SIG-BT-00000001".to_string(),
            stage: "initial_risk".to_string(),
            entry_geometry_mode: "preserve_signal_levels".to_string(),
            timestamp: 1_700_000_000_000,
            side: "long".to_string(),
            regime: "bullish".to_string(),
            reason: "max_drawdown_reached".to_string(),
            equity: 9_500.0,
            peak_equity: 10_000.0,
            drawdown_pct: 5.0,
            daily_realized_pnl: -200.0,
            expected_reward_bps: 200.0,
            expected_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    fn temp_dir(tag: &str) -> String {
        let path = format!("/tmp/northflow_rpt_{}_{}", std::process::id(), tag);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn writes_summary_json() {
        let dir = temp_dir("json");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/backtest_summary.json")).unwrap();
        assert!(
            content.contains("\"total_trades\""),
            "missing field: {content}"
        );
        assert!(content.contains("\"win_rate\""));
        assert!(content.contains("\"net_pnl\""));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_trades_csv() {
        let dir = temp_dir("trades");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[test_trade()],
            &[],
            &[],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/trades.csv")).unwrap();
        assert!(content.contains("TRD-SIG-BT-00000001"), "trade_id missing");
        assert!(content.contains("BTCUSDT"), "symbol missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_equity_curve_csv() {
        let dir = temp_dir("equity");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &test_equity(),
            &[],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/equity_curve.csv")).unwrap();
        assert!(
            content.contains("timestamp,equity,drawdown_pct"),
            "header missing"
        );
        assert!(content.contains("5000"), "equity missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trades_csv_header_contains_required_fields() {
        let dir = temp_dir("header");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/trades.csv")).unwrap();
        let header = content.lines().next().unwrap_or("");
        for field in &[
            "trade_id",
            "signal_id",
            "symbol",
            "strategy_id",
            "regime",
            "side",
            "entry_time",
            "exit_time",
            "entry_price",
            "exit_price",
            "stop_loss",
            "take_profit",
            "qty",
            "gross_pnl",
            "fee",
            "slippage",
            "net_pnl",
            "reward_risk",
            "bars_held",
            "exit_reason",
            "entry_reason",
            "filters_passed",
            "filters_failed",
            "expected_edge_bps",
            "actual_edge_bps",
        ] {
            assert!(
                header.contains(field),
                "header missing field '{field}': {header}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_risk_rejections_csv_with_header() {
        let dir = temp_dir("rrejections");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[test_rejection()],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/risk_rejections.csv")).unwrap();
        assert!(
            content.contains("signal_id,stage,entry_geometry_mode,timestamp,side,regime,reason"),
            "header missing: {content}"
        );
        assert!(content.contains("SIG-BT-00000001"), "signal_id missing");
        assert!(content.contains("initial_risk"), "stage missing");
        assert!(content.contains("max_drawdown_reached"), "reason missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_empty_risk_rejections_csv_with_header() {
        let dir = temp_dir("rrejections_empty");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/risk_rejections.csv")).unwrap();
        assert!(
            content.contains("signal_id,stage,entry_geometry_mode,timestamp,side,regime,reason"),
            "header missing for empty file: {content}"
        );
        // Only header, no data rows
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "empty rejections should produce only header"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_signal_flow_summary_json() {
        let dir = temp_dir("sigflow");
        let mut flow = SignalFlowSummary::default();
        flow.signals_generated = 10;
        flow.signals_preapproved = 8;
        flow.signals_rejected_initial_risk = 2;
        flow.trades_opened = 7;
        flow.trades_closed = 7;
        flow.risk_rejections = 3;
        flow.rejections_max_drawdown = 1;
        flow.rejections_daily_loss = 1;
        flow.rejections_other = 1;

        ReportWriter::write_all(&dir, &test_summary(), &[], &[], &[], &flow).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/signal_flow_summary.json")).unwrap();
        assert!(
            content.contains("\"entry_geometry_mode\""),
            "missing entry_geometry_mode"
        );
        assert!(
            content.contains("\"signals_generated\": 10"),
            "missing signals_generated"
        );
        assert!(
            content.contains("\"signals_preapproved\": 8"),
            "missing signals_preapproved"
        );
        assert!(
            content.contains("\"trades_opened\": 7"),
            "missing trades_opened"
        );
        assert!(
            content.contains("\"risk_rejections\": 3"),
            "missing risk_rejections"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn risk_rejections_csv_escapes_fields_with_commas() {
        let dir = temp_dir("rrejections_escape");
        let r = RiskRejection {
            signal_id: "SIG-BT-00000001".to_string(),
            stage: "initial_risk".to_string(),
            entry_geometry_mode: "preserve_signal_levels".to_string(),
            timestamp: 1_700_000_000_000,
            side: "long".to_string(),
            regime: "bull,ish".to_string(), // contains comma — must be escaped
            reason: "max_drawdown_reached".to_string(),
            equity: 9_500.0,
            peak_equity: 10_000.0,
            drawdown_pct: 5.0,
            daily_realized_pnl: -100.0,
            expected_reward_bps: 200.0,
            expected_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        };
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[r],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/risk_rejections.csv")).unwrap();
        assert!(
            content.contains("\"bull,ish\""),
            "comma in regime must be CSV-escaped: {content}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn csv_escape_handles_commas_and_quotes() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn writes_risk_rejections_csv_with_stage_header() {
        let dir = temp_dir("stage_header");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[test_rejection()],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/risk_rejections.csv")).unwrap();
        let header = content.lines().next().unwrap_or("");
        assert!(
            header.starts_with("signal_id,stage,"),
            "stage must be second column in header: {header}"
        );
        assert!(
            header.contains("timestamp"),
            "header must still have timestamp: {header}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_risk_rejections_csv_stage_value() {
        let dir = temp_dir("stage_value");
        let mut actual_entry_rej = test_rejection();
        actual_entry_rej.stage = "actual_entry".to_string();
        actual_entry_rej.signal_id = "SIG-BT-00000002".to_string();
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[test_rejection(), actual_entry_rej],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/risk_rejections.csv")).unwrap();
        assert!(
            content.contains("initial_risk"),
            "initial_risk stage must appear in rows: {content}"
        );
        assert!(
            content.contains("actual_entry"),
            "actual_entry stage must appear in rows: {content}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_empty_risk_rejections_csv_with_stage_header() {
        let dir = temp_dir("stage_header_empty");
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/risk_rejections.csv")).unwrap();
        let header = content.lines().next().unwrap_or("");
        assert!(
            header.contains("signal_id,stage,entry_geometry_mode,timestamp"),
            "entry_geometry_mode must be in header even when no rejections: {header}"
        );
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "empty rejections should produce only header"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn risk_rejections_csv_escapes_stage() {
        let dir = temp_dir("stage_escape");
        let r = RiskRejection {
            signal_id: "SIG-BT-00000001".to_string(),
            stage: "initial,risk".to_string(), // comma must be escaped
            entry_geometry_mode: "preserve_signal_levels".to_string(),
            timestamp: 1_700_000_000_000,
            side: "long".to_string(),
            regime: "bullish".to_string(),
            reason: "max_drawdown_reached".to_string(),
            equity: 9_500.0,
            peak_equity: 10_000.0,
            drawdown_pct: 5.0,
            daily_realized_pnl: -100.0,
            expected_reward_bps: 200.0,
            expected_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        };
        ReportWriter::write_all(
            &dir,
            &test_summary(),
            &[],
            &[],
            &[r],
            &SignalFlowSummary::default(),
        )
        .unwrap();
        let content = std::fs::read_to_string(format!("{dir}/risk_rejections.csv")).unwrap();
        assert!(
            content.contains("\"initial,risk\""),
            "comma in stage must be CSV-escaped: {content}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
