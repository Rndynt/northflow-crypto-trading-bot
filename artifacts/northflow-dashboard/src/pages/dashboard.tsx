import { useGetResearchSummary, useListRuns } from "@workspace/api-client-react";
import { Layout } from "@/components/layout";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";
import { Link } from "wouter";
import { format } from "date-fns";
import { Activity, CheckCircle, XCircle, TrendingUp, TrendingDown, Clock } from "lucide-react";
import { formatNum, formatCurrency, formatPercent } from "@/lib/format";

export default function Dashboard() {
  const { data: summary, isLoading: isLoadingSummary } = useGetResearchSummary();
  const { data: runs, isLoading: isLoadingRuns } = useListRuns();

  const recentRuns = runs?.slice(0, 5) || [];

  return (
    <Layout>
      <div className="space-y-6">
        <div>
          <h1 className="text-2xl font-mono tracking-tight font-bold">Research Summary</h1>
          <p className="text-sm text-muted-foreground">Aggregated performance across all backtests.</p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          <StatCard
            title="Total Runs"
            value={summary?.totalRuns}
            isLoading={isLoadingSummary}
            icon={<Activity size={16} className="text-muted-foreground" />}
          />
          <StatCard
            title="Avg Win Rate"
            value={summary ? formatPercent(summary.avgWinRate) : undefined}
            isLoading={isLoadingSummary}
            icon={<CheckCircle size={16} className="text-muted-foreground" />}
            valueClassName={summary && summary.avgWinRate >= 0.5 ? "text-success" : "text-destructive"}
          />
          <StatCard
            title="Avg PnL"
            value={summary ? formatCurrency(summary.avgPnl) : undefined}
            isLoading={isLoadingSummary}
            icon={summary && summary.avgPnl >= 0 ? <TrendingUp size={16} className="text-success" /> : <TrendingDown size={16} className="text-destructive" />}
            valueClassName={summary && summary.avgPnl >= 0 ? "text-success" : "text-destructive"}
          />
          <StatCard
            title="Success Rate"
            value={summary && summary.totalRuns > 0 ? formatPercent(summary.completedRuns / summary.totalRuns) : undefined}
            isLoading={isLoadingSummary}
            icon={<CheckCircle size={16} className="text-muted-foreground" />}
          />
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          <Card className="lg:col-span-2 border-border bg-card">
            <CardHeader className="pb-3 border-b border-border">
              <CardTitle className="text-base font-mono flex items-center justify-between">
                <span>Recent Runs</span>
                <Link href="/runs">
                  <span className="text-xs text-primary hover:underline cursor-pointer">View All</span>
                </Link>
              </CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              {isLoadingRuns ? (
                <div className="p-6 space-y-4">
                  {[...Array(5)].map((_, i) => (
                    <Skeleton key={i} className="h-10 w-full bg-border" />
                  ))}
                </div>
              ) : recentRuns.length === 0 ? (
                <div className="p-8 text-center text-muted-foreground font-mono text-sm">
                  No backtest runs found.
                </div>
              ) : (
                <div className="divide-y divide-border">
                  {recentRuns.map((run) => (
                    <Link key={run.id} href={`/runs/${run.id}`}>
                      <div className="flex items-center justify-between p-4 hover:bg-accent/50 cursor-pointer transition-colors group">
                        <div className="flex items-center gap-4">
                          <StatusBadge status={run.status} />
                          <div>
                            <div className="font-mono text-sm group-hover:text-primary transition-colors">
                              {run.symbol}
                            </div>
                            <div className="text-xs text-muted-foreground">{run.strategy}</div>
                          </div>
                        </div>
                        <div className="flex items-center gap-8 text-right">
                          {run.totalPnl != null && (
                            <div>
                              <div className="text-xs text-muted-foreground font-mono">PnL</div>
                              <div className={`font-mono text-sm ${run.totalPnl >= 0 ? "text-success" : "text-destructive"}`}>
                                {formatCurrency(run.totalPnl)}
                              </div>
                            </div>
                          )}
                          <div>
                            <div className="text-xs text-muted-foreground font-mono">Date</div>
                            <div className="font-mono text-sm">{format(new Date(run.createdAt), "MMM dd HH:mm")}</div>
                          </div>
                        </div>
                      </div>
                    </Link>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>

          <Card className="border-border bg-card">
            <CardHeader className="pb-3 border-b border-border">
              <CardTitle className="text-base font-mono">Top Symbols</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              {isLoadingSummary ? (
                <div className="p-6 space-y-4">
                  {[...Array(3)].map((_, i) => (
                    <Skeleton key={i} className="h-8 w-full bg-border" />
                  ))}
                </div>
              ) : summary?.topSymbols.length === 0 ? (
                <div className="p-8 text-center text-muted-foreground font-mono text-sm">
                  Not enough data.
                </div>
              ) : (
                <div className="divide-y divide-border">
                  {summary?.topSymbols.map((symbol) => (
                    <div key={symbol} className="flex justify-between p-4 items-center">
                      <span className="font-mono text-sm">{symbol}</span>
                      <Activity size={14} className="text-primary" />
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      </div>
    </Layout>
  );
}

function StatCard({ title, value, isLoading, icon, valueClassName }: { title: string; value?: string | number; isLoading?: boolean; icon?: React.ReactNode; valueClassName?: string }) {
  return (
    <Card className="border-border bg-card rounded-none">
      <CardContent className="p-6">
        <div className="flex items-center justify-between mb-4">
          <span className="text-xs font-mono text-muted-foreground uppercase tracking-wider">{title}</span>
          {icon}
        </div>
        {isLoading ? (
          <Skeleton className="h-8 w-24 bg-border" />
        ) : (
          <div className={`text-2xl font-mono ${valueClassName || "text-foreground"}`}>
            {value !== undefined ? value : "-"}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

export function StatusBadge({ status }: { status: string }) {
  let badgeClass = "";
  switch (status) {
    case "completed": badgeClass = "bg-success/20 text-success border-success/30"; break;
    case "failed": badgeClass = "bg-destructive/20 text-destructive border-destructive/30"; break;
    case "running": badgeClass = "bg-warning/20 text-warning border-warning/30"; break;
    default: badgeClass = "bg-muted text-muted-foreground border-border"; break;
  }

  return (
    <Badge variant="outline" className={`font-mono text-[10px] uppercase rounded-none tracking-wider ${badgeClass}`}>
      {status}
    </Badge>
  );
}
