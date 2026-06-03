"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { usePositions, useStatus } from "@/hooks/useAriaData";
import { api, fmt, fmtPct, fmtPnl, formatDuration } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useState } from "react";
import {
  Clock, TrendingUp, AlertTriangle, Target, Layers,
  DollarSign, BarChart2, X, RefreshCw, CheckCircle2, XCircle,
} from "lucide-react";

function PriceLevelBar({ entry, sl, tp, current, side }: {
  entry: number; sl: number; tp: number; current?: number; side: string;
}) {
  if (!entry || !sl || !tp) return null;
  const isLong = side === "LONG";
  const low  = isLong ? sl   : tp;
  const high = isLong ? tp   : sl;
  const range = high - low;
  if (range <= 0) return null;

  const entryPct   = ((entry   - low) / range) * 100;
  const currentPct = current ? ((current - low) / range) * 100 : null;

  return (
    <div className="mt-4 space-y-1.5">
      <div className="flex justify-between text-[10px] text-muted-foreground">
        <span className="text-loss">{isLong ? "SL" : "TP"} {fmt(isLong ? sl : tp)}</span>
        <span className="text-muted-foreground">Entry {fmt(entry)}</span>
        <span className="text-profit">{isLong ? "TP" : "SL"} {fmt(isLong ? tp : sl)}</span>
      </div>
      <div className="relative h-3 w-full rounded-full overflow-hidden">
        <div className="absolute inset-0 bg-gradient-to-r from-loss/30 via-secondary to-profit/30 rounded-full" />
        <div className="absolute top-0 bottom-0 w-0.5 bg-foreground/60 rounded-full"
          style={{ left: `${Math.min(Math.max(entryPct, 0), 100)}%` }} />
        {currentPct != null && (
          <div className={cn("absolute top-0 bottom-0 w-1 rounded-full", currentPct >= entryPct ? "bg-profit" : "bg-loss")}
            style={{ left: `${Math.min(Math.max(currentPct, 0), 100)}%` }} />
        )}
      </div>
      {current && (
        <div className="flex justify-center text-[10px] font-mono">
          <span className="text-muted-foreground">Current: <span className="text-foreground font-bold">{fmt(current)}</span></span>
        </div>
      )}
    </div>
  );
}

function Toast({ msg, ok }: { msg: string; ok: boolean }) {
  return (
    <div className={cn(
      "fixed bottom-20 md:bottom-6 left-1/2 -translate-x-1/2 z-50 flex items-center gap-2 rounded-lg px-4 py-2.5 text-[13px] font-medium shadow-xl border",
      ok ? "bg-profit/15 text-profit border-profit/30" : "bg-destructive/15 text-destructive border-destructive/30"
    )}>
      {ok ? <CheckCircle2 className="h-4 w-4 shrink-0" /> : <XCircle className="h-4 w-4 shrink-0" />}
      {msg}
    </div>
  );
}

