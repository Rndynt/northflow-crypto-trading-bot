"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useSignals, useStatus } from "@/hooks/useAriaData";
import { fmt, timeAgo } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useState } from "react";
import {
  Radio, TrendingUp, TrendingDown, Target, Zap,
  ChevronDown, ChevronUp, Brain, AlertCircle,
} from "lucide-react";

function ConfidenceBar({ value, label, color }: { value: number; label?: string; color?: string }) {
  const auto = value >= 70 ? "bg-profit" : value >= 55 ? "bg-warning" : "bg-loss";
  const textAuto = value >= 70 ? "text-profit" : value >= 55 ? "text-warning" : "text-loss";
  return (
    <div className="space-y-1">
      {label && <p className="text-[10px] text-muted-foreground">{label}</p>}
      <div className="flex items-center gap-2">
        <div className="flex-1 h-1.5 rounded-full bg-secondary overflow-hidden">
          <div className={cn("h-full rounded-full transition-all", color ?? auto)} style={{ width: `${value}%` }} />
        </div>
        <span className={cn("text-[11px] font-mono tabular-nums font-bold w-7 text-right", textAuto)}>{value}</span>
      </div>
    </div>
  );
}

function DecisionBadge({ decision }: { decision?: string }) {
  if (!decision) return null;
  const variants: Record<string, { variant: "profit" | "loss" | "warning" | "secondary"; label: string }> = {
    Go:   { variant: "profit",  label: "LLM GO ✓" },
    NoGo: { variant: "loss",    label: "LLM NO-GO ✗" },
    Wait: { variant: "warning", label: "LLM WAIT ⏸" },
  };
  const v = variants[decision] ?? { variant: "secondary" as const, label: decision };
  return <Badge variant={v.variant} className="font-bold text-[10px]">{v.label}</Badge>;
}

