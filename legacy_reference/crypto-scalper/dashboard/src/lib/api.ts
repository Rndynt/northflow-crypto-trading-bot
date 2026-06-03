export const API_BASE = "/aria-api";

export interface MetricsSnapshot {
  mode: string;
  equity: number;
  peak_equity: number;
  open_positions: number;
  daily_pnl: number;
  trades_today: number;
  signals_today: number;
  llm_go: number;
  llm_nogo: number;
  llm_wait: number;
  llm_avg_confidence: number;
  llm_avg_latency_ms: number;
  llm_offline_fallbacks: number;
  active_lessons: number;
  last_update_ts: number;
}

export interface SurvivalState {
  mode: string;
  score: number;
  drawdown_pct: number;
  daily_loss_pct: number;
  loss_streak: number;
  cooldown_until?: string;
  is_frozen: boolean;
  death_line_pct: number;
  auto_flat_triggered: boolean;
}

export interface PositionEntry {
  signal_id: string;
  client_id: string;
  symbol: string;
  side: string;
  size: number;
  entry_price: number;
  stop_loss: number;
  take_profit: number;
  strategy: string;
  opened_at: string;
  duration_mins: number;
  trailing_activated: boolean;
  breakeven_activated: boolean;
  partial_taken: boolean;
  partial_realized_pnl: number;
  current_price?: number;
  unrealized_pnl?: number;
  unrealized_pnl_pct?: number;
}

export interface TradeEntry {
  signal_id: string;
  symbol: string;
  direction: string;
  strategy: string;
  regime: string;
  entry_time: string;
  exit_time: string;
  pnl_usd: number;
  pnl_pct: number;
  is_win: boolean;
  ta_confidence?: number;
  llm_confidence?: number;
  entry_price: number;
  exit_price: number;
  stop_loss: number;
  take_profit: number;
  size: number;
  partial_taken: boolean;
  partial_realized_pnl: number;
}

export interface SignalEntry {
  signal_id: string;
  symbol: string;
  side: string;
  strategy: string;
  ta_confidence: number;
  entry: number;
  stop_loss: number;
  take_profit: number;
  regime: string;
  reason: string;
  ts: number;
  llm_confidence?: number;
  llm_decision?: string;
  llm_summary?: string;
  offline_fallback?: boolean;
  indicators?: Record<string, number | string>;
}

export interface ScreeningBiasEntry {
  symbol: string;
  bias: string;
  allows_long: boolean;
  allows_short: boolean;
  ts: number;
}

export interface StrategyHealthEntry {
  name: string;
  total_trades: number;
  wins: number;
  losses: number;
  win_rate: number;
  total_pnl: number;
  avg_pnl: number;
  loss_streak: number;
  enabled: boolean;
  size_multiplier: number;
}

export interface SharedSnapshot {
  equity: number;
  initial_equity: number;
  peak_equity: number;
  realized_pnl_today: number;
  unrealized_pnl: number;
  total_equity: number;
  open_positions: number;
  survival_mode: string;
  survival_score: number;
  drawdown_pct: number;
  current_regime: string;
  strategy_health: Record<string, StrategyHealthEntry>;
}

export interface ConfigSummary {
  mode: string;
  exchange: string;
  symbol_count: number;
  max_leverage: number;
  risk_per_trade_pct: number;
  max_drawdown_pct: number;
  max_open_positions: number;
  partial_tp_enabled: boolean;
  max_hold_secs: number;
  metrics_bind: string;
  active_strategies: string[];
}

export interface StatusResponse {
  metrics: MetricsSnapshot;
  survival?: SurvivalState;
  positions: PositionEntry[];
  config: ConfigSummary;
  shared: SharedSnapshot;
  ts: number;
}

export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  page: number;
  per_page: number;
}

export interface Lesson {
  id: string;
  content: string;
  created_at?: string;
  confidence?: number;
}

export interface ApiEvent {
  event_type: string;
  data: Record<string, unknown>;
  ts: number;
}

export interface ControlResponse {
  ok: boolean;
  message: string;
}

export interface ControlConfigPayload {
  max_leverage?: number;
  risk_per_trade_pct?: number;
  max_open_positions?: number;
  max_daily_loss_pct?: number;
  max_hold_secs?: number;
  breakeven_r?: number;
}

async function apiFetch<T>(path: string): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`);
  if (!res.ok) throw new Error(`API ${path} returned ${res.status}`);
  return res.json();
}

async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body != null ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`POST ${path} returned ${res.status}: ${text}`);
  }
  return res.json();
}

export const api = {
  status:   () => apiFetch<StatusResponse>("/api/status"),
  metrics:  () => apiFetch<MetricsSnapshot>("/api/metrics"),
  positions:() => apiFetch<PositionEntry[]>("/api/positions"),
  trades:   (page = 1, per_page = 50) =>
    apiFetch<PaginatedResponse<TradeEntry>>(`/api/trades?page=${page}&per_page=${per_page}`),
  signals:  () => apiFetch<SignalEntry[]>("/api/signals"),
  screening:() => apiFetch<ScreeningBiasEntry[]>("/api/screening"),
  survival: () => apiFetch<SurvivalState>("/api/survival"),
  lessons:  () => apiFetch<Lesson[]>("/api/lessons"),
  config:   () => apiFetch<ConfigSummary>("/api/config"),
  healthz:  () => fetch(`${API_BASE}/healthz`).then((r) => r.text()),

  control: {
    freeze:   (reason?: string) =>
      apiPost<ControlResponse>("/api/control/freeze", reason ? { reason } : {}),
    unfreeze: () =>
      apiPost<ControlResponse>("/api/control/unfreeze"),
    flat:     (reason?: string) =>
      apiPost<ControlResponse>("/api/control/flat", reason ? { reason } : {}),
    close:    (symbol: string) =>
      apiPost<ControlResponse>("/api/control/close", { symbol }),
    updateConfig: (payload: ControlConfigPayload) =>
      apiPost<ControlResponse>("/api/control/config", payload),
  },
};

export function fmt(n: number, decimals = 2): string {
  return n.toLocaleString("en-US", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

export function fmtPct(n: number): string {
  return `${n >= 0 ? "+" : ""}${fmt(n)}%`;
}

export function fmtUsd(n: number): string {
  return `$${fmt(Math.abs(n))}`;
}

export function fmtPnl(n: number): string {
  const sign = n >= 0 ? "+" : "-";
  return `${sign}$${fmt(Math.abs(n))}`;
}

export function timeAgo(ts: number | string): string {
  const date = typeof ts === "number" ? ts * 1000 : new Date(ts).getTime();
  const diff = Date.now() - date;
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return new Date(date).toLocaleDateString();
}

export function formatDuration(mins: number): string {
  if (mins < 60) return `${mins}m`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}
