"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";
import { useState, useEffect } from "react";
import {
  LayoutDashboard, TrendingUp, History, Radio,
  ShieldAlert, BarChart2, BookOpen, Settings, Zap, Menu, X,
  ScanSearch, Gamepad2, Brain,
} from "lucide-react";
import { useHealth, useStatus } from "@/hooks/useAriaData";

const nav = [
  { href: "/overview",   label: "Overview",   icon: LayoutDashboard },
  { href: "/positions",  label: "Positions",  icon: TrendingUp },
  { href: "/trades",     label: "Trades",     icon: History },
  { href: "/signals",    label: "Signals",    icon: Radio },
  { href: "/screening",  label: "Screening",  icon: ScanSearch },
  { href: "/survival",   label: "Survival",   icon: ShieldAlert },
  { href: "/strategies", label: "Strategies", icon: BarChart2 },
  { href: "/lessons",    label: "Lessons",    icon: BookOpen },
  { href: "/control",    label: "Control",    icon: Gamepad2, highlight: true },
  { href: "/config",     label: "Config",     icon: Settings },
];

const bottomNav = [
  { href: "/overview",   label: "Overview",   icon: LayoutDashboard },
  { href: "/positions",  label: "Positions",  icon: TrendingUp },
  { href: "/signals",    label: "Signals",    icon: Radio },
  { href: "/screening",  label: "Screen",     icon: ScanSearch },
  { href: "/control",    label: "Control",    icon: Gamepad2 },
];

