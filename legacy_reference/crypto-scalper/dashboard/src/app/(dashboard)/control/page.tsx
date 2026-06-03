"use client";
import { Header } from "@/components/layout/Header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { usePositions, useStatus, useConfig } from "@/hooks/useAriaData";
import { api, fmt, ControlConfigPayload } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useState } from "react";
import {
  Snowflake, Play, Zap, X, Settings, AlertTriangle,
  Gamepad2, RefreshCw, CheckCircle2, XCircle, Brain, ShieldOff,
} from "lucide-react";

function ActionBtn({
  label, icon: Icon, variant = "default", onClick, loading, disabled,
}: {
  label: string;
  icon: React.ElementType;
  variant?: "default" | "danger" | "warning" | "success" | "info";
  onClick: () => void;
  loading?: boolean;
  disabled?: boolean;
}) {
  const colors = {
    default: "bg-secondary text-foreground hover:bg-secondary/70 border-border",
    danger:  "bg-destructive/10 text-destructive hover:bg-destructive/20 border-destructive/40",
    warning: "bg-warning/10 text-warning hover:bg-warning/20 border-warning/40",
    success: "bg-profit/10 text-profit hover:bg-profit/20 border-profit/40",
    info:    "bg-info/10 text-info hover:bg-info/20 border-info/40",
  };
  return (
    <button
      onClick={onClick}
      disabled={disabled || loading}
      className={cn(
        "flex items-center justify-center gap-2 rounded-lg border px-4 py-3 text-[13px] font-semibold transition-all select-none",
        "disabled:opacity-40 disabled:cursor-not-allowed",
        colors[variant]
      )}
    >
      {loading
        ? <RefreshCw className="h-4 w-4 animate-spin shrink-0" />
        : <Icon className="h-4 w-4 shrink-0" />}
      {label}
    </button>
  );
}

function Toast({ msg, ok }: { msg: string; ok: boolean }) {
  return (
    <div className={cn(
      "fixed bottom-20 md:bottom-6 left-1/2 -translate-x-1/2 z-50 flex items-center gap-2 rounded-lg px-4 py-2.5 text-[13px] font-medium shadow-xl border",
      ok
        ? "bg-profit/15 text-profit border-profit/30"
        : "bg-destructive/15 text-destructive border-destructive/30"
    )}>
      {ok ? <CheckCircle2 className="h-4 w-4 shrink-0" /> : <XCircle className="h-4 w-4 shrink-0" />}
      {msg}
    </div>
  );
}

