import { useState } from "react";
import { useListRuns } from "@workspace/api-client-react";
import { Layout } from "@/components/layout";
import { StatusBadge } from "@/pages/dashboard";
import { formatCurrency, formatPercent, formatNum } from "@/lib/format";
import { format } from "date-fns";
import { Link } from "wouter";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { Search } from "lucide-react";

export default function RunsList() {
  const { data: runs, isLoading } = useListRuns();
  const [search, setSearch] = useState("");

  const filteredRuns = runs?.filter(run => 
    run.symbol.toLowerCase().includes(search.toLowerCase()) || 
    run.strategy.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <Layout>
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-mono tracking-tight font-bold">All Backtest Runs</h1>
            <p className="text-sm text-muted-foreground">Historical research runs and simulations.</p>
          </div>
          <div className="relative w-64">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input 
              placeholder="Search symbol or strategy..." 
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-9 font-mono text-sm bg-card border-border rounded-none focus-visible:ring-primary"
            />
          </div>
        </div>

        <div className="border border-border bg-card">
          <Table>
            <TableHeader>
              <TableRow className="border-border hover:bg-transparent">
                <TableHead className="font-mono text-xs text-muted-foreground">Date</TableHead>
                <TableHead className="font-mono text-xs text-muted-foreground">Status</TableHead>
                <TableHead className="font-mono text-xs text-muted-foreground">Symbol</TableHead>
                <TableHead className="font-mono text-xs text-muted-foreground">Strategy</TableHead>
                <TableHead className="font-mono text-xs text-muted-foreground text-right">Trades</TableHead>
                <TableHead className="font-mono text-xs text-muted-foreground text-right">Win Rate</TableHead>
                <TableHead className="font-mono text-xs text-muted-foreground text-right">Sharpe</TableHead>
                <TableHead className="font-mono text-xs text-muted-foreground text-right">PnL</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                Array.from({ length: 5 }).map((_, i) => (
                  <TableRow key={i} className="border-border">
                    <TableCell colSpan={8} className="p-4">
                      <Skeleton className="h-6 w-full bg-border" />
                    </TableCell>
                  </TableRow>
                ))
              ) : filteredRuns?.length === 0 ? (
                <TableRow className="border-border hover:bg-transparent">
                  <TableCell colSpan={8} className="h-32 text-center text-muted-foreground font-mono text-sm">
                    No runs found.
                  </TableCell>
                </TableRow>
              ) : (
                filteredRuns?.map((run) => (
                  <TableRow key={run.id} className="border-border hover:bg-accent/50 cursor-pointer transition-colors group">
                    <TableCell className="font-mono text-xs">
                      <Link href={`/runs/${run.id}`} className="absolute inset-0 z-10" />
                      {format(new Date(run.createdAt), "yyyy-MM-dd")}
                    </TableCell>
                    <TableCell><StatusBadge status={run.status} /></TableCell>
                    <TableCell className="font-mono text-sm font-medium group-hover:text-primary">{run.symbol}</TableCell>
                    <TableCell className="font-mono text-sm">{run.strategy}</TableCell>
                    <TableCell className="font-mono text-sm text-right tabular-nums">{run.totalTrades ?? "-"}</TableCell>
                    <TableCell className="font-mono text-sm text-right tabular-nums">{formatPercent(run.winRate)}</TableCell>
                    <TableCell className="font-mono text-sm text-right tabular-nums">{formatNum(run.sharpeRatio)}</TableCell>
                    <TableCell className={`font-mono text-sm text-right tabular-nums font-medium ${run.totalPnl != null ? (run.totalPnl >= 0 ? "text-success" : "text-destructive") : ""}`}>
                      {formatCurrency(run.totalPnl)}
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </div>
      </div>
    </Layout>
  );
}
