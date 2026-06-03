"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useStatus, useConfig } from "@/hooks/useAriaData";
import { fmt, fmtPnl } from "@/lib/api";
import { cn } from "@/lib/utils";
import { BarChart2, TrendingUp, TrendingDown, Flame, CheckCircle2, XCircle, Zap } from "lucide-react";

function WinBar({ wins, losses }: { wins: number; losses: number }) {
  const total = wins + losses;
  if (total === 0) return <div className="h-2 w-full rounded-full bg-secondary" />;
  const winPct = (wins / total) * 100;
  return (
    <div className="h-2 w-full rounded-full bg-secondary overflow-hidden flex">
      <div className="bg-profit/70 h-full rounded-l-full transition-all" style={{ width: `${winPct}%` }} />
      <div className="bg-loss/50 h-full rounded-r-full transition-all" style={{ width: `${100 - winPct}%` }} />
    </div>
  );
}

function StatBox({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div className="rounded-lg bg-secondary/60 px-3 py-2.5">
      <p className="text-[10px] text-muted-foreground uppercase tracking-wide mb-1">{label}</p>
      <p className={cn("text-[13px] font-mono tabular-nums font-bold", color ?? "text-foreground")}>{value}</p>
    </div>
  );
}

export default function StrategiesPage() {
  const { data: status } = useStatus();
  const { data: config } = useConfig();

  const strategies  = Object.values(status?.shared?.strategy_health ?? {});
  const sorted      = [...strategies].sort((a, b) => b.total_pnl - a.total_pnl);
  const active      = config?.active_strategies ?? [];

  const totalTrades = strategies.reduce((s, x) => s + x.total_trades, 0);
  const totalPnl    = strategies.reduce((s, x) => s + x.total_pnl, 0);
  const totalWins   = strategies.reduce((s, x) => s + x.wins, 0);
  const totalLosses = strategies.reduce((s, x) => s + x.losses, 0);
  const overallWR   = totalTrades > 0 ? (totalWins / totalTrades) * 100 : 0;
  const bestStrategy   = sorted[0];
  const worstStrategy  = sorted[sorted.length - 1];
  const enabledCount   = strategies.filter(s => s.enabled).length;

  return (
    <div className="flex flex-col h-full">
      <Header title="Strategy Health" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-4">

        {/* Overview stats */}
        {strategies.length > 0 && (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            {[
              { label: "Active Strategies", value: `${enabledCount} / ${strategies.length}`, color: enabledCount > 0 ? "text-profit" : "text-muted-foreground" },
              { label: "Total Trades",      value: String(totalTrades),                       color: "text-foreground" },
              { label: "Overall Win Rate",  value: `${fmt(overallWR, 1)}%`,                   color: overallWR >= 50 ? "text-profit" : "text-loss" },
              { label: "Combined P&L",      value: fmtPnl(totalPnl),                         color: totalPnl >= 0 ? "text-profit" : "text-loss" },
            ].map(({ label, value, color }) => (
              <Card key={label}>
                <CardContent className="p-3">
                  <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-1">{label}</p>
                  <p className={cn("text-[15px] font-mono font-bold tabular-nums", color)}>{value}</p>
                </CardContent>
              </Card>
            ))}
          </div>
        )}

        {/* Active strategies list from config */}
        {active.length > 0 && (
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <Zap className="h-3.5 w-3.5 text-primary" />
                <CardTitle>Configured Active Strategies</CardTitle>
              </div>
            </CardHeader>
            <CardContent>
              <div className="flex flex-wrap gap-2">
                {active.map((s) => {
                  const health = strategies.find(h => h.name === s || h.name.includes(s));
                  return (
                    <Badge key={s} variant={health?.enabled !== false ? "profit" : "muted"} className="font-mono text-[11px] px-3 py-1">
                      {health?.enabled !== false
                        ? <CheckCircle2 className="h-3 w-3 mr-1" />
                        : <XCircle className="h-3 w-3 mr-1" />}
                      {s.replace(/_/g, " ")}
                    </Badge>
                  );
                })}
              </div>
            </CardContent>
          </Card>
        )}

        {/* Best / Worst callouts */}
        {bestStrategy && worstStrategy && bestStrategy.name !== worstStrategy.name && (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <Card className="border-profit/20 bg-profit/3">
              <CardContent className="p-4 flex items-center gap-3">
                <TrendingUp className="h-8 w-8 text-profit shrink-0" />
                <div>
                  <p className="text-[10px] text-muted-foreground uppercase tracking-wide">Best Strategy</p>
                  <p className="text-[14px] font-bold">{bestStrategy.name.replace(/_/g, " ")}</p>
                  <p className="text-[12px] font-mono text-profit font-bold">{fmtPnl(bestStrategy.total_pnl)} · {fmt(bestStrategy.win_rate * 100, 1)}% WR</p>
                </div>
              </CardContent>
            </Card>
            <Card className="border-loss/20 bg-loss/3">
              <CardContent className="p-4 flex items-center gap-3">
                <TrendingDown className="h-8 w-8 text-loss shrink-0" />
                <div>
                  <p className="text-[10px] text-muted-foreground uppercase tracking-wide">Worst Strategy</p>
                  <p className="text-[14px] font-bold">{worstStrategy.name.replace(/_/g, " ")}</p>
                  <p className="text-[12px] font-mono text-loss font-bold">{fmtPnl(worstStrategy.total_pnl)} · {fmt(worstStrategy.win_rate * 100, 1)}% WR</p>
                </div>
              </CardContent>
            </Card>
          </div>
        )}

        {strategies.length === 0 && (
          <Card>
            <CardContent className="flex flex-col items-center justify-center py-20 gap-3">
              <div className="h-12 w-12 rounded-2xl bg-secondary flex items-center justify-center">
                <BarChart2 className="h-6 w-6 text-muted-foreground/50" />
              </div>
              <p className="text-sm text-muted-foreground">No strategy data yet</p>
              <p className="text-[11px] text-muted-foreground/60">Run some trades to see strategy performance</p>
            </CardContent>
          </Card>
        )}

        {/* Strategy cards */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
          {sorted.map((s) => {
            const winRate = s.win_rate * 100;
            const pnlBarWidth = Math.min(Math.abs(s.total_pnl) / (Math.max(...strategies.map(x => Math.abs(x.total_pnl))) || 1) * 100, 100);

            return (
              <Card key={s.name} className={cn(
                "border transition-opacity",
                !s.enabled && "opacity-60 border-dashed",
                s.enabled && s.total_pnl > 0 && "border-profit/15",
                s.enabled && s.total_pnl < 0 && "border-loss/15",
              )}>
                <CardHeader className="pb-3">
                  <div className="flex items-center justify-between gap-2 flex-wrap">
                    <div className="flex items-center gap-2 flex-wrap">
                      <CardTitle className="capitalize">{s.name.replace(/_/g, " ")}</CardTitle>
                      {!s.enabled && <Badge variant="muted">Disabled</Badge>}
                      {s.enabled && s.size_multiplier < 1 && (
                        <Badge variant="warning">×{fmt(s.size_multiplier, 2)} size</Badge>
                      )}
                      {s.loss_streak >= 5 && <Badge variant="loss" className="flex items-center gap-1"><Flame className="h-2.5 w-2.5" />{s.loss_streak} streak</Badge>}
                    </div>
                    <Badge variant={s.total_pnl >= 0 ? "profit" : "loss"} className="font-mono text-[12px]">
                      {fmtPnl(s.total_pnl)}
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent className="space-y-4">
                  {/* Win Rate bar */}
                  <div>
                    <div className="flex justify-between text-[11px] mb-2">
                      <span className="text-muted-foreground">Win Rate</span>
                      <span className={cn("font-mono font-bold", winRate >= 50 ? "text-profit" : "text-loss")}>
                        {fmt(winRate, 1)}%
                      </span>
                    </div>
                    <WinBar wins={s.wins} losses={s.losses} />
                    <div className="flex justify-between text-[10px] mt-1">
                      <span className="text-profit font-semibold">{s.wins}W</span>
                      <span className="text-muted-foreground">{s.total_trades} trades</span>
                      <span className="text-loss font-semibold">{s.losses}L</span>
                    </div>
                  </div>

                  {/* Stats grid */}
                  <div className="grid grid-cols-3 gap-2">
                    <StatBox label="Total P&L"   value={fmtPnl(s.total_pnl)}   color={s.total_pnl >= 0 ? "text-profit" : "text-loss"} />
                    <StatBox label="Avg P&L"     value={fmtPnl(s.avg_pnl)}     color={s.avg_pnl >= 0 ? "text-profit" : "text-loss"} />
                    <StatBox label="Size ×"      value={`×${fmt(s.size_multiplier, 2)}`} color={s.size_multiplier >= 1 ? "text-foreground" : "text-warning"} />
                    <StatBox label="Trades"      value={String(s.total_trades)} />
                    <StatBox label="Loss Streak" value={String(s.loss_streak)}  color={s.loss_streak >= 5 ? "text-loss" : s.loss_streak >= 3 ? "text-warning" : "text-muted-foreground"} />
                    <StatBox label="Status"      value={s.enabled ? "ON" : "OFF"} color={s.enabled ? "text-profit" : "text-muted-foreground"} />
                  </div>

                  {/* P&L bar */}
                  <div className="space-y-1.5">
                    <p className="text-[10px] text-muted-foreground">Relative P&L</p>
                    <div className="flex items-center gap-2">
                      {s.total_pnl >= 0
                        ? <TrendingUp className="h-3.5 w-3.5 text-profit shrink-0" />
                        : <TrendingDown className="h-3.5 w-3.5 text-loss shrink-0" />}
                      <div className="flex-1 h-2 rounded-full bg-secondary overflow-hidden">
                        <div
                          className={cn("h-full rounded-full transition-all", s.total_pnl >= 0 ? "bg-profit" : "bg-loss")}
                          style={{ width: `${pnlBarWidth}%` }}
                        />
                      </div>
                    </div>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>

        {/* Comparison table */}
        {strategies.length > 1 && (
          <Card>
            <CardHeader><CardTitle>Strategy Comparison Table</CardTitle></CardHeader>
            <CardContent className="p-0">
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-border bg-secondary/20">
                      {["Strategy", "Trades", "W", "L", "Win Rate", "Total P&L", "Avg P&L", "Loss Streak", "Size ×", "Status"].map(h => (
                        <th key={h} className="text-left px-3 py-2.5 text-[10px] uppercase tracking-widest text-muted-foreground font-semibold whitespace-nowrap">{h}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/40">
                    {sorted.map((s) => (
                      <tr key={s.name} className={cn("hover:bg-secondary/40 transition-colors", !s.enabled && "opacity-50")}>
                        <td className="px-3 py-2.5 font-semibold capitalize">{s.name.replace(/_/g, " ")}</td>
                        <td className="px-3 py-2.5 font-mono tabular-nums">{s.total_trades}</td>
                        <td className="px-3 py-2.5 font-mono tabular-nums text-profit">{s.wins}</td>
                        <td className="px-3 py-2.5 font-mono tabular-nums text-loss">{s.losses}</td>
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
