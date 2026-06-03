//! Report manifest — deterministic description of all generated report files.
//!
//! Does not use system time. Does not use random IDs.
//! Uses relative paths (e.g. reports/trades.csv), even when `reports_dir` is
//! configured as an absolute path for local file output.
//! File entries are sorted by path ascending.

use std::fs;
use std::path::Path;

use crate::backtest::metrics::EquityPoint;
use crate::core::{NorthflowError, Trade};
use crate::report::attribution::AttributionReport;
use crate::report::diagnostics::DiagnosticReport;

// ── ReportFileEntry ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReportFileEntry {
    pub path: String,
    pub kind: String,
    pub rows: usize,
}

// ── ReportManifest ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReportManifest {
    pub phase: String,
    pub generated_by: String,
    pub files: Vec<ReportFileEntry>,
}

// ── ManifestWriter ────────────────────────────────────────────────────────────

pub struct ManifestWriter;

impl ManifestWriter {
    /// Build a manifest from the known report outputs.
    ///
    /// File entries are sorted by path ascending for deterministic output.
    /// Row counts reflect the actual data written.
    ///
    /// The manifest is intentionally machine-independent: paths are display-only
    /// relative paths and never absolute paths, even if `reports_dir` is an
    /// absolute path such as `/tmp/reports`.
    pub fn build(
        reports_dir: &str,
        trades: &[Trade],
        equity_curve: &[EquityPoint],
        attribution: &AttributionReport,
        risk_rejections_count: usize,
        diagnostic: &DiagnosticReport,
    ) -> ReportManifest {
        let dir = manifest_display_dir(reports_dir);

        let mut entries: std::collections::BTreeMap<String, (&str, usize)> =
            std::collections::BTreeMap::new();

        entries.insert(
            format!("{dir}/attribution_by_exit_reason.csv"),
            (
                "attribution_by_exit_reason",
                attribution.by_exit_reason.len(),
            ),
        );
        entries.insert(
            format!("{dir}/attribution_by_filter.csv"),
            ("attribution_by_filter", attribution.by_filter.len()),
        );
        entries.insert(
            format!("{dir}/attribution_by_regime.csv"),
            ("attribution_by_regime", attribution.by_regime.len()),
        );
        entries.insert(
            format!("{dir}/attribution_by_side.csv"),
            ("attribution_by_side", attribution.by_side.len()),
        );
        entries.insert(
            format!("{dir}/attribution_summary.json"),
            ("attribution_summary", 1),
        );
        entries.insert(format!("{dir}/audit_report.json"), ("audit", 1));
        entries.insert(format!("{dir}/backtest_summary.json"), ("summary", 1));
        entries.insert(
            format!("{dir}/cost_edge_distribution.csv"),
            ("cost_edge_distribution", diagnostic.cost_edge_distribution.buckets.len()),
        );
        entries.insert(
            format!("{dir}/equity_curve.csv"),
            ("equity_curve", equity_curve.len()),
        );
        entries.insert(
            format!("{dir}/monthly_summary.csv"),
            ("monthly_summary", diagnostic.monthly.len()),
        );
        entries.insert(
            format!("{dir}/rejection_by_stage_reason.csv"),
            (
                "rejection_by_stage_reason",
                diagnostic.rejection_by_stage_reason.len(),
            ),
        );
        entries.insert(format!("{dir}/report_manifest.json"), ("manifest", 1));
        entries.insert(
            format!("{dir}/risk_rejections.csv"),
            ("risk_rejections", risk_rejections_count),
        );
        entries.insert(
            format!("{dir}/signal_diagnostics.csv"),
            ("signal_diagnostics", trades.len()),
        );
        entries.insert(
            format!("{dir}/signal_flow_summary.json"),
            ("signal_flow_summary", 1),
        );
        entries.insert(
            format!("{dir}/trade_distribution_summary.json"),
            ("trade_distribution_summary", 1),
        );
        entries.insert(format!("{dir}/trades.csv"), ("trades", trades.len()));

        let files = entries
            .into_iter()
            .map(|(path, (kind, rows))| ReportFileEntry {
                path,
                kind: kind.to_string(),
                rows,
            })
            .collect();

        ReportManifest {
            phase: "phase_7_reports_and_attribution".to_string(),
            generated_by: "northflow_research".to_string(),
            files,
        }
    }

