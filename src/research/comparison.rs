//! Comparison mode runner — aggregates per-strategy run results and writes summary files.
//!
//! Output files:
//!   <base_reports_dir>/comparison_summary.csv
//!   <base_reports_dir>/comparison_summary.json

use std::fs;
use std::path::Path;

use crate::backtest::metrics::BacktestSummary;
use crate::backtest::risk_trace::SignalFlowSummary;
use crate::core::NorthflowError;
use crate::report::TradeDistributionSummary;

// ── ComparisonRunResult ───────────────────────────────────────────────────────

/// Result of one symbol × strategy backtest run in comparison mode.
#[derive(Debug, Clone)]
pub struct ComparisonRunResult {
    pub symbol: String,
    pub strategy_id: String,
    pub reports_dir: String,
    pub status: String,
    pub error: String,
    pub total_trades: usize,
    pub win_rate: f64,
    pub net_pnl: f64,
    pub gross_pnl: f64,
    pub total_fee: f64,
    pub total_slippage: f64,
    pub total_cost: f64,
    pub profit_factor: f64,
    pub expectancy: f64,
    pub max_drawdown: f64,
    pub max_consecutive_losses: usize,
    pub avg_expected_edge_bps: f64,
    pub avg_actual_edge_bps: f64,
    pub avg_edge_realization_bps: f64,
    pub avg_total_cost_bps: f64,
    pub signals_generated: usize,
    pub signals_preapproved: usize,
    pub signals_rejected_initial_risk: usize,
    pub signals_rejected_actual_entry: usize,
    pub trades_opened: usize,
    pub trades_closed: usize,
    pub risk_rejections: usize,
    pub dominant_rejection_reason: String,
    pub dominant_rejection_count: usize,
}

impl ComparisonRunResult {
    /// Build an error result for a run that failed or had no data.
    pub fn error(symbol: &str, strategy_id: &str, reports_dir: &str, error: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            strategy_id: strategy_id.to_string(),
            reports_dir: reports_dir.to_string(),
            status: "error".to_string(),
            error: error.to_string(),
            total_trades: 0,
            win_rate: 0.0,
            net_pnl: 0.0,
            gross_pnl: 0.0,
            total_fee: 0.0,
            total_slippage: 0.0,
            total_cost: 0.0,
            profit_factor: 0.0,
            expectancy: 0.0,
            max_drawdown: 0.0,
            max_consecutive_losses: 0,
            avg_expected_edge_bps: 0.0,
            avg_actual_edge_bps: 0.0,
            avg_edge_realization_bps: 0.0,
            avg_total_cost_bps: 0.0,
            signals_generated: 0,
            signals_preapproved: 0,
            signals_rejected_initial_risk: 0,
            signals_rejected_actual_entry: 0,
            trades_opened: 0,
            trades_closed: 0,
            risk_rejections: 0,
            dominant_rejection_reason: String::new(),
            dominant_rejection_count: 0,
        }
    }

    /// Build an ok result from a completed backtest run.
    pub fn ok(
        symbol: &str,
        strategy_id: &str,
        reports_dir: &str,
        summary: &BacktestSummary,
        _signal_flow: &SignalFlowSummary,
        dist: &TradeDistributionSummary,
    ) -> Self {
        let pf = if summary.profit_factor.is_infinite() || summary.profit_factor.is_nan() {
            0.0
        } else {
            summary.profit_factor
        };
        Self {
            symbol: symbol.to_string(),
            strategy_id: strategy_id.to_string(),
            reports_dir: reports_dir.to_string(),
            status: "ok".to_string(),
            error: String::new(),
            total_trades: summary.total_trades,
            win_rate: summary.win_rate,
            net_pnl: summary.net_pnl,
            gross_pnl: summary.gross_pnl,
            total_fee: summary.total_fee,
            total_slippage: summary.total_slippage,
            total_cost: dist.total_cost,
            profit_factor: pf,
            expectancy: summary.expectancy,
            max_drawdown: summary.max_drawdown,
            max_consecutive_losses: summary.max_consecutive_losses,
            avg_expected_edge_bps: dist.avg_expected_edge_bps,
            avg_actual_edge_bps: dist.avg_actual_edge_bps,
            avg_edge_realization_bps: dist.avg_edge_realization_bps,
            avg_total_cost_bps: dist.avg_total_cost_bps,
            signals_generated: dist.signals_generated,
            signals_preapproved: dist.signals_preapproved,
            signals_rejected_initial_risk: dist.signals_rejected_initial_risk,
            signals_rejected_actual_entry: dist.signals_rejected_actual_entry,
            trades_opened: dist.trades_opened,
            trades_closed: dist.trades_closed,
            risk_rejections: dist.risk_rejections,
            dominant_rejection_reason: dist.dominant_rejection_reason.clone(),
            dominant_rejection_count: dist.dominant_rejection_count,
        }
    }
}

