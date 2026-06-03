//! Strategy Diagnostic Reports — post Phase 7 analytics patch.
//!
//! Builds and writes five diagnostic files:
//!   signal_diagnostics.csv
//!   rejection_by_stage_reason.csv
//!   monthly_summary.csv
//!   cost_edge_distribution.csv
//!   trade_distribution_summary.json
//!
//! All calculations are deterministic. No external dependencies.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::backtest::risk_trace::{RiskRejection, SignalFlowSummary};
use crate::core::{NorthflowError, Trade};
use crate::core::trade::TradeExitReason;
use crate::report::{csv_escape, json_str};

// ── Month key helper ──────────────────────────────────────────────────────────

/// Convert a Unix millisecond timestamp to `"YYYY-MM"` (UTC, no timezone dep).
///
/// Uses Howard Hinnant's civil-from-days algorithm.
/// `div_euclid` gives floor division for both positive and negative timestamps.
pub(crate) fn month_key_from_ms(timestamp_ms: i64) -> String {
    let days = timestamp_ms.div_euclid(86_400_000_i64);

    // Hinnant civil_from_days
    let z: i64 = days + 719_468;
    let era: i64 = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe: i64 = z - era * 146_097;
    let yoe: i64 = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y: i64 = yoe + era * 400;
    let doy: i64 = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp: i64 = (5 * doy + 2) / 153;
    let m: i64 = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };

    format!("{year:04}-{m:02}")
}

// ── Supporting computation helpers ───────────────────────────────────────────

#[inline]
fn entry_notional(t: &Trade) -> f64 {
    t.entry_price * t.quantity
}

#[inline]
fn bps_of(value: f64, notional: f64) -> f64 {
    if notional > 0.0 { value / notional * 10_000.0 } else { 0.0 }
}

// ── MonthlySummaryRow ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MonthlySummaryRow {
    pub month: String,
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub gross_pnl: f64,
    pub fee: f64,
    pub slippage: f64,
    pub total_cost: f64,
    pub net_pnl: f64,
    pub profit_factor: f64,
    pub profit_factor_inf: bool,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub expectancy: f64,
    pub max_consecutive_losses: usize,
    pub avg_reward_risk: f64,
    pub avg_expected_edge_bps: f64,
    pub avg_actual_edge_bps: f64,
    pub avg_edge_realization_bps: f64,
    pub avg_total_cost_bps: f64,
    pub take_profit_count: usize,
    pub stop_loss_count: usize,
    pub time_exit_count: usize,
    pub end_of_backtest_count: usize,
}

// ── RejectionByStageReasonRow ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RejectionByStageReasonRow {
    pub stage: String,
    pub entry_geometry_mode: String,
    pub reason: String,
    pub count: usize,
    pub unique_signals: usize,
    pub avg_equity: f64,
    pub avg_drawdown_pct: f64,
    pub avg_daily_realized_pnl: f64,
    pub avg_expected_reward_bps: f64,
    pub avg_expected_cost_bps: f64,
    pub avg_expected_net_edge_bps: f64,
    pub min_expected_net_edge_bps: f64,
    pub max_expected_net_edge_bps: f64,
}

// ── CostEdgeRow ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CostEdgeRow {
    pub bucket: String,
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub avg_expected_edge_bps: f64,
    pub avg_actual_edge_bps: f64,
    pub avg_edge_realization_bps: f64,
    pub avg_total_cost_bps: f64,
    pub avg_net_pnl_bps: f64,
    pub net_pnl: f64,
}

// ── CostEdgeDistribution ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CostEdgeDistribution {
    pub buckets: Vec<CostEdgeRow>,
}

// ── TradeDistributionSummary ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TradeDistributionSummary {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub gross_pnl: f64,
    pub fee: f64,
    pub slippage: f64,
    pub total_cost: f64,
    pub net_pnl: f64,
    pub avg_expected_edge_bps: f64,
    pub avg_actual_edge_bps: f64,
    pub avg_edge_realization_bps: f64,
    pub avg_total_cost_bps: f64,
    pub cost_to_gross_loss_ratio: f64,
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

// ── DiagnosticReport ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    pub monthly: Vec<MonthlySummaryRow>,
    pub rejection_by_stage_reason: Vec<RejectionByStageReasonRow>,
    pub cost_edge_distribution: CostEdgeDistribution,
    pub trade_distribution: TradeDistributionSummary,
}

// ── DiagnosticEngine ─────────────────────────────────────────────────────────

pub struct DiagnosticEngine;

impl DiagnosticEngine {
    /// Build the full diagnostic report from backtest outputs.
    pub fn build(
        trades: &[Trade],
        risk_rejections: &[RiskRejection],
        signal_flow: &SignalFlowSummary,
    ) -> DiagnosticReport {
        let monthly = Self::build_monthly(trades);
        let rejection_by_stage_reason = Self::build_rejection_groups(risk_rejections);
        let cost_edge_distribution = Self::build_cost_edge(trades);
        let trade_distribution =
            Self::build_trade_distribution(trades, risk_rejections, signal_flow);

        DiagnosticReport {
            monthly,
            rejection_by_stage_reason,
            cost_edge_distribution,
            trade_distribution,
        }
    }

    // ── monthly summary ───────────────────────────────────────────────────────

    fn build_monthly(trades: &[Trade]) -> Vec<MonthlySummaryRow> {
        // Group trade indices by month key (BTreeMap keeps sorted order)
        let mut month_indices: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, t) in trades.iter().enumerate() {
            let m = month_key_from_ms(t.entry_time);
            month_indices.entry(m).or_default().push(i);
        }

        let mut rows = Vec::with_capacity(month_indices.len());

