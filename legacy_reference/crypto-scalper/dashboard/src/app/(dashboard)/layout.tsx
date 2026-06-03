import { Sidebar } from "@/components/layout/Sidebar";
import { EventFeed } from "@/components/layout/EventFeed";

export default function DashboardLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-screen overflow-hidden bg-background">
      <Sidebar />
      <div className="flex flex-1 overflow-hidden min-w-0">
        <main className="flex-1 overflow-y-auto min-w-0 pt-12 pb-16 md:pt-0 md:pb-0">
          {children}
        </main>
        <aside className="hidden xl:flex w-[260px] flex-col border-l border-border bg-card shrink-0">
          <EventFeed />
        </aside>
      </div>
    </div>
  );
}