// ── ComparisonSummary ─────────────────────────────────────────────────────────

/// Aggregate of all symbol × strategy runs in comparison mode.
#[derive(Debug, Clone)]
pub struct ComparisonSummary {
    pub runs: Vec<ComparisonRunResult>,
}

// ── ComparisonWriter ──────────────────────────────────────────────────────────

pub struct ComparisonWriter;

impl ComparisonWriter {
    /// Write comparison_summary.csv and comparison_summary.json to `base_reports_dir`.
    pub fn write_all(
        base_reports_dir: &str,
        summary: &ComparisonSummary,
    ) -> Result<(), NorthflowError> {
        let dir = Path::new(base_reports_dir);
        fs::create_dir_all(dir).map_err(|e| {
            NorthflowError::DataError(format!(
                "cannot create comparison dir '{base_reports_dir}': {e}"
            ))
        })?;

        Self::write_csv(dir, summary)?;
        Self::write_json(dir, summary)?;

        Ok(())
    }

    // ── CSV ───────────────────────────────────────────────────────────────────

    pub const CSV_HEADER: &'static str = "symbol,strategy_id,reports_dir,status,error,total_trades,win_rate,net_pnl,gross_pnl,\
         total_fee,total_slippage,total_cost,profit_factor,expectancy,max_drawdown,\
         max_consecutive_losses,avg_expected_edge_bps,avg_actual_edge_bps,\
         avg_edge_realization_bps,avg_total_cost_bps,signals_generated,signals_preapproved,\
         signals_rejected_initial_risk,signals_rejected_actual_entry,trades_opened,\
         trades_closed,risk_rejections,dominant_rejection_reason,dominant_rejection_count";