export function Sidebar() {
  const pathname = usePathname();
  const { data: health } = useHealth();
  const { data: status } = useStatus();
  const online = health === "ok";
  const [pending, setPending] = useState<string | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);

  const llmError = (status?.metrics?.llm_offline_fallbacks ?? 0) > 0;
  const isFrozen = status?.survival?.is_frozen ?? false;

  useEffect(() => {
    setPending(null);
    setDrawerOpen(false);
  }, [pathname]);

  function isActive(href: string) {
    if (pending) return pending === href;
    return pathname === href || pathname.startsWith(href + "/");
  }

  function NavLinks({ onClick }: { onClick?: () => void }) {
    return (
      <>
        {nav.map(({ href, label, icon: Icon, highlight }) => {
          const active = isActive(href);
          return (
            <Link
              key={href}
              href={href}
              onClick={() => { setPending(href); onClick?.(); }}
              className={cn(
                "group flex items-center gap-3 px-3 h-10 w-full rounded-md text-[13px] font-medium transition-colors duration-100 select-none",
                active
                  ? "bg-primary/10 text-primary"
                  : highlight
                    ? "text-warning hover:bg-warning/10 hover:text-warning"
                    : "text-muted-foreground hover:bg-secondary hover:text-foreground"
              )}
            >
              <Icon className={cn(
                "h-4 w-4 shrink-0",
                active ? "text-primary" : highlight ? "text-warning group-hover:text-warning" : "text-muted-foreground group-hover:text-foreground"
              )} />
              <span className="truncate">{label}</span>
              {href === "/control" && isFrozen && (
                <span className="ml-auto text-[9px] font-bold bg-warning/20 text-warning px-1.5 py-0.5 rounded">FROZEN</span>
              )}
            </Link>
          );
        })}
      </>
    );
  }

  return (
    <>
      {/* ── Desktop sidebar ── */}
      <aside className="hidden md:flex h-screen w-[200px] flex-col shrink-0 border-r border-border bg-card">
        <div className="flex h-12 items-center gap-3 px-4 border-b border-border shrink-0">
          <div className="flex h-7 w-7 items-center justify-center rounded-md bg-primary/15 ring-1 ring-primary/30 shrink-0">
            <Zap className="h-3.5 w-3.5 text-primary" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-[13px] font-bold leading-none tracking-widest">ARIA</p>
            <p className="text-[10px] text-muted-foreground leading-none mt-1">Crypto Scalper</p>
          </div>
          <span className={cn("h-2 w-2 rounded-full shrink-0", online ? "bg-primary animate-pulse" : "bg-muted-foreground/40")} />
        </div>

        {/* LLM error banner */}
        {llmError && (
          <div className="mx-2 mt-2 flex items-center gap-2 rounded-md bg-destructive/15 border border-destructive/30 px-2.5 py-1.5">
            <Brain className="h-3 w-3 text-destructive shrink-0" />
            <span className="text-[10px] font-bold text-destructive">LLM OFFLINE</span>
          </div>
        )}
        {isFrozen && (
          <div className="mx-2 mt-1.5 flex items-center gap-2 rounded-md bg-warning/10 border border-warning/30 px-2.5 py-1.5">
            <span className="h-2 w-2 rounded-full bg-warning animate-pulse shrink-0" />
            <span className="text-[10px] font-bold text-warning">TRADING FROZEN</span>
          </div>
        )}

        <nav className="flex-1 overflow-y-auto py-2 px-2 space-y-0.5">
          <NavLinks />
        </nav>

        <div className="border-t border-border p-2 shrink-0">
          <div className={cn(
            "flex items-center gap-2 px-3 h-8 rounded-md text-[11px] font-medium",
            online ? "text-primary" : "text-muted-foreground"
          )}>
            <span className={cn("h-1.5 w-1.5 rounded-full shrink-0", online ? "bg-primary animate-pulse" : "bg-muted-foreground/40")} />
            {online ? "Connected" : "Offline"}
          </div>
        </div>
      </aside>

      {/* ── Mobile top bar ── */}
      <div className="md:hidden fixed top-0 inset-x-0 z-50 flex h-12 items-center px-3 gap-3 border-b border-border bg-card">
        <button
          onClick={() => setDrawerOpen(true)}
          className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground hover:bg-secondary hover:text-foreground transition-colors"
        >
          <Menu className="h-5 w-5" />
        </button>
        <div className="flex h-6 w-6 items-center justify-center rounded-md bg-primary/15 ring-1 ring-primary/30">
          <Zap className="h-3.5 w-3.5 text-primary" />
        </div>
        <span className="text-[13px] font-bold tracking-widest">ARIA</span>
        {llmError && (
          <span className="flex items-center gap-1 text-[9px] font-bold text-destructive bg-destructive/10 px-1.5 py-0.5 rounded">
            <Brain className="h-2.5 w-2.5" /> LLM OFFLINE
          </span>
        )}
        {isFrozen && (
          <span className="text-[9px] font-bold text-warning bg-warning/10 px-1.5 py-0.5 rounded">FROZEN</span>
        )}
        <span className={cn("h-1.5 w-1.5 rounded-full", online ? "bg-primary animate-pulse" : "bg-muted-foreground/40")} />
        <span className="ml-auto text-[10px] text-muted-foreground font-medium">
          {online ? "● live" : "○ offline"}
        </span>
      </div>

      {/* ── Mobile drawer sidebar ── */}
      {drawerOpen && (
        <div className="md:hidden fixed inset-0 z-50 flex">
          <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={() => setDrawerOpen(false)} />
          <aside className="relative w-[220px] h-full flex flex-col bg-card border-r border-border shadow-2xl">
            <div className="flex h-12 items-center gap-3 px-4 border-b border-border shrink-0">
              <div className="flex h-7 w-7 items-center justify-center rounded-md bg-primary/15 ring-1 ring-primary/30 shrink-0">
                <Zap className="h-3.5 w-3.5 text-primary" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-[13px] font-bold leading-none tracking-widest">ARIA</p>
                <p className="text-[10px] text-muted-foreground leading-none mt-1">Crypto Scalper</p>
              </div>
              <button onClick={() => setDrawerOpen(false)} className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-secondary transition-colors">
                <X className="h-4 w-4" />
              </button>
            </div>
            {llmError && (
              <div className="mx-2 mt-2 flex items-center gap-2 rounded-md bg-destructive/15 border border-destructive/30 px-2.5 py-1.5">
                <Brain className="h-3 w-3 text-destructive shrink-0" />
                <span className="text-[10px] font-bold text-destructive">LLM OFFLINE</span>
              </div>
            )}
            <nav className="flex-1 overflow-y-auto py-2 px-2 space-y-0.5">
              <NavLinks onClick={() => setDrawerOpen(false)} />
            </nav>
            <div className="border-t border-border p-2 shrink-0">
              <div className={cn(
                "flex items-center gap-2 px-3 h-8 rounded-md text-[11px] font-medium",
                online ? "text-primary" : "text-muted-foreground"
              )}>
                <span className={cn("h-1.5 w-1.5 rounded-full shrink-0", online ? "bg-primary animate-pulse" : "bg-muted-foreground/40")} />
                {online ? "Connected" : "Offline"}
              </div>
            </div>
          </aside>
        </div>
      )}

      {/* ── Mobile bottom nav ── */}
      <nav className="md:hidden fixed bottom-0 inset-x-0 z-40 flex h-16 items-stretch border-t border-border bg-card">
        {bottomNav.map(({ href, label, icon: Icon }) => {
          const active = isActive(href);
          return (
            <Link
              key={href}
              href={href}
              onClick={() => setPending(href)}
              className={cn(
                "flex flex-1 flex-col items-center justify-center gap-1.5 select-none transition-colors duration-100",
                active ? "text-primary" : "text-muted-foreground"
              )}
            >
              <Icon className="h-5 w-5 shrink-0" />
              <span className="text-[10px] font-medium leading-none">{label}</span>
            </Link>
          );
        })}
      </nav>
    </>
  );
}
