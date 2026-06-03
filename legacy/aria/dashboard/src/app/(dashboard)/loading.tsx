export default function Loading() {
  return (
    <div className="flex flex-col h-full animate-fade-in">
      <div className="h-[52px] border-b border-border flex items-center px-5 gap-4">
        <div className="h-4 w-24 rounded-md bg-secondary animate-pulse" />
        <div className="ml-auto flex items-center gap-3">
          <div className="h-5 w-12 rounded-md bg-secondary animate-pulse" />
          <div className="h-4 w-20 rounded-md bg-secondary animate-pulse" />
        </div>
      </div>
      <div className="flex-1 p-4 md:p-6 grid grid-cols-2 lg:grid-cols-4 gap-4 content-start">
        {Array.from({ length: 8 }).map((_, i) => (
          <div
            key={i}
            className="h-24 rounded-xl bg-card border border-border animate-pulse"
            style={{ animationDelay: `${i * 60}ms` }}
          />
        ))}
      </div>
    </div>
  );
}
