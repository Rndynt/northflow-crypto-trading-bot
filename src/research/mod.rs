//! Research orchestrator — Phase 7: Reports and Attribution.
//!
//! Runs the deterministic backtest for each configured symbol and writes:
//!   reports/backtest_summary.json
//!   reports/trades.csv
//!   reports/equity_curve.csv
//!   reports/risk_rejections.csv
//!   reports/signal_flow_summary.json
//!   reports/attribution_summary.json
//!   reports/attribution_by_regime.csv
//!   reports/attribution_by_exit_reason.csv
//!   reports/attribution_by_side.csv
//!   reports/attribution_by_filter.csv
//!   reports/audit_report.json
//!   reports/report_manifest.json
//!
//! Paper and live modes remain disabled.

use std::path::Path;

use crate::backtest::{BacktestEngine, ReportWriter};
use crate::config::ResearchConfig;
use crate::core::Timeframe;
use crate::market::{DataQualityIssueKind, OhlcvLoader};
use crate::report::{
    AttributionEngine, AttributionWriter, DiagnosticEngine, DiagnosticWriter, ManifestWriter,
    ReportAuditor,
};

/// Run Phase 7 research: deterministic backtest + full attribution report generation.
///
/// Validates config, loads market data, runs the backtest engine, prints a
/// truthful summary, and writes all report files.  Does not claim the strategy is
/// profitable.  Does not give trading advice.
pub fn run_research(cfg: &ResearchConfig) -> Result<(), String> {
    println!("=================================================================");
    println!(" Northflow — Phase 7: Reports and Attribution");
    println!("=================================================================");
    println!();

    cfg.validate_timeframes().map_err(|e| format!("{e}"))?;

    println!("  Timeframe model:");
    println!(
        "    entry_timeframe        = \"{}\"  (1m  → entry & execution)",
        cfg.entry_timeframe
    );
    println!(
        "    screening_timeframe    = \"{}\" (15m → regime bias)",
        cfg.screening_timeframe
    );
    println!(
        "    confirmation_timeframe = \"{}\"  (5m  → confirmation)",
        cfg.confirmation_timeframe
    );
    println!();
    println!("  Entry geometry mode:");
    println!(
        "    entry_geometry_mode    = \"{}\"",
        cfg.entry_geometry_mode
    );
    println!();
    println!("  paper mode  DISABLED — research engine not yet validated for paper");
    println!("  live mode   DISABLED — paper/live parity not yet proven");
    println!();
    println!("  Note: backtest results are historical simulation only.");
    println!("        Do not use as financial advice or profitability claims.");
    println!();

    for symbol in &cfg.symbols {
        run_symbol(cfg, symbol);
    }

    println!("Indicators ready:");
    println!("  EMA 8 / 21 / 50 / 200");
    println!("  ATR 14 (Wilder smoothing)");
    println!("  VWAP (session-cumulative)");
    println!("  Volume SMA 20");
    println!();
    println!("Strategy engine ready:");
    println!("  screened_vwap_scalp");
    println!("  Output: Signal only");
    println!();
    println!("Risk model ready:");
    println!("  position sizing");
    println!("  cost model");
    println!("  risk guards");
    println!("  Output: RiskAssessment only");
    println!();
    println!("Backtest engine ready:");
    println!("  conservative intrabar fill model");
    println!("  no lookahead across 5m / 15m candles");
    println!("  deterministic signal IDs (SIG-BT-XXXXXXXX)");
    println!();

    Ok(())
}

