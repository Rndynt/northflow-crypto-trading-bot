//! Phase 7 — Reports and Attribution.
//!
//! This module provides the attribution, audit, manifest, validation, and
//! diagnostics layers that sit on top of the Phase 6 low-level report writer
//! in src/backtest/report.rs.
//!
//! Output files written by this module:
//!   reports/attribution_summary.json
//!   reports/attribution_by_regime.csv
//!   reports/attribution_by_exit_reason.csv
//!   reports/attribution_by_side.csv
//!   reports/attribution_by_filter.csv
//!   reports/audit_report.json
//!   reports/report_manifest.json
//!   reports/signal_diagnostics.csv
//!   reports/rejection_by_stage_reason.csv
//!   reports/monthly_summary.csv
//!   reports/cost_edge_distribution.csv
//!   reports/trade_distribution_summary.json

pub mod attribution;
pub mod audit;
pub mod diagnostics;
pub mod manifest;
pub mod validation;

pub use attribution::{
    AttributionBucket, AttributionEngine, AttributionReport, AttributionSummary,
};
pub use audit::{AuditIssue, AuditReport, AuditSeverity, ReportAuditor};
pub use diagnostics::{
    CostEdgeDistribution, CostEdgeRow, DiagnosticEngine, DiagnosticReport, DiagnosticWriter,
    MonthlySummaryRow, RejectionByStageReasonRow, TradeDistributionSummary,
};
pub use manifest::{ManifestWriter, ReportFileEntry, ReportManifest};
pub use validation::{TradeValidator, ValidationResult};

use std::fs;
use std::path::Path;

use crate::core::NorthflowError;

// ── AttributionWriter ─────────────────────────────────────────────────────────

/// Writes all Phase 7 attribution, audit, and manifest files to `reports_dir`.
pub struct AttributionWriter;

impl AttributionWriter {
    /// Write all Phase 7 output files.
    ///
    /// Files written:
    ///   attribution_summary.json
    ///   attribution_by_regime.csv
    ///   attribution_by_exit_reason.csv
    ///   attribution_by_side.csv
    ///   attribution_by_filter.csv
    ///   audit_report.json
    ///   report_manifest.json
    pub fn write_all(
        reports_dir: &str,
        attribution: &AttributionReport,
        audit: &AuditReport,
        manifest: &ReportManifest,
    ) -> Result<(), NorthflowError> {
        let dir = Path::new(reports_dir);
        fs::create_dir_all(dir).map_err(|e| {
            NorthflowError::DataError(format!("cannot create reports dir '{reports_dir}': {e}"))
        })?;

        Self::write_attribution_summary(dir, &attribution.summary)?;
        Self::write_bucket_csv(dir, "attribution_by_regime.csv", &attribution.by_regime)?;
        Self::write_bucket_csv(
            dir,
            "attribution_by_exit_reason.csv",
            &attribution.by_exit_reason,
        )?;
        Self::write_bucket_csv(dir, "attribution_by_side.csv", &attribution.by_side)?;
        Self::write_bucket_csv(dir, "attribution_by_filter.csv", &attribution.by_filter)?;
        Self::write_bucket_csv(dir, "attribution_by_strategy.csv", &attribution.by_strategy)?;
        Self::write_audit_json(dir, audit)?;
        manifest::ManifestWriter::write(reports_dir, manifest)?;

        Ok(())
    }

    // ── attribution_summary.json ──────────────────────────────────────────────

