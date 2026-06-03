"use client";
import { useState, useMemo } from "react";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useTrades } from "@/hooks/useAriaData";
import { fmt, fmtPnl, timeAgo } from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  ChevronLeft, ChevronRight, History, TrendingUp, TrendingDown,
  Filter, ChevronDown, ChevronUp, Target, ArrowRight,
} from "lucide-react";
import type { TradeEntry } from "@/lib/api";

const STRATEGY_COLORS: Record<string, "info" | "profit" | "warning" | "secondary"> = {
  ema_ribbon: "info", vwap_scalp: "profit", squeeze: "warning",
  mean_reversion: "secondary", momentum: "secondary", screened_vwap_scalp: "info",
  order_flow: "secondary",
};

function ConfBar({ value }: { value: number }) {
  const color = value >= 70 ? "bg-profit" : value >= 50 ? "bg-warning" : "bg-loss";
  return (
    <div className="flex items-center gap-1.5">
      <div className="w-12 h-1.5 rounded-full bg-secondary overflow-hidden">
        <div className={cn("h-full rounded-full", color)} style={{ width: `${(value / 100) * 100}%` }} />
      </div>
      <span className="text-[10px] font-mono tabular-nums w-5 text-right">{value}</span>
    </div>
  );
}

function RRBadge({ entry, sl, tp }: { entry: number; sl: number; tp: number }) {
  if (!entry || !sl || !tp) return null;
  const rr = Math.abs(tp - entry) / Math.abs(entry - sl);
  return (
    <Badge variant={rr >= 2 ? "profit" : rr >= 1.5 ? "warning" : "muted"} className="font-mono text-[10px]">
      R:R {fmt(rr, 2)}
    </Badge>
  );
}

function PriceLevelBar({ entry, sl, tp, exit, side }: {
  entry: number; sl: number; tp: number; exit: number; side: string;
}) {
  if (!entry || !sl || !tp) return null;
  const isLong = side === "LONG";
  const low  = isLong ? sl : tp;
  const high = isLong ? tp : sl;
  const range = high - low;
  if (range <= 0) return null;
  const clamp = (v: number) => Math.min(Math.max(((v - low) / range) * 100, 0), 100);
  const entryPct  = clamp(entry);
  const exitPct   = exit > 0 ? clamp(exit) : null;

  return (
    <div className="mt-2 space-y-1">
      <div className="flex justify-between text-[10px] text-muted-foreground">
        <span className="text-loss">{isLong ? "SL" : "TP"} {fmt(isLong ? sl : tp)}</span>
        <span className="text-muted-foreground">Entry {fmt(entry)}</span>
        <span className="text-profit">{isLong ? "TP" : "SL"} {fmt(isLong ? tp : sl)}</span>
      </div>
      <div className="relative h-2.5 w-full rounded-full overflow-hidden">
        <div className="absolute inset-0 bg-gradient-to-r from-loss/25 via-secondary to-profit/25 rounded-full" />
        <div className="absolute top-0 bottom-0 w-0.5 bg-foreground/50 rounded-full"
          style={{ left: `${entryPct}%` }} />
        {exitPct != null && (
          <div className={cn("absolute top-0 bottom-0 w-1 rounded-full shadow-sm",
            exitPct >= entryPct && isLong ? "bg-profit" :
            exitPct <= entryPct && !isLong ? "bg-profit" : "bg-loss"
          )}
            style={{ left: `${exitPct}%` }} />
        )}
      </div>
      {exit > 0 && (
        <div className="text-center text-[10px] font-mono text-muted-foreground">
          Exit: <span className="text-foreground font-semibold">{fmt(exit)}</span>
        </div>
      )}
    </div>
  );
}

