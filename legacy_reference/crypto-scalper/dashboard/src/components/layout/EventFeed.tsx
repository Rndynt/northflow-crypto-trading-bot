"use client";
import { useState, useCallback } from "react";
import { useSse } from "@/hooks/useSse";
import type { ApiEvent } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ScrollArea } from "@/components/ui/scroll-area";

interface FeedItem {
  id: number;
  type: string;
  text: string;
  ts: number;
  color: string;
  dot: string;
}

let idSeq = 0;

const TYPE_CONFIG: Record<string, { color: string; dot: string; label: string }> = {
  signal:   { color: "text-info",             dot: "bg-info",             label: "SIG" },
  fill:     { color: "text-profit",           dot: "bg-profit",           label: "FILL" },
  close:    { color: "text-foreground",       dot: "bg-muted-foreground", label: "CLOSE" },
  partial:  { color: "text-warning",          dot: "bg-warning",          label: "PART" },
  sl_moved: { color: "text-muted-foreground", dot: "bg-muted-foreground", label: "SL" },
  survival: { color: "text-warning",          dot: "bg-warning",          label: "SRV" },
  equity:   { color: "text-muted-foreground", dot: "bg-muted-foreground", label: "EQ" },
  screening:{ color: "text-muted-foreground", dot: "bg-muted-foreground", label: "SCR" },
  error:    { color: "text-loss",             dot: "bg-loss",             label: "ERR" },
};

function describeEvent(event: ApiEvent): string {
  const d = event.data as Record<string, unknown>;
  switch (event.event_type) {
    case "signal":   return `${d.side} ${d.symbol} @${d.entry} · ${d.strategy} · ${d.ta_confidence}`;
    case "fill":     return `${d.side} ${d.symbol} ×${d.size} @${d.fill_price}`;
    case "close":    return `${d.side} ${d.symbol} · ${Number(d.pnl_usd) >= 0 ? "+" : ""}$${Number(d.pnl_usd ?? 0).toFixed(2)} · ${d.reason}`;
    case "partial":  return `${d.symbol} partial +$${Number(d.pnl_usd ?? 0).toFixed(2)}`;
    case "sl_moved": return `SL→${d.new_sl} ${d.symbol}`;
    case "survival": return `${d.mode} score=${Number(d.score ?? 0).toFixed(2)}`;
    case "equity":   return `$${Number(d.equity ?? 0).toFixed(2)}`;
    case "screening":return `${d.symbol} ${d.bias}`;
    case "error":    return String(d.reason ?? d.message ?? "unknown");
    default:         return JSON.stringify(d).slice(0, 50);
  }
}

export function EventFeed() {
  const [items, setItems] = useState<FeedItem[]>([]);

  const onEvent = useCallback((event: ApiEvent) => {
    const cfg = TYPE_CONFIG[event.event_type] ?? {
      color: "text-muted-foreground",
      dot: "bg-muted-foreground",
      label: "EVT",
    };
    setItems((prev) => [{
      id: idSeq++,
      type: event.event_type,
      text: describeEvent(event),
      ts: event.ts,
      color: cfg.color,
      dot: cfg.dot,
    }, ...prev].slice(0, 100));
  }, []);

  useSse("/aria-api/api/events", onEvent);

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <p className="text-[10px] font-bold uppercase tracking-widest text-muted-foreground">Live Events</p>
        <div className="flex items-center gap-1.5">
          <span className="inline-block h-1.5 w-1.5 rounded-full bg-profit animate-pulse-green" />
          <span className="text-[10px] text-muted-foreground font-mono">SSE</span>
        </div>
      </div>

      <ScrollArea className="flex-1">
        <div className="px-3 py-2 space-y-px">
          {items.length === 0 && (
            <p className="text-[11px] text-muted-foreground py-6 text-center">Waiting for events…</p>
          )}
          {items.map((item) => (
            <div
              key={item.id}
              className="flex items-start gap-2.5 py-1.5 animate-slide-in border-b border-border/30 last:border-0"
            >
              <span className="text-[10px] font-mono tabular-nums text-muted-foreground/50 shrink-0 pt-0.5 w-[42px]">
                {new Date(item.ts * 1000).toLocaleTimeString("en-US", {
                  hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false,
                })}
              </span>
              <div className="flex items-start gap-1.5 min-w-0">
                <span className={cn("mt-[5px] h-1.5 w-1.5 rounded-full shrink-0", item.dot)} />
                <span className={cn("text-[11px] font-mono leading-relaxed break-words min-w-0", item.color)}>
                  {item.text}
                </span>
              </div>
            </div>
          ))}
        </div>
      </ScrollArea>
    </div>
  );
}
