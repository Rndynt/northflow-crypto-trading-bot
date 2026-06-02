import { Router } from "express";
import { db } from "@workspace/db";
import { backtestRunsTable, tradesTable, insertBacktestRunSchema } from "@workspace/db";
import { eq, avg, count, max, min, sql } from "drizzle-orm";

const router = Router();

// GET /api/research/runs
router.get("/runs", async (req, res) => {
  try {
    const runs = await db
      .select()
      .from(backtestRunsTable)
      .orderBy(sql`${backtestRunsTable.createdAt} DESC`);
    res.json(runs.map(formatRun));
  } catch (err) {
    req.log.error({ err }, "Failed to list runs");
    res.status(500).json({ error: "Internal server error" });
  }
});

// POST /api/research/runs
router.post("/runs", async (req, res) => {
  const parsed = insertBacktestRunSchema.safeParse({
    ...req.body,
    status: "pending",
  });
  if (!parsed.success) {
    return res.status(400).json({ error: "Invalid input", details: parsed.error.issues });
  }
  try {
    const [run] = await db.insert(backtestRunsTable).values(parsed.data).returning();
    res.status(201).json(formatRun(run));
  } catch (err) {
    req.log.error({ err }, "Failed to create run");
    res.status(500).json({ error: "Internal server error" });
  }
});

// GET /api/research/runs/:id
router.get("/runs/:id", async (req, res) => {
  const id = Number(req.params.id);
  if (isNaN(id)) return res.status(400).json({ error: "Invalid id" });
  try {
    const [run] = await db
      .select()
      .from(backtestRunsTable)
      .where(eq(backtestRunsTable.id, id));
    if (!run) return res.status(404).json({ error: "Not found" });
    res.json(formatRun(run));
  } catch (err) {
    req.log.error({ err }, "Failed to get run");
    res.status(500).json({ error: "Internal server error" });
  }
});

// DELETE /api/research/runs/:id
router.delete("/runs/:id", async (req, res) => {
  const id = Number(req.params.id);
  if (isNaN(id)) return res.status(400).json({ error: "Invalid id" });
  try {
    await db.delete(backtestRunsTable).where(eq(backtestRunsTable.id, id));
    res.status(204).send();
  } catch (err) {
    req.log.error({ err }, "Failed to delete run");
    res.status(500).json({ error: "Internal server error" });
  }
});

// GET /api/research/runs/:id/trades
router.get("/runs/:id/trades", async (req, res) => {
  const id = Number(req.params.id);
  if (isNaN(id)) return res.status(400).json({ error: "Invalid id" });
  try {
    const trades = await db
      .select()
      .from(tradesTable)
      .where(eq(tradesTable.runId, id));
    res.json(trades.map(formatTrade));
  } catch (err) {
    req.log.error({ err }, "Failed to list trades");
    res.status(500).json({ error: "Internal server error" });
  }
});

// GET /api/research/summary
router.get("/summary", async (req, res) => {
  try {
    const [stats] = await db
      .select({
        totalRuns: count(),
        completedRuns: sql<number>`count(*) filter (where ${backtestRunsTable.status} = 'completed')`,
        failedRuns: sql<number>`count(*) filter (where ${backtestRunsTable.status} = 'failed')`,
        avgWinRate: avg(backtestRunsTable.winRate),
        avgPnl: avg(backtestRunsTable.totalPnl),
        bestPnl: max(backtestRunsTable.totalPnl),
        worstPnl: min(backtestRunsTable.totalPnl),
      })
      .from(backtestRunsTable);

    const symbolRows = await db
      .select({ symbol: backtestRunsTable.symbol })
      .from(backtestRunsTable)
      .groupBy(backtestRunsTable.symbol)
      .limit(5);

    res.json({
      totalRuns: Number(stats.totalRuns ?? 0),
      completedRuns: Number(stats.completedRuns ?? 0),
      failedRuns: Number(stats.failedRuns ?? 0),
      avgWinRate: Number(stats.avgWinRate ?? 0),
      avgPnl: Number(stats.avgPnl ?? 0),
      bestPnl: Number(stats.bestPnl ?? 0),
      worstPnl: Number(stats.worstPnl ?? 0),
      topSymbols: symbolRows.map((r) => r.symbol),
    });
  } catch (err) {
    req.log.error({ err }, "Failed to get summary");
    res.status(500).json({ error: "Internal server error" });
  }
});

function formatRun(r: typeof backtestRunsTable.$inferSelect) {
  return {
    id: r.id,
    symbol: r.symbol,
    strategy: r.strategy,
    status: r.status,
    totalTrades: r.totalTrades,
    winRate: r.winRate,
    totalPnl: r.totalPnl,
    maxDrawdown: r.maxDrawdown,
    sharpeRatio: r.sharpeRatio,
    startDate: r.startDate,
    endDate: r.endDate,
    notes: r.notes,
    createdAt: r.createdAt.toISOString(),
  };
}

function formatTrade(t: typeof tradesTable.$inferSelect) {
  return {
    id: t.id,
    runId: t.runId,
    side: t.side,
    entryPrice: t.entryPrice,
    exitPrice: t.exitPrice,
    quantity: t.quantity,
    pnl: t.pnl,
    fee: t.fee,
    entryTime: t.entryTime,
    exitTime: t.exitTime,
  };
}

export default router;