function TradeDetail({ t }: { t: TradeEntry }) {
  if (!t) return null;
  const slDist = t.entry_price > 0 && t.stop_loss > 0
    ? Math.abs(((t.entry_price - t.stop_loss) / t.entry_price) * 100) : null;
  const tpDist = t.entry_price > 0 && t.take_profit > 0
    ? Math.abs(((t.take_profit - t.entry_price) / t.entry_price) * 100) : null;
  const exitDist = t.entry_price > 0 && t.exit_price > 0
    ? ((t.exit_price - t.entry_price) / t.entry_price) * 100 * (t.direction === "LONG" ? 1 : -1) : null;

  return (
    <tr className={cn("border-b border-border/60", t.is_win ? "bg-profit/4" : "bg-loss/4")}>
      <td colSpan={99} className="px-4 py-4">
        <div className="space-y-4 max-w-4xl">

          {/* Price visualization */}
          <PriceLevelBar
            entry={t.entry_price} sl={t.stop_loss} tp={t.take_profit}
            exit={t.exit_price} side={t.direction}
          />

          {/* Price grid */}
          <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-7 gap-2">
            {[
              { label: "Entry Price",  value: t.entry_price > 0 ? fmt(t.entry_price) : "—", color: "text-foreground",    bg: "" },
              { label: "Exit Price",   value: t.exit_price  > 0 ? fmt(t.exit_price)  : "—", color: t.is_win ? "text-profit" : "text-loss", bg: "" },
              { label: "Stop Loss",    value: t.stop_loss   > 0 ? fmt(t.stop_loss)   : "—", color: "text-loss",           bg: "bg-loss/5" },
              { label: "Take Profit",  value: t.take_profit > 0 ? fmt(t.take_profit) : "—", color: "text-profit",         bg: "bg-profit/5" },
              { label: "Size",         value: t.size        > 0 ? fmt(t.size, 4)     : "—", color: "text-foreground",     bg: "" },
              { label: "SL Distance",  value: slDist != null ? `${fmt(slDist, 3)}%`  : "—", color: "text-loss/80",         bg: "" },
              { label: "TP Distance",  value: tpDist != null ? `${fmt(tpDist, 3)}%`  : "—", color: "text-profit/80",       bg: "" },
            ].map(({ label, value, color, bg }) => (
              <div key={label} className={cn("rounded-lg px-3 py-2.5 border border-border/40", bg || "bg-secondary/60")}>
                <p className="text-[9px] text-muted-foreground uppercase tracking-wide mb-1">{label}</p>
                <p className={cn("text-[12px] font-mono tabular-nums font-bold", color)}>{value}</p>
              </div>
            ))}
          </div>

          {/* Price movement */}
          {t.entry_price > 0 && t.exit_price > 0 && (
            <div className="flex items-center gap-2 text-[11px] flex-wrap">
              <span className="text-muted-foreground">Price moved:</span>
              <span className="font-mono text-foreground">{fmt(t.entry_price)}</span>
              <ArrowRight className="h-3 w-3 text-muted-foreground" />
              <span className={cn("font-mono font-bold", t.is_win ? "text-profit" : "text-loss")}>{fmt(t.exit_price)}</span>
              {exitDist != null && (
                <Badge variant={exitDist >= 0 ? "profit" : "loss"} className="font-mono text-[10px]">
                  {exitDist >= 0 ? "+" : ""}{fmt(exitDist, 3)}%
                </Badge>
              )}
              <RRBadge entry={t.entry_price} sl={t.stop_loss} tp={t.take_profit} />
            </div>
          )}

          {/* Partial TP */}
          {t.partial_taken && (
            <div className="flex items-center gap-2 rounded-lg bg-profit/8 border border-profit/20 px-3 py-2">
              <Target className="h-3.5 w-3.5 text-profit shrink-0" />
              <span className="text-[12px] font-semibold text-profit">Partial TP Taken</span>
              {t.partial_realized_pnl > 0 && (
                <span className="text-[12px] font-mono text-profit ml-auto">+${fmt(t.partial_realized_pnl)}</span>
              )}
            </div>
          )}

          {/* Metadata */}
          <div className="flex flex-wrap gap-3 text-[10px] text-muted-foreground border-t border-border/30 pt-2">
            <span>ID: <span className="font-mono text-foreground">{t.signal_id}</span></span>
            <span>Opened: <span className="text-foreground">{new Date(t.entry_time).toLocaleString()}</span></span>
            <span>Closed: <span className="text-foreground">{new Date(t.exit_time).toLocaleString()}</span></span>
          </div>
        </div>
      </td>
    </tr>
  );
}

