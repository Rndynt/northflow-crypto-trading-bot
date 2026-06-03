"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useScreening, useStatus } from "@/hooks/useAriaData";
import { timeAgo } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ScanSearch, TrendingUp, TrendingDown, Minus, RefreshCw } from "lucide-react";

function BiasIcon({ bias }: { bias: string }) {
  if (bias === "bullish") return <TrendingUp className="h-4 w-4 text-profit" />;
  if (bias === "bearish") return <TrendingDown className="h-4 w-4 text-loss" />;
  return <Minus className="h-4 w-4 text-muted-foreground" />;
}

function BiasBar({ allows_long, allows_short }: { allows_long: boolean; allows_short: boolean }) {
  return (
    <div className="flex gap-1">
      <div className={cn(
        "flex-1 h-2 rounded-l-full",
        allows_long ? "bg-profit" : "bg-secondary"
      )} />
      <div className={cn(
        "flex-1 h-2 rounded-r-full",
        allows_short ? "bg-loss" : "bg-secondary"
      )} />
    </div>
  );
}

export default function ScreeningPage() {
  const { data: biases, isLoading, mutate } = useScreening();
  const { data: status } = useStatus();

  const bullish  = biases?.filter(b => b.bias === "bullish").length ?? 0;
  const bearish  = biases?.filter(b => b.bias === "bearish").length ?? 0;
  const neutral  = biases?.filter(b => b.bias !== "bullish" && b.bias !== "bearish").length ?? 0;
  const bothOk   = biases?.filter(b => b.allows_long && b.allows_short).length ?? 0;
  const longOnly = biases?.filter(b => b.allows_long && !b.allows_short).length ?? 0;
  const shortOnly= biases?.filter(b => !b.allows_long && b.allows_short).length ?? 0;
  const blocked  = biases?.filter(b => !b.allows_long && !b.allows_short).length ?? 0;

  const sorted = biases ? [...biases].sort((a, b) => {
    const order = { bullish: 0, bearish: 1 };
    return (order[a.bias as keyof typeof order] ?? 2) - (order[b.bias as keyof typeof order] ?? 2) || a.symbol.localeCompare(b.symbol);
  }) : [];

  return (
    <div className="flex flex-col h-full">
      <Header title="HTF Screening" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-4">

        {/* Summary stats */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {[
            { label: "Bullish Bias",  value: bullish,   color: "text-profit", bg: "bg-profit/10" },
            { label: "Bearish Bias",  value: bearish,   color: "text-loss",   bg: "bg-loss/10" },
            { label: "Neutral",       value: neutral,   color: "text-muted-foreground", bg: "bg-secondary/60" },
            { label: "Blocked",       value: blocked,   color: "text-warning", bg: "bg-warning/10" },
          ].map(({ label, value, color, bg }) => (
            <Card key={label}>
              <CardContent className={cn("p-3 flex items-center gap-3 rounded-lg", bg)}>
                <div>
                  <p className="text-[10px] uppercase tracking-widest text-muted-foreground">{label}</p>
                  <p className={cn("text-[20px] font-mono font-bold tabular-nums", color)}>{value}</p>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        {/* Direction filter summary */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {[
            { label: "Both Allowed",  value: bothOk,    color: "text-profit" },
            { label: "Long Only",     value: longOnly,  color: "text-profit/70" },
            { label: "Short Only",    value: shortOnly, color: "text-loss/70" },
            { label: "No Trades",     value: blocked,   color: "text-warning" },
          ].map(({ label, value, color }) => (
            <Card key={label}>
              <CardContent className="p-3">
                <p className="text-[10px] text-muted-foreground uppercase tracking-widest">{label}</p>
                <p className={cn("text-[16px] font-mono font-bold tabular-nums mt-0.5", color)}>{value}</p>
              </CardContent>
            </Card>
          ))}
        </div>

        {/* Main table */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <ScanSearch className="h-4 w-4 text-muted-foreground" />
                <CardTitle>Per-Symbol Screening Bias</CardTitle>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-[11px] text-muted-foreground">{biases?.length ?? 0} symbols</span>
                <button
                  onClick={() => mutate()}
                  className="flex h-7 w-7 items-center justify-center rounded-lg text-muted-foreground hover:text-foreground hover:bg-secondary transition-colors"
                >
                  <RefreshCw className={cn("h-3.5 w-3.5", isLoading && "animate-spin")} />
                </button>
              </div>
            </div>
          </CardHeader>
          <CardContent className="p-0">
            {isLoading && (
              <div className="flex items-center justify-center py-16 text-sm text-muted-foreground">
                <RefreshCw className="h-4 w-4 animate-spin mr-2" /> Loading screening data…
              </div>
            )}
            {!isLoading && sorted.length === 0 && (
              <div className="flex flex-col items-center justify-center py-16 gap-3">
                <div className="h-14 w-14 rounded-2xl bg-secondary flex items-center justify-center">
                  <ScanSearch className="h-7 w-7 text-muted-foreground/40" />
                </div>
                <p className="text-sm font-medium text-muted-foreground">No screening data yet</p>
                <p className="text-[11px] text-muted-foreground/60">
                  HTF bias data appears here once the SignalAgent runs its screening pass
                </p>
              </div>
            )}
            {sorted.length > 0 && (
              <>
                {/* Desktop table */}
                <div className="hidden md:block overflow-x-auto">
                  <table className="w-full text-xs">
                    <thead>
                      <tr className="border-b border-border bg-secondary/20">
                        {["Symbol", "Bias", "Direction", "Allows Long", "Allows Short", "Long/Short Bar", "Updated"].map(h => (
                          <th key={h} className="text-left px-4 py-2.5 text-[10px] uppercase tracking-widest text-muted-foreground font-semibold whitespace-nowrap">{h}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-border/40">
                      {sorted.map(b => (
                        <tr key={b.symbol} className={cn(
                          "hover:bg-secondary/30 transition-colors",
                          !b.allows_long && !b.allows_short && "opacity-50"
                        )}>
                          <td className="px-4 py-3 font-bold text-[14px] font-mono">{b.symbol}</td>
                          <td className="px-4 py-3">
                            <div className="flex items-center gap-2">
                              <BiasIcon bias={b.bias} />
                              <Badge variant={
                                b.bias === "bullish" ? "profit" :
                                b.bias === "bearish" ? "loss" : "muted"
                              } className="capitalize">
                                {b.bias}
                              </Badge>
                            </div>
                          </td>
                          <td className="px-4 py-3">
                            {b.allows_long && b.allows_short && <Badge variant="profit">Both</Badge>}
                            {b.allows_long && !b.allows_short && <Badge variant="profit">Long Only</Badge>}
                            {!b.allows_long && b.allows_short && <Badge variant="loss">Short Only</Badge>}
                            {!b.allows_long && !b.allows_short && <Badge variant="warning">Blocked</Badge>}
                          </td>
                          <td className="px-4 py-3">
                            {b.allows_long
                              ? <span className="flex items-center gap-1 text-profit font-semibold"><TrendingUp className="h-3 w-3" /> YES</span>
                              : <span className="text-muted-foreground/40">—</span>}
                          </td>
                          <td className="px-4 py-3">
                            {b.allows_short
                              ? <span className="flex items-center gap-1 text-loss font-semibold"><TrendingDown className="h-3 w-3" /> YES</span>
                              : <span className="text-muted-foreground/40">—</span>}
                          </td>
                          <td className="px-4 py-3 w-32">
                            <BiasBar allows_long={b.allows_long} allows_short={b.allows_short} />
                            <div className="flex justify-between mt-0.5">
                              <span className="text-[9px] text-profit">L</span>
                              <span className="text-[9px] text-loss">S</span>
                            </div>
                          </td>
                          <td className="px-4 py-3 text-muted-foreground text-[11px] whitespace-nowrap">{timeAgo(b.ts)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>

                {/* Mobile cards */}
                <div className="md:hidden divide-y divide-border/40">
                  {sorted.map(b => (
                    <div key={b.symbol} className={cn(
                      "px-4 py-3 flex items-center gap-3",
                      b.bias === "bullish" ? "border-l-2 border-l-profit/40" :
                      b.bias === "bearish" ? "border-l-2 border-l-loss/40" :
                      "border-l-2 border-l-border"
                    )}>
                      <BiasIcon bias={b.bias} />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="font-bold text-[14px] font-mono">{b.symbol}</span>
                          <Badge variant={b.bias === "bullish" ? "profit" : b.bias === "bearish" ? "loss" : "muted"} className="capitalize text-[10px]">
                            {b.bias}
                          </Badge>
                        </div>
                        <div className="flex gap-2 mt-1">
                          {b.allows_long && <Badge variant="profit" className="text-[10px]">✓ Long</Badge>}
                          {b.allows_short && <Badge variant="loss" className="text-[10px]">✓ Short</Badge>}
                          {!b.allows_long && !b.allows_short && <Badge variant="warning" className="text-[10px]">Blocked</Badge>}
                        </div>
                      </div>
                      <div className="w-16">
                        <BiasBar allows_long={b.allows_long} allows_short={b.allows_short} />
                      </div>
                      <span className="text-[10px] text-muted-foreground shrink-0">{timeAgo(b.ts)}</span>
                    </div>
                  ))}
                </div>
              </>
            )}
          </CardContent>
        </Card>

        {/* Legend */}
        <Card>
          <CardContent className="p-4">
            <p className="text-[11px] font-semibold text-muted-foreground uppercase tracking-wide mb-2">How screening works</p>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-[11px] text-muted-foreground/80">
              <div>• <strong className="text-profit">Bullish bias</strong> — HTF candles show uptrend; long entries favored</div>
              <div>• <strong className="text-loss">Bearish bias</strong> — HTF candles show downtrend; short entries favored</div>
              <div>• <strong className="text-foreground">Both allowed</strong> — Signals in either direction may pass</div>
              <div>• <strong className="text-warning">Blocked</strong> — HTF is ranging/choppy; no new entries for this symbol</div>
            </div>
          </CardContent>
        </Card>

      </div>
    </div>
  );
}