    fn write_attribution_summary(dir: &Path, s: &AttributionSummary) -> Result<(), NorthflowError> {
        let json = format!(
            concat!(
                "{{\n",
                "  \"total_trades\": {},\n",
                "  \"total_signals_with_trades\": {},\n",
                "  \"unique_signal_ids\": {},\n",
                "  \"unique_trade_ids\": {},\n",
                "  \"avg_expected_edge_bps\": {:.6},\n",
                "  \"avg_actual_edge_bps\": {:.6},\n",
                "  \"edge_realization_bps\": {:.6},\n",
                "  \"positive_expected_edge_trades\": {},\n",
                "  \"positive_actual_edge_trades\": {},\n",
                "  \"filters_passed_count\": {},\n",
                "  \"filters_failed_count\": {}\n",
                "}}\n"
            ),
            s.total_trades,
            s.total_signals_with_trades,
            s.unique_signal_ids,
            s.unique_trade_ids,
            s.avg_expected_edge_bps,
            s.avg_actual_edge_bps,
            s.edge_realization_bps,
            s.positive_expected_edge_trades,
            s.positive_actual_edge_trades,
            s.filters_passed_count,
            s.filters_failed_count,
        );
        let path = dir.join("attribution_summary.json");
        fs::write(&path, json)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── attribution bucket CSV ────────────────────────────────────────────────

    /// Stable CSV header for all attribution bucket files.
    pub const BUCKET_CSV_HEADER: &'static str = "key,trades,wins,losses,win_rate,net_pnl,gross_pnl,total_fee,total_slippage,avg_net_pnl,avg_expected_edge_bps,avg_actual_edge_bps,avg_bars_held";

    fn write_bucket_csv(
        dir: &Path,
        filename: &str,
        buckets: &[AttributionBucket],
    ) -> Result<(), NorthflowError> {
        let mut out = String::new();
        out.push_str(Self::BUCKET_CSV_HEADER);
        out.push('\n');
        for b in buckets {
            out.push_str(&format!(
                "{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}\n",
                csv_escape(&b.key),
                b.trades,
                b.wins,
                b.losses,
                b.win_rate,
                b.net_pnl,
                b.gross_pnl,
                b.total_fee,
                b.total_slippage,
                b.avg_net_pnl,
                b.avg_expected_edge_bps,
                b.avg_actual_edge_bps,
                b.avg_bars_held,
            ));
        }
        let path = dir.join(filename);
        fs::write(&path, out)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── audit_report.json ─────────────────────────────────────────────────────

    fn write_audit_json(dir: &Path, audit: &AuditReport) -> Result<(), NorthflowError> {
        let passed_str = if audit.passed { "true" } else { "false" };

        let mut issues_str = String::new();
        for (i, issue) in audit.issues.iter().enumerate() {
            let comma = if i + 1 < audit.issues.len() { "," } else { "" };
            let sev = json_str(issue.severity.as_str());
            let code = json_str(&issue.code);
            let msg = json_str(&issue.message);
            let tid = match &issue.trade_id {
                Some(s) => json_str(s),
                None => "null".to_string(),
            };
            let sid = match &issue.signal_id {
                Some(s) => json_str(s),
                None => "null".to_string(),
            };
            issues_str.push_str(&format!(
                "    {{\"severity\":{sev},\"code\":{code},\"message\":{msg},\"trade_id\":{tid},\"signal_id\":{sid}}}{comma}\n"
            ));
        }

        let issues_block = if audit.issues.is_empty() {
            "  []".to_string()
        } else {
            format!("  [\n{issues_str}  ]")
        };

        let json = format!(
            "{{\n  \"passed\": {passed_str},\n  \"total_trades\": {},\n  \"error_count\": {},\n  \"warning_count\": {},\n  \"issues\": {issues_block}\n}}\n",
            audit.total_trades, audit.error_count, audit.warning_count
        );

        let path = dir.join("audit_report.json");
        fs::write(&path, json)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }
}

// ── CSV helpers ───────────────────────────────────────────────────────────────

/// Escape a CSV field: quote it when it contains commas, quotes, or newlines.
pub(crate) fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

/// Minimal JSON string escaping.
pub(crate) fn json_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::EquityPoint;
    use crate::backtest::risk_trace::SignalFlowSummary;
    use crate::core::{
        Trade, position::PositionId, side::Side, signal::SignalId, signal::StrategyId,
        symbol::Symbol, trade::TradeExitReason, trade::TradeId,
    };
    use crate::report::attribution::AttributionEngine;
    use crate::report::audit::ReportAuditor;
    use crate::report::diagnostics::DiagnosticEngine;
    use crate::report::manifest::ManifestWriter;

    fn empty_diag() -> crate::report::diagnostics::DiagnosticReport {
        DiagnosticEngine::build(&[], &[], &SignalFlowSummary::default())
    }

    fn valid_trade(n: u32) -> Trade {
        let sid = format!("SIG-BT-{n:08}");
        let tid = format!("TRD-SIG-BT-{n:08}");
        let pid = format!("POS-SIG-BT-{n:08}");
        Trade {
            trade_id: TradeId::new(&tid),
            signal_id: SignalId::new(&sid),
            position_id: PositionId::new(&pid),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            strategy_id: StrategyId::new("screened_vwap_scalp"),
            regime: "bullish".to_string(),
            side: Side::Long,
            entry_time: 1_700_000_000_000 + n as i64 * 60_000,
            exit_time: 1_700_000_060_000 + n as i64 * 60_000,
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
            entry_reason: "ema_cross".to_string(),
            filters_passed: vec!["atr_valid".to_string()],
            filters_failed: vec!["some_failed".to_string()],
            expected_edge_bps: 192.0,
            actual_edge_bps: 173.3,
        }
    }

    fn tmp_dir(tag: &str) -> String {
        format!("/tmp/nf_writer_test_{}_{tag}", std::process::id())
    }

    #[test]
    fn attribution_writer_writes_summary_json() {
        let dir = tmp_dir("summary");
        let trades = vec![valid_trade(1)];
        let attr = AttributionEngine::build(&trades);
        let audit = ReportAuditor::audit_trades(&trades);
        let manifest = ManifestWriter::build(&dir, &trades, &[], &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/attribution_summary.json")).unwrap();
        assert!(content.contains("\"total_trades\": 1"));
        assert!(content.contains("\"unique_signal_ids\": 1"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attribution_writer_writes_regime_csv() {
        let dir = tmp_dir("regime");
        let trades = vec![valid_trade(1)];
        let attr = AttributionEngine::build(&trades);
        let audit = ReportAuditor::audit_trades(&trades);
        let manifest = ManifestWriter::build(&dir, &trades, &[], &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/attribution_by_regime.csv")).unwrap();
        assert!(content.starts_with("key,trades,wins,losses,win_rate,"));
        assert!(content.contains("bullish"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attribution_writer_writes_exit_reason_csv() {
        let dir = tmp_dir("exit_reason");
        let trades = vec![valid_trade(1)];
        let attr = AttributionEngine::build(&trades);
        let audit = ReportAuditor::audit_trades(&trades);
        let manifest = ManifestWriter::build(&dir, &trades, &[], &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        let content =
            std::fs::read_to_string(format!("{dir}/attribution_by_exit_reason.csv")).unwrap();
        assert!(content.contains("take_profit"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attribution_writer_writes_side_csv() {
        let dir = tmp_dir("side");
        let trades = vec![valid_trade(1)];
        let attr = AttributionEngine::build(&trades);
        let audit = ReportAuditor::audit_trades(&trades);
        let manifest = ManifestWriter::build(&dir, &trades, &[], &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/attribution_by_side.csv")).unwrap();
        assert!(content.contains("long"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attribution_writer_writes_filter_csv() {
        let dir = tmp_dir("filter");
        let trades = vec![valid_trade(1)];
        let attr = AttributionEngine::build(&trades);
        let audit = ReportAuditor::audit_trades(&trades);
        let manifest = ManifestWriter::build(&dir, &trades, &[], &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/attribution_by_filter.csv")).unwrap();
        assert!(content.contains("passed:atr_valid"));
        assert!(content.contains("failed:some_failed"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attribution_writer_writes_audit_json() {
        let dir = tmp_dir("audit");
        let trades = vec![valid_trade(1)];
        let attr = AttributionEngine::build(&trades);
        let audit = ReportAuditor::audit_trades(&trades);
        let manifest = ManifestWriter::build(&dir, &trades, &[], &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/audit_report.json")).unwrap();
        assert!(content.contains("\"passed\": true"));
        assert!(content.contains("\"total_trades\": 1"));
        assert!(content.contains("\"error_count\": 0"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attribution_writer_writes_manifest_json() {
        let dir = tmp_dir("manifest");
        let trades = vec![valid_trade(1)];
        let equity: Vec<EquityPoint> = vec![];
        let attr = AttributionEngine::build(&trades);
        let audit = ReportAuditor::audit_trades(&trades);
        let manifest = ManifestWriter::build(&dir, &trades, &equity, &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        let content = std::fs::read_to_string(format!("{dir}/report_manifest.json")).unwrap();
        assert!(content.contains("phase_7_reports_and_attribution"));
        assert!(content.contains("northflow_research"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn attribution_csv_headers_are_stable() {
        assert_eq!(
            AttributionWriter::BUCKET_CSV_HEADER,
            "key,trades,wins,losses,win_rate,net_pnl,gross_pnl,total_fee,total_slippage,avg_net_pnl,avg_expected_edge_bps,avg_actual_edge_bps,avg_bars_held"
        );
    }

    #[test]
    fn attribution_csv_escape_handles_commas_and_quotes() {
        assert_eq!(csv_escape("hello,world"), "\"hello,world\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("no_special"), "no_special");
    }

    #[test]
    fn attribution_writer_empty_trades_writes_headers_only() {
        let dir = tmp_dir("empty");
        let attr = AttributionEngine::build(&[]);
        let audit = ReportAuditor::audit_trades(&[]);
        let manifest = ManifestWriter::build(&dir, &[], &[], &attr, 0, &empty_diag());
        AttributionWriter::write_all(&dir, &attr, &audit, &manifest).unwrap();

        for filename in &[
            "attribution_by_regime.csv",
            "attribution_by_exit_reason.csv",
            "attribution_by_side.csv",
            "attribution_by_filter.csv",
            "attribution_by_strategy.csv",
        ] {
            let content = std::fs::read_to_string(format!("{dir}/{filename}")).unwrap();
            let lines: Vec<&str> = content.lines().collect();
            assert_eq!(
                lines.len(),
                1,
                "{filename} must have header line only when no trades; got {lines:?}"
            );
            assert!(
                lines[0].starts_with("key,trades,"),
                "{filename} header mismatch"
            );
        }

        let summary = std::fs::read_to_string(format!("{dir}/attribution_summary.json")).unwrap();
        assert!(summary.contains("\"total_trades\": 0"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