fn run_symbol(cfg: &ResearchConfig, symbol: &str) {
    let csv_path = Path::new(&cfg.data_dir).join(format!("{symbol}.csv"));

    if !csv_path.exists() {
        println!("Symbol: {symbol}");
        println!("  No historical CSV found.");
        println!("  Expected path: {}", csv_path.display());
        println!("  Place a 1m OHLCV CSV file with columns:");
        println!("    timestamp,open,high,low,close,volume");
        println!();
        return;
    }

    // Print data quality summary.
    let data_quality_ok = print_data_quality(cfg, symbol, &csv_path);
    if !data_quality_ok {
        println!("  Skipping backtest — fix data quality errors first.");
        println!();
        return;
    }

    // Run backtest.
    println!("Running backtest replay...");
    match BacktestEngine::run(cfg, symbol) {
        Err(e) => {
            println!("  Backtest error: {e}");
            println!();
        }
        Ok(None) => {
            println!("  No data returned from backtest.");
            println!();
        }
        Ok(Some(result)) => {
            let s = &result.summary;
            println!("  Backtest complete:");
            println!("    Total trades:           {}", s.total_trades);
            println!("    Win rate:               {:.2}%", s.win_rate);
            println!("    Net PnL:                {:.2}", s.net_pnl);
            println!("    Gross PnL:              {:.2}", s.gross_pnl);
            println!("    Total fees:             {:.2}", s.total_fee);
            println!("    Total slippage:         {:.2}", s.total_slippage);
            let pf_str = if result.summary.profit_factor.is_infinite() {
                "inf".to_string()
            } else {
                format!("{:.4}", result.summary.profit_factor)
            };
            println!("    Profit factor:          {pf_str}");
            println!("    Max drawdown:           {:.2}%", s.max_drawdown);
            println!("    Max consecutive losses: {}", s.max_consecutive_losses);
            println!();

            // ── Signal flow summary ───────────────────────────────────────────
            let flow = &result.signal_flow;
            println!("  Signal flow:");
            println!("    signals generated:          {}", flow.signals_generated);
            println!(
                "    signals preapproved:        {}",
                flow.signals_preapproved
            );
            println!(
                "    rejected initial risk:      {}",
                flow.signals_rejected_initial_risk
            );
            println!(
                "    rejected actual entry:      {}",
                flow.signals_rejected_actual_entry
            );
            println!("    trades opened:              {}", flow.trades_opened);
            println!("    trades closed:              {}", flow.trades_closed);
            println!("    risk rejection rows:        {}", flow.risk_rejections);
            if flow.risk_rejections > 0 {
                println!(
                    "      max_drawdown:             {}",
                    flow.rejections_max_drawdown
                );
                println!(
                    "      daily_loss:               {}",
                    flow.rejections_daily_loss
                );
                println!(
                    "      reward_risk:              {}",
                    flow.rejections_reward_risk
                );
                println!(
                    "      expected_net_edge:        {}",
                    flow.rejections_expected_net_edge
                );
                println!("      other:                    {}", flow.rejections_other);
            }
            println!();

            // ── Phase 6: base report files ────────────────────────────────────
            match ReportWriter::write_all(
                &cfg.reports_dir,
                &result.summary,
                &result.trades,
                &result.equity_curve,
                &result.risk_rejections,
                &result.signal_flow,
            ) {
                Ok(()) => {
                    println!("  Base reports written:");
                    println!("    {}/backtest_summary.json", cfg.reports_dir);
                    println!("    {}/trades.csv", cfg.reports_dir);
                    println!("    {}/equity_curve.csv", cfg.reports_dir);
                    println!("    {}/risk_rejections.csv", cfg.reports_dir);
                    println!("    {}/signal_flow_summary.json", cfg.reports_dir);
                }
                Err(e) => {
                    println!("  Warning: could not write base reports: {e}");
                }
            }
            println!("Base backtest reports written.");
            println!();

            // ── Diagnostics (built before manifest so counts are available) ───
            let diagnostics = DiagnosticEngine::build(
                &result.trades,
                &result.risk_rejections,
                &result.signal_flow,
            );

            // ── Phase 7: attribution, audit, and manifest ─────────────────────
            let attribution = AttributionEngine::build(&result.trades);
            let audit = ReportAuditor::audit_trades(&result.trades);
            let manifest = ManifestWriter::build(
                &cfg.reports_dir,
                &result.trades,
                &result.equity_curve,
                &attribution,
                result.risk_rejections.len(),
                &diagnostics,
            );

            // Audit summary — print before writing so the user sees results
            // even if file I/O fails.
            println!("  Audit report:");
            println!(
                "    passed:   {}",
                if audit.passed { "true" } else { "false" }
            );
            println!("    errors:   {}", audit.error_count);
            println!("    warnings: {}", audit.warning_count);

            if !audit.passed {
                println!(
                    "  Warning: audit found {} error(s) — check audit_report.json",
                    audit.error_count
                );
                for issue in audit
                    .issues
                    .iter()
                    .filter(|i| i.severity == crate::report::AuditSeverity::Error)
                {
                    println!("    [ERROR] {} — {}", issue.code, issue.message);
                }
            }
            println!();

            // Attribution summary.
            let attr_s = &attribution.summary;
            println!("  Attribution summary:");
            println!("    Unique signals:         {}", attr_s.unique_signal_ids);
            println!(
                "    Avg expected edge bps:  {:.2}",
                attr_s.avg_expected_edge_bps
            );
            println!(
                "    Avg actual edge bps:    {:.2}",
                attr_s.avg_actual_edge_bps
            );
            println!(
                "    Edge realization bps:   {:.2}",
                attr_s.edge_realization_bps
            );
            println!();

            // Write Phase 7 files. Do not panic on write failure — warn clearly.
            match AttributionWriter::write_all(&cfg.reports_dir, &attribution, &audit, &manifest) {
                Ok(()) => {
                    println!("  Phase 7 reports written:");
                    println!("    {}/attribution_summary.json", cfg.reports_dir);
                    println!("    {}/attribution_by_regime.csv", cfg.reports_dir);
                    println!("    {}/attribution_by_exit_reason.csv", cfg.reports_dir);
                    println!("    {}/attribution_by_side.csv", cfg.reports_dir);
                    println!("    {}/attribution_by_filter.csv", cfg.reports_dir);
                    println!("    {}/audit_report.json", cfg.reports_dir);
                    println!("    {}/report_manifest.json", cfg.reports_dir);
                }
                Err(e) => {
                    println!("  Warning: could not write Phase 7 reports: {e}");
                }
            }
            println!("Phase 7 attribution reports written.");
            println!();

            // ── Diagnostic report files ───────────────────────────────────────
            let d = &diagnostics.trade_distribution;
            match DiagnosticWriter::write_all_with_trades(
                &cfg.reports_dir,
                &diagnostics,
                &result.trades,
            ) {
                Ok(()) => {
                    println!("  Diagnostic reports written:");
                    println!("    {}/signal_diagnostics.csv", cfg.reports_dir);
                    println!("    {}/rejection_by_stage_reason.csv", cfg.reports_dir);
                    println!("    {}/monthly_summary.csv", cfg.reports_dir);
                    println!("    {}/cost_edge_distribution.csv", cfg.reports_dir);
                    println!("    {}/trade_distribution_summary.json", cfg.reports_dir);
                }
                Err(e) => {
                    println!("  Warning: could not write diagnostic reports: {e}");
                }
            }
            println!("Diagnostics:");
            println!("  avg total cost bps:        {:.2}", d.avg_total_cost_bps);
            println!(
                "  avg edge realization bps:  {:.2}",
                d.avg_edge_realization_bps
            );
            if !d.dominant_rejection_reason.is_empty() {
                println!(
                    "  dominant rejection:        {} ({})",
                    d.dominant_rejection_reason, d.dominant_rejection_count
                );
            }
            println!();
        }
    }
}

