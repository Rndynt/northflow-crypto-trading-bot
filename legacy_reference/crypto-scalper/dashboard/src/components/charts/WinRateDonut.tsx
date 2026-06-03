"use client";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";

interface Props {
  winRate: number;
  wins: number;
  losses: number;
}

export function WinRateDonut({ winRate, wins, losses }: Props) {
  const data = [
    { name: "Wins", value: wins || 0 },
    { name: "Losses", value: losses || 0 },
  ];

  const total = wins + losses;
  if (total === 0) {
    return (
      <div className="flex items-center justify-center h-full text-xs text-muted-foreground">
        No trades
      </div>
    );
  }

  return (
    <div className="relative h-full w-full">
      <ResponsiveContainer width="100%" height="100%">
        <PieChart>
          <Pie
            data={data}
            cx="50%"
            cy="50%"
            innerRadius="62%"
            outerRadius="82%"
            startAngle={90}
            endAngle={-270}
            dataKey="value"
            isAnimationActive={false}
            strokeWidth={0}
          >
            <Cell fill="#22c55e" fillOpacity={0.85} />
            <Cell fill="#ef4444" fillOpacity={0.6} />
          </Pie>
          <Tooltip
            content={({ active, payload }) => {
              if (!active || !payload?.[0]) return null;
              const d = payload[0];
              return (
                <div className="rounded border border-border bg-popover px-2 py-1 text-xs">
                  {d.name}: {d.value}
                </div>
              );
            }}
          />
        </PieChart>
      </ResponsiveContainer>
      <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
        <span className="text-lg font-bold font-mono tabular-nums leading-none">
          {(winRate * 100).toFixed(0)}%
        </span>
        <span className="text-[10px] text-muted-foreground mt-0.5">win rate</span>
      </div>
    </div>
  );
}