        for (month, indices) in &month_indices {
            let count = indices.len();
            let mut wins = 0usize;
            let mut gross_pnl = 0.0_f64;
            let mut fee = 0.0_f64;
            let mut slippage = 0.0_f64;
            let mut net_pnl_sum = 0.0_f64;
            let mut win_net_pnl = 0.0_f64;
            let mut loss_net_pnl = 0.0_f64;
            let mut gross_winners = 0.0_f64;
            let mut gross_losers = 0.0_f64;
            let mut reward_risk_sum = 0.0_f64;
            let mut exp_edge_sum = 0.0_f64;
            let mut act_edge_sum = 0.0_f64;
            let mut total_cost_bps_sum = 0.0_f64;
            let mut tp_count = 0usize;
            let mut sl_count = 0usize;
            let mut te_count = 0usize;
            let mut eob_count = 0usize;

            for &i in indices {
                let t = &trades[i];
                if t.net_pnl > 0.0 {
                    wins += 1;
                    win_net_pnl += t.net_pnl;
                    gross_winners += t.gross_pnl;
                } else {
                    loss_net_pnl += t.net_pnl;
                    gross_losers += t.gross_pnl;
                }
                gross_pnl += t.gross_pnl;
                fee += t.fee;
                slippage += t.slippage;
                net_pnl_sum += t.net_pnl;
                reward_risk_sum += t.reward_risk;
                exp_edge_sum += t.expected_edge_bps;
                act_edge_sum += t.actual_edge_bps;
                let n = entry_notional(t);
                total_cost_bps_sum += bps_of(t.fee + t.slippage, n);
                match t.exit_reason {
                    TradeExitReason::TakeProfit | TradeExitReason::PartialTakeProfit => {
                        tp_count += 1
                    }
                    TradeExitReason::StopLoss => sl_count += 1,
                    TradeExitReason::TimeExit => te_count += 1,
                    TradeExitReason::EndOfBacktest => eob_count += 1,
                    _ => {}
                }
            }

            let losses = count - wins;
            let n = count as f64;
            let win_rate = if count > 0 { wins as f64 / n * 100.0 } else { 0.0 };

            let (profit_factor, profit_factor_inf) =
                if gross_losers.abs() < 1e-12 && gross_winners > 0.0 {
                    (f64::INFINITY, true)
                } else if gross_losers.abs() < 1e-12 {
                    (0.0, false)
                } else {
                    (gross_winners / gross_losers.abs(), false)
                };

            let avg_win = if wins > 0 { win_net_pnl / wins as f64 } else { 0.0 };
            let avg_loss = if losses > 0 {
                loss_net_pnl / losses as f64
            } else {
                0.0
            };
            let expectancy = if count > 0 { net_pnl_sum / n } else { 0.0 };

            // Max consecutive losses within this month (ordered by original trade order)
            let max_consec_losses = {
                let mut max = 0usize;
                let mut cur = 0usize;
                for &i in indices {
                    if trades[i].net_pnl <= 0.0 {
                        cur += 1;
                        if cur > max {
                            max = cur;
                        }
                    } else {
                        cur = 0;
                    }
                }
                max
            };

            let avg_rr = if count > 0 { reward_risk_sum / n } else { 0.0 };
            let avg_exp = if count > 0 { exp_edge_sum / n } else { 0.0 };
            let avg_act = if count > 0 { act_edge_sum / n } else { 0.0 };
            let avg_edge_real = avg_act - avg_exp;
            let avg_cost_bps = if count > 0 { total_cost_bps_sum / n } else { 0.0 };

            rows.push(MonthlySummaryRow {
                month: month.clone(),
                trades: count,
                wins,
                losses,
                win_rate,
                gross_pnl,
                fee,
                slippage,
                total_cost: fee + slippage,
                net_pnl: net_pnl_sum,
                profit_factor,
                profit_factor_inf,
                avg_win,
                avg_loss,
                expectancy,
                max_consecutive_losses: max_consec_losses,
                avg_reward_risk: avg_rr,
                avg_expected_edge_bps: avg_exp,
                avg_actual_edge_bps: avg_act,
                avg_edge_realization_bps: avg_edge_real,
                avg_total_cost_bps: avg_cost_bps,
                take_profit_count: tp_count,
                stop_loss_count: sl_count,
                time_exit_count: te_count,
                end_of_backtest_count: eob_count,
            });
        }

