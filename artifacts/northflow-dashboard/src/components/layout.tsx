import { Link, useLocation } from "wouter";
import { Activity, LayoutDashboard, List, PlusSquare } from "lucide-react";
import { cn } from "@/lib/utils";

interface LayoutProps {
  children: React.ReactNode;
}

export function Layout({ children }: LayoutProps) {
  const [location] = useLocation();

  const navItems = [
    { href: "/", label: "Dashboard", icon: LayoutDashboard },
    { href: "/runs", label: "All Runs", icon: List },
    { href: "/runs/new", label: "New Run", icon: PlusSquare },
  ];

  return (
    <div className="flex h-screen overflow-hidden bg-background text-foreground dark">
      {/* Sidebar */}
      <aside className="w-64 border-r border-border bg-card flex flex-col">
        <div className="h-16 flex items-center px-6 border-b border-border">
          <div className="flex items-center gap-3 text-primary">
            <Activity size={20} />
            <span className="font-mono font-bold tracking-widest uppercase">NORTHFLOW</span>
          </div>
        </div>
        
        <nav className="flex-1 py-6 px-4 space-y-2">
          <div className="text-xs font-mono text-muted-foreground mb-4 px-2 uppercase tracking-wider">Research</div>
          {navItems.map((item) => (
            <Link key={item.href} href={item.href}>
              <div
                className={cn(
                  "flex items-center gap-3 px-3 py-2 text-sm font-medium transition-colors cursor-pointer",
                  location === item.href || (item.href !== "/" && location.startsWith(item.href))
                    ? "bg-accent text-primary"
                    : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
                )}
              >
                <item.icon size={16} />
                {item.label}
              </div>
            </Link>
          ))}
        </nav>
        
        <div className="p-4 border-t border-border">
          <div className="text-xs font-mono text-muted-foreground flex items-center justify-between">
            <span>SYSTEM</span>
            <span className="text-success">ONLINE</span>
          </div>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-auto flex flex-col">
        <div className="flex-1 p-8">
          <div className="max-w-7xl mx-auto">
            {children}
          </div>
        </div>
      </main>
    </div>
  );
}
