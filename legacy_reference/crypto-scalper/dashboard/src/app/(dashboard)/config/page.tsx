"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useConfig, useStatus } from "@/hooks/useAriaData";
import { fmt } from "@/lib/api";
import { cn } from "@/lib/utils";
import { Settings, Shield, Zap, Clock, BarChart2, Info, AlertTriangle, CheckCircle2 } from "lucide-react";

function Row({ label, value, badge, sub, highlight }: {
  label: string; value: string; badge?: React.ReactNode; sub?: string; highlight?: "warn" | "danger" | "good";
}) {
  return (
    <div className={cn(
      "flex items-start justify-between py-2.5 border-b border-border/40 last:border-0 gap-3",
      highlight === "warn"   && "bg-warning/3",
      highlight === "danger" && "bg-loss/3",
      highlight === "good"   && "bg-profit/3",
    )}>
      <div>
        <span className="text-[11px] text-muted-foreground">{label}</span>
        {sub && <p className="text-[10px] text-muted-foreground/60 mt-0.5">{sub}</p>}
      </div>
      <div className="flex items-center gap-2 shrink-0">
        {badge}
        <span className="text-[12px] font-mono tabular-nums font-semibold text-right">{value}</span>
      </div>
    </div>
  );
}

function Section({ icon: Icon, title, children }: { icon: React.ElementType; title: string; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Icon className="h-3.5 w-3.5 text-muted-foreground" />
          <CardTitle>{title}</CardTitle>
        </div>
      </CardHeader>
      <CardContent className="pb-2">{children}</CardContent>
    </Card>
  );
}

