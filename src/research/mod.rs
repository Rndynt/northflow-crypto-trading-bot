//! Research orchestrator — Phase 7 + Strategy Comparison Runner.
//!
//! Supports three strategy run modes:
//!   single     — run one strategy, write reports to reports_dir
//!   comparison — run multiple strategies independently, each in its own subfolder
//!   multi      — reserved for future portfolio mode; currently returns ConfigError
//!
//! Single mode preserves current behavior exactly.
//!
//! Paper and live modes remain disabled.

pub mod comparison;

use std::path::Path;

use crate::backtest::{BacktestEngine, ReportWriter};
use crate::config::ResearchConfig;
use crate::core::Timeframe;
use crate::market::{DataQualityIssueKind, OhlcvLoader};
use crate::report::{
    AttributionEngine, AttributionSummary, AttributionWriter, AuditSeverity, DiagnosticEngine,
    DiagnosticWriter, ManifestWriter, ReportAuditor, TradeDistributionSummary,
};

use comparison::{ComparisonRunResult, ComparisonSummary, ComparisonWriter};

// ── CompletedResearchRun ──────────────────────────────────────────────────────

/// Data returned from a completed single symbol × strategy backtest run.
///
/// Used by single mode for verbose printing and by comparison mode for
/// building the aggregate summary.
struct CompletedResearchRun {
    // Core metrics
    total_trades: usize,
    win_rate: f64,
    net_pnl: f64,
    gross_pnl: f64,
    total_fee: f64,
    total_slippage: f64,
    profit_factor: f64,
    expectancy: f64,
    max_drawdown: f64,
    max_consecutive_losses: usize,
    // Signal flow
    signals_generated: usize,
    signals_preapproved: usize,
    signals_rejected_initial_risk: usize,
    signals_rejected_actual_entry: usize,
    trades_opened: usize,
    trades_closed: usize,
    risk_rejections: usize,
    rejections_max_drawdown: usize,
    rejections_daily_loss: usize,
    rejections_reward_risk: usize,
    rejections_expected_net_edge: usize,
    rejections_other: usize,
    // Diagnostics
    trade_distribution: TradeDistributionSummary,
    // Attribution
    attr_unique_signal_ids: usize,
    attr_avg_expected_edge_bps: f64,
    attr_avg_actual_edge_bps: f64,
    attr_edge_realization_bps: f64,
    // Audit
    audit_passed: bool,
    audit_error_count: usize,
    audit_warning_count: usize,
    audit_errors: Vec<(String, String)>,
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run Phase 7 research with strategy comparison runner support.
///
/// Validates config, dispatches by strategy_run_mode, and runs the appropriate
/// single-strategy or comparison backtest.
pub fn run_research(cfg: &ResearchConfig) -> Result<(), String> {
    println!("=================================================================");
    println!(" Northflow — Phase 7: Reports and Attribution");
    println!("=================================================================");
    println!();

    cfg.validate_timeframes().map_err(|e| format!("{e}"))?;
    cfg.validate_strategy_config().map_err(|e| format!("{e}"))?;
    cfg.validate_strategy_runner_config()
        .map_err(|e| format!("{e}"))?;

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

    match cfg.strategy_run_mode.as_str() {
        "single" => run_single_strategy(cfg),
        "comparison" => run_strategy_comparison(cfg),
        "multi" => Err("multi-strategy portfolio backtest is not implemented yet; \
             use strategy_run_mode = \"comparison\""
            .to_string()),
        other => Err(format!(
            "unknown strategy_run_mode: '{other}'. \
             Valid values: 'single', 'comparison', 'multi'"
        )),
    }
}

// ── Single mode ───────────────────────────────────────────────────────────────

fn run_single_strategy(cfg: &ResearchConfig) -> Result<(), String> {
    let strategies = cfg.selected_strategies().map_err(|e| format!("{e}"))?;
    let strategy_id = strategies
        .into_iter()
        .next()
        .unwrap_or_else(|| cfg.strategy_id.clone());
    let run_cfg = cfg.with_strategy_for_run(&strategy_id, cfg.reports_dir.clone());

    println!("Backtest run mode: single");
    println!("Strategy:");
    println!("  strategy_id = {strategy_id}");
    if strategy_id == "screened_vwap_scalp_v2" {
        let v2 = run_cfg.v2_config();
        println!(
            "    v2_require_strict_confirmation  = {}",
            v2.require_strict_confirmation
        );
        println!(
            "    v2_require_ema_ribbon_alignment = {}",
            v2.require_ema_ribbon_alignment
        );
        println!(
            "    v2_allow_neutral_confirmation   = {}",
            v2.allow_neutral_confirmation
        );
        println!(
            "    v2_min_expected_reward_bps      = {:.1}",
            v2.min_expected_reward_bps
        );
        println!(
            "    v2_min_expected_net_edge_bps    = {:.1}",
            v2.min_expected_net_edge_bps
        );
        println!(
            "    v2_min_atr_bps                  = {:.1}",
            v2.min_atr_bps
        );
        println!(
            "    v2_max_atr_bps                  = {:.1}",
            v2.max_atr_bps
        );
        println!(
            "    v2_tp_atr_multiple              = {:.2}",
            v2.tp_atr_multiple
        );
        println!(
            "    v2_sl_atr_multiple              = {:.2}",
            v2.sl_atr_multiple
        );
        println!(
            "    v2_min_volume_ratio             = {:.2}",
            v2.min_volume_ratio
        );
        println!(
            "    v2_vwap_distance_atr_min        = {:.2}",
            v2.vwap_distance_atr_min
        );
        println!(
            "    v2_vwap_distance_atr_max        = {:.2}",
            v2.vwap_distance_atr_max
        );
        println!("    v2_cooldown_bars                = {}", v2.cooldown_bars);
        println!("    v2_enable_long                  = {}", v2.enable_long);
        println!("    v2_enable_short                 = {}", v2.enable_short);
    }
    println!("Reports dir: {}", run_cfg.reports_dir);
    println!();

    for symbol in &cfg.symbols {
        run_symbol_verbose(&run_cfg, symbol);
    }

    println!("Indicators ready:");
    println!("  EMA 8 / 21 / 50 / 200");
    println!("  ATR 14 (Wilder smoothing)");
    println!("  VWAP (session-cumulative)");
    println!("  Volume SMA 20");
    println!();
    println!("Strategy engine ready:");
    println!("  active: {strategy_id}");
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

// ── Comparison mode ───────────────────────────────────────────────────────────

fn run_strategy_comparison(cfg: &ResearchConfig) -> Result<(), String> {
    let base_reports_dir = cfg.reports_dir.clone();
    let strategies = cfg.selected_strategies().map_err(|e| format!("{e}"))?;

    println!("Backtest run mode: comparison");
    println!("Strategies:");
    for s in &strategies {
        println!("  - {s}");
    }
    println!("Base reports dir: {base_reports_dir}");
    println!();

    let mut runs: Vec<ComparisonRunResult> = Vec::new();

    for symbol in &cfg.symbols {
        for strategy_id in &strategies {
            let reports_dir = if cfg.symbols.len() == 1 {
                format!("{base_reports_dir}/{strategy_id}")
            } else {
                format!("{base_reports_dir}/{symbol}/{strategy_id}")
            };

            println!("Running comparison strategy:");
            println!("  symbol: {symbol}");
            println!("  strategy_id: {strategy_id}");
            println!("  reports_dir: {reports_dir}");

            let run_cfg = cfg.with_strategy_for_run(strategy_id, reports_dir.clone());

            match run_symbol_strategy(&run_cfg, symbol) {
                Err(e) => {
                    println!("  Error: {e}");
                    println!();
                    runs.push(ComparisonRunResult::error(
                        symbol,
                        strategy_id,
                        &reports_dir,
                        &e,
                    ));
                }
                Ok(None) => {
                    let msg = format!("no CSV found at {}/{symbol}.csv", cfg.data_dir);
                    println!("  {msg}");
                    println!();
                    runs.push(ComparisonRunResult::error(
                        symbol,
                        strategy_id,
                        &reports_dir,
                        &msg,
                    ));
                }
                Ok(Some(run)) => {
                    println!(
                        "  Done: {} trades, net_pnl {:.2}",
                        run.total_trades, run.net_pnl
                    );
                    println!();
                    let dist = &run.trade_distribution;
                    let summary_stub = crate::backtest::metrics::BacktestSummary {
                        total_trades: run.total_trades,
                        win_rate: run.win_rate,
                        net_pnl: run.net_pnl,
                        gross_pnl: run.gross_pnl,
                        total_fee: run.total_fee,
                        total_slippage: run.total_slippage,
                        profit_factor: run.profit_factor,
                        expectancy: run.expectancy,
                        avg_win: 0.0,
                        avg_loss: 0.0,
                        max_drawdown: run.max_drawdown,
                        max_consecutive_losses: run.max_consecutive_losses,
                        avg_trade_duration: 0.0,
                    };
                    let flow_stub = crate::backtest::risk_trace::SignalFlowSummary::default();
                    runs.push(ComparisonRunResult::ok(
                        symbol,
                        strategy_id,
                        &reports_dir,
                        &summary_stub,
                        &flow_stub,
                        dist,
                    ));
                }
            }
        }
    }

    let summary = ComparisonSummary { runs };

    match ComparisonWriter::write_all(&base_reports_dir, &summary) {
        Ok(()) => {
            println!("Comparison summary written:");
            println!("  {base_reports_dir}/comparison_summary.csv");
            println!("  {base_reports_dir}/comparison_summary.json");
        }
        Err(e) => {
            println!("Warning: could not write comparison summary: {e}");
        }
    }
    println!();

    Ok(())
}

// ── Core symbol × strategy runner (shared by both modes) ─────────────────────

/// Run a single symbol × strategy backtest, write all report files, and return
/// the completed run data.
///
/// Returns `Ok(None)` when the historical CSV does not exist.
/// Returns `Err` on data quality errors or backtest failures.
fn run_symbol_strategy(
    cfg: &ResearchConfig,
    symbol: &str,
) -> Result<Option<CompletedResearchRun>, String> {
    let csv_path = Path::new(&cfg.data_dir).join(format!("{symbol}.csv"));

    if !csv_path.exists() {
        return Ok(None);
    }

    let result = BacktestEngine::run(cfg, symbol).map_err(|e| format!("{e}"))?;

    let result = match result {
        None => return Ok(None),
        Some(r) => r,
    };

    // Build Phase 7 report data.
    let diagnostics =
        DiagnosticEngine::build(&result.trades, &result.risk_rejections, &result.signal_flow);
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

    // Write Phase 6 base reports.
    ReportWriter::write_all(
        &cfg.reports_dir,
        &result.summary,
        &result.trades,
        &result.equity_curve,
        &result.risk_rejections,
        &result.signal_flow,
    )
    .map_err(|e| format!("{e}"))?;

    // Write Phase 7 attribution, audit, manifest.
    AttributionWriter::write_all(&cfg.reports_dir, &attribution, &audit, &manifest)
        .map_err(|e| format!("{e}"))?;

    // Write diagnostic reports.
    DiagnosticWriter::write_all_with_trades(&cfg.reports_dir, &diagnostics, &result.trades)
        .map_err(|e| format!("{e}"))?;

    let attr_s: &AttributionSummary = &attribution.summary;
    let dist = diagnostics.trade_distribution;
    let s = &result.summary;
    let flow = &result.signal_flow;

    let audit_errors: Vec<(String, String)> = audit
        .issues
        .iter()
        .filter(|i| i.severity == AuditSeverity::Error)
        .map(|i| (i.code.clone(), i.message.clone()))
        .collect();

    Ok(Some(CompletedResearchRun {
        total_trades: s.total_trades,
        win_rate: s.win_rate,
        net_pnl: s.net_pnl,
        gross_pnl: s.gross_pnl,
        total_fee: s.total_fee,
        total_slippage: s.total_slippage,
        profit_factor: s.profit_factor,
        expectancy: s.expectancy,
        max_drawdown: s.max_drawdown,
        max_consecutive_losses: s.max_consecutive_losses,
        signals_generated: flow.signals_generated,
        signals_preapproved: flow.signals_preapproved,
        signals_rejected_initial_risk: flow.signals_rejected_initial_risk,
        signals_rejected_actual_entry: flow.signals_rejected_actual_entry,
        trades_opened: flow.trades_opened,
        trades_closed: flow.trades_closed,
        risk_rejections: flow.risk_rejections,
        rejections_max_drawdown: flow.rejections_max_drawdown,
        rejections_daily_loss: flow.rejections_daily_loss,
        rejections_reward_risk: flow.rejections_reward_risk,
        rejections_expected_net_edge: flow.rejections_expected_net_edge,
        rejections_other: flow.rejections_other,
        trade_distribution: dist,
        attr_unique_signal_ids: attr_s.unique_signal_ids,
        attr_avg_expected_edge_bps: attr_s.avg_expected_edge_bps,
        attr_avg_actual_edge_bps: attr_s.avg_actual_edge_bps,
        attr_edge_realization_bps: attr_s.edge_realization_bps,
        audit_passed: audit.passed,
        audit_error_count: audit.error_count,
        audit_warning_count: audit.warning_count,
        audit_errors,
    }))
}

// ── Verbose single-mode symbol runner ─────────────────────────────────────────

/// Run a symbol in single mode with full verbose CLI output.
/// Preserves existing single-mode behavior exactly.
fn run_symbol_verbose(cfg: &ResearchConfig, symbol: &str) {
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

    let data_quality_ok = print_data_quality(cfg, symbol, &csv_path);
    if !data_quality_ok {
        println!("  Skipping backtest — fix data quality errors first.");
        println!();
        return;
    }

    println!("Running backtest replay...");
    match run_symbol_strategy(cfg, symbol) {
        Err(e) => {
            println!("  Backtest error: {e}");
            println!();
        }
        Ok(None) => {
            println!("  No data returned from backtest.");
            println!();
        }
        Ok(Some(run)) => {
            let pf_str = if run.profit_factor.is_infinite() {
                "inf".to_string()
            } else {
                format!("{:.4}", run.profit_factor)
            };
            println!("  Backtest complete:");
            println!("    Total trades:           {}", run.total_trades);
            println!("    Win rate:               {:.2}%", run.win_rate);
            println!("    Net PnL:                {:.2}", run.net_pnl);
            println!("    Gross PnL:              {:.2}", run.gross_pnl);
            println!("    Total fees:             {:.2}", run.total_fee);
            println!("    Total slippage:         {:.2}", run.total_slippage);
            println!("    Profit factor:          {pf_str}");
            println!("    Max drawdown:           {:.2}%", run.max_drawdown);
            println!("    Max consecutive losses: {}", run.max_consecutive_losses);
            println!();

            println!("  Signal flow:");
            println!("    signals generated:          {}", run.signals_generated);
            println!(
                "    signals preapproved:        {}",
                run.signals_preapproved
            );
            println!(
                "    rejected initial risk:      {}",
                run.signals_rejected_initial_risk
            );
            println!(
                "    rejected actual entry:      {}",
                run.signals_rejected_actual_entry
            );
            println!("    trades opened:              {}", run.trades_opened);
            println!("    trades closed:              {}", run.trades_closed);
            println!("    risk rejection rows:        {}", run.risk_rejections);
            if run.risk_rejections > 0 {
                println!(
                    "      max_drawdown:             {}",
                    run.rejections_max_drawdown
                );
                println!(
                    "      daily_loss:               {}",
                    run.rejections_daily_loss
                );
                println!(
                    "      reward_risk:              {}",
                    run.rejections_reward_risk
                );
                println!(
                    "      expected_net_edge:        {}",
                    run.rejections_expected_net_edge
                );
                println!("      other:                    {}", run.rejections_other);
            }
            println!();

            println!("  Base reports written:");
            println!("    {}/backtest_summary.json", cfg.reports_dir);
            println!("    {}/trades.csv", cfg.reports_dir);
            println!("    {}/equity_curve.csv", cfg.reports_dir);
            println!("    {}/risk_rejections.csv", cfg.reports_dir);
            println!("    {}/signal_flow_summary.json", cfg.reports_dir);
            println!("Base backtest reports written.");
            println!();

            println!("  Audit report:");
            println!(
                "    passed:   {}",
                if run.audit_passed { "true" } else { "false" }
            );
            println!("    errors:   {}", run.audit_error_count);
            println!("    warnings: {}", run.audit_warning_count);

            if !run.audit_passed {
                println!(
                    "  Warning: audit found {} error(s) — check audit_report.json",
                    run.audit_error_count
                );
                for (code, msg) in &run.audit_errors {
                    println!("    [ERROR] {code} — {msg}");
                }
            }
            println!();

            println!("  Attribution summary:");
            println!("    Unique signals:         {}", run.attr_unique_signal_ids);
            println!(
                "    Avg expected edge bps:  {:.2}",
                run.attr_avg_expected_edge_bps
            );
            println!(
                "    Avg actual edge bps:    {:.2}",
                run.attr_avg_actual_edge_bps
            );
            println!(
                "    Edge realization bps:   {:.2}",
                run.attr_edge_realization_bps
            );
            println!();

            println!("  Phase 7 reports written:");
            println!("    {}/attribution_summary.json", cfg.reports_dir);
            println!("    {}/attribution_by_regime.csv", cfg.reports_dir);
            println!("    {}/attribution_by_exit_reason.csv", cfg.reports_dir);
            println!("    {}/attribution_by_side.csv", cfg.reports_dir);
            println!("    {}/attribution_by_filter.csv", cfg.reports_dir);
            println!("    {}/attribution_by_strategy.csv", cfg.reports_dir);
            println!("    {}/audit_report.json", cfg.reports_dir);
            println!("    {}/report_manifest.json", cfg.reports_dir);
            println!("Phase 7 attribution reports written.");
            println!();

            let d = &run.trade_distribution;
            println!("  Diagnostic reports written:");
            println!("    {}/signal_diagnostics.csv", cfg.reports_dir);
            println!("    {}/rejection_by_stage_reason.csv", cfg.reports_dir);
            println!("    {}/monthly_summary.csv", cfg.reports_dir);
            println!("    {}/cost_edge_distribution.csv", cfg.reports_dir);
            println!("    {}/trade_distribution_summary.json", cfg.reports_dir);
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

// ── Data quality printer ──────────────────────────────────────────────────────

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
