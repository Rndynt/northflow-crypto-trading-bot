import type { Metadata, Viewport } from "next";
import "./globals.css";
import { SWRProvider } from "@/components/layout/SWRProvider";

export const metadata: Metadata = {
  title: "ARIA Dashboard",
  description: "Autonomous Realtime Intelligence Analyst — Crypto Trading Bot",
  manifest: "/manifest.json",
  appleWebApp: { capable: true, statusBarStyle: "black-translucent", title: "ARIA" },
};

export const viewport: Viewport = {
  themeColor: "#09090b",
  width: "device-width",
  initialScale: 1,
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className="dark">
      <body className="antialiased">
        <SWRProvider>{children}</SWRProvider>
      </body>
    </html>
  );
}