export default function TradesPage() {
  const [page, setPage]       = useState(1);
  const [filterDir, setFilterDir]     = useState<"ALL" | "LONG" | "SHORT">("ALL");
  const [filterResult, setFilterResult] = useState<"ALL" | "WIN" | "LOSS">("ALL");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const { data, isLoading } = useTrades(page, 100);

  const allTrades  = data?.items ?? [];
  const total      = data?.total ?? 0;
  const totalPages = Math.ceil(total / 100);

  const toggleRow = (id: string) => setExpanded(prev => {
    const next = new Set(prev);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });

  const trades = useMemo(() => allTrades.filter((t) => {
    if (filterDir !== "ALL" && t.direction !== filterDir) return false;
    if (filterResult === "WIN"  && !t.is_win) return false;
    if (filterResult === "LOSS" &&  t.is_win) return false;
    return true;
  }), [allTrades, filterDir, filterResult]);

  const wins         = trades.filter((t) => t.is_win).length;
  const losses       = trades.length - wins;
  const totalPnl     = trades.reduce((s, t) => s + t.pnl_usd, 0);
  const winRate      = trades.length > 0 ? (wins / trades.length) * 100 : 0;
  const grossProfit  = trades.filter(t => t.is_win).reduce((s, t) => s + t.pnl_usd, 0);
  const grossLoss    = Math.abs(trades.filter(t => !t.is_win).reduce((s, t) => s + t.pnl_usd, 0));
  const profitFactor = grossLoss > 0 ? grossProfit / grossLoss : grossProfit > 0 ? 999 : 0;
  const avgWin       = wins   > 0 ? grossProfit / wins   : 0;
  const avgLoss      = losses > 0 ? grossLoss   / losses : 0;

  const byStrategy = useMemo(() => {
    const map: Record<string, { wins: number; losses: number; pnl: number; trades: number }> = {};
    trades.forEach((t) => {
      if (!map[t.strategy]) map[t.strategy] = { wins: 0, losses: 0, pnl: 0, trades: 0 };
      map[t.strategy].trades++;
      map[t.strategy].pnl += t.pnl_usd;
      if (t.is_win) map[t.strategy].wins++; else map[t.strategy].losses++;
    });
    return Object.entries(map).sort((a, b) => b[1].pnl - a[1].pnl);
  }, [trades]);

  const byRegime = useMemo(() => {
    const map: Record<string, { wins: number; total: number; pnl: number }> = {};
    trades.forEach((t) => {
      if (!map[t.regime]) map[t.regime] = { wins: 0, total: 0, pnl: 0 };
      map[t.regime].total++;
      map[t.regime].pnl += t.pnl_usd;
      if (t.is_win) map[t.regime].wins++;
    });
    return Object.entries(map).sort((a, b) => b[1].total - a[1].total);
  }, [trades]);

  return (
    <div className="flex flex-col h-full">
      <Header title="Trade History" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-4">

        {/* Stats bar */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
          {[
            { label: "Total",     value: String(total),                                    color: "" },
            { label: "Win Rate",  value: `${fmt(winRate, 1)}%`,                            color: winRate >= 50 ? "text-profit" : "text-loss" },
            { label: "W / L",     value: `${wins} / ${losses}`,                           color: "" },
            { label: "Net P&L",   value: `${totalPnl >= 0 ? "+" : ""}$${fmt(totalPnl)}`,  color: totalPnl >= 0 ? "text-profit" : "text-loss" },
            { label: "P.Factor",  value: profitFactor > 100 ? "∞" : fmt(profitFactor),    color: profitFactor >= 1.5 ? "text-profit" : profitFactor >= 1 ? "text-warning" : "text-loss" },
            { label: "Avg Win",   value: `+$${fmt(avgWin)}`,                              color: "text-profit" },
            { label: "Avg Loss",  value: `-$${fmt(avgLoss)}`,                             color: "text-loss" },
            { label: "Gross P",   value: `+$${fmt(grossProfit)}`,                         color: "text-profit" },
          ].map(({ label, value, color }) => (
            <Card key={label}>
              <CardContent className="p-3">
                <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-1">{label}</p>
                <p className={cn("text-[13px] font-mono font-bold tabular-nums", color || "text-foreground")}>{value}</p>
              </CardContent>
            </Card>
          ))}
        </div>

        {/* Filters */}
        <div className="flex items-center gap-2 flex-wrap">
          <Filter className="h-3.5 w-3.5 text-muted-foreground" />
          {(["ALL", "LONG", "SHORT"] as const).map((d) => (
            <button key={d} onClick={() => setFilterDir(d)}
              className={cn("px-2.5 py-1 rounded-md text-[11px] font-semibold transition-colors",
                filterDir === d
                  ? d === "LONG" ? "bg-profit/20 text-profit" : d === "SHORT" ? "bg-loss/20 text-loss" : "bg-primary/20 text-primary"
                  : "text-muted-foreground hover:bg-secondary")}>
              {d}
            </button>
          ))}
          <div className="w-px h-4 bg-border mx-1" />
          {(["ALL", "WIN", "LOSS"] as const).map((r) => (
            <button key={r} onClick={() => setFilterResult(r)}
              className={cn("px-2.5 py-1 rounded-md text-[11px] font-semibold transition-colors",
                filterResult === r
                  ? r === "WIN" ? "bg-profit/20 text-profit" : r === "LOSS" ? "bg-loss/20 text-loss" : "bg-primary/20 text-primary"
                  : "text-muted-foreground hover:bg-secondary")}>
              {r}
            </button>
          ))}
          <span className="ml-auto text-[10px] text-muted-foreground">
            Click any row to see entry / SL / TP / exit details
          </span>
        </div>

        {/* Strategy + Regime */}
        {byStrategy.length > 0 && (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
            <Card>
              <CardHeader><CardTitle>By Strategy</CardTitle></CardHeader>
              <CardContent className="space-y-2.5">
                {byStrategy.map(([name, s]) => {
                  const wr = s.trades > 0 ? (s.wins / s.trades) * 100 : 0;
                  return (
                    <div key={name} className="space-y-1">
                      <div className="flex items-center justify-between text-[11px]">
                        <div className="flex items-center gap-2">
                          <Badge variant={STRATEGY_COLORS[name] ?? "secondary"} className="text-[10px]">{name.replace(/_/g, " ")}</Badge>
                          <span className="text-muted-foreground">{s.trades}t</span>
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="font-mono">{fmt(wr, 1)}%</span>
                          <span className={cn("font-mono font-bold", s.pnl >= 0 ? "text-profit" : "text-loss")}>{fmtPnl(s.pnl)}</span>
                        </div>
                      </div>
                      <div className="h-1.5 rounded-full bg-secondary overflow-hidden flex">
                        <div className="bg-profit/60 h-full rounded-l-full" style={{ width: `${wr}%` }} />
                        <div className="bg-loss/40 h-full rounded-r-full" style={{ width: `${100 - wr}%` }} />
                      </div>
                    </div>
                  );
                })}
              </CardContent>
            </Card>
            <Card>
              <CardHeader><CardTitle>By Market Regime</CardTitle></CardHeader>
              <CardContent className="space-y-2.5">
                {byRegime.map(([regime, s]) => {
                  const wr = s.total > 0 ? (s.wins / s.total) * 100 : 0;
                  return (
                    <div key={regime} className="space-y-1">
                      <div className="flex items-center justify-between text-[11px]">
                        <div className="flex items-center gap-2">
                          <Badge variant="muted" className="text-[10px]">{regime || "unknown"}</Badge>
                          <span className="text-muted-foreground">{s.total}t</span>
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="font-mono">{fmt(wr, 1)}%</span>
                          <span className={cn("font-mono font-bold", s.pnl >= 0 ? "text-profit" : "text-loss")}>{fmtPnl(s.pnl)}</span>
                        </div>
                      </div>
                      <div className="h-1.5 rounded-full bg-secondary overflow-hidden flex">
                        <div className="bg-profit/60 h-full rounded-l-full" style={{ width: `${wr}%` }} />
                        <div className="bg-loss/40 h-full rounded-r-full" style={{ width: `${100 - wr}%` }} />
                      </div>
                    </div>
                  );
                })}
              </CardContent>
            </Card>
          </div>
        )}

        {/* Trade table */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between gap-2 flex-wrap">
              <CardTitle>Closed Trades <span className="text-muted-foreground text-[12px] font-normal">(↓ click row for details)</span></CardTitle>
              {totalPages > 1 && (
                <div className="flex items-center gap-2">
                  <span className="text-[11px] text-muted-foreground">Page {page}/{totalPages}</span>
                  <button onClick={() => setPage(p => Math.max(1, p - 1))} disabled={page === 1}
                    className="h-7 w-7 rounded-lg flex items-center justify-center border border-border hover:bg-secondary disabled:opacity-40 transition-colors">
                    <ChevronLeft className="h-3.5 w-3.5" />
                  </button>
                  <button onClick={() => setPage(p => Math.min(totalPages, p + 1))} disabled={page === totalPages}
                    className="h-7 w-7 rounded-lg flex items-center justify-center border border-border hover:bg-secondary disabled:opacity-40 transition-colors">
                    <ChevronRight className="h-3.5 w-3.5" />
                  </button>
                </div>
              )}
            </div>
          </CardHeader>
          <CardContent className="p-0">
            {isLoading && (
              <div className="flex items-center justify-center py-16 text-sm text-muted-foreground">Loading…</div>
            )}
            {!isLoading && trades.length === 0 && (
              <div className="flex flex-col items-center justify-center py-16 gap-3">
                <div className="h-12 w-12 rounded-2xl bg-secondary flex items-center justify-center">
                  <History className="h-6 w-6 text-muted-foreground/50" />
                </div>
                <p className="text-sm text-muted-foreground">No trades match filter</p>
              </div>
            )}
            {trades.length > 0 && (
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-border bg-secondary/20">
                      {[
                        { h: "#",       cls: "" },
                        { h: "Dir",     cls: "" },
                        { h: "Symbol",  cls: "" },
                        { h: "Strategy",cls: "hidden sm:table-cell" },
                        { h: "Regime",  cls: "hidden lg:table-cell" },
                        { h: "Entry",   cls: "hidden md:table-cell" },
                        { h: "Exit",    cls: "hidden md:table-cell" },
                        { h: "SL",      cls: "hidden lg:table-cell" },
                        { h: "TP",      cls: "hidden lg:table-cell" },
                        { h: "Size",    cls: "hidden xl:table-cell" },
                        { h: "P&L ($)", cls: "" },
                        { h: "P&L (%)", cls: "" },
                        { h: "Partial", cls: "hidden sm:table-cell" },
                        { h: "TA",      cls: "hidden xl:table-cell" },
                        { h: "LLM",     cls: "hidden xl:table-cell" },
                        { h: "Closed",  cls: "hidden sm:table-cell" },
                        { h: "",        cls: "" },
                      ].map(({ h, cls }) => (
                        <th key={h} className={cn("text-left px-3 py-2.5 text-[10px] uppercase tracking-widest text-muted-foreground font-semibold whitespace-nowrap", cls)}>{h}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {trades.map((t, i) => {
                      const isOpen = expanded.has(t.signal_id);
                      return (
                        <>
                          <tr
                            key={t.signal_id}
                            onClick={() => toggleRow(t.signal_id)}
                            className={cn(
                              "border-b border-border/40 cursor-pointer transition-colors select-none",
                              isOpen ? "bg-secondary/60" : t.is_win ? "hover:bg-profit/5" : "hover:bg-loss/5"
                            )}
                          >
                            <td className="px-3 py-2.5 text-muted-foreground font-mono">{(page - 1) * 100 + i + 1}</td>
                            <td className="px-3 py-2.5">
                              <div className="flex items-center gap-1">
                                {t.direction === "LONG"
                                  ? <TrendingUp className="h-3 w-3 text-profit" />
                                  : <TrendingDown className="h-3 w-3 text-loss" />}
                                <Badge variant={t.direction === "LONG" ? "profit" : "loss"} className="text-[9px] px-1.5 py-0">{t.direction}</Badge>
                              </div>
                            </td>
                            <td className="px-3 py-2.5 font-bold text-[13px] whitespace-nowrap">{t.symbol}</td>
                            <td className="px-3 py-2.5 hidden sm:table-cell">
                              <Badge variant={STRATEGY_COLORS[t.strategy] ?? "secondary"} className="text-[9px]">
                                {t.strategy.replace(/_/g, " ")}
                              </Badge>
                            </td>
                            <td className="px-3 py-2.5 text-muted-foreground hidden lg:table-cell text-[11px] whitespace-nowrap">{t.regime || "—"}</td>
                            <td className="px-3 py-2.5 font-mono tabular-nums text-[11px] hidden md:table-cell whitespace-nowrap">
                              {t.entry_price > 0 ? fmt(t.entry_price) : "—"}
                            </td>
                            <td className={cn("px-3 py-2.5 font-mono tabular-nums text-[11px] hidden md:table-cell whitespace-nowrap font-semibold",
                              t.is_win ? "text-profit" : "text-loss")}>
                              {t.exit_price > 0 ? fmt(t.exit_price) : "—"}
                            </td>
                            <td className="px-3 py-2.5 font-mono tabular-nums text-[11px] text-loss hidden lg:table-cell whitespace-nowrap">
                              {t.stop_loss > 0 ? fmt(t.stop_loss) : "—"}
                            </td>
                            <td className="px-3 py-2.5 font-mono tabular-nums text-[11px] text-profit hidden lg:table-cell whitespace-nowrap">
                              {t.take_profit > 0 ? fmt(t.take_profit) : "—"}
                            </td>
                            <td className="px-3 py-2.5 font-mono tabular-nums text-[11px] hidden xl:table-cell whitespace-nowrap">
                              {t.size > 0 ? fmt(t.size, 4) : "—"}
                            </td>
                            <td className={cn("px-3 py-2.5 font-mono tabular-nums font-bold whitespace-nowrap text-[12px]",
                              t.is_win ? "text-profit" : "text-loss")}>
                              {t.pnl_usd >= 0 ? "+" : ""}${fmt(t.pnl_usd)}
                            </td>
                            <td className={cn("px-3 py-2.5 font-mono tabular-nums whitespace-nowrap text-[11px]",
                              t.is_win ? "text-profit" : "text-loss")}>
                              {t.pnl_pct >= 0 ? "+" : ""}{fmt(t.pnl_pct, 3)}%
                            </td>
                            <td className="px-3 py-2.5 hidden sm:table-cell">
                              {t.partial_taken
                                ? <Badge variant="profit" className="text-[9px] flex items-center gap-0.5"><Target className="h-2.5 w-2.5" />Partial</Badge>
                                : <span className="text-muted-foreground/40">—</span>}
                            </td>
                            <td className="px-3 py-2.5 hidden xl:table-cell">
                              {t.ta_confidence != null ? <ConfBar value={t.ta_confidence} /> : <span className="text-muted-foreground">—</span>}
                            </td>
                            <td className="px-3 py-2.5 hidden xl:table-cell">
                              {t.llm_confidence != null ? <ConfBar value={t.llm_confidence} /> : <span className="text-muted-foreground">—</span>}
                            </td>
                            <td className="px-3 py-2.5 text-muted-foreground whitespace-nowrap hidden sm:table-cell text-[11px]">{timeAgo(t.exit_time)}</td>
                            <td className="px-3 py-2.5">
                              {isOpen
                                ? <ChevronUp className="h-3.5 w-3.5 text-muted-foreground" />
                                : <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />}
                            </td>
                          </tr>
                          {isOpen && <TradeDetail key={`detail-${t.signal_id}`} t={t} />}
                        </>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </CardContent>
        </Card>

      </div>
    </div>
  );
}