        rows
    }

    // ── rejection groups ──────────────────────────────────────────────────────

    fn build_rejection_groups(rejections: &[RiskRejection]) -> Vec<RejectionByStageReasonRow> {
        // Group by (stage, entry_geometry_mode, reason)
        type Key = (String, String, String);
        let mut groups: BTreeMap<Key, Vec<&RiskRejection>> = BTreeMap::new();

        for r in rejections {
            let key = (
                r.stage.clone(),
                r.entry_geometry_mode.clone(),
                r.reason.clone(),
            );
            groups.entry(key).or_default().push(r);
        }

        let mut rows = Vec::with_capacity(groups.len());

        for ((stage, mode, reason), group) in &groups {
            let count = group.len();
            let mut unique_sigs: std::collections::BTreeSet<&str> =
                std::collections::BTreeSet::new();
            let mut equity_sum = 0.0_f64;
            let mut drawdown_sum = 0.0_f64;
            let mut daily_pnl_sum = 0.0_f64;
            let mut reward_bps_sum = 0.0_f64;
            let mut cost_bps_sum = 0.0_f64;
            let mut net_edge_sum = 0.0_f64;
            let mut net_edge_min = f64::MAX;
            let mut net_edge_max = f64::MIN;

            for r in group {
                unique_sigs.insert(r.signal_id.as_str());
                equity_sum += r.equity;
                drawdown_sum += r.drawdown_pct;
                daily_pnl_sum += r.daily_realized_pnl;
                reward_bps_sum += r.expected_reward_bps;
                cost_bps_sum += r.expected_cost_bps;
                net_edge_sum += r.expected_net_edge_bps;
                if r.expected_net_edge_bps < net_edge_min {
                    net_edge_min = r.expected_net_edge_bps;
                }
                if r.expected_net_edge_bps > net_edge_max {
                    net_edge_max = r.expected_net_edge_bps;
                }
            }

            let n = count as f64;
            let (min_edge, max_edge) = if count == 0 {
                (0.0, 0.0)
            } else {
                (net_edge_min, net_edge_max)
            };

            rows.push(RejectionByStageReasonRow {
                stage: stage.clone(),
                entry_geometry_mode: mode.clone(),
                reason: reason.clone(),
                count,
                unique_signals: unique_sigs.len(),
                avg_equity: if count > 0 { equity_sum / n } else { 0.0 },
                avg_drawdown_pct: if count > 0 { drawdown_sum / n } else { 0.0 },
                avg_daily_realized_pnl: if count > 0 { daily_pnl_sum / n } else { 0.0 },
                avg_expected_reward_bps: if count > 0 { reward_bps_sum / n } else { 0.0 },
                avg_expected_cost_bps: if count > 0 { cost_bps_sum / n } else { 0.0 },
                avg_expected_net_edge_bps: if count > 0 { net_edge_sum / n } else { 0.0 },
                min_expected_net_edge_bps: min_edge,
                max_expected_net_edge_bps: max_edge,
            });
        }

        rows
    }

    // ── cost / edge distribution ──────────────────────────────────────────────

    fn build_cost_edge(trades: &[Trade]) -> CostEdgeDistribution {
        const BUCKET_NAMES: &[&str] = &[
            "edge_lt_0",
            "edge_0_5",
            "edge_5_10",
            "edge_10_15",
            "edge_15_20",
            "edge_20_30",
            "edge_30_50",
            "edge_gte_50",
        ];

        // Accumulator per bucket: (wins, losses, exp_edge_sum, act_edge_sum, cost_bps_sum, net_pnl_bps_sum, net_pnl_sum)
        struct Acc {
            wins: usize,
            losses: usize,
            exp_edge_sum: f64,
            act_edge_sum: f64,
            cost_bps_sum: f64,
            net_pnl_bps_sum: f64,
            net_pnl_sum: f64,
        }

        let mut accs: Vec<Acc> = (0..8)
            .map(|_| Acc {
                wins: 0,
                losses: 0,
                exp_edge_sum: 0.0,
                act_edge_sum: 0.0,
                cost_bps_sum: 0.0,
                net_pnl_bps_sum: 0.0,
                net_pnl_sum: 0.0,
            })
            .collect();

        for t in trades {
            let edge = t.expected_edge_bps;
            let bucket_idx = if edge < 0.0 {
                0
            } else if edge < 5.0 {
                1
            } else if edge < 10.0 {
                2
            } else if edge < 15.0 {
                3
            } else if edge < 20.0 {
                4
            } else if edge < 30.0 {
                5
            } else if edge < 50.0 {
                6
            } else {
                7
            };

            let acc = &mut accs[bucket_idx];
            if t.net_pnl > 0.0 {
                acc.wins += 1;
            } else {
                acc.losses += 1;
            }
            let n = entry_notional(t);
            acc.exp_edge_sum += t.expected_edge_bps;
            acc.act_edge_sum += t.actual_edge_bps;
            acc.cost_bps_sum += bps_of(t.fee + t.slippage, n);
            acc.net_pnl_bps_sum += bps_of(t.net_pnl, n);
            acc.net_pnl_sum += t.net_pnl;
        }

        let buckets = BUCKET_NAMES
            .iter()
            .zip(accs.iter())
            .map(|(&name, acc)| {
                let count = acc.wins + acc.losses;
                let n = count as f64;
                let win_rate = if count > 0 {
                    acc.wins as f64 / n * 100.0
                } else {
                    0.0
                };
                CostEdgeRow {
                    bucket: name.to_string(),
                    trades: count,
                    wins: acc.wins,
                    losses: acc.losses,
                    win_rate,
                    avg_expected_edge_bps: if count > 0 {
                        acc.exp_edge_sum / n
                    } else {
                        0.0
                    },
                    avg_actual_edge_bps: if count > 0 {
                        acc.act_edge_sum / n
                    } else {
                        0.0
                    },
                    avg_edge_realization_bps: if count > 0 {
                        (acc.act_edge_sum - acc.exp_edge_sum) / n
                    } else {
                        0.0
                    },
                    avg_total_cost_bps: if count > 0 {
                        acc.cost_bps_sum / n
                    } else {
                        0.0
                    },
                    avg_net_pnl_bps: if count > 0 {
                        acc.net_pnl_bps_sum / n
                    } else {
                        0.0
                    },
                    net_pnl: acc.net_pnl_sum,
                }
            })
            .collect();

        CostEdgeDistribution { buckets }
    }

    // ── trade distribution summary ────────────────────────────────────────────

    fn build_trade_distribution(
        trades: &[Trade],
        risk_rejections: &[RiskRejection],
        signal_flow: &SignalFlowSummary,
    ) -> TradeDistributionSummary {
        let total = trades.len();
        let mut wins = 0usize;
        let mut gross_pnl = 0.0_f64;
        let mut fee = 0.0_f64;
        let mut slippage = 0.0_f64;
        let mut exp_edge_sum = 0.0_f64;
        let mut act_edge_sum = 0.0_f64;
        let mut cost_bps_sum = 0.0_f64;

        for t in trades {
            if t.net_pnl > 0.0 {
                wins += 1;
            }
            gross_pnl += t.gross_pnl;
            fee += t.fee;
            slippage += t.slippage;
            exp_edge_sum += t.expected_edge_bps;
            act_edge_sum += t.actual_edge_bps;
            let n = entry_notional(t);
            cost_bps_sum += bps_of(t.fee + t.slippage, n);
        }

        let losing_trades = total - wins;
        let n = total as f64;
        let win_rate = if total > 0 { wins as f64 / n * 100.0 } else { 0.0 };
        let total_cost = fee + slippage;
        let net_pnl = gross_pnl - total_cost;

        let avg_exp = if total > 0 { exp_edge_sum / n } else { 0.0 };
        let avg_act = if total > 0 { act_edge_sum / n } else { 0.0 };
        let avg_cost_bps = if total > 0 { cost_bps_sum / n } else { 0.0 };

        let cost_to_gross_loss_ratio = if gross_pnl.abs() > 1e-12 {
            total_cost / gross_pnl.abs()
        } else {
            0.0
        };

        // Dominant rejection reason
        let mut reason_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for r in risk_rejections {
            *reason_counts.entry(r.reason.as_str()).or_insert(0) += 1;
        }
        let (dominant_reason, dominant_count) = reason_counts
            .iter()
            .max_by_key(|&(_, &c)| c)
            .map(|(&r, &c)| (r.to_string(), c))
            .unwrap_or_default();

        TradeDistributionSummary {
            total_trades: total,
            winning_trades: wins,
            losing_trades,
            win_rate,
            gross_pnl,
            fee,
            slippage,
            total_cost,
            net_pnl,
            avg_expected_edge_bps: avg_exp,
            avg_actual_edge_bps: avg_act,
            avg_edge_realization_bps: avg_act - avg_exp,
            avg_total_cost_bps: avg_cost_bps,
            cost_to_gross_loss_ratio,
            signals_generated: signal_flow.signals_generated,
            signals_preapproved: signal_flow.signals_preapproved,
            signals_rejected_initial_risk: signal_flow.signals_rejected_initial_risk,
            signals_rejected_actual_entry: signal_flow.signals_rejected_actual_entry,
            trades_opened: signal_flow.trades_opened,
            trades_closed: signal_flow.trades_closed,
            risk_rejections: risk_rejections.len(),
            dominant_rejection_reason: dominant_reason,
            dominant_rejection_count: dominant_count,
        }
    }
}

// ── DiagnosticWriter ─────────────────────────────────────────────────────────

pub struct DiagnosticWriter;

