"use client";
import { SWRConfig } from "swr";

export function SWRProvider({ children }: { children: React.ReactNode }) {
  return (
    <SWRConfig
      value={{
        revalidateOnFocus: false,
        shouldRetryOnError: true,
        errorRetryInterval: 3000,
        errorRetryCount: 20,
        dedupingInterval: 1000,
      }}
    >
      {children}
    </SWRConfig>
  );
}