export default function ControlPage() {
  const { data: positions, mutate: refreshPositions } = usePositions();
  const { data: status, mutate: refreshStatus } = useStatus();
  const { data: config, mutate: refreshConfig } = useConfig();

  const isFrozen = status?.survival?.is_frozen ?? false;
  const llmFallbacks = status?.metrics?.llm_offline_fallbacks ?? 0;

  const [loading, setLoading] = useState<string | null>(null);
  const [toast, setToast] = useState<{ msg: string; ok: boolean } | null>(null);
  const [closeSymbol, setCloseSymbol] = useState("");
  const [freezeReason, setFreezeReason] = useState("manual");
  const [flatReason, setFlatReason] = useState("manual flat all");

  const [cfgForm, setCfgForm] = useState<ControlConfigPayload>({});
  const [cfgLoading, setCfgLoading] = useState(false);

  function showToast(msg: string, ok: boolean) {
    setToast({ msg, ok });
    setTimeout(() => setToast(null), 4000);
  }

  async function run(key: string, fn: () => Promise<{ ok: boolean; message: string }>) {
    setLoading(key);
    try {
      const res = await fn();
      showToast(res.message, res.ok);
      refreshStatus();
      refreshPositions();
    } catch (e) {
      showToast(String(e), false);
    } finally {
      setLoading(null);
    }
  }

  async function handleConfigSave() {
    if (Object.keys(cfgForm).length === 0) return;
    setCfgLoading(true);
    try {
      const res = await api.control.updateConfig(cfgForm);
      showToast(res.message, res.ok);
      setCfgForm({});
      refreshConfig();
      refreshStatus();
    } catch (e) {
      showToast(String(e), false);
    } finally {
      setCfgLoading(false);
    }
  }

  return (
    <div className="flex flex-col h-full">
      <Header title="Control Panel" />
      <div className="flex-1 overflow-y-auto p-4 md:p-5 space-y-5">

        {/* Status banner */}
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
          <Card className={cn("border-l-4", isFrozen ? "border-l-warning" : "border-l-profit")}>
            <CardContent className="p-3 flex items-center gap-3">
              {isFrozen
                ? <Snowflake className="h-5 w-5 text-warning animate-pulse shrink-0" />
                : <Play className="h-5 w-5 text-profit shrink-0" />}
              <div>
                <p className="text-[10px] text-muted-foreground uppercase tracking-widest">Bot State</p>
                <p className={cn("text-[14px] font-bold", isFrozen ? "text-warning" : "text-profit")}>
                  {isFrozen ? "FROZEN" : "TRADING ACTIVE"}
                </p>
              </div>
            </CardContent>
          </Card>

          <Card className={cn("border-l-4", llmFallbacks > 0 ? "border-l-destructive" : "border-l-profit")}>
            <CardContent className="p-3 flex items-center gap-3">
              <Brain className={cn("h-5 w-5 shrink-0", llmFallbacks > 0 ? "text-destructive" : "text-profit")} />
              <div>
                <p className="text-[10px] text-muted-foreground uppercase tracking-widest">LLM Status</p>
                <p className={cn("text-[14px] font-bold", llmFallbacks > 0 ? "text-destructive" : "text-profit")}>
                  {llmFallbacks > 0 ? `OFFLINE (${llmFallbacks} fallbacks)` : "ONLINE"}
                </p>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardContent className="p-3 flex items-center gap-3">
              <Zap className="h-5 w-5 text-info shrink-0" />
              <div>
                <p className="text-[10px] text-muted-foreground uppercase tracking-widest">Open Positions</p>
                <p className="text-[14px] font-bold text-info">{positions?.length ?? 0}</p>
              </div>
            </CardContent>
          </Card>
        </div>

        {/* ── Main control actions ── */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <Gamepad2 className="h-4 w-4 text-muted-foreground" />
              <CardTitle>Trading Control</CardTitle>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            {/* Freeze / Unfreeze */}
            <div className="space-y-2">
              <p className="text-[11px] text-muted-foreground uppercase tracking-wide font-semibold">Freeze / Unfreeze</p>
              <div className="flex flex-wrap gap-2">
                <div className="flex-1 min-w-[180px]">
                  <input
                    type="text"
                    value={freezeReason}
                    onChange={e => setFreezeReason(e.target.value)}
                    placeholder="Reason for freeze…"
                    className="w-full rounded-md border border-border bg-secondary/50 px-3 py-2 text-[12px] text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary/50"
                  />
                </div>
                <ActionBtn
                  label="Freeze Trading"
                  icon={Snowflake}
                  variant="warning"
                  loading={loading === "freeze"}
                  onClick={() => run("freeze", () => api.control.freeze(freezeReason))}
                />
                <ActionBtn
                  label="Unfreeze Trading"
                  icon={Play}
                  variant="success"
                  loading={loading === "unfreeze"}
                  onClick={() => run("unfreeze", () => api.control.unfreeze())}
                />
              </div>
            </div>

            <div className="border-t border-border/50" />

            {/* Flat all */}
            <div className="space-y-2">
              <p className="text-[11px] text-muted-foreground uppercase tracking-wide font-semibold">Emergency Actions</p>
              <div className="flex flex-wrap gap-2">
                <div className="flex-1 min-w-[180px]">
                  <input
                    type="text"
                    value={flatReason}
                    onChange={e => setFlatReason(e.target.value)}
                    placeholder="Reason for flat all…"
                    className="w-full rounded-md border border-border bg-secondary/50 px-3 py-2 text-[12px] text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary/50"
                  />
                </div>
                <ActionBtn
                  label="FLAT ALL Positions"
                  icon={ShieldOff}
                  variant="danger"
                  loading={loading === "flat"}
                  onClick={() => {
                    if (!confirm(`FLAT ALL open positions? Reason: "${flatReason}"`)) return;
                    run("flat", () => api.control.flat(flatReason));
                  }}
                />
              </div>
              <p className="text-[10px] text-muted-foreground">
                ⚠ Flat All closes every open position at market price immediately. Use in emergencies only.
              </p>
            </div>
          </CardContent>
        </Card>

        {/* ── Close individual positions ── */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <X className="h-4 w-4 text-muted-foreground" />
              <CardTitle>Close Individual Position</CardTitle>
            </div>
          </CardHeader>
          <CardContent className="space-y-3">
            {/* Manual symbol entry */}
            <div className="flex gap-2">
              <input
                type="text"
                value={closeSymbol}
                onChange={e => setCloseSymbol(e.target.value.toUpperCase())}
                placeholder="Symbol e.g. BTCUSDT"
                className="flex-1 rounded-md border border-border bg-secondary/50 px-3 py-2 text-[12px] text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary/50 uppercase font-mono"
              />
              <ActionBtn
                label="Close Position"
                icon={X}
                variant="danger"
                loading={loading === `close-${closeSymbol}`}
                disabled={!closeSymbol.trim()}
                onClick={() => {
                  const sym = closeSymbol.trim().toUpperCase();
                  if (!confirm(`Close ${sym} at market?`)) return;
                  run(`close-${sym}`, () => api.control.close(sym));
                  setCloseSymbol("");
                }}
              />
            </div>

            {/* Open positions quick-close */}
            {positions && positions.length > 0 && (
              <div className="space-y-2">
                <p className="text-[10px] text-muted-foreground uppercase tracking-wide">Quick close from open positions:</p>
                <div className="space-y-1.5">
                  {positions.map(p => {
                    const pnl = p.unrealized_pnl ?? 0;
                    const isLong = p.side === "LONG";
                    return (
                      <div key={p.client_id} className="flex items-center gap-3 rounded-lg bg-secondary/40 px-3 py-2">
                        <Badge variant={isLong ? "profit" : "loss"} className="text-[10px] shrink-0">{p.side}</Badge>
                        <span className="font-bold text-[13px]">{p.symbol}</span>
                        <span className="text-[11px] text-muted-foreground">{p.strategy.replace(/_/g, " ")}</span>
                        <span className={cn("text-[12px] font-mono font-semibold ml-auto", pnl >= 0 ? "text-profit" : "text-loss")}>
                          {pnl >= 0 ? "+" : ""}{fmt(pnl, 2)} USD
                        </span>
                        <button
                          onClick={() => {
                            if (!confirm(`Close ${p.symbol} at market?`)) return;
                            run(`close-${p.symbol}`, () => api.control.close(p.symbol));
                          }}
                          disabled={loading === `close-${p.symbol}`}
                          className="flex items-center gap-1 rounded-md bg-destructive/10 border border-destructive/30 text-destructive px-2.5 py-1 text-[11px] font-semibold hover:bg-destructive/20 transition-colors disabled:opacity-40"
                        >
                          {loading === `close-${p.symbol}`
                            ? <RefreshCw className="h-3 w-3 animate-spin" />
                            : <X className="h-3 w-3" />}
                          Close
                        </button>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {(!positions || positions.length === 0) && (
              <p className="text-center py-4 text-[12px] text-muted-foreground">No open positions</p>
            )}
          </CardContent>
        </Card>

        {/* ── Runtime Config ── */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <Settings className="h-4 w-4 text-muted-foreground" />
              <CardTitle>Runtime Config</CardTitle>
              <span className="ml-auto text-[10px] text-muted-foreground">Changes apply immediately without restart</span>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            {/* Current values */}
            {config && (
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-2 mb-4">
                {[
                  { label: "Max Leverage",   value: `${config.max_leverage}x` },
                  { label: "Risk Per Trade",  value: `${config.risk_per_trade_pct}%` },
                  { label: "Max Positions",   value: String(config.max_open_positions) },
                  { label: "Max Drawdown",    value: `${config.max_drawdown_pct}%` },
                  { label: "Max Hold",        value: `${Math.round(config.max_hold_secs / 60)}m` },
                  { label: "Mode",            value: config.mode },
                ].map(({ label, value }) => (
                  <div key={label} className="rounded-lg bg-secondary/40 px-3 py-2">
                    <p className="text-[9px] text-muted-foreground uppercase tracking-wide">{label}</p>
                    <p className="text-[13px] font-mono font-bold text-foreground">{value}</p>
                    <p className="text-[9px] text-muted-foreground/60">current</p>
                  </div>
                ))}
              </div>
            )}

            <div className="border-t border-border/50 pt-4">
              <p className="text-[11px] text-muted-foreground uppercase tracking-wide font-semibold mb-3">Update Values</p>
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
                {[
                  { key: "max_leverage" as keyof ControlConfigPayload, label: "Max Leverage", placeholder: `current: ${config?.max_leverage ?? "—"}x`, type: "number", min: 1, max: 125 },
                  { key: "risk_per_trade_pct" as keyof ControlConfigPayload, label: "Risk Per Trade %", placeholder: `current: ${config?.risk_per_trade_pct ?? "—"}%`, type: "number", min: 0.1, max: 10 },
                  { key: "max_open_positions" as keyof ControlConfigPayload, label: "Max Open Positions", placeholder: `current: ${config?.max_open_positions ?? "—"}`, type: "number", min: 1, max: 20 },
                  { key: "max_daily_loss_pct" as keyof ControlConfigPayload, label: "Max Daily Loss %", placeholder: `current: ${config?.max_drawdown_pct ?? "—"}%`, type: "number", min: 0.5, max: 50 },
                  { key: "max_hold_secs" as keyof ControlConfigPayload, label: "Max Hold Secs", placeholder: `current: ${config?.max_hold_secs ?? "—"}s`, type: "number", min: 60, max: 86400 },
                  { key: "breakeven_r" as keyof ControlConfigPayload, label: "Breakeven R-Multiple", placeholder: "e.g. 1.0", type: "number", min: 0.5, max: 5 },
                ].map(({ key, label, placeholder, type, min, max }) => (
                  <div key={key}>
                    <label className="block text-[10px] text-muted-foreground uppercase tracking-wide mb-1">{label}</label>
                    <input
                      type={type}
                      min={min}
                      max={max}
                      step="any"
                      placeholder={placeholder}
                      value={(cfgForm[key] as number | undefined) ?? ""}
                      onChange={e => {
                        const v = e.target.value === "" ? undefined : Number(e.target.value);
                        setCfgForm(f => ({ ...f, [key]: v }));
                      }}
                      className="w-full rounded-md border border-border bg-secondary/50 px-3 py-2 text-[12px] text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary/50 font-mono"
                    />
                  </div>
                ))}
              </div>

              <div className="mt-4 flex items-center gap-3">
                <button
                  onClick={handleConfigSave}
                  disabled={cfgLoading || Object.values(cfgForm).every(v => v === undefined)}
                  className="flex items-center gap-2 rounded-lg bg-primary text-primary-foreground px-4 py-2 text-[13px] font-semibold hover:bg-primary/90 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  {cfgLoading
                    ? <RefreshCw className="h-4 w-4 animate-spin" />
                    : <Settings className="h-4 w-4" />}
                  Apply Changes
                </button>
                <button
                  onClick={() => setCfgForm({})}
                  className="text-[12px] text-muted-foreground hover:text-foreground transition-colors"
                >
                  Clear
                </button>
                {Object.keys(cfgForm).filter(k => cfgForm[k as keyof ControlConfigPayload] !== undefined).length > 0 && (
                  <div className="flex flex-wrap gap-1.5 ml-2">
                    {Object.entries(cfgForm).filter(([, v]) => v !== undefined).map(([k, v]) => (
                      <Badge key={k} variant="info" className="text-[10px] font-mono">{k}={v}</Badge>
                    ))}
                  </div>
                )}
              </div>

              <div className="mt-3 flex items-start gap-2 text-[11px] text-muted-foreground">
                <AlertTriangle className="h-3 w-3 shrink-0 mt-0.5 text-warning" />
                <span>Config changes are runtime-only — they reset when the bot restarts. Edit <code className="bg-secondary px-1 rounded">config/default.toml</code> for permanent changes.</span>
              </div>
            </div>
          </CardContent>
        </Card>

      </div>

      {toast && <Toast msg={toast.msg} ok={toast.ok} />}
    </div>
  );
}