impl DiagnosticWriter {
    /// Write all 5 diagnostic files to `reports_dir`.
    pub fn write_all(
        reports_dir: &str,
        report: &DiagnosticReport,
    ) -> Result<(), NorthflowError> {
        let dir = Path::new(reports_dir);
        fs::create_dir_all(dir).map_err(|e| {
            NorthflowError::DataError(format!(
                "cannot create reports dir '{reports_dir}': {e}"
            ))
        })?;

        Self::write_signal_diagnostics(dir, &report.trade_distribution, report)?;
        Self::write_rejection_by_stage_reason(dir, &report.rejection_by_stage_reason)?;
        Self::write_monthly_summary(dir, &report.monthly)?;
        Self::write_cost_edge_distribution(dir, &report.cost_edge_distribution)?;
        Self::write_trade_distribution_summary(dir, &report.trade_distribution)?;

        Ok(())
    }

    // We pass trades indirectly via the DiagnosticReport.
    // signal_diagnostics.csv is built from trades directly in research/mod.rs
    // but to keep the writer self-contained we embed a helper that re-derives
    // per-trade rows from the same fields already on Trade.

    fn write_signal_diagnostics(
        dir: &Path,
        _summary: &TradeDistributionSummary,
        report: &DiagnosticReport,
    ) -> Result<(), NorthflowError> {
        // signal_diagnostics.csv rows are stored in the report's trade_distribution
        // only for aggregate data.  The per-trade rows come from a separate slice
        // that DiagnosticWriter::write_all_with_trades uses.
        // This overload writes an empty-body file; the real write happens via
        // write_all_with_trades (called from research/mod.rs).
        // We expose write_signal_diagnostics_rows for tests.
        let _ = report; // unused in this path; handled by caller
        // write header-only placeholder (will be overwritten by full path)
        let path = dir.join("signal_diagnostics.csv");
        if !path.exists() {
            let header = "trade_id,signal_id,month,symbol,strategy_id,regime,side,entry_time,exit_time,duration_ms,entry_price,exit_price,stop_loss,take_profit,qty,gross_pnl,fee,slippage,total_cost,net_pnl,reward_risk,bars_held,exit_reason,expected_edge_bps,actual_edge_bps,edge_realization_bps,fee_bps,slippage_bps,total_cost_bps,net_pnl_bps,filters_passed,filters_failed,entry_reason\n";
            fs::write(&path, header).map_err(|e| {
                NorthflowError::DataError(format!("cannot write {}: {e}", path.display()))
            })?;
        }
        Ok(())
    }