    /// Write the manifest as `report_manifest.json` to `reports_dir`.
    pub fn write(reports_dir: &str, manifest: &ReportManifest) -> Result<(), NorthflowError> {
        let dir = Path::new(reports_dir);
        fs::create_dir_all(dir).map_err(|e| {
            NorthflowError::DataError(format!("cannot create reports dir '{reports_dir}': {e}"))
        })?;

        let json = Self::to_json(manifest);

        let path = dir.join("report_manifest.json");
        fs::write(&path, json)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    // ── JSON formatting ───────────────────────────────────────────────────────

    fn to_json(manifest: &ReportManifest) -> String {
        let mut file_entries = String::new();
        let n = manifest.files.len();
        for (i, f) in manifest.files.iter().enumerate() {
            let comma = if i + 1 < n { "," } else { "" };
            let path = json_str(&f.path);
            let kind = json_str(&f.kind);
            file_entries.push_str(&format!(
                "    {{\"path\":{path},\"kind\":{kind},\"rows\":{}}}{comma}\n",
                f.rows
            ));
        }

        let phase = json_str(&manifest.phase);
        let generated_by = json_str(&manifest.generated_by);
        let files_block = if manifest.files.is_empty() {
            "  []".to_string()
        } else {
            format!("  [\n{file_entries}  ]")
        };

        format!(
            "{{\n  \"phase\": {phase},\n  \"generated_by\": {generated_by},\n  \"files\": {files_block}\n}}\n"
        )
    }
}

/// Convert the configured reports directory into a deterministic display-only
/// relative path for the manifest.
///
/// The actual writer still uses `reports_dir` exactly as configured. This helper
/// only controls manifest metadata so the file is portable across machines.
fn manifest_display_dir(reports_dir: &str) -> String {
    let trimmed = reports_dir.trim().trim_matches('/');
    if trimmed.is_empty() {
        return "reports".to_string();
    }

    let path = Path::new(trimmed);
    let display = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("reports");

    display.to_string()
}

/// Minimal JSON string escaping.
fn json_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::EquityPoint;
    use crate::report::attribution::AttributionEngine;

    fn empty_equity() -> Vec<EquityPoint> {
        vec![]
    }

    fn make_manifest(reports_dir: &str) -> ReportManifest {
        use crate::backtest::risk_trace::SignalFlowSummary;
        use crate::report::diagnostics::DiagnosticEngine;
        let diag = DiagnosticEngine::build(&[], &[], &SignalFlowSummary::default());
        ManifestWriter::build(
            reports_dir,
            &[],
            &empty_equity(),
            &AttributionEngine::build(&[]),
            0,
            &diag,
        )
    }

    #[test]
    fn manifest_contains_required_files() {
        let m = make_manifest("reports");
        let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();

        for required in &[
            "reports/backtest_summary.json",
            "reports/trades.csv",
            "reports/equity_curve.csv",
            "reports/attribution_summary.json",
            "reports/attribution_by_regime.csv",
            "reports/attribution_by_exit_reason.csv",
            "reports/attribution_by_side.csv",
            "reports/attribution_by_filter.csv",
            "reports/audit_report.json",
            "reports/report_manifest.json",
            "reports/risk_rejections.csv",
            "reports/signal_flow_summary.json",
        ] {
            assert!(
                paths.contains(required),
                "manifest missing required file: {required}"
            );
        }
    }

    #[test]
    fn manifest_uses_relative_paths() {
        let m = make_manifest("reports");
        for f in &m.files {
            assert!(
                !f.path.starts_with('/'),
                "manifest paths must be relative, got: {}",
                f.path
            );
        }
    }

    #[test]
    fn manifest_normalizes_absolute_reports_dir_to_relative_paths() {
        let m = make_manifest("/tmp/northflow/reports");
        for f in &m.files {
            assert!(
                !f.path.starts_with('/'),
                "manifest paths must remain relative even for absolute reports_dir, got: {}",
                f.path
            );
            assert!(
                f.path.starts_with("reports/"),
                "manifest should use the reports directory basename, got: {}",
                f.path
            );
        }
    }

    #[test]
    fn manifest_sorts_files_by_path() {
        let m = make_manifest("reports");
        let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "manifest files must be sorted by path");
    }

    #[test]
    fn manifest_has_stable_phase_name() {
        let m = make_manifest("reports");
        assert_eq!(m.phase, "phase_7_reports_and_attribution");
        assert_eq!(m.generated_by, "northflow_research");
    }

    #[test]
    fn manifest_json_is_valid_structure() {
        let dir = format!("/tmp/nf_manifest_test_{}", std::process::id());
        let m = make_manifest(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        ManifestWriter::write(&dir, &m).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/report_manifest.json")).unwrap();
        assert!(content.contains("\"phase\""));
        assert!(content.contains("phase_7_reports_and_attribution"));
        assert!(content.contains("\"files\""));
        assert!(
            !content.contains(&format!("\"path\":\"{dir}/")),
            "manifest JSON must not include absolute output paths"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
