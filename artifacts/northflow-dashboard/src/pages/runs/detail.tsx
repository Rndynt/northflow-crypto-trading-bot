import { useGetRun, useListRunTrades, useDeleteRun, getListRunsQueryKey } from "@workspace/api-client-react";
import { Layout } from "@/components/layout";
import { StatusBadge } from "@/pages/dashboard";
import { formatCurrency, formatPercent, formatNum } from "@/lib/format";
import { format } from "date-fns";
import { useRoute, useLocation } from "wouter";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { ArrowLeft, Trash2 } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useToast } from "@/hooks/use-toast";

export default function RunDetail() {
  const [, params] = useRoute("/runs/:id");
  const id = parseInt(params?.id || "0", 10);
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const { toast } = useToast();

  const { data: run, isLoading: isLoadingRun } = useGetRun(id, { query: { enabled: !!id, queryKey: [`/api/research/runs/${id}`] } });
  const { data: trades, isLoading: isLoadingTrades } = useListRunTrades(id, { query: { enabled: !!id, queryKey: [`/api/research/runs/${id}/trades`] } });
  const deleteMutation = useDeleteRun();

  const handleDelete = () => {
    if (confirm("Are you sure you want to delete this run?")) {
      deleteMutation.mutate({ id }, {
        onSuccess: () => {
          toast({ title: "Run deleted successfully" });
          queryClient.invalidateQueries({ queryKey: getListRunsQueryKey() });
          setLocation("/runs");
        },
        onError: () => {
          toast({ title: "Failed to delete run", variant: "destructive" });
        }
      });
    }
  };

  if (isLoadingRun) {
    return (
      <Layout>
        <div className="space-y-6">
          <Skeleton className="h-8 w-64 bg-border" />
          <Skeleton className="h-32 w-full bg-border" />
        </div>
      </Layout>
    );
  }

  if (!run) {
    return (
      <Layout>
        <div className="p-8 text-center font-mono text-muted-foreground">Run not found.</div>
      </Layout>
    );
  }

  return (
    <Layout>
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <Button variant="ghost" size="icon" onClick={() => setLocation("/runs")} className="h-8 w-8 text-muted-foreground hover:text-foreground">
              <ArrowLeft size={16} />
            </Button>
            <div>
              <div className="flex items-center gap-3">
                <h1 className="text-2xl font-mono tracking-tight font-bold">{run.symbol}</h1>
                <StatusBadge status={run.status} />
              </div>
              <p className="text-sm font-mono text-muted-foreground">{run.strategy} • {format(new Date(run.createdAt), "MMM dd, yyyy HH:mm")}</p>
            </div>
          </div>
          <Button variant="destructive" size="sm" className="rounded-none font-mono text-xs" onClick={handleDelete} disabled={deleteMutation.isPending}>
            <Trash2 size={14} className="mr-2" />
            Delete Run
          </Button>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-px bg-border border border-border">
          <MetricBox label="Total PnL" value={formatCurrency(run.totalPnl)} valueClassName={run.totalPnl != null ? (run.totalPnl >= 0 ? "text-success" : "text-destructive") : ""} />
          <MetricBox label="Win Rate" value={formatPercent(run.winRate)} />
          <MetricBox label="Trades" value={formatNum(run.totalTrades, 0)} />
          <MetricBox label="Sharpe" value={formatNum(run.sharpeRatio)} />
          <MetricBox label="Max Drawdown" value={run.maxDrawdown != null ? `${formatPercent(run.maxDrawdown)}` : "-"} valueClassName="text-destructive" />
          <MetricBox label="Dates" value={run.startDate ? `${format(new Date(run.startDate), "MM/dd/yy")} - ${run.endDate ? format(new Date(run.endDate), "MM/dd/yy") : "Now"}` : "-"} />
        </div>

        {run.notes && (
          <Card className="border-border bg-card rounded-none">
            <CardContent className="p-4 font-mono text-sm text-muted-foreground whitespace-pre-wrap">
              {run.notes}
            </CardContent>
          </Card>
        )}

        <div className="space-y-4">
          <h2 className="text-lg font-mono font-bold tracking-tight">Trade Ledger</h2>
          <div className="border border-border bg-card">
            <Table>
              <TableHeader>
                <TableRow className="border-border hover:bg-transparent">
                  <TableHead className="font-mono text-xs text-muted-foreground">Entry Time</TableHead>
                  <TableHead className="font-mono text-xs text-muted-foreground">Side</TableHead>
                  <TableHead className="font-mono text-xs text-muted-foreground text-right">Size</TableHead>
                  <TableHead className="font-mono text-xs text-muted-foreground text-right">Entry Px</TableHead>
                  <TableHead className="font-mono text-xs text-muted-foreground text-right">Exit Px</TableHead>
                  <TableHead className="font-mono text-xs text-muted-foreground text-right">Fees</TableHead>
                  <TableHead className="font-mono text-xs text-muted-foreground text-right">PnL</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoadingTrades ? (
                  <TableRow>
                    <TableCell colSpan={7} className="p-4"><Skeleton className="h-6 w-full bg-border" /></TableCell>
                  </TableRow>
                ) : trades?.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={7} className="h-32 text-center text-muted-foreground font-mono text-sm">No trades executed.</TableCell>
                  </TableRow>
                ) : (
                  trades?.map((trade) => (
                    <TableRow key={trade.id} className="border-border hover:bg-accent/50 font-mono text-sm tabular-nums">
                      <TableCell>{format(new Date(trade.entryTime), "MM/dd HH:mm")}</TableCell>
                      <TableCell>
                        <span className={trade.side === 'long' ? "text-success" : "text-destructive"}>
                          {trade.side.toUpperCase()}
                        </span>
                      </TableCell>
                      <TableCell className="text-right">{formatNum(trade.quantity, 4)}</TableCell>
                      <TableCell className="text-right">{formatCurrency(trade.entryPrice)}</TableCell>
                      <TableCell className="text-right">{formatCurrency(trade.exitPrice)}</TableCell>
                      <TableCell className="text-right text-muted-foreground">{formatCurrency(trade.fee)}</TableCell>
                      <TableCell className={`text-right font-medium ${trade.pnl >= 0 ? "text-success" : "text-destructive"}`}>
                        {formatCurrency(trade.pnl)}
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>
    </Layout>
  );
}

function MetricBox({ label, value, valueClassName = "text-foreground" }: { label: string; value: React.ReactNode; valueClassName?: string }) {
  return (
    <div className="bg-card p-4 flex flex-col justify-center">
      <div className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider mb-1">{label}</div>
      <div className={`font-mono text-lg ${valueClassName}`}>{value}</div>
    </div>
  );
}