    fn write_csv(dir: &Path, summary: &ComparisonSummary) -> Result<(), NorthflowError> {
        let mut out = String::new();
        out.push_str(Self::CSV_HEADER);
        out.push('\n');

        for r in &summary.runs {
            let pf_str = if r.profit_factor == 0.0 && r.status == "ok" {
                "0.000000".to_string()
            } else {
                format!("{:.6}", r.profit_factor)
            };
            let row = format!(
                "{},{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{:.6},{:.6},{},{:.6},{:.6},{:.6},{:.6},{},{},{},{},{},{},{},{},{}\n",
                csv_escape(&r.symbol),
                csv_escape(&r.strategy_id),
                csv_escape(&r.reports_dir),
                csv_escape(&r.status),
                csv_escape(&r.error),
                r.total_trades,
                r.win_rate,
                r.net_pnl,
                r.gross_pnl,
                r.total_fee,
                r.total_slippage,
                r.total_cost,
                pf_str,
                r.expectancy,
                r.max_drawdown,
                r.max_consecutive_losses,
                r.avg_expected_edge_bps,
                r.avg_actual_edge_bps,
                r.avg_edge_realization_bps,
                r.avg_total_cost_bps,
                r.signals_generated,
                r.signals_preapproved,
                r.signals_rejected_initial_risk,
                r.signals_rejected_actual_entry,
                r.trades_opened,
                r.trades_closed,
                r.risk_rejections,
                csv_escape(&r.dominant_rejection_reason),
                r.dominant_rejection_count,
            );
            out.push_str(&row);
        }

        let path = dir.join("comparison_summary.csv");
        fs::write(&path, out)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── JSON ──────────────────────────────────────────────────────────────────

    fn write_json(dir: &Path, summary: &ComparisonSummary) -> Result<(), NorthflowError> {
        let mut runs_str = String::new();

        for (i, r) in summary.runs.iter().enumerate() {
            let comma = if i + 1 < summary.runs.len() { "," } else { "" };
            let pf = if r.profit_factor.is_nan() || r.profit_factor.is_infinite() {
                0.0_f64
            } else {
                r.profit_factor
            };
            let run_json = format!(
                concat!(
                    "    {{\n",
                    "      \"symbol\": {symbol},\n",
                    "      \"strategy_id\": {strategy_id},\n",
                    "      \"reports_dir\": {reports_dir},\n",
                    "      \"status\": {status},\n",
                    "      \"error\": {error},\n",
                    "      \"total_trades\": {total_trades},\n",
                    "      \"win_rate\": {win_rate:.6},\n",
                    "      \"net_pnl\": {net_pnl:.6},\n",
                    "      \"gross_pnl\": {gross_pnl:.6},\n",
                    "      \"total_fee\": {total_fee:.6},\n",
                    "      \"total_slippage\": {total_slippage:.6},\n",
                    "      \"total_cost\": {total_cost:.6},\n",
                    "      \"profit_factor\": {profit_factor:.6},\n",
                    "      \"expectancy\": {expectancy:.6},\n",
                    "      \"max_drawdown\": {max_drawdown:.6},\n",
                    "      \"max_consecutive_losses\": {max_consecutive_losses},\n",
                    "      \"avg_expected_edge_bps\": {avg_expected_edge_bps:.6},\n",
                    "      \"avg_actual_edge_bps\": {avg_actual_edge_bps:.6},\n",
                    "      \"avg_edge_realization_bps\": {avg_edge_realization_bps:.6},\n",
                    "      \"avg_total_cost_bps\": {avg_total_cost_bps:.6},\n",
                    "      \"signals_generated\": {signals_generated},\n",
                    "      \"signals_preapproved\": {signals_preapproved},\n",
                    "      \"signals_rejected_initial_risk\": {signals_rejected_initial_risk},\n",
                    "      \"signals_rejected_actual_entry\": {signals_rejected_actual_entry},\n",
                    "      \"trades_opened\": {trades_opened},\n",
                    "      \"trades_closed\": {trades_closed},\n",
                    "      \"risk_rejections\": {risk_rejections},\n",
                    "      \"dominant_rejection_reason\": {dominant_rejection_reason},\n",
                    "      \"dominant_rejection_count\": {dominant_rejection_count}\n",
                    "    }}{comma}\n"
                ),
                symbol = json_str(&r.symbol),
                strategy_id = json_str(&r.strategy_id),
                reports_dir = json_str(&r.reports_dir),
                status = json_str(&r.status),
                error = json_str(&r.error),
                total_trades = r.total_trades,
                win_rate = r.win_rate,
                net_pnl = r.net_pnl,
                gross_pnl = r.gross_pnl,
                total_fee = r.total_fee,
                total_slippage = r.total_slippage,
                total_cost = r.total_cost,
                profit_factor = pf,
                expectancy = r.expectancy,
                max_drawdown = r.max_drawdown,
                max_consecutive_losses = r.max_consecutive_losses,
                avg_expected_edge_bps = r.avg_expected_edge_bps,
                avg_actual_edge_bps = r.avg_actual_edge_bps,
                avg_edge_realization_bps = r.avg_edge_realization_bps,
                avg_total_cost_bps = r.avg_total_cost_bps,
                signals_generated = r.signals_generated,
                signals_preapproved = r.signals_preapproved,
                signals_rejected_initial_risk = r.signals_rejected_initial_risk,
                signals_rejected_actual_entry = r.signals_rejected_actual_entry,
                trades_opened = r.trades_opened,
                trades_closed = r.trades_closed,
                risk_rejections = r.risk_rejections,
                dominant_rejection_reason = json_str(&r.dominant_rejection_reason),
                dominant_rejection_count = r.dominant_rejection_count,
                comma = comma,
            );
            runs_str.push_str(&run_json);
        }

        let runs_block = if summary.runs.is_empty() {
            "  []".to_string()
        } else {
            format!("  [\n{runs_str}  ]")
        };

        let json = format!("{{\n  \"mode\": \"comparison\",\n  \"runs\": {runs_block}\n}}\n");

        let path = dir.join("comparison_summary.json");
        fs::write(&path, json)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }
}

// ── CSV/JSON helpers ──────────────────────────────────────────────────────────

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

fn json_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::BacktestSummary;
    use crate::backtest::risk_trace::SignalFlowSummary;
    use crate::report::TradeDistributionSummary;

    fn empty_dist() -> TradeDistributionSummary {
        TradeDistributionSummary {
            total_trades: 0,
            winning_trades: 0,
            losing_trades: 0,
            win_rate: 0.0,
            gross_pnl: 0.0,
            fee: 0.0,
            slippage: 0.0,
            total_cost: 0.0,
            net_pnl: 0.0,
            avg_expected_edge_bps: 0.0,
            avg_actual_edge_bps: 0.0,
            avg_edge_realization_bps: 0.0,
            avg_total_cost_bps: 0.0,
            cost_to_gross_loss_ratio: 0.0,
            signals_generated: 5,
            signals_preapproved: 3,
            signals_rejected_initial_risk: 2,
            signals_rejected_actual_entry: 0,
            trades_opened: 3,
            trades_closed: 3,
            risk_rejections: 2,
            dominant_rejection_reason: "reward_risk".to_string(),
            dominant_rejection_count: 2,
        }
    }