/// Print data quality for the symbol.  Returns `true` if no errors.
fn print_data_quality(_cfg: &ResearchConfig, symbol: &str, csv_path: &Path) -> bool {
    use crate::market::CandleStore;

    let load_result = match OhlcvLoader::load_file(csv_path) {
        Ok(r) => r,
        Err(e) => {
            println!("  Error loading {symbol}: {e}");
            return false;
        }
    };

    let quality = &load_result.quality;
    let store = match CandleStore::build_from_1m(load_result.candles) {
        Ok(s) => s,
        Err(e) => {
            println!("  Error building candle store: {e}");
            return false;
        }
    };

    let dup_count = quality
        .issues
        .iter()
        .filter(|i| i.kind == DataQualityIssueKind::DuplicateTimestamp)
        .count();

    println!("Symbol:                {symbol}");
    println!("Source:                {}", csv_path.display());
    println!("1m candles:            {}", store.len(Timeframe::OneMinute));
    println!(
        "5m candles:            {}",
        store.len(Timeframe::FiveMinute)
    );
    println!(
        "15m candles:           {}",
        store.len(Timeframe::FifteenMinute)
    );
    println!("Data quality errors:   {}", quality.error_count());
    println!("Duplicate timestamps:  {dup_count}");
    println!("Missing gaps:          {}", quality.missing_gaps.len());

    if quality.error_count() > 0 {
        println!();
        println!("  Data quality errors:");
        for issue in quality.issues.iter().filter(|i| i.kind.is_error()) {
            match issue.row {
                Some(row) => println!("    [{}] row {row}: {}", issue.kind, issue.message),
                None => println!("    [{}] {}", issue.kind, issue.message),
            }
        }
        return false;
    }

    if !quality.missing_gaps.is_empty() {
        println!();
        println!("  Missing 1m gaps (warnings):");
        for gap in &quality.missing_gaps {
            println!(
                "    {} missing candle(s) after ts={}  (expected ts={})",
                gap.missing_count, gap.from_timestamp, gap.expected_next_timestamp
            );
        }
    }

    println!();
    true
}
