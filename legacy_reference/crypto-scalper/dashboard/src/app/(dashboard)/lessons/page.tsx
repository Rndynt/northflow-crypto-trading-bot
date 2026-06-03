"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useLessons, useStatus } from "@/hooks/useAriaData";
import { cn } from "@/lib/utils";
import { fmt } from "@/lib/api";
import { BookOpen, Lightbulb, Brain, Star, Clock } from "lucide-react";

function ConfidenceRing({ value }: { value: number }) {
  const pct = Math.min(Math.max(value * 100, 0), 100);
  const r = 16;
  const circ = 2 * Math.PI * r;
  const offset = circ - (pct / 100) * circ;
  const color = pct >= 70 ? "#16c784" : pct >= 40 ? "#f0a500" : "#ea3943";
  return (
    <div className="relative flex items-center justify-center h-10 w-10 shrink-0">
      <svg className="absolute inset-0 -rotate-90" width="40" height="40" viewBox="0 0 40 40">
        <circle cx="20" cy="20" r={r} fill="none" stroke="hsl(230 12% 14%)" strokeWidth="4" />
        <circle cx="20" cy="20" r={r} fill="none" stroke={color} strokeWidth="4"
          strokeDasharray={circ} strokeDashoffset={offset} strokeLinecap="round" />
      </svg>
      <span className="text-[10px] font-mono font-bold" style={{ color }}>{Math.round(pct)}</span>
    </div>
  );
}

