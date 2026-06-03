"use client";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  ResponsiveContainer,
  Tooltip,
  Cell,
} from "recharts";
import type { TradeEntry } from "@/lib/api";

interface Props {
  trades: TradeEntry[];
}

export function PnlBarChart({ trades }: Props) {
  const data = trades.slice(0, 30).reverse().map((t, i) => ({
    i,
    pnl: t.pnl_usd,
    symbol: t.symbol,
    is_win: t.is_win,
  }));

  if (!data.length) {
    return (
      <div className="flex items-center justify-center h-full text-xs text-muted-foreground">
        No trades yet
      </div>
    );
  }

  return (
    <ResponsiveContainer width="100%" height="100%">
      <BarChart data={data} margin={{ top: 4, right: 4, left: 0, bottom: 0 }}>
        <XAxis dataKey="i" hide />
        <YAxis hide domain={["auto", "auto"]} />
        <Tooltip
          content={({ active, payload }) => {
            if (!active || !payload?.[0]) return null;
            const d = payload[0].payload;
            return (
              <div className="rounded border border-border bg-popover px-2 py-1 text-xs">
                <span className="text-muted-foreground">{d.symbol} </span>
                <span className={d.pnl >= 0 ? "text-profit" : "text-loss"}>
                  {d.pnl >= 0 ? "+" : ""}${d.pnl.toFixed(2)}
                </span>
              </div>
            );
          }}
        />
        <Bar dataKey="pnl" radius={[2, 2, 0, 0]} isAnimationActive={false}>
          {data.map((entry, index) => (
            <Cell
              key={index}
              fill={entry.is_win ? "#22c55e" : "#ef4444"}
              fillOpacity={0.8}
            />
          ))}
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  );
}