export default function SignalsPage() {
  const { data: signals, isLoading } = useSignals();
  const { data: status } = useStatus();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const metrics = status?.metrics;

  const totalSignals = signals?.length ?? 0;
  const longSignals  = signals?.filter(s => s.side === "LONG").length ?? 0;
  const shortSignals = signals?.filter(s => s.side === "SHORT").length ?? 0;
  const avgConf      = totalSignals > 0
    ? (signals?.reduce((s, sig) => s + sig.ta_confidence, 0) ?? 0) / totalSignals : 0;
  const highConf     = signals?.filter(s => s.ta_confidence >= 70).length ?? 0;
  const llmFallbacks = metrics?.llm_offline_fallbacks ?? 0;

  function toggle(id: string) {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <div className="flex flex-col h-full">
      <Header title="Signal Feed" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-4">

        {/* Signal stats */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {[
            { label: "Signals Buffered", value: String(totalSignals), color: "text-foreground" },
            { label: "Long Signals",     value: String(longSignals),  color: "text-profit" },
            { label: "Short Signals",    value: String(shortSignals), color: "text-loss" },
            { label: "High Conf (≥70)",  value: String(highConf),     color: "text-primary" },
          ].map(({ label, value, color }) => (
            <Card key={label}>
              <CardContent className="p-3">
                <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-1">{label}</p>
                <p className={cn("text-[16px] font-mono font-bold tabular-nums", color)}>{value}</p>
              </CardContent>
            </Card>
          ))}
        </div>

        {/* LLM metrics */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {[
            { label: "LLM Go",        value: String(metrics?.llm_go ?? 0),   color: "text-profit",   icon: Zap },
            { label: "LLM No-Go",     value: String(metrics?.llm_nogo ?? 0), color: "text-loss",     icon: Zap },
            { label: "LLM Wait",      value: String(metrics?.llm_wait ?? 0), color: "text-warning",  icon: Zap },
            { label: "LLM Fallbacks", value: String(llmFallbacks),           color: llmFallbacks > 0 ? "text-destructive" : "text-muted-foreground", icon: Brain },
          ].map(({ label, value, color, icon: Icon }) => (
            <Card key={label} className={cn(label === "LLM Fallbacks" && llmFallbacks > 0 ? "border-destructive/30" : "")}>
              <CardContent className="p-3 flex items-center gap-2">
                <Icon className={cn("h-3.5 w-3.5 shrink-0", color)} />
                <div>
                  <p className="text-[10px] text-muted-foreground">{label}</p>
                  <p className={cn("text-[14px] font-mono font-bold", color)}>{value}</p>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        {/* LLM offline warning */}
        {llmFallbacks > 0 && (
          <div className="flex items-center gap-3 rounded-lg bg-destructive/10 border border-destructive/30 px-4 py-3">
            <AlertCircle className="h-4 w-4 text-destructive shrink-0" />
            <div>
              <p className="text-[12px] font-semibold text-destructive">LLM is offline / fallback mode</p>
              <p className="text-[11px] text-destructive/70 mt-0.5">
                Brain agent failed {llmFallbacks} time(s). Signals are being evaluated by TA-only heuristics without LLM reasoning.
                Check OPENROUTER_API_KEY or ANTHROPIC_API_KEY environment variables.
              </p>
            </div>
          </div>
        )}

        {/* Signal list */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <Radio className="h-3.5 w-3.5 text-primary animate-pulse" />
              <CardTitle>Recent Signals</CardTitle>
              {signals && signals.length > 0 && (
                <span className="text-[10px] text-muted-foreground ml-auto">
                  {signals.length} signals · avg conf <span className="font-mono text-foreground">{fmt(avgConf, 1)}</span>
                </span>
              )}
            </div>
          </CardHeader>
          <CardContent className="p-0">
            {isLoading && (
              <div className="flex items-center justify-center py-12 text-sm text-muted-foreground">Loading…</div>
            )}
            {!isLoading && (!signals || signals.length === 0) && (
              <div className="flex flex-col items-center justify-center py-12 gap-3">
                <div className="h-12 w-12 rounded-2xl bg-secondary flex items-center justify-center">
                  <Radio className="h-6 w-6 text-muted-foreground/50" />
                </div>
                <p className="text-sm text-muted-foreground">No signals in buffer</p>
                <p className="text-[11px] text-muted-foreground/60">Signals appear here when strategies fire</p>
              </div>
            )}
            <div className="divide-y divide-border/40">
              {signals?.map((s) => {
                const isOpen = expanded.has(s.signal_id);
                const slDist = s.entry > 0 && s.stop_loss > 0
                  ? Math.abs(((s.entry - s.stop_loss) / s.entry) * 100) : null;
                const tpDist = s.entry > 0 && s.take_profit > 0
                  ? Math.abs(((s.take_profit - s.entry) / s.entry) * 100) : null;
                const rr = s.entry > 0 && s.stop_loss > 0 && s.take_profit > 0
                  ? Math.abs(s.take_profit - s.entry) / Math.abs(s.entry - s.stop_loss) : null;

                return (
                  <div key={s.signal_id} className={cn(
                    "transition-colors",
                    s.side === "LONG" ? "border-l-2 border-l-profit/30" : "border-l-2 border-l-loss/30"
                  )}>
                    {/* Clickable header row */}
                    <button
                      onClick={() => toggle(s.signal_id)}
                      className="w-full text-left px-4 py-3 hover:bg-secondary/30 transition-colors"
                    >
                      <div className="flex items-center gap-2 flex-wrap">
                        {s.side === "LONG"
                          ? <TrendingUp className="h-4 w-4 text-profit shrink-0" />
                          : <TrendingDown className="h-4 w-4 text-loss shrink-0" />}
                        <Badge variant={s.side === "LONG" ? "profit" : "loss"} className="font-bold text-[10px]">{s.side}</Badge>
                        <span className="font-bold text-[15px]">{s.symbol}</span>
                        <Badge variant="secondary" className="font-mono text-[10px]">{s.strategy.replace(/_/g, " ")}</Badge>
                        <Badge variant="muted" className="hidden sm:inline-flex text-[10px]">{s.regime}</Badge>
                        {s.llm_decision && <DecisionBadge decision={s.llm_decision} />}
                        {s.offline_fallback && (
                          <Badge variant="warning" className="text-[10px] flex items-center gap-1">
                            <Brain className="h-2.5 w-2.5" /> TA-Only
                          </Badge>
                        )}
                        <div className="flex items-center gap-1 ml-auto">
                          <span className="text-[11px] text-muted-foreground whitespace-nowrap">{timeAgo(s.ts)}</span>
                          {isOpen
                            ? <ChevronUp className="h-3.5 w-3.5 text-muted-foreground" />
                            : <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />}
                        </div>
                      </div>

                      {/* Compact summary row */}
                      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
                        <span>Entry <span className="font-mono text-foreground font-semibold">{fmt(s.entry)}</span></span>
                        <span>SL <span className="font-mono text-loss font-semibold">{fmt(s.stop_loss)}</span></span>
                        <span>TP <span className="font-mono text-profit font-semibold">{fmt(s.take_profit)}</span></span>
                        {rr != null && <span>R:R <span className={cn("font-mono font-semibold", rr >= 2 ? "text-profit" : rr >= 1.5 ? "text-warning" : "text-loss")}>{fmt(rr, 2)}</span></span>}
                        <span>TA <span className={cn("font-mono font-semibold", s.ta_confidence >= 70 ? "text-profit" : s.ta_confidence >= 55 ? "text-warning" : "text-loss")}>{s.ta_confidence}</span></span>
                        {s.llm_confidence != null && <span>LLM <span className="font-mono text-info font-semibold">{s.llm_confidence}</span></span>}
                      </div>
                    </button>

                    {/* Expanded detail */}
                    {isOpen && (
                      <div className="px-4 pb-4 space-y-3 bg-secondary/10 border-t border-border/30">
                        {/* Price levels grid */}
                        <div className="pt-3 grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-2">
                          {[
                            { label: "Entry",       value: fmt(s.entry),      color: "text-foreground" },
                            { label: "Stop Loss",   value: fmt(s.stop_loss),  color: "text-loss" },
                            { label: "Take Profit", value: fmt(s.take_profit),color: "text-profit" },
                            { label: "SL Distance", value: slDist != null ? `${fmt(slDist, 2)}%` : "—", color: "text-loss/80" },
                            { label: "TP Distance", value: tpDist != null ? `${fmt(tpDist, 2)}%` : "—", color: "text-profit/80" },
                            { label: "R:R",         value: rr != null ? fmt(rr, 2) : "—", color: rr != null && rr >= 2 ? "text-profit" : "text-warning" },
                          ].map(({ label, value, color }) => (
                            <div key={label} className="rounded-lg bg-card px-3 py-2 border border-border/40">
                              <p className="text-[9px] text-muted-foreground uppercase tracking-wide mb-0.5">{label}</p>
                              <p className={cn("text-[12px] font-mono tabular-nums font-semibold", color)}>{value}</p>
                            </div>
                          ))}
                        </div>

                        {/* Confidence bars */}
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 rounded-lg bg-card border border-border/40 p-3">
                          <ConfidenceBar value={s.ta_confidence} label="TA Confidence" />
                          {s.llm_confidence != null && (
                            <ConfidenceBar value={s.llm_confidence} label="LLM Confidence" color="bg-info" />
                          )}
                        </div>

                        {/* TA Reason */}
                        {s.reason && (
                          <div className="rounded-lg bg-card border border-border/40 px-3 py-2.5">
                            <p className="text-[9px] text-muted-foreground uppercase tracking-wide mb-1 flex items-center gap-1">
                              <Target className="h-2.5 w-2.5" /> TA Analysis Reason
                            </p>
                            <p className="text-[12px] text-foreground/85 leading-relaxed">{s.reason}</p>
                          </div>
                        )}

                        {/* LLM Summary / Response */}
                        {s.llm_summary && (
                          <div className="rounded-lg bg-info/5 border border-info/20 px-3 py-2.5">
                            <p className="text-[9px] text-info/70 uppercase tracking-wide mb-1 flex items-center gap-1">
                              <Brain className="h-2.5 w-2.5 text-info" /> LLM Brain Response
                              {s.offline_fallback && <span className="text-warning ml-2">(OFFLINE — TA fallback used)</span>}
                            </p>
                            <p className="text-[12px] text-foreground/85 leading-relaxed whitespace-pre-line">{s.llm_summary}</p>
                          </div>
                        )}

                        {/* LLM decision + no summary note */}
                        {s.llm_decision && !s.llm_summary && (
                          <div className="rounded-lg bg-card border border-border/40 px-3 py-2.5">
                            <p className="text-[9px] text-muted-foreground uppercase tracking-wide mb-1 flex items-center gap-1">
                              <Brain className="h-2.5 w-2.5" /> LLM Decision
                            </p>
                            <div className="flex items-center gap-2">
                              <DecisionBadge decision={s.llm_decision} />
                              {s.offline_fallback && (
                                <span className="text-[11px] text-warning">LLM offline — TA heuristic used</span>
                              )}
                            </div>
                          </div>
                        )}

                        {/* Indicators */}
                        {s.indicators && Object.keys(s.indicators).length > 0 && (
                          <div className="rounded-lg bg-card border border-border/40 px-3 py-2.5">
                            <p className="text-[9px] text-muted-foreground uppercase tracking-wide mb-2 flex items-center gap-1">
                              <Zap className="h-2.5 w-2.5" /> Technical Indicators
                            </p>
                            <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2">
                              {Object.entries(s.indicators).map(([k, v]) => (
                                <div key={k} className="rounded bg-secondary/60 px-2.5 py-1.5">
                                  <p className="text-[9px] text-muted-foreground uppercase tracking-wide">{k}</p>
                                  <p className="text-[12px] font-mono font-semibold text-foreground">
                                    {typeof v === "number" ? fmt(v, 4) : String(v)}
                                  </p>
                                </div>
                              ))}
                            </div>
                          </div>
                        )}

                        {/* Signal ID */}
                        <p className="text-[10px] text-muted-foreground/50 font-mono">ID: {s.signal_id}</p>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </CardContent>
        </Card>

      </div>
    </div>
  );
}