export default function ConfigPage() {
  const { data: config, isLoading } = useConfig();
  const { data: status } = useStatus();

  const shared   = status?.shared;
  const metrics  = status?.metrics;
  const isLive   = config?.mode === "live";
  const isPaper  = config?.mode === "paper";

  return (
    <div className="flex flex-col h-full">
      <Header title="Runtime Config" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-3">

        {isLoading && (
          <div className="flex items-center justify-center py-20 text-sm text-muted-foreground">Loading config…</div>
        )}

        {config && (
          <>
            {/* Mode alert banner */}
            {isLive && (
              <div className="flex items-center gap-3 rounded-xl border border-profit/25 bg-profit/5 px-4 py-3">
                <CheckCircle2 className="h-4 w-4 text-profit shrink-0" />
                <div>
                  <p className="text-[12px] font-semibold text-profit">Live Trading Mode</p>
                  <p className="text-[11px] text-muted-foreground">Real orders are being placed on {config.exchange}. All risk limits are active.</p>
                </div>
              </div>
            )}
            {isPaper && (
              <div className="flex items-center gap-3 rounded-xl border border-info/25 bg-info/5 px-4 py-3">
                <Info className="h-4 w-4 text-info shrink-0" />
                <div>
                  <p className="text-[12px] font-semibold text-info">Paper Trading Mode</p>
                  <p className="text-[11px] text-muted-foreground">Simulated fills only. No real money at risk.</p>
                </div>
              </div>
            )}

            {/* Quick summary row */}
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              {[
                { label: "Mode",      value: config.mode,              color: isPaper ? "text-info" : isLive ? "text-profit" : "text-muted-foreground" },
                { label: "Exchange",  value: config.exchange,          color: "text-foreground" },
                { label: "Symbols",   value: String(config.symbol_count), color: "text-foreground" },
                { label: "Max Lev",   value: `${config.max_leverage}×`, color: config.max_leverage > 10 ? "text-warning" : "text-foreground" },
              ].map(({ label, value, color }) => (
                <Card key={label}>
                  <CardContent className="p-3">
                    <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-1">{label}</p>
                    <p className={cn("text-[15px] font-mono font-bold tabular-nums", color)}>{value}</p>
                  </CardContent>
                </Card>
              ))}
            </div>

            <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">

              <Section icon={Zap} title="Mode & Exchange">
                <Row label="Run Mode"    value={config.mode}
                  badge={<Badge variant={isPaper ? "info" : isLive ? "profit" : "muted"}>{config.mode}</Badge>} />
                <Row label="Exchange"    value={config.exchange} />
                <Row label="Symbols"     value={String(config.symbol_count)} sub="Total trading pairs configured" />
                <Row label="Max Leverage" value={`${config.max_leverage}×`}
                  highlight={config.max_leverage > 20 ? "danger" : config.max_leverage > 10 ? "warn" : "good"}
                  badge={config.max_leverage > 20 ? <AlertTriangle className="h-3 w-3 text-loss" /> : undefined} />
                <Row label="Metrics Bind" value={config.metrics_bind} sub="API server address" />
              </Section>

              <Section icon={Shield} title="Risk Parameters">
                <Row label="Risk Per Trade"     value={`${fmt(config.risk_per_trade_pct, 2)}%`}
                  sub="% of equity risked per trade"
                  highlight={config.risk_per_trade_pct > 2 ? "warn" : undefined} />
                <Row label="Max Drawdown"       value={`${fmt(config.max_drawdown_pct, 2)}%`}
                  sub="Triggers survival mode" />
                <Row label="Max Open Positions" value={String(config.max_open_positions)}
                  sub="Concurrent positions limit" />
                <Row label="Partial TP"         value={config.partial_tp_enabled ? "Enabled" : "Disabled"}
                  badge={<Badge variant={config.partial_tp_enabled ? "profit" : "muted"}>{config.partial_tp_enabled ? "ON" : "OFF"}</Badge>} />
                <Row label="Max Hold Time"      value={`${Math.floor(config.max_hold_secs / 60)}m (${config.max_hold_secs}s)`}
                  sub="Auto-close after this duration" />
              </Section>

              <Section icon={BarChart2} title="Active Strategies">
                <div className="flex flex-wrap gap-2 py-2">
                  {config.active_strategies.map((s) => {
                    const health = Object.values(shared?.strategy_health ?? {}).find(h => h.name === s);
                    return (
                      <div key={s} className="flex flex-col gap-1 rounded-lg border border-border bg-secondary/40 px-3 py-2 min-w-[140px]">
                        <div className="flex items-center gap-1.5">
                          {health?.enabled !== false
                            ? <CheckCircle2 className="h-3 w-3 text-profit shrink-0" />
                            : <AlertTriangle className="h-3 w-3 text-warning shrink-0" />}
                          <span className="text-[11px] font-semibold font-mono capitalize">{s.replace(/_/g, " ")}</span>
                        </div>
                        {health && (
                          <div className="text-[10px] text-muted-foreground">
                            {health.total_trades} trades · {fmt(health.win_rate * 100, 1)}% WR · ×{fmt(health.size_multiplier, 2)}
                          </div>
                        )}
                      </div>
                    );
                  })}
                  {config.active_strategies.length === 0 && (
                    <p className="text-[11px] text-muted-foreground">No strategies configured</p>
                  )}
                </div>
              </Section>

              <Section icon={Clock} title="Runtime Metrics">
                <Row label="Signals Today"      value={String(metrics?.signals_today ?? 0)} />
                <Row label="Trades Today"       value={String(metrics?.trades_today ?? 0)} />
                <Row label="LLM Go"             value={String(metrics?.llm_go ?? 0)}       />
                <Row label="LLM No-Go"          value={String(metrics?.llm_nogo ?? 0)}     />
                <Row label="LLM Wait"           value={String(metrics?.llm_wait ?? 0)}     />
                <Row label="LLM Avg Confidence" value={`${fmt(metrics?.llm_avg_confidence ?? 0, 1)}%`} />
                <Row label="LLM Avg Latency"    value={`${metrics?.llm_avg_latency_ms ?? 0}ms`}
                  highlight={(metrics?.llm_avg_latency_ms ?? 0) > 5000 ? "warn" : undefined} />
                <Row label="Offline Fallbacks"  value={String(metrics?.llm_offline_fallbacks ?? 0)}
                  highlight={(metrics?.llm_offline_fallbacks ?? 0) > 0 ? "warn" : undefined} />
                <Row label="Active Lessons"     value={String(metrics?.active_lessons ?? 0)} />
              </Section>

              <Section icon={Info} title="Survival Config">
                <Row label="Current Mode"    value={shared?.survival_mode ?? "—"} />
                <Row label="Survival Score"  value={fmt((shared?.survival_score ?? 0) * 100, 1)} />
                <Row label="Drawdown"        value={`${fmt(shared?.drawdown_pct ?? 0, 2)}%`}
                  highlight={(shared?.drawdown_pct ?? 0) > 5 ? "warn" : undefined} />
                <Row label="Current Regime"  value={shared?.current_regime ?? "—"} />
                <Row label="Open Positions"  value={String(shared?.open_positions ?? 0)} />
                <Row label="Total Equity"    value={`$${fmt(shared?.total_equity ?? 0)}`} />
              </Section>

              <Section icon={Settings} title="Full Config Snapshot">
                <div className="rounded-lg bg-secondary/40 p-3 overflow-x-auto">
                  <pre className="text-[10px] font-mono text-muted-foreground whitespace-pre-wrap break-all leading-relaxed">
{JSON.stringify({
  mode: config.mode,
  exchange: config.exchange,
  symbol_count: config.symbol_count,
  max_leverage: config.max_leverage,
  risk_per_trade_pct: config.risk_per_trade_pct,
  max_drawdown_pct: config.max_drawdown_pct,
  max_open_positions: config.max_open_positions,
  partial_tp_enabled: config.partial_tp_enabled,
  max_hold_secs: config.max_hold_secs,
  metrics_bind: config.metrics_bind,
  active_strategies: config.active_strategies,
}, null, 2)}
                  </pre>
                </div>
              </Section>

            </div>
          </>
        )}

      </div>
    </div>
  );
}
