"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { EquityMiniChart } from "@/components/charts/EquityMiniChart";
import { PnlBarChart } from "@/components/charts/PnlBarChart";
import { WinRateDonut } from "@/components/charts/WinRateDonut";
import { useStatus, useTrades, usePositions, useSurvival } from "@/hooks/useAriaData";
import { fmt, fmtPct, fmtPnl, formatDuration } from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  TrendingUp, TrendingDown, DollarSign, Activity,
  Brain, Layers, Clock, ShieldCheck, BarChart2,
  Zap, Target, Percent, AlertTriangle, ArrowUpRight, ArrowDownRight,
} from "lucide-react";
import { useState, useEffect } from "react";

interface EquityPoint { t: string; v: number; }

function KpiCard({
  title, value, sub, icon: Icon, color, trend, trendUp,
}: {
  title: string; value: string; sub?: string;
  icon: React.ElementType; color: string; trend?: string; trendUp?: boolean;
}) {
  return (
    <Card className="relative overflow-hidden">
      <CardContent className="p-4">
        <div className="flex items-start justify-between mb-2">
          <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground">{title}</p>
          <div className={cn("flex items-center justify-center h-7 w-7 rounded-lg", color + "/10")}>
            <Icon className={cn("h-3.5 w-3.5", color)} />
          </div>
        </div>
        <p className={cn("text-[22px] font-bold font-mono tabular-nums leading-none", color)}>{value}</p>
        <div className="flex items-center justify-between mt-1.5">
          {sub && <p className="text-[11px] text-muted-foreground">{sub}</p>}
          {trend && (
            <span className={cn(
              "text-[10px] font-semibold font-mono flex items-center gap-0.5",
              trendUp === undefined ? "text-muted-foreground" :
              trendUp ? "text-profit" : "text-loss"
            )}>
              {trendUp === true && <ArrowUpRight className="h-3 w-3" />}
              {trendUp === false && <ArrowDownRight className="h-3 w-3" />}
              {trend}
            </span>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function StatRow({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div className="flex items-center justify-between py-1.5 border-b border-border/30 last:border-0">
      <span className="text-[11px] text-muted-foreground">{label}</span>
      <span className={cn("text-[11px] font-mono tabular-nums font-semibold", color ?? "text-foreground")}>{value}</span>
    </div>
  );
}

export default function OverviewPage() {
  const { data: status } = useStatus();
  const { data: trades } = useTrades(1, 200);
  const { data: positions } = usePositions();
  const { data: survival } = useSurvival();
  const [equityHistory, setEquityHistory] = useState<EquityPoint[]>([]);

  const shared        = status?.shared;
  const metrics       = status?.metrics;
  const equity        = shared?.equity ?? 0;
  const initialEquity = shared?.initial_equity ?? equity;
  const peakEquity    = shared?.peak_equity ?? equity;
  const pnlToday      = shared?.realized_pnl_today ?? 0;
  const unrealizedPnl = shared?.unrealized_pnl ?? 0;
  const totalEquity   = shared?.total_equity ?? equity;
  const drawdown      = shared?.drawdown_pct ?? 0;
  const openPos       = shared?.open_positions ?? 0;
  const totalPnl      = equity - initialEquity;
  const totalPnlPct   = initialEquity > 0 ? ((equity - initialEquity) / initialEquity) * 100 : 0;

  const closedTrades  = trades?.items ?? [];
  const wins          = closedTrades.filter((t) => t.is_win).length;
  const losses        = closedTrades.filter((t) => !t.is_win).length;
  const totalTrades   = wins + losses;
  const winRate       = totalTrades > 0 ? wins / totalTrades : 0;
  const grossProfit   = closedTrades.filter((t) => t.is_win).reduce((s, t) => s + t.pnl_usd, 0);
  const grossLoss     = Math.abs(closedTrades.filter((t) => !t.is_win).reduce((s, t) => s + t.pnl_usd, 0));
  const profitFactor  = grossLoss > 0 ? grossProfit / grossLoss : grossProfit > 0 ? 999 : 0;
  const avgWin        = wins > 0 ? grossProfit / wins : 0;
  const avgLoss       = losses > 0 ? grossLoss / losses : 0;
  const expectancy    = totalTrades > 0 ? (winRate * avgWin - (1 - winRate) * avgLoss) : 0;
  const maxWin        = closedTrades.filter(t=>t.is_win).reduce((m,t)=>Math.max(m,t.pnl_usd),0);
  const maxLoss       = closedTrades.filter(t=>!t.is_win).reduce((m,t)=>Math.min(m,t.pnl_usd),0);

  const taAvg = closedTrades.filter(t => t.ta_confidence != null).reduce((s, t) => s + (t.ta_confidence ?? 0), 0) / (closedTrades.filter(t => t.ta_confidence != null).length || 1);
  const llmAvg = closedTrades.filter(t => t.llm_confidence != null).reduce((s, t) => s + (t.llm_confidence ?? 0), 0) / (closedTrades.filter(t => t.llm_confidence != null).length || 1);

  useEffect(() => {
    if (!equity) return;
    const now = new Date();
    const t = now.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
    setEquityHistory((prev) => [...prev, { t, v: equity }].slice(-180));
  }, [equity]);

  const survivalMode = survival?.mode ?? shared?.survival_mode ?? "nominal";
  const survivalScore = (survival?.score ?? shared?.survival_score ?? 1) * 100;

  return (
    <div className="flex flex-col h-full">
      <Header title="Overview" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-4">

        {/* KPI Row — 4 main */}
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
          <KpiCard title="Total Equity"   value={`$${fmt(equity)}`}       sub={`Initial $${fmt(initialEquity)}`}
            icon={DollarSign}  color="text-foreground" trend={fmtPct(totalPnlPct)} trendUp={totalPnlPct >= 0} />
          <KpiCard title="Daily P&L"      value={fmtPnl(pnlToday)}        sub={`${metrics?.trades_today ?? 0} trades today`}
            icon={pnlToday >= 0 ? TrendingUp : TrendingDown} color={pnlToday >= 0 ? "text-profit" : "text-loss"}
            trend={pnlToday !== 0 ? fmtPct((pnlToday / initialEquity) * 100) : undefined} trendUp={pnlToday >= 0} />
          <KpiCard title="Drawdown"       value={fmtPct(-drawdown)}       sub={`Peak $${fmt(peakEquity)}`}
            icon={Activity} color={drawdown > 5 ? "text-loss" : drawdown > 2 ? "text-warning" : "text-profit"}
            trend={`Death line ${fmt((survival?.death_line_pct ?? 0) * 100, 1)}%`} />
          <KpiCard title="Open Positions" value={String(openPos)}         sub={`Max ${status?.config?.max_open_positions ?? "—"}`}
            icon={Layers} color={openPos > 0 ? "text-info" : "text-muted-foreground"}
            trend={unrealizedPnl !== 0 ? fmtPnl(unrealizedPnl) : undefined} trendUp={unrealizedPnl >= 0} />
        </div>

        {/* Secondary KPIs */}
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-3">
          {[
            { label: "Win Rate",       value: `${fmt(winRate * 100, 1)}%`,       color: winRate >= 0.5 ? "text-profit" : "text-loss" },
            { label: "Profit Factor",  value: profitFactor > 100 ? "∞" : fmt(profitFactor), color: profitFactor >= 1.5 ? "text-profit" : profitFactor >= 1 ? "text-warning" : "text-loss" },
            { label: "Expectancy",     value: fmtPnl(expectancy),               color: expectancy >= 0 ? "text-profit" : "text-loss" },
            { label: "Total Trades",   value: String(totalTrades),               color: "text-foreground" },
            { label: "Unrealized",     value: fmtPnl(unrealizedPnl),            color: unrealizedPnl >= 0 ? "text-profit" : "text-loss" },
            { label: "Total P&L",      value: fmtPnl(totalPnl),                 color: totalPnl >= 0 ? "text-profit" : "text-loss" },
          ].map(({ label, value, color }) => (
            <Card key={label}>
              <CardContent className="p-3">
                <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-1">{label}</p>
                <p className={cn("text-[14px] font-mono font-bold tabular-nums", color)}>{value}</p>
              </CardContent>
            </Card>
          ))}
        </div>

        {/* Charts Row */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
          <Card className="lg:col-span-2">
            <CardHeader>
              <div className="flex items-center justify-between flex-wrap gap-2">
                <CardTitle>Equity Curve (Live)</CardTitle>
                <div className="flex items-center gap-3">
                  <span className="text-[11px] text-muted-foreground font-mono">Total EQ: <span className="text-foreground font-bold">${fmt(totalEquity)}</span></span>
                  <span className={cn("text-[11px] font-mono tabular-nums font-semibold", totalPnlPct >= 0 ? "text-profit" : "text-loss")}>
                    {fmtPct(totalPnlPct)}
                  </span>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <div className="h-40 md:h-52">
                <EquityMiniChart data={equityHistory} color={totalPnlPct >= 0 ? "#16c784" : "#ea3943"} />
              </div>
              <div className="grid grid-cols-3 gap-2 mt-3">
                {[
                  { label: "Realized", value: fmtPnl(pnlToday), color: pnlToday >= 0 ? "text-profit" : "text-loss" },
                  { label: "Unrealized", value: fmtPnl(unrealizedPnl), color: unrealizedPnl >= 0 ? "text-profit" : "text-loss" },
                  { label: "Peak", value: `$${fmt(peakEquity)}`, color: "text-profit" },
                ].map(({ label, value, color }) => (
                  <div key={label} className="rounded-lg bg-secondary/50 px-3 py-2 text-center">
                    <p className="text-[10px] text-muted-foreground mb-0.5">{label}</p>
                    <p className={cn("text-[12px] font-mono tabular-nums font-bold", color)}>{value}</p>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle>Win Rate</CardTitle>
                <span className="text-[11px] text-muted-foreground">{totalTrades} trades</span>
              </div>
            </CardHeader>
            <CardContent>
              <div className="h-36">
                <WinRateDonut winRate={winRate} wins={wins} losses={losses} />
              </div>
              <div className="grid grid-cols-2 gap-2 mt-3">
                <div className="rounded-lg bg-profit/5 border border-profit/15 px-3 py-2 text-center">
                  <p className="text-[10px] text-profit/70 mb-0.5">Avg Win</p>
                  <p className="text-[12px] font-mono font-bold text-profit">+${fmt(avgWin)}</p>
                </div>
                <div className="rounded-lg bg-loss/5 border border-loss/15 px-3 py-2 text-center">
                  <p className="text-[10px] text-loss/70 mb-0.5">Avg Loss</p>
                  <p className="text-[12px] font-mono font-bold text-loss">-${fmt(avgLoss)}</p>
                </div>
              </div>
            </CardContent>
          </Card>
        </div>

        {/* Trade bars + Stats */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
          <Card className="lg:col-span-2">
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle>Last 30 Trades (P&L)</CardTitle>
                <div className="flex gap-2">
                  <span className="text-[10px] text-profit">Best: +${fmt(maxWin)}</span>
                  <span className="text-[10px] text-loss">Worst: -${fmt(Math.abs(maxLoss))}</span>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <div className="h-32 md:h-40">
                <PnlBarChart trades={closedTrades} />
              </div>
            </CardContent>
          </Card>

          <div className="space-y-3">
            <Card>
              <CardHeader><CardTitle>Performance Stats</CardTitle></CardHeader>
              <CardContent className="space-y-0">
                <StatRow label="Profit Factor"  value={profitFactor > 100 ? "∞" : fmt(profitFactor)} color={profitFactor >= 1.5 ? "text-profit" : profitFactor >= 1 ? "text-warning" : "text-loss"} />
                <StatRow label="Gross Profit"   value={`+$${fmt(grossProfit)}`}  color="text-profit" />
                <StatRow label="Gross Loss"     value={`-$${fmt(grossLoss)}`}    color="text-loss" />
                <StatRow label="Expectancy"     value={fmtPnl(expectancy)}       color={expectancy >= 0 ? "text-profit" : "text-loss"} />
                <StatRow label="Best Trade"     value={`+$${fmt(maxWin)}`}       color="text-profit" />
                <StatRow label="Worst Trade"    value={fmtPnl(maxLoss)}          color="text-loss" />
              </CardContent>
            </Card>
          </div>
        </div>

        {/* LLM + Regime + Survival */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <Brain className="h-3.5 w-3.5 text-primary" />
                <CardTitle>LLM Brain</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="space-y-0">
              <StatRow label="Decisions (Go)"      value={String(metrics?.llm_go ?? 0)}       color="text-profit" />
              <StatRow label="Decisions (No-Go)"   value={String(metrics?.llm_nogo ?? 0)}     color="text-loss" />
              <StatRow label="Decisions (Wait)"    value={String(metrics?.llm_wait ?? 0)}     color="text-warning" />
              <StatRow label="Avg Confidence"      value={`${fmt(metrics?.llm_avg_confidence ?? 0, 1)}%`} />
              <StatRow label="Avg Latency"         value={`${metrics?.llm_avg_latency_ms ?? 0}ms`} />
              <StatRow label="Offline Fallbacks"   value={String(metrics?.llm_offline_fallbacks ?? 0)} color={(metrics?.llm_offline_fallbacks ?? 0) > 0 ? "text-warning" : "text-muted-foreground"} />
              <StatRow label="Avg TA Confidence"   value={`${fmt(taAvg, 1)}`} />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <BarChart2 className="h-3.5 w-3.5 text-muted-foreground" />
                <CardTitle>Market Regime</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="space-y-0">
              <StatRow label="Current Regime"   value={shared?.current_regime ?? "—"} />
              <StatRow label="Active Strategies" value={String(status?.config?.active_strategies?.length ?? 0)} />
              <StatRow label="Signals Today"    value={String(metrics?.signals_today ?? 0)} />
              <StatRow label="Active Lessons"   value={String(metrics?.active_lessons ?? 0)} />
              <StatRow label="Exchange"         value={status?.config?.exchange ?? "—"} />
              <StatRow label="Symbols"          value={String(status?.config?.symbol_count ?? 0)} />
              <StatRow label="Mode"             value={status?.config?.mode ?? "—"} />
            </CardContent>
          </Card>

          <Card className={cn("border-2",
            survivalMode === "danger" || survivalMode === "frozen" ? "border-loss/30" :
            survivalMode === "caution" ? "border-warning/25" : "border-profit/15"
          )}>
            <CardHeader>
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <ShieldCheck className="h-3.5 w-3.5 text-muted-foreground" />
                  <CardTitle>Survival State</CardTitle>
                </div>
                <Badge variant={
                  survivalMode === "nominal" ? "profit" :
                  survivalMode === "caution" ? "warning" :
                  survivalMode === "danger" || survivalMode === "frozen" ? "loss" : "secondary"
                }>{survivalMode}</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              <div>
                <div className="flex justify-between text-[11px] mb-1.5">
                  <span className="text-muted-foreground">Survival Score</span>
                  <span className={cn("font-mono font-bold", survivalScore >= 70 ? "text-profit" : survivalScore >= 40 ? "text-warning" : "text-loss")}>
                    {fmt(survivalScore, 1)}
                  </span>
                </div>
                <div className="h-2 w-full rounded-full bg-secondary overflow-hidden">
                  <div className={cn("h-full rounded-full transition-all", survivalScore >= 70 ? "bg-profit" : survivalScore >= 40 ? "bg-warning" : "bg-loss")}
                    style={{ width: `${Math.min(survivalScore, 100)}%` }} />
                </div>
              </div>
              <div className="space-y-0">
                <StatRow label="Drawdown"        value={fmtPct(drawdown)}                  color={drawdown > 5 ? "text-loss" : drawdown > 2 ? "text-warning" : "text-profit"} />
                <StatRow label="Daily Loss"      value={fmtPct(survival?.daily_loss_pct ?? 0)} color={(survival?.daily_loss_pct ?? 0) > 2 ? "text-loss" : "text-muted-foreground"} />
                <StatRow label="Loss Streak"     value={String(survival?.loss_streak ?? 0)} color={(survival?.loss_streak ?? 0) >= 3 ? "text-warning" : "text-muted-foreground"} />
                <StatRow label="Frozen"          value={survival?.is_frozen ? "YES" : "No"} color={survival?.is_frozen ? "text-loss" : "text-muted-foreground"} />
                <StatRow label="Auto-Flat"       value={survival?.auto_flat_triggered ? "YES" : "No"} color={survival?.auto_flat_triggered ? "text-loss" : "text-muted-foreground"} />
              </div>
            </CardContent>
          </Card>
        </div>

        {/* Open Positions */}
        {positions && positions.length > 0 && (
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle>Open Positions</CardTitle>
                <Badge variant="info">{positions.length} open</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2">
              {positions.map((p) => {
                const pnl = p.unrealized_pnl ?? 0;
                const pnlPct = p.unrealized_pnl_pct ?? 0;
                const rr = p.entry_price > 0 && p.stop_loss > 0 && p.take_profit > 0
                  ? Math.abs(p.take_profit - p.entry_price) / Math.abs(p.entry_price - p.stop_loss) : null;
                return (
                  <div key={p.client_id} className={cn(
                    "flex flex-wrap items-center gap-2 rounded-lg px-3 py-2.5 border",
                    p.side === "LONG" ? "bg-profit/5 border-profit/15" : "bg-loss/5 border-loss/15"
                  )}>
                    <Badge variant={p.side === "LONG" ? "profit" : "loss"}>{p.side}</Badge>
                    <span className="text-[13px] font-bold">{p.symbol}</span>
                    <span className="text-[11px] text-muted-foreground font-mono hidden sm:inline">{p.strategy.replace(/_/g, " ")}</span>
                    <span className="text-[11px] text-muted-foreground font-mono">@{fmt(p.entry_price)}</span>
                    {rr && <Badge variant="secondary">R:R {fmt(rr, 1)}</Badge>}
                    <span className="text-[11px] text-muted-foreground ml-auto flex items-center gap-1"><Clock className="h-3 w-3" />{formatDuration(p.duration_mins)}</span>
                    {p.unrealized_pnl != null && (
                      <span className={cn("text-[12px] font-mono font-bold tabular-nums", pnl >= 0 ? "text-profit" : "text-loss")}>
                        {fmtPnl(pnl)} ({fmtPct(pnlPct)})
                      </span>
                    )}
                    {p.trailing_activated && <Badge variant="info">TSL</Badge>}
                    {p.breakeven_activated && <Badge variant="secondary">BE</Badge>}
                    {p.partial_taken && <Badge variant="profit">Partial</Badge>}
                  </div>
                );
              })}
            </CardContent>
          </Card>
        )}

        {/* Strategy breakdown */}
        {Object.keys(shared?.strategy_health ?? {}).length > 0 && (
          <Card>
            <CardHeader><CardTitle>Strategy Breakdown</CardTitle></CardHeader>
            <CardContent className="p-0">
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-border">
                      {["Strategy", "Trades", "Win Rate", "Total P&L", "Avg P&L", "Streak", "Size ×", "Status"].map((h) => (
                        <th key={h} className="text-left px-3 py-2.5 text-[10px] uppercase tracking-widest text-muted-foreground font-semibold whitespace-nowrap">{h}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/40">
                    {Object.values(shared?.strategy_health ?? {}).sort((a, b) => b.total_pnl - a.total_pnl).map((s) => (
                      <tr key={s.name} className={cn("hover:bg-secondary/40 transition-colors", !s.enabled && "opacity-50")}>
                        <td className="px-3 py-2.5 font-semibold">{s.name.replace(/_/g, " ")}</td>
                        <td className="px-3 py-2.5 font-mono tabular-nums">{s.total_trades}</td>
                        <td className={cn("px-3 py-2.5 font-mono tabular-nums font-bold", s.win_rate >= 0.5 ? "text-profit" : "text-loss")}>{fmt(s.win_rate * 100, 1)}%</td>
                        <td className={cn("px-3 py-2.5 font-mono tabular-nums font-bold", s.total_pnl >= 0 ? "text-profit" : "text-loss")}>{fmtPnl(s.total_pnl)}</td>
                        <td className={cn("px-3 py-2.5 font-mono tabular-nums", s.avg_pnl >= 0 ? "text-profit" : "text-loss")}>{fmtPnl(s.avg_pnl)}</td>
                        <td className={cn("px-3 py-2.5 font-mono tabular-nums", s.loss_streak >= 3 ? "text-warning" : "text-muted-foreground")}>{s.loss_streak}</td>
                        <td className="px-3 py-2.5 font-mono tabular-nums">×{fmt(s.size_multiplier, 2)}</td>
                        <td className="px-3 py-2.5"><Badge variant={s.enabled ? "profit" : "muted"}>{s.enabled ? "ON" : "OFF"}</Badge></td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </CardContent>
          </Card>
        )}

      </div>
    </div>
  );
}