export default function LessonsPage() {
  const { data: lessons, isLoading } = useLessons();
  const { data: status } = useStatus();
  const metrics = status?.metrics;

  const totalLessons = lessons?.length ?? 0;
  const highConfLessons = lessons?.filter(l => (l.confidence ?? 0) >= 0.7).length ?? 0;
  const medConfLessons  = lessons?.filter(l => (l.confidence ?? 0) >= 0.4 && (l.confidence ?? 0) < 0.7).length ?? 0;
  const lowConfLessons  = lessons?.filter(l => (l.confidence ?? 0) < 0.4).length ?? 0;
  const avgConf = totalLessons > 0
    ? (lessons?.reduce((s, l) => s + (l.confidence ?? 0), 0) ?? 0) / totalLessons : 0;

  const sorted = [...(lessons ?? [])].sort((a, b) => (b.confidence ?? 0) - (a.confidence ?? 0));

  return (
    <div className="flex flex-col h-full">
      <Header title="AI Lessons" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-4">

        {/* Stats */}
        {!isLoading && totalLessons > 0 && (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            {[
              { label: "Total Lessons",   value: String(totalLessons),   color: "text-foreground" },
              { label: "High Confidence", value: String(highConfLessons), color: "text-profit" },
              { label: "Medium Conf",     value: String(medConfLessons),  color: "text-warning" },
              { label: "Avg Confidence",  value: `${fmt(avgConf * 100, 1)}%`, color: avgConf >= 0.7 ? "text-profit" : avgConf >= 0.4 ? "text-warning" : "text-loss" },
            ].map(({ label, value, color }) => (
              <Card key={label}>
                <CardContent className="p-3">
                  <p className="text-[10px] uppercase tracking-widest text-muted-foreground mb-1">{label}</p>
                  <p className={cn("text-[16px] font-mono font-bold tabular-nums", color)}>{value}</p>
                </CardContent>
              </Card>
            ))}
          </div>
        )}

        {/* Brain context */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <Brain className="h-3.5 w-3.5 text-primary" />
              <CardTitle>Learning System</CardTitle>
            </div>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              {[
                { label: "Active Lessons",   value: String(metrics?.active_lessons ?? 0) },
                { label: "LLM Decisions",    value: String((metrics?.llm_go ?? 0) + (metrics?.llm_nogo ?? 0)) },
                { label: "LLM Avg Conf",     value: `${fmt(metrics?.llm_avg_confidence ?? 0, 1)}%` },
                { label: "Offline Fallbacks",value: String(metrics?.llm_offline_fallbacks ?? 0) },
              ].map(({ label, value }) => (
                <div key={label} className="rounded-lg bg-secondary/60 px-3 py-2">
                  <p className="text-[10px] text-muted-foreground mb-0.5">{label}</p>
                  <p className="text-[13px] font-mono font-bold">{value}</p>
                </div>
              ))}
            </div>
            <p className="mt-3 text-[11px] text-muted-foreground leading-relaxed">
              ARIA's LLM brain extracts lessons from closed trades and applies them to future decisions.
              High-confidence lessons carry more weight in decision-making. Lessons are pruned when contradicted by new evidence.
            </p>
          </CardContent>
        </Card>

        {isLoading && (
          <div className="flex items-center justify-center py-20 text-sm text-muted-foreground">Loading lessons…</div>
        )}

        {!isLoading && totalLessons === 0 && (
          <Card>
            <CardContent className="flex flex-col items-center justify-center py-20 gap-3">
              <div className="h-14 w-14 rounded-2xl bg-secondary flex items-center justify-center">
                <BookOpen className="h-7 w-7 text-muted-foreground/50" />
              </div>
              <p className="text-sm font-medium text-muted-foreground">No lessons learned yet</p>
              <p className="text-[11px] text-muted-foreground/60 text-center max-w-xs">
                ARIA extracts lessons after closed trades. Complete a few trades to start building the knowledge base.
              </p>
            </CardContent>
          </Card>
        )}

        {sorted.length > 0 && (
          <div className="space-y-3">
            {sorted.map((lesson, i) => {
              const conf = lesson.confidence ?? 0;
              const confPct = conf * 100;
              const confLabel = confPct >= 70 ? "High" : confPct >= 40 ? "Medium" : "Low";
              const confVariant = confPct >= 70 ? "profit" : confPct >= 40 ? "warning" : "muted";

              return (
                <Card key={lesson.id ?? i} className={cn(
                  "border-l-4",
                  confPct >= 70 ? "border-l-profit/50" : confPct >= 40 ? "border-l-warning/50" : "border-l-muted"
                )}>
                  <CardContent className="p-4">
                    <div className="flex items-start gap-3">
                      {lesson.confidence != null
                        ? <ConfidenceRing value={conf} />
                        : (
                          <div className="flex items-center justify-center h-10 w-10 rounded-xl bg-primary/10 ring-1 ring-primary/20 shrink-0">
                            <Lightbulb className="h-4.5 w-4.5 text-primary" />
                          </div>
                        )
                      }

                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-2 flex-wrap">
                          <Badge variant="secondary" className="font-mono text-[10px]">#{i + 1}</Badge>
                          {lesson.confidence != null && (
                            <Badge variant={confVariant} className="text-[10px]">
                              {confLabel} — {fmt(confPct, 0)}%
                            </Badge>
                          )}
                          {lesson.created_at && (
                            <span className="text-[10px] text-muted-foreground flex items-center gap-1 ml-auto">
                              <Clock className="h-3 w-3" />
                              {new Date(lesson.created_at).toLocaleDateString(undefined, { month: "short", day: "numeric" })}
                            </span>
                          )}
                        </div>

                        {lesson.confidence != null && (
                          <div className="mb-2">
                            <div className="h-1.5 w-full rounded-full bg-secondary overflow-hidden">
                              <div
                                className={cn("h-full rounded-full transition-all",
                                  confPct >= 70 ? "bg-profit" : confPct >= 40 ? "bg-warning" : "bg-loss")}
                                style={{ width: `${confPct}%` }}
                              />
                            </div>
                          </div>
                        )}

                        <p className="text-[13px] leading-relaxed text-foreground">{lesson.content}</p>
                      </div>
                    </div>
                  </CardContent>
                </Card>
              );
            })}
          </div>
        )}

      </div>
    </div>
  );
}
