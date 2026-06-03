"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useSurvival, useStatus, useTrades } from "@/hooks/useAriaData";
import { fmt, fmtPnl } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ShieldAlert, ShieldCheck, ShieldX, Flame, Snowflake, Clock, AlertTriangle, TrendingDown, Activity } from "lucide-react";

function GaugeBar({ value, max = 100, label, unit = "%", thresholds, inverted = false }: {
  value: number; max?: number; label: string; unit?: string;
  thresholds: { warn: number; danger: number }; inverted?: boolean;
}) {
  const pct = Math.min((value / max) * 100, 100);
  const isWarn   = value >= thresholds.warn;
  const isDanger = value >= thresholds.danger;
  const barColor  = inverted
    ? (isDanger ? "bg-profit" : isWarn ? "bg-warning" : "bg-loss")
    : (isDanger ? "bg-loss" : isWarn ? "bg-warning" : "bg-profit");
  const textColor = inverted
    ? (isDanger ? "text-profit" : isWarn ? "text-warning" : "text-loss")
    : (isDanger ? "text-loss" : isWarn ? "text-warning" : "text-profit");

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-[11px] text-muted-foreground">{label}</span>
        <span className={cn("text-[13px] font-mono font-bold tabular-nums", textColor)}>
          {fmt(value, 2)}{unit}
        </span>
      </div>
      <div className="h-2.5 w-full rounded-full bg-secondary overflow-hidden">
        <div className={cn("h-full rounded-full transition-all duration-500", barColor)} style={{ width: `${pct}%` }} />
      </div>
      <div className="flex justify-between text-[9px] text-muted-foreground/60">
        <span>0</span>
        <span className="text-warning">{thresholds.warn}{unit}</span>
        <span className="text-loss">{thresholds.danger}{unit}</span>
        <span>{max}{unit}</span>
      </div>
    </div>
  );
}

function ScoreRing({ score }: { score: number }) {
  const pct = Math.min(Math.max(score, 0), 100);
  const r = 38;
  const circ = 2 * Math.PI * r;
  const offset = circ - (pct / 100) * circ;
  const color = pct >= 70 ? "#16c784" : pct >= 40 ? "#f0a500" : "#ea3943";

  return (
    <div className="relative flex items-center justify-center h-28 w-28">
      <svg className="absolute inset-0 -rotate-90" width="112" height="112" viewBox="0 0 112 112">
        <circle cx="56" cy="56" r={r} fill="none" stroke="hsl(230 12% 14%)" strokeWidth="8" />
        <circle cx="56" cy="56" r={r} fill="none" stroke={color} strokeWidth="8"
          strokeDasharray={circ} strokeDashoffset={offset} strokeLinecap="round"
          style={{ transition: "stroke-dashoffset 0.6s ease" }} />
      </svg>
      <div className="text-center">
        <p className="text-[22px] font-bold font-mono tabular-nums" style={{ color }}>{fmt(pct, 0)}</p>
        <p className="text-[10px] text-muted-foreground">score</p>
      </div>
    </div>
  );
}