    fn write_rejection_by_stage_reason(
        dir: &Path,
        rows: &[RejectionByStageReasonRow],
    ) -> Result<(), NorthflowError> {
        let mut out = String::from("stage,entry_geometry_mode,reason,count,unique_signals,avg_equity,avg_drawdown_pct,avg_daily_realized_pnl,avg_expected_reward_bps,avg_expected_cost_bps,avg_expected_net_edge_bps,min_expected_net_edge_bps,max_expected_net_edge_bps\n");
        for r in rows {
            out.push_str(&format!(
                "{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}\n",
                csv_escape(&r.stage),
                csv_escape(&r.entry_geometry_mode),
                csv_escape(&r.reason),
                r.count,
                r.unique_signals,
                r.avg_equity,
                r.avg_drawdown_pct,
                r.avg_daily_realized_pnl,
                r.avg_expected_reward_bps,
                r.avg_expected_cost_bps,
                r.avg_expected_net_edge_bps,
                r.min_expected_net_edge_bps,
                r.max_expected_net_edge_bps,
            ));
        }
        let path = dir.join("rejection_by_stage_reason.csv");
        fs::write(&path, out)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    fn write_monthly_summary(
        dir: &Path,
        rows: &[MonthlySummaryRow],
    ) -> Result<(), NorthflowError> {
        let mut out = String::from("month,trades,wins,losses,win_rate,gross_pnl,fee,slippage,total_cost,net_pnl,profit_factor,avg_win,avg_loss,expectancy,max_consecutive_losses,avg_reward_risk,avg_expected_edge_bps,avg_actual_edge_bps,avg_edge_realization_bps,avg_total_cost_bps,take_profit_count,stop_loss_count,time_exit_count,end_of_backtest_count\n");
        for r in rows {
            let pf_str = if r.profit_factor_inf {
                "inf".to_string()
            } else {
                format!("{:.6}", r.profit_factor)
            };
            out.push_str(&format!(
                "{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{:.6},{:.6},{:.6},{},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{},{}\n",
                csv_escape(&r.month),
                r.trades,
                r.wins,
                r.losses,
                r.win_rate,
                r.gross_pnl,
                r.fee,
                r.slippage,
                r.total_cost,
                r.net_pnl,
                pf_str,
                r.avg_win,
                r.avg_loss,
                r.expectancy,
                r.max_consecutive_losses,
                r.avg_reward_risk,
                r.avg_expected_edge_bps,
                r.avg_actual_edge_bps,
                r.avg_edge_realization_bps,
                r.avg_total_cost_bps,
                r.take_profit_count,
                r.stop_loss_count,
                r.time_exit_count,
                r.end_of_backtest_count,
            ));
        }
        let path = dir.join("monthly_summary.csv");
        fs::write(&path, out)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    fn write_cost_edge_distribution(
        dir: &Path,
        dist: &CostEdgeDistribution,
    ) -> Result<(), NorthflowError> {
        let mut out = String::from("bucket,trades,wins,losses,win_rate,avg_expected_edge_bps,avg_actual_edge_bps,avg_edge_realization_bps,avg_total_cost_bps,avg_net_pnl_bps,net_pnl\n");
        for r in &dist.buckets {
            out.push_str(&format!(
                "{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}\n",
                csv_escape(&r.bucket),
                r.trades,
                r.wins,
                r.losses,
                r.win_rate,
                r.avg_expected_edge_bps,
                r.avg_actual_edge_bps,
                r.avg_edge_realization_bps,
                r.avg_total_cost_bps,
                r.avg_net_pnl_bps,
                r.net_pnl,
            ));
        }
        let path = dir.join("cost_edge_distribution.csv");
        fs::write(&path, out)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    fn write_trade_distribution_summary(
        dir: &Path,
        s: &TradeDistributionSummary,
    ) -> Result<(), NorthflowError> {
        let dominant_reason = json_str(&s.dominant_rejection_reason);
        let json = format!(
            concat!(
                "{{\n",
                "  \"total_trades\": {},\n",
                "  \"winning_trades\": {},\n",
                "  \"losing_trades\": {},\n",
                "  \"win_rate\": {:.6},\n",
                "  \"gross_pnl\": {:.6},\n",
                "  \"fee\": {:.6},\n",
                "  \"slippage\": {:.6},\n",
                "  \"total_cost\": {:.6},\n",
                "  \"net_pnl\": {:.6},\n",
                "  \"avg_expected_edge_bps\": {:.6},\n",
                "  \"avg_actual_edge_bps\": {:.6},\n",
                "  \"avg_edge_realization_bps\": {:.6},\n",
                "  \"avg_total_cost_bps\": {:.6},\n",
                "  \"cost_to_gross_loss_ratio\": {:.6},\n",
                "  \"signals_generated\": {},\n",
                "  \"signals_preapproved\": {},\n",
                "  \"signals_rejected_initial_risk\": {},\n",
                "  \"signals_rejected_actual_entry\": {},\n",
                "  \"trades_opened\": {},\n",
                "  \"trades_closed\": {},\n",
                "  \"risk_rejections\": {},\n",
                "  \"dominant_rejection_reason\": {},\n",
                "  \"dominant_rejection_count\": {}\n",
                "}}\n"
            ),
            s.total_trades,
            s.winning_trades,
            s.losing_trades,
            s.win_rate,
            s.gross_pnl,
            s.fee,
            s.slippage,
            s.total_cost,
            s.net_pnl,
            s.avg_expected_edge_bps,
            s.avg_actual_edge_bps,
            s.avg_edge_realization_bps,
            s.avg_total_cost_bps,
            s.cost_to_gross_loss_ratio,
            s.signals_generated,
            s.signals_preapproved,
            s.signals_rejected_initial_risk,
            s.signals_rejected_actual_entry,
            s.trades_opened,
            s.trades_closed,
            s.risk_rejections,
            dominant_reason,
            s.dominant_rejection_count,
        );
        let path = dir.join("trade_distribution_summary.json");
        fs::write(&path, json)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }

    /// Full write including signal_diagnostics.csv rows from `trades`.
    ///
    /// This is the primary entry point called from `src/research/mod.rs`.
    pub fn write_all_with_trades(
        reports_dir: &str,
        report: &DiagnosticReport,
        trades: &[Trade],
    ) -> Result<(), NorthflowError> {
        let dir = Path::new(reports_dir);
        fs::create_dir_all(dir).map_err(|e| {
            NorthflowError::DataError(format!(
                "cannot create reports dir '{reports_dir}': {e}"
            ))
        })?;

        Self::write_signal_diagnostics_rows(dir, trades)?;
        Self::write_rejection_by_stage_reason(dir, &report.rejection_by_stage_reason)?;
        Self::write_monthly_summary(dir, &report.monthly)?;
        Self::write_cost_edge_distribution(dir, &report.cost_edge_distribution)?;
        Self::write_trade_distribution_summary(dir, &report.trade_distribution)?;

        Ok(())
    }

    /// Write signal_diagnostics.csv from a trade slice.
    pub(crate) fn write_signal_diagnostics_rows(
        dir: &Path,
        trades: &[Trade],
    ) -> Result<(), NorthflowError> {
        let header = "trade_id,signal_id,month,symbol,strategy_id,regime,side,entry_time,exit_time,duration_ms,entry_price,exit_price,stop_loss,take_profit,qty,gross_pnl,fee,slippage,total_cost,net_pnl,reward_risk,bars_held,exit_reason,expected_edge_bps,actual_edge_bps,edge_realization_bps,fee_bps,slippage_bps,total_cost_bps,net_pnl_bps,filters_passed,filters_failed,entry_reason\n";
        let mut out = String::from(header);

        for t in trades {
            let month = month_key_from_ms(t.entry_time);
            let duration_ms = (t.exit_time - t.entry_time).max(0);
            let total_cost = t.fee + t.slippage;
            let edge_real = t.actual_edge_bps - t.expected_edge_bps;
            let n = entry_notional(t);
            let fee_bps = bps_of(t.fee, n);
            let slip_bps = bps_of(t.slippage, n);
            let cost_bps = bps_of(total_cost, n);
            let net_pnl_bps = bps_of(t.net_pnl, n);

            let filters_passed_str = t.filters_passed.join("|");
            let filters_failed_str = t.filters_failed.join("|");

            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{}\n",
                csv_escape(t.trade_id.as_str()),
                csv_escape(t.signal_id.as_str()),
                csv_escape(&month),
                csv_escape(t.symbol.as_str()),
                csv_escape(t.strategy_id.as_str()),
                csv_escape(&t.regime),
                csv_escape(t.side.as_str()),
                t.entry_time,
                t.exit_time,
                duration_ms,
                t.entry_price,
                t.exit_price,
                t.stop_loss,
                t.take_profit,
                t.quantity,
                t.gross_pnl,
                t.fee,
                t.slippage,
                total_cost,
                t.net_pnl,
                t.reward_risk,
                t.bars_held,
                csv_escape(t.exit_reason.as_str()),
                t.expected_edge_bps,
                t.actual_edge_bps,
                edge_real,
                fee_bps,
                slip_bps,
                cost_bps,
                net_pnl_bps,
                csv_escape(&filters_passed_str),
                csv_escape(&filters_failed_str),
                csv_escape(&t.entry_reason),
            ));
        }

        let path = dir.join("signal_diagnostics.csv");
        fs::write(&path, out)
            .map_err(|e| NorthflowError::DataError(format!("cannot write {}: {e}", path.display())))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::risk_trace::SignalFlowSummary;
    use crate::core::{
        Trade,
        position::PositionId,
        side::Side,
        signal::{SignalId, StrategyId},
        symbol::Symbol,
        trade::{TradeExitReason, TradeId},
    };

    // ── helpers ───────────────────────────────────────────────────────────────

    fn make_trade(
        n: u32,
        entry_time: i64,
        net_pnl: f64,
        gross_pnl: f64,
        fee: f64,
        slippage: f64,
        expected_edge_bps: f64,
        actual_edge_bps: f64,
        exit_reason: TradeExitReason,
        filters_passed: Vec<String>,
        filters_failed: Vec<String>,
        entry_reason: &str,
    ) -> Trade {
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
            entry_time,
            exit_time: entry_time + 300_000,
            entry_price: 40_000.0,
            exit_price: if net_pnl > 0.0 { 40_400.0 } else { 39_600.0 },
            stop_loss: 39_600.0,
            take_profit: 40_400.0,
            quantity: 0.1,
            gross_pnl,
            fee,
            slippage,
            net_pnl,
            reward_risk: 2.0,
            bars_held: 5,
            exit_reason,
            entry_reason: entry_reason.to_string(),
            filters_passed,
            filters_failed,
            expected_edge_bps,
            actual_edge_bps,
        }
    }

    fn simple_trade(n: u32, entry_time: i64, net_pnl: f64, edge: f64) -> Trade {
        let gross = if net_pnl > 0.0 { net_pnl + 8.0 } else { net_pnl + 8.0 };
        make_trade(
            n,
            entry_time,
            net_pnl,
            gross,
            5.0,
            3.0,
            edge,
            edge - 10.0,
            TradeExitReason::TakeProfit,
            vec!["atr_valid".to_string()],
            vec![],
            "ema_cross",
        )
    }

    fn make_rejection(reason: &str, stage: &str, mode: &str) -> RiskRejection {
        RiskRejection {
            signal_id: "SIG-BT-00000001".to_string(),
            stage: stage.to_string(),
            entry_geometry_mode: mode.to_string(),
            timestamp: 1_700_000_000_000,
            side: "long".to_string(),
            regime: "bullish".to_string(),
            reason: reason.to_string(),
            equity: 9_800.0,
            peak_equity: 10_000.0,
            drawdown_pct: 2.0,
            daily_realized_pnl: -50.0,
            expected_reward_bps: 200.0,
            expected_cost_bps: 8.0,
            expected_net_edge_bps: 192.0,
        }
    }

    fn tmp_dir(tag: &str) -> String {
        format!("/tmp/nf_diag_test_{}_{tag}", std::process::id())
    }

    // ── month key tests ───────────────────────────────────────────────────────

    #[test]
    fn month_key_from_ms_2024_01() {
        assert_eq!(month_key_from_ms(1_704_067_200_000), "2024-01");
    }

    #[test]
    fn month_key_from_ms_2024_02() {
        assert_eq!(month_key_from_ms(1_706_745_600_000), "2024-02");
    }

    #[test]
    fn month_key_from_ms_2024_12() {
        assert_eq!(month_key_from_ms(1_735_689_540_000), "2024-12");
    }

    // ── signal diagnostics tests ──────────────────────────────────────────────

    #[test]
    fn signal_diagnostics_empty_trades_writes_header() {
        let dir = tmp_dir("sig_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::path::Path::new(&dir);
        DiagnosticWriter::write_signal_diagnostics_rows(path, &[]).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/signal_diagnostics.csv")).unwrap();
        assert!(content.starts_with("trade_id,signal_id,month,"));
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "header only, no data rows");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signal_diagnostics_row_computes_total_cost() {
        let t = make_trade(
            1,
            1_704_067_200_000,
            52.0,
            60.0,
            5.0,
            3.0,
            20.0,
            15.0,
            TradeExitReason::TakeProfit,
            vec![],
            vec![],
            "ema_cross",
        );
        let dir = tmp_dir("sig_cost");
        std::fs::create_dir_all(&dir).unwrap();
        DiagnosticWriter::write_signal_diagnostics_rows(std::path::Path::new(&dir), &[t]).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/signal_diagnostics.csv")).unwrap();
        let data_line = content.lines().nth(1).unwrap();
        let fields: Vec<&str> = data_line.split(',').collect();
        // total_cost is field index 18 (0-based)
        let total_cost: f64 = fields[18].parse().unwrap();
        assert!((total_cost - 8.0).abs() < 1e-6, "total_cost should be 8.0, got {total_cost}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signal_diagnostics_row_computes_cost_bps() {
        let t = make_trade(
            1,
            1_704_067_200_000,
            52.0,
            60.0,
            5.0,
            3.0,
            20.0,
            15.0,
            TradeExitReason::TakeProfit,
            vec![],
            vec![],
            "ema_cross",
        );
        // entry_notional = 40000 * 0.1 = 4000; total_cost_bps = 8/4000*10000 = 20 bps
        let dir = tmp_dir("sig_cbps");
        std::fs::create_dir_all(&dir).unwrap();
        DiagnosticWriter::write_signal_diagnostics_rows(std::path::Path::new(&dir), &[t]).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/signal_diagnostics.csv")).unwrap();
        let data_line = content.lines().nth(1).unwrap();
        let fields: Vec<&str> = data_line.split(',').collect();
        // total_cost_bps is at field index 28
        let cost_bps: f64 = fields[28].parse().unwrap();
        assert!((cost_bps - 20.0).abs() < 1e-4, "cost_bps should be 20.0, got {cost_bps}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signal_diagnostics_row_computes_edge_realization() {
        let t = make_trade(
            1,
            1_704_067_200_000,
            52.0,
            60.0,
            5.0,
            3.0,
            20.0,
            15.0,
            TradeExitReason::TakeProfit,
            vec![],
            vec![],
            "ema_cross",
        );
        // edge_realization = 15.0 - 20.0 = -5.0
        let dir = tmp_dir("sig_er");
        std::fs::create_dir_all(&dir).unwrap();
        DiagnosticWriter::write_signal_diagnostics_rows(std::path::Path::new(&dir), &[t]).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/signal_diagnostics.csv")).unwrap();
        let data_line = content.lines().nth(1).unwrap();
        let fields: Vec<&str> = data_line.split(',').collect();
        // edge_realization_bps is at field index 25
        let edge_real: f64 = fields[25].parse().unwrap();
        assert!((edge_real - (-5.0)).abs() < 1e-4, "edge_real should be -5.0, got {edge_real}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signal_diagnostics_row_has_month_key() {
        let t = simple_trade(1, 1_704_067_200_000, 10.0, 15.0);
        let dir = tmp_dir("sig_month");
        std::fs::create_dir_all(&dir).unwrap();
        DiagnosticWriter::write_signal_diagnostics_rows(std::path::Path::new(&dir), &[t]).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/signal_diagnostics.csv")).unwrap();
        assert!(content.contains("2024-01"), "month key must appear in row");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signal_diagnostics_csv_escapes_filters_and_reason() {
        let t = make_trade(
            1,
            1_704_067_200_000,
            10.0,
            18.0,
            5.0,
            3.0,
            15.0,
            10.0,
            TradeExitReason::TakeProfit,
            vec!["filter,one".to_string(), "filter_two".to_string()],
            vec!["bad,filter".to_string()],
            "reason with, comma",
        );
        let dir = tmp_dir("sig_escape");
        std::fs::create_dir_all(&dir).unwrap();
        DiagnosticWriter::write_signal_diagnostics_rows(std::path::Path::new(&dir), &[t]).unwrap();
        let content = std::fs::read_to_string(format!("{dir}/signal_diagnostics.csv")).unwrap();
        // fields with commas must be quoted
        assert!(content.contains("\"filter,one|filter_two\"") || content.contains("\"filter,one"));
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── rejection grouped tests ───────────────────────────────────────────────

    #[test]
    fn rejection_by_stage_reason_groups_by_stage_mode_reason() {
        let rejections = vec![
            make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels"),
            make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels"),
            make_rejection("reward_risk_below_minimum", "initial_risk", "preserve_signal_levels"),
        ];
        let rows = DiagnosticEngine::build_rejection_groups(&rejections);
        assert_eq!(rows.len(), 2);
        let edge_row = rows.iter().find(|r| r.reason == "expected_net_edge_not_positive").unwrap();
        assert_eq!(edge_row.count, 2);
        let rr_row = rows.iter().find(|r| r.reason == "reward_risk_below_minimum").unwrap();
        assert_eq!(rr_row.count, 1);
    }

    #[test]
    fn rejection_by_stage_reason_counts_unique_signals() {
        let mut r1 = make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels");
        r1.signal_id = "SIG-01".to_string();
        let mut r2 = make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels");
        r2.signal_id = "SIG-01".to_string();
        let mut r3 = make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels");
        r3.signal_id = "SIG-02".to_string();

        let rows = DiagnosticEngine::build_rejection_groups(&[r1, r2, r3]);
        assert_eq!(rows[0].unique_signals, 2);
    }

    #[test]
    fn rejection_by_stage_reason_averages_cost_and_edge() {
        let mut r1 = make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels");
        r1.expected_net_edge_bps = 100.0;
        let mut r2 = make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels");
        r2.expected_net_edge_bps = 200.0;

        let rows = DiagnosticEngine::build_rejection_groups(&[r1, r2]);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!((row.avg_expected_net_edge_bps - 150.0).abs() < 1e-6);
        assert!((row.min_expected_net_edge_bps - 100.0).abs() < 1e-6);
        assert!((row.max_expected_net_edge_bps - 200.0).abs() < 1e-6);
    }

    #[test]
    fn rejection_by_stage_reason_sorts_stably() {
        let rows = DiagnosticEngine::build_rejection_groups(&[
            make_rejection("reward_risk_below_minimum", "initial_risk", "preserve_signal_levels"),
            make_rejection("expected_net_edge_not_positive", "actual_entry", "reanchor_to_actual_entry"),
            make_rejection("max_drawdown_reached", "initial_risk", "preserve_signal_levels"),
        ]);
        // Sorted by (stage, mode, reason) ascending
        // actual_entry < initial_risk (alphabetical)
        assert_eq!(rows[0].stage, "actual_entry");
        assert_eq!(rows[1].stage, "initial_risk");
        assert_eq!(rows[1].reason, "max_drawdown_reached");
        assert_eq!(rows[2].reason, "reward_risk_below_minimum");
    }

    // ── monthly summary tests ─────────────────────────────────────────────────

    #[test]
    fn monthly_summary_groups_by_month() {
        // Jan 2024 and Feb 2024
        let trades = vec![
            simple_trade(1, 1_704_067_200_000, 10.0, 20.0), // Jan
            simple_trade(2, 1_704_153_600_000, -5.0, 20.0), // Jan
            simple_trade(3, 1_706_745_600_000, 8.0, 20.0),  // Feb
        ];
        let rows = DiagnosticEngine::build_monthly(&trades);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].month, "2024-01");
        assert_eq!(rows[1].month, "2024-02");
        assert_eq!(rows[0].trades, 2);
        assert_eq!(rows[1].trades, 1);
    }

    #[test]
    fn monthly_summary_computes_win_rate() {
        let trades = vec![
            simple_trade(1, 1_704_067_200_000, 10.0, 20.0),
            simple_trade(2, 1_704_153_600_000, -5.0, 20.0),
            simple_trade(3, 1_704_240_000_000, 8.0, 20.0),
            simple_trade(4, 1_704_326_400_000, -3.0, 20.0),
        ];
        let rows = DiagnosticEngine::build_monthly(&trades);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.wins, 2);
        assert_eq!(r.losses, 2);
        assert!((r.win_rate - 50.0).abs() < 1e-6);
    }

    #[test]
    fn monthly_summary_computes_profit_factor() {
        let trades = vec![
            simple_trade(1, 1_704_067_200_000, 10.0, 18.0),
            simple_trade(2, 1_704_153_600_000, -5.0, -3.0),
        ];
        let rows = DiagnosticEngine::build_monthly(&trades);
        let r = &rows[0];
        // gross winners = 18, gross losers = 3, pf = 6
        assert!((r.profit_factor - 6.0).abs() < 1e-4);
        assert!(!r.profit_factor_inf);
    }

    #[test]
    fn monthly_summary_counts_exit_reasons() {
        let trades = vec![
            make_trade(1, 1_704_067_200_000, 10.0, 18.0, 5.0, 3.0, 20.0, 15.0, TradeExitReason::TakeProfit, vec![], vec![], "e"),
            make_trade(2, 1_704_153_600_000, -5.0, -3.0, 5.0, 3.0, 20.0, 5.0, TradeExitReason::StopLoss, vec![], vec![], "e"),
            make_trade(3, 1_704_240_000_000, -2.0, 0.0, 5.0, 3.0, 20.0, 8.0, TradeExitReason::TimeExit, vec![], vec![], "e"),
            make_trade(4, 1_704_326_400_000, -1.0, 0.0, 5.0, 3.0, 20.0, 8.0, TradeExitReason::EndOfBacktest, vec![], vec![], "e"),
        ];
        let rows = DiagnosticEngine::build_monthly(&trades);
        let r = &rows[0];
        assert_eq!(r.take_profit_count, 1);
        assert_eq!(r.stop_loss_count, 1);
        assert_eq!(r.time_exit_count, 1);
        assert_eq!(r.end_of_backtest_count, 1);
    }

    #[test]
    fn monthly_summary_computes_avg_total_cost_bps() {
        // entry_price=40000, qty=0.1, notional=4000, fee+slip=8, cost_bps=20
        let t = simple_trade(1, 1_704_067_200_000, 10.0, 20.0);
        let rows = DiagnosticEngine::build_monthly(&[t]);
        let r = &rows[0];
        assert!((r.avg_total_cost_bps - 20.0).abs() < 1e-4);
    }

    #[test]
    fn monthly_summary_sorts_by_month() {
        let trades = vec![
            simple_trade(1, 1_711_929_600_000, 5.0, 20.0),  // Apr
            simple_trade(2, 1_704_067_200_000, 5.0, 20.0),  // Jan
            simple_trade(3, 1_706_745_600_000, 5.0, 20.0),  // Feb
        ];
        let rows = DiagnosticEngine::build_monthly(&trades);
        assert_eq!(rows[0].month, "2024-01");
        assert_eq!(rows[1].month, "2024-02");
        assert_eq!(rows[2].month, "2024-04");
    }

    // ── cost edge distribution tests ──────────────────────────────────────────

    #[test]
    fn cost_edge_distribution_includes_all_buckets() {
        let dist = DiagnosticEngine::build_cost_edge(&[]);
        assert_eq!(dist.buckets.len(), 8);
        let names: Vec<&str> = dist.buckets.iter().map(|b| b.bucket.as_str()).collect();
        assert_eq!(
            names,
            &["edge_lt_0", "edge_0_5", "edge_5_10", "edge_10_15", "edge_15_20", "edge_20_30", "edge_30_50", "edge_gte_50"]
        );
    }

    #[test]
    fn cost_edge_distribution_assigns_edge_10_15_bucket() {
        let t = simple_trade(1, 1_704_067_200_000, 10.0, 12.0); // edge=12 → bucket edge_10_15
        let dist = DiagnosticEngine::build_cost_edge(&[t]);
        let bucket = dist.buckets.iter().find(|b| b.bucket == "edge_10_15").unwrap();
        assert_eq!(bucket.trades, 1);
        assert_eq!(bucket.wins, 1);
    }

    #[test]
    fn cost_edge_distribution_assigns_edge_gte_50_bucket() {
        let t = simple_trade(1, 1_704_067_200_000, 10.0, 75.0); // edge=75 → gte_50
        let dist = DiagnosticEngine::build_cost_edge(&[t]);
        let bucket = dist.buckets.iter().find(|b| b.bucket == "edge_gte_50").unwrap();
        assert_eq!(bucket.trades, 1);
    }

    #[test]
    fn cost_edge_distribution_computes_avg_net_pnl_bps() {
        let t = make_trade(
            1,
            1_704_067_200_000,
            20.0,
            28.0,
            5.0,
            3.0,
            20.0,
            15.0,
            TradeExitReason::TakeProfit,
            vec![],
            vec![],
            "e",
        );
        // net_pnl=20, notional=4000, net_pnl_bps=50
        let dist = DiagnosticEngine::build_cost_edge(&[t]);
        let bucket = dist.buckets.iter().find(|b| b.bucket == "edge_20_30").unwrap();
        assert!((bucket.avg_net_pnl_bps - 50.0).abs() < 1e-4);
    }

    // ── JSON summary tests ────────────────────────────────────────────────────

    #[test]
    fn trade_distribution_summary_empty_is_zero() {
        let flow = SignalFlowSummary::default();
        let s = DiagnosticEngine::build_trade_distribution(&[], &[], &flow);
        assert_eq!(s.total_trades, 0);
        assert!((s.win_rate).abs() < 1e-9);
        assert!((s.cost_to_gross_loss_ratio).abs() < 1e-9);
        assert_eq!(s.dominant_rejection_reason, "");
        assert_eq!(s.dominant_rejection_count, 0);
    }

    #[test]
    fn trade_distribution_summary_calculates_total_cost() {
        let t = simple_trade(1, 1_704_067_200_000, 10.0, 20.0); // fee=5, slip=3
        let flow = SignalFlowSummary::default();
        let s = DiagnosticEngine::build_trade_distribution(&[t], &[], &flow);
        assert!((s.total_cost - 8.0).abs() < 1e-6);
    }

    #[test]
    fn trade_distribution_summary_finds_dominant_rejection() {
        let rejections = vec![
            make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels"),
            make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels"),
            make_rejection("reward_risk_below_minimum", "initial_risk", "preserve_signal_levels"),
        ];
        let flow = SignalFlowSummary::default();
        let s = DiagnosticEngine::build_trade_distribution(&[], &rejections, &flow);
        assert_eq!(s.dominant_rejection_reason, "expected_net_edge_not_positive");
        assert_eq!(s.dominant_rejection_count, 2);
    }

    #[test]
    fn trade_distribution_summary_uses_signal_flow_counts() {
        let mut flow = SignalFlowSummary::default();
        flow.signals_generated = 100;
        flow.signals_preapproved = 10;
        flow.trades_opened = 9;
        flow.trades_closed = 9;
        let s = DiagnosticEngine::build_trade_distribution(&[], &[], &flow);
        assert_eq!(s.signals_generated, 100);
        assert_eq!(s.signals_preapproved, 10);
        assert_eq!(s.trades_opened, 9);
    }

    // ── writer tests ──────────────────────────────────────────────────────────

    #[test]
    fn diagnostic_writer_writes_all_files() {
        let dir = tmp_dir("writer_all");
        let t = simple_trade(1, 1_704_067_200_000, 10.0, 20.0);
        let r = make_rejection("expected_net_edge_not_positive", "initial_risk", "preserve_signal_levels");
        let flow = SignalFlowSummary::default();
        let report = DiagnosticEngine::build(&[t.clone()], &[r], &flow);
        DiagnosticWriter::write_all_with_trades(&dir, &report, &[t]).unwrap();

        for fname in &[
            "signal_diagnostics.csv",
            "rejection_by_stage_reason.csv",
            "monthly_summary.csv",
            "cost_edge_distribution.csv",
            "trade_distribution_summary.json",
        ] {
            let path = format!("{dir}/{fname}");
            let content = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{fname} must exist"));
            assert!(!content.is_empty(), "{fname} must not be empty");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_includes_diagnostic_files() {
        use crate::report::manifest::ManifestWriter;
        use crate::report::attribution::AttributionEngine;
        use crate::backtest::metrics::EquityPoint;

        let flow = SignalFlowSummary::default();
        let t = simple_trade(1, 1_704_067_200_000, 10.0, 20.0);
        let diag = DiagnosticEngine::build(&[t.clone()], &[], &flow);
        let attr = AttributionEngine::build(&[t]);
        let equity: Vec<EquityPoint> = vec![];
        let m = ManifestWriter::build("reports", &[], &equity, &attr, 0, &diag);

        let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
        for required in &[
            "reports/signal_diagnostics.csv",
            "reports/rejection_by_stage_reason.csv",
            "reports/monthly_summary.csv",
            "reports/cost_edge_distribution.csv",
            "reports/trade_distribution_summary.json",
        ] {
            assert!(paths.contains(required), "manifest missing {required}");
        }
    }

    #[test]
    fn manifest_counts_cost_edge_distribution_as_8_rows() {
        use crate::report::manifest::ManifestWriter;
        use crate::report::attribution::AttributionEngine;
        use crate::backtest::metrics::EquityPoint;

        let flow = SignalFlowSummary::default();
        let diag = DiagnosticEngine::build(&[], &[], &flow);
        let attr = AttributionEngine::build(&[]);
        let equity: Vec<EquityPoint> = vec![];
        let m = ManifestWriter::build("reports", &[], &equity, &attr, 0, &diag);

        let ced = m.files.iter().find(|f| f.path.ends_with("cost_edge_distribution.csv")).unwrap();
        assert_eq!(ced.rows, 8);
    }
}