export default function PositionsPage() {
  const { data: positions, isLoading, mutate } = usePositions();
  const { data: status } = useStatus();
  const shared = status?.shared;
  const maxHoldSecs = status?.config?.max_hold_secs ?? 3600;

  const [closingSymbol, setClosingSymbol] = useState<string | null>(null);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);

  const totalUnrealized = positions?.reduce((s, p) => s + (p.unrealized_pnl ?? 0), 0) ?? 0;
  const totalPartial    = positions?.reduce((s, p) => s + (p.partial_realized_pnl ?? 0), 0) ?? 0;

  function showToast(msg: string, ok: boolean) {
    setToast({ msg, ok });
    setTimeout(() => setToast(null), 4000);
  }

  async function handleClose(symbol: string) {
    if (!confirm(`Close ${symbol} position at market? This cannot be undone.`)) return;
    setClosingSymbol(symbol);
    try {
      const res = await api.control.close(symbol);
      showToast(res.message, res.ok);
      setTimeout(() => mutate(), 1500);
    } catch (e) {
      showToast(String(e), false);
    } finally {
      setClosingSymbol(null);
    }
  }

  return (
    <div className="flex flex-col h-full">
      <Header title="Open Positions" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-3">

        {/* Summary bar */}
        {positions && positions.length > 0 && (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            {[
              { label: "Open Positions", value: String(positions.length), icon: Layers,     color: "text-info" },
              { label: "Unrealized P&L", value: fmtPnl(totalUnrealized),  icon: DollarSign, color: totalUnrealized >= 0 ? "text-profit" : "text-loss" },
              { label: "Partial Taken",  value: `+$${fmt(totalPartial)}`, icon: Target,     color: "text-profit" },
              { label: "Total Equity",   value: `$${fmt(shared?.total_equity ?? 0)}`, icon: BarChart2, color: "text-foreground" },
            ].map(({ label, value, icon: Icon, color }) => (
              <Card key={label}>
                <CardContent className="p-3 flex items-center gap-3">
                  <div className={cn("h-8 w-8 rounded-lg flex items-center justify-center shrink-0 bg-secondary/60")}>
                    <Icon className={cn("h-4 w-4", color)} />
                  </div>
                  <div>
                    <p className="text-[10px] uppercase tracking-widest text-muted-foreground">{label}</p>
                    <p className={cn("text-[14px] font-mono font-bold tabular-nums", color)}>{value}</p>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        )}

        {isLoading && (
          <div className="flex items-center justify-center py-20 text-sm text-muted-foreground">
            Loading positions…
          </div>
        )}

        {!isLoading && (!positions || positions.length === 0) && (
          <Card>
            <CardContent className="flex flex-col items-center justify-center py-20 gap-3">
              <div className="h-14 w-14 rounded-2xl bg-secondary flex items-center justify-center">
                <TrendingUp className="h-7 w-7 text-muted-foreground/50" />
              </div>
              <p className="text-sm font-medium text-muted-foreground">No open positions</p>
              <p className="text-[11px] text-muted-foreground/60">ARIA opens positions when signals align with risk rules</p>
            </CardContent>
          </Card>
        )}

        {positions?.map((p) => {
          const rr = p.entry_price > 0 && p.stop_loss > 0 && p.take_profit > 0
            ? Math.abs(p.take_profit - p.entry_price) / Math.abs(p.entry_price - p.stop_loss) : null;
          const holdPct    = (p.duration_mins * 60) / maxHoldSecs;
          const nearExpiry = holdPct > 0.8;
          const isLong     = p.side === "LONG";
          const pnl        = p.unrealized_pnl ?? 0;
          const pnlPct     = p.unrealized_pnl_pct ?? 0;
          const isClosing  = closingSymbol === p.symbol;

          const slDist = p.entry_price > 0 && p.stop_loss > 0
            ? Math.abs(((p.entry_price - p.stop_loss) / p.entry_price) * 100) : null;
          const tpDist = p.entry_price > 0 && p.take_profit > 0
            ? Math.abs(((p.take_profit - p.entry_price) / p.entry_price) * 100) : null;

          return (
            <Card key={p.client_id} className={cn(
              "border-l-4 transition-colors",
              isLong ? "border-l-profit/50 border-profit/20" : "border-l-loss/50 border-loss/20"
            )}>
              <CardContent className="p-4">
                {/* Header row */}
                <div className="flex items-start justify-between gap-3 flex-wrap mb-4">
                  <div className="flex items-center gap-3">
                    <Badge variant={isLong ? "profit" : "loss"} className="text-xs px-2.5 py-1 font-bold">
                      {p.side}
                    </Badge>
                    <div>
                      <p className="text-[17px] font-bold leading-tight">{p.symbol}</p>
                      <p className="text-[11px] text-muted-foreground">{p.strategy.replace(/_/g, " ")}</p>
                    </div>
                    <div className="hidden sm:flex flex-wrap gap-1.5">
                      {p.trailing_activated && <Badge variant="info" className="text-[10px]">Trailing SL</Badge>}
                      {p.breakeven_activated && <Badge variant="secondary" className="text-[10px]">Breakeven</Badge>}
                      {p.partial_taken && <Badge variant="profit" className="text-[10px]">Partial TP</Badge>}
                      {nearExpiry && <Badge variant="warning" className="text-[10px] flex items-center gap-1"><AlertTriangle className="h-2.5 w-2.5" />Near Expiry</Badge>}
                    </div>
                  </div>

                  <div className="flex items-start gap-3">
                    <div className="text-right">
                      {p.unrealized_pnl != null ? (
                        <>
                          <p className={cn("text-[20px] font-bold font-mono tabular-nums leading-tight", pnl >= 0 ? "text-profit" : "text-loss")}>
                            {fmtPnl(pnl)}
                          </p>
                          <p className={cn("text-[12px] font-mono tabular-nums", pnl >= 0 ? "text-profit/70" : "text-loss/70")}>
                            {fmtPct(pnlPct)}
                          </p>
                        </>
                      ) : (
                        <p className="text-[13px] text-muted-foreground">Updating…</p>
                      )}
                      <div className={cn("flex items-center justify-end gap-1 text-[11px] mt-1", nearExpiry ? "text-warning" : "text-muted-foreground")}>
                        {nearExpiry && <AlertTriangle className="h-3 w-3" />}
                        <Clock className="h-3 w-3" />
                        {formatDuration(p.duration_mins)}
                      </div>
                    </div>

                    {/* Close button */}
                    <button
                      onClick={() => handleClose(p.symbol)}
                      disabled={isClosing}
                      title={`Close ${p.symbol} at market`}
                      className={cn(
                        "flex items-center gap-1.5 rounded-lg border px-3 py-2 text-[12px] font-semibold transition-all",
                        "bg-destructive/10 text-destructive border-destructive/30 hover:bg-destructive/25",
                        "disabled:opacity-40 disabled:cursor-not-allowed"
                      )}
                    >
                      {isClosing
                        ? <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                        : <X className="h-3.5 w-3.5" />}
                      <span className="hidden sm:block">Close</span>
                    </button>
                  </div>
                </div>

                {/* Price level visualization */}
                <PriceLevelBar
                  entry={p.entry_price} sl={p.stop_loss} tp={p.take_profit}
                  current={p.current_price} side={p.side}
                />

                {/* Price grid */}
                <div className="mt-3 grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-6 gap-2">
                  {[
                    { label: "Entry Price",  value: fmt(p.entry_price),  color: "text-foreground" },
                    { label: "Stop Loss",    value: fmt(p.stop_loss),    color: "text-loss" },
                    { label: "Take Profit",  value: fmt(p.take_profit),  color: "text-profit" },
                    { label: "Size",         value: fmt(p.size, 4),      color: "text-foreground" },
                    { label: "SL Distance",  value: slDist != null ? `${fmt(slDist, 2)}%` : "—", color: "text-loss/80" },
                    { label: "TP Distance",  value: tpDist != null ? `${fmt(tpDist, 2)}%` : "—", color: "text-profit/80" },
                  ].map(({ label, value, color }) => (
                    <div key={label} className="rounded-lg bg-secondary/60 px-3 py-2">
                      <p className="text-[9px] text-muted-foreground uppercase tracking-wide mb-1">{label}</p>
                      <p className={cn("text-[12px] font-mono tabular-nums font-semibold", color)}>{value}</p>
                    </div>
                  ))}
                </div>

                {/* Footer row */}
                <div className="mt-3 flex flex-wrap items-center gap-2">
                  {rr != null && <Badge variant="secondary" className="font-mono">R:R {fmt(rr, 2)}</Badge>}
                  {p.partial_taken && p.partial_realized_pnl > 0 && (
                    <Badge variant="profit" className="font-mono">Partial +${fmt(p.partial_realized_pnl)}</Badge>
                  )}
                  <div className="ml-auto flex items-center gap-2">
                    <span className="text-[10px] text-muted-foreground">Hold {fmt(holdPct * 100, 0)}%</span>
                    <div className="w-24 h-1.5 rounded-full bg-secondary overflow-hidden">
                      <div className={cn("h-full rounded-full transition-all", nearExpiry ? "bg-warning" : "bg-primary")}
                        style={{ width: `${Math.min(holdPct * 100, 100)}%` }} />
                    </div>
                  </div>
                  <span className="text-[10px] text-muted-foreground font-mono">ID: {p.signal_id.slice(0, 8)}</span>
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>

      {toast && <Toast msg={toast.msg} ok={toast.ok} />}
    </div>
  );
}