export default function SurvivalPage() {
  const { data: survival } = useSurvival();
  const { data: status }   = useStatus();
  const { data: trades }   = useTrades(1, 200);
  const shared = status?.shared;

  const mode      = survival?.mode ?? shared?.survival_mode ?? "nominal";
  const score     = (survival?.score ?? shared?.survival_score ?? 1) * 100;
  const drawdown  = survival?.drawdown_pct ?? shared?.drawdown_pct ?? 0;
  const isFrozen  = survival?.is_frozen ?? false;
  const lossStreak = survival?.loss_streak ?? 0;
  const dailyLoss  = survival?.daily_loss_pct ?? 0;
  const deathLine  = survival?.death_line_pct ?? 0;
  const autoFlat   = survival?.auto_flat_triggered ?? false;

  const equity        = shared?.equity ?? 0;
  const peakEquity    = shared?.peak_equity ?? 0;
  const initialEquity = shared?.initial_equity ?? equity;
  const distToPeak    = peakEquity > 0 ? ((peakEquity - equity) / peakEquity) * 100 : 0;
  const totalReturn   = initialEquity > 0 ? ((equity - initialEquity) / initialEquity) * 100 : 0;

  const closedTrades = trades?.items ?? [];
  const recentLosses = closedTrades.filter(t => !t.is_win).slice(0, 10);
  const recentWins   = closedTrades.filter(t => t.is_win).slice(0, 10);

  const modeConfig = {
    nominal:  { icon: ShieldCheck, color: "text-profit",          bg: "bg-profit/10",   border: "border-profit/20",  badge: "profit"   as const, label: "Nominal — Trading normally" },
    caution:  { icon: ShieldAlert, color: "text-warning",         bg: "bg-warning/10",  border: "border-warning/25", badge: "warning"  as const, label: "Caution — Reduced sizing" },
    danger:   { icon: ShieldAlert, color: "text-loss",            bg: "bg-loss/10",     border: "border-loss/30",    badge: "loss"     as const, label: "Danger — High risk state" },
    frozen:   { icon: ShieldX,     color: "text-loss",            bg: "bg-loss/10",     border: "border-loss/40",    badge: "loss"     as const, label: "Frozen — No new positions" },
    cooldown: { icon: Snowflake,   color: "text-info",            bg: "bg-info/10",     border: "border-info/20",    badge: "info"     as const, label: "Cooldown — Waiting to resume" },
    flat:     { icon: ShieldX,     color: "text-warning",         bg: "bg-warning/10",  border: "border-warning/25", badge: "warning"  as const, label: "Flat-All — All positions closed" },
  }[mode] ?? { icon: ShieldCheck, color: "text-muted-foreground", bg: "bg-secondary",   border: "border-border",     badge: "muted"    as const, label: mode };

  const Icon = modeConfig.icon;

  return (
    <div className="flex flex-col h-full">
      <Header title="Survival Monitor" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-4">

        {/* Hero */}
        <Card className={cn("border-2", modeConfig.border)}>
          <CardContent className="p-5">
            <div className="flex flex-wrap items-center gap-5">
              <ScoreRing score={score} />
              <div className="flex-1 min-w-0 space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <div className={cn("flex items-center justify-center h-9 w-9 rounded-xl shrink-0", modeConfig.bg)}>
                    <Icon className={cn("h-5 w-5", modeConfig.color)} />
                  </div>
                  <h2 className="text-[20px] font-bold">{modeConfig.label}</h2>
                  <Badge variant={modeConfig.badge} className="text-[11px]">{mode.toUpperCase()}</Badge>
                  {isFrozen && <Badge variant="loss" className="flex items-center gap-1 animate-pulse"><Snowflake className="h-3 w-3" />FROZEN</Badge>}
                  {autoFlat && <Badge variant="warning" className="flex items-center gap-1"><AlertTriangle className="h-3 w-3" />AUTO-FLAT</Badge>}
                </div>
                {survival?.cooldown_until && (
                  <p className="text-[12px] text-muted-foreground flex items-center gap-1.5">
                    <Clock className="h-3.5 w-3.5" />
                    Cooldown until <span className="font-mono text-foreground">{new Date(survival.cooldown_until).toLocaleTimeString()}</span>
                  </p>
                )}
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 mt-2">
                  {[
                    { label: "Drawdown",    value: `${fmt(drawdown, 2)}%`, color: drawdown > 5 ? "text-loss" : drawdown > 2 ? "text-warning" : "text-profit" },
                    { label: "Daily Loss",  value: `${fmt(dailyLoss, 2)}%`, color: dailyLoss > 2 ? "text-loss" : "text-muted-foreground" },
                    { label: "Loss Streak", value: String(lossStreak), color: lossStreak >= 5 ? "text-loss" : lossStreak >= 3 ? "text-warning" : "text-muted-foreground" },
                    { label: "Death Line",  value: `${fmt(deathLine * 100, 1)}%`, color: "text-loss" },
                  ].map(({ label, value, color }) => (
                    <div key={label} className="rounded-lg bg-secondary/60 px-3 py-2">
                      <p className="text-[10px] text-muted-foreground mb-0.5">{label}</p>
                      <p className={cn("text-[13px] font-mono font-bold tabular-nums", color)}>{value}</p>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Risk gauges + equity */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <Activity className="h-3.5 w-3.5 text-muted-foreground" />
                <CardTitle>Risk Gauges</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="space-y-5">
              <GaugeBar value={drawdown}   max={15}  label="Current Drawdown (%)"    thresholds={{ warn: 4, danger: 7 }} />
              <GaugeBar value={dailyLoss}  max={5}   label="Daily Loss (%)"          thresholds={{ warn: 1.5, danger: 2.5 }} />
              <GaugeBar value={lossStreak} max={10}  label="Loss Streak (trades)"    unit="" thresholds={{ warn: 3, danger: 5 }} />
              <GaugeBar
                value={Math.max(0, (1 - deathLine) * 100)}
                max={30}
                label="Distance to Death Line (%)"
                thresholds={{ warn: 10, danger: 20 }}
                inverted
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <TrendingDown className="h-3.5 w-3.5 text-muted-foreground" />
                <CardTitle>Equity & Drawdown</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              {[
                { label: "Current Equity",    value: `$${fmt(equity)}`,        color: "text-foreground" },
                { label: "Peak Equity",        value: `$${fmt(peakEquity)}`,   color: "text-profit" },
                { label: "Initial Equity",     value: `$${fmt(initialEquity)}`, color: "text-muted-foreground" },
                { label: "Realized Today",     value: fmtPnl(shared?.realized_pnl_today ?? 0), color: (shared?.realized_pnl_today ?? 0) >= 0 ? "text-profit" : "text-loss" },
                { label: "Unrealized P&L",     value: fmtPnl(shared?.unrealized_pnl ?? 0),     color: (shared?.unrealized_pnl ?? 0) >= 0 ? "text-profit" : "text-loss" },
                { label: "Total Return",       value: `${totalReturn >= 0 ? "+" : ""}${fmt(totalReturn, 2)}%`, color: totalReturn >= 0 ? "text-profit" : "text-loss" },
                { label: "From Peak",          value: `-${fmt(distToPeak, 2)}%`, color: distToPeak > 5 ? "text-loss" : distToPeak > 2 ? "text-warning" : "text-muted-foreground" },
              ].map(({ label, value, color }) => (
                <div key={label} className="flex items-center justify-between py-1 border-b border-border/30 last:border-0">
                  <span className="text-[11px] text-muted-foreground">{label}</span>
                  <span className={cn("text-[12px] font-mono tabular-nums font-bold", color)}>{value}</span>
                </div>
              ))}
            </CardContent>
          </Card>
        </div>

        {/* Streak analysis */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <Flame className="h-3.5 w-3.5 text-loss" />
                <CardTitle>Recent Losses</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="p-0">
              {recentLosses.length === 0 ? (
                <div className="py-8 text-center text-sm text-muted-foreground">No recent losses</div>
              ) : (
                <div className="divide-y divide-border/40">
                  {recentLosses.map((t) => (
                    <div key={t.signal_id} className="px-4 py-2.5 flex items-center gap-3 hover:bg-secondary/30 transition-colors">
                      <Badge variant="loss" className="shrink-0">{t.direction}</Badge>
                      <span className="font-bold text-[13px]">{t.symbol}</span>
                      <span className="text-[11px] text-muted-foreground hidden sm:block">{t.strategy.replace(/_/g, " ")}</span>
                      <span className="text-[12px] font-mono text-loss font-bold ml-auto">{fmtPnl(t.pnl_usd)}</span>
                      <span className="text-[11px] font-mono text-loss/70">{fmt(t.pnl_pct, 2)}%</span>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <ShieldCheck className="h-3.5 w-3.5 text-profit" />
                <CardTitle>Recent Wins</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="p-0">
              {recentWins.length === 0 ? (
                <div className="py-8 text-center text-sm text-muted-foreground">No recent wins</div>
              ) : (
                <div className="divide-y divide-border/40">
                  {recentWins.map((t) => (
                    <div key={t.signal_id} className="px-4 py-2.5 flex items-center gap-3 hover:bg-secondary/30 transition-colors">
                      <Badge variant="profit" className="shrink-0">{t.direction}</Badge>
                      <span className="font-bold text-[13px]">{t.symbol}</span>
                      <span className="text-[11px] text-muted-foreground hidden sm:block">{t.strategy.replace(/_/g, " ")}</span>
                      <span className="text-[12px] font-mono text-profit font-bold ml-auto">{fmtPnl(t.pnl_usd)}</span>
                      <span className="text-[11px] font-mono text-profit/70">+{fmt(t.pnl_pct, 2)}%</span>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </div>

      </div>
    </div>
  );
}