    fn test_summary() -> BacktestSummary {
        BacktestSummary {
            total_trades: 3,
            win_rate: 66.67,
            net_pnl: 120.0,
            gross_pnl: 150.0,
            total_fee: 20.0,
            total_slippage: 10.0,
            profit_factor: 2.5,
            expectancy: 40.0,
            avg_win: 75.0,
            avg_loss: -30.0,
            max_drawdown: 2.0,
            max_consecutive_losses: 1,
            avg_trade_duration: 300.0,
        }
    }

    fn ok_result(strategy_id: &str) -> ComparisonRunResult {
        ComparisonRunResult::ok(
            "BTCUSDT",
            strategy_id,
            &format!("reports/comparison/{strategy_id}"),
            &test_summary(),
            &SignalFlowSummary::default(),
            &empty_dist(),
        )
    }

    fn temp_dir(tag: &str) -> String {
        let path = format!("/tmp/nf_cmp_{}_{}", std::process::id(), tag);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn comparison_summary_csv_header_is_stable() {
        assert!(
            ComparisonWriter::CSV_HEADER
                .starts_with("symbol,strategy_id,reports_dir,status,error,"),
            "CSV header must start with required fields"
        );
        assert!(
            ComparisonWriter::CSV_HEADER
                .contains("dominant_rejection_reason,dominant_rejection_count"),
            "CSV header must end with rejection fields"
        );
    }

    #[test]
    fn comparison_summary_csv_writes_ok_row() {
        let dir = temp_dir("csv_ok");
        let summary = ComparisonSummary {
            runs: vec![ok_result("screened_vwap_scalp")],
        };
        ComparisonWriter::write_all(&dir, &summary).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/comparison_summary.csv")).unwrap();
        assert!(content.contains("BTCUSDT"), "must contain symbol");
        assert!(
            content.contains("screened_vwap_scalp"),
            "must contain strategy"
        );
        assert!(content.contains(",ok,"), "must contain status=ok");
        assert!(content.contains("3,"), "must contain total_trades=3");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn comparison_summary_csv_writes_error_row() {
        let dir = temp_dir("csv_err");
        let summary = ComparisonSummary {
            runs: vec![ComparisonRunResult::error(
                "ETHUSDT",
                "screened_vwap_scalp_v2",
                "reports/comparison/screened_vwap_scalp_v2",
                "data quality errors",
            )],
        };
        ComparisonWriter::write_all(&dir, &summary).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/comparison_summary.csv")).unwrap();
        assert!(content.contains("ETHUSDT"), "must contain symbol");
        assert!(content.contains(",error,"), "must contain status=error");
        assert!(
            content.contains("data quality errors"),
            "must contain error message"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn comparison_summary_csv_escapes_errors() {
        let dir = temp_dir("csv_escape");
        let summary = ComparisonSummary {
            runs: vec![ComparisonRunResult::error(
                "BTCUSDT",
                "screened_vwap_scalp",
                "reports/comparison/screened_vwap_scalp",
                "error with, comma",
            )],
        };
        ComparisonWriter::write_all(&dir, &summary).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/comparison_summary.csv")).unwrap();
        assert!(
            content.contains("\"error with, comma\""),
            "must CSV-escape error messages containing commas: {content}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn comparison_summary_json_writes_runs() {
        let dir = temp_dir("json_runs");
        let summary = ComparisonSummary {
            runs: vec![
                ok_result("screened_vwap_scalp"),
                ok_result("screened_vwap_scalp_v2"),
            ],
        };
        ComparisonWriter::write_all(&dir, &summary).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/comparison_summary.json")).unwrap();
        assert!(
            content.contains("\"mode\": \"comparison\""),
            "must have mode field"
        );
        assert!(
            content.contains("screened_vwap_scalp"),
            "must contain v1 strategy"
        );
        assert!(
            content.contains("screened_vwap_scalp_v2"),
            "must contain v2 strategy"
        );
        assert!(
            content.contains("\"status\": \"ok\""),
            "must have status ok"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn comparison_summary_json_empty_runs() {
        let dir = temp_dir("json_empty");
        let summary = ComparisonSummary { runs: vec![] };
        ComparisonWriter::write_all(&dir, &summary).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/comparison_summary.json")).unwrap();
        assert!(content.contains("\"mode\": \"comparison\""));
        assert!(
            content.contains("[]"),
            "empty runs must produce empty array"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn comparison_summary_paths_are_relative() {
        let dir = temp_dir("paths_rel");
        let summary = ComparisonSummary {
            runs: vec![ok_result("screened_vwap_scalp")],
        };
        ComparisonWriter::write_all(&dir, &summary).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/comparison_summary.csv")).unwrap();
        let data_line = content.lines().nth(1).unwrap_or("");
        assert!(
            !data_line.starts_with('/'),
            "reports_dir must be relative, got: {data_line}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
