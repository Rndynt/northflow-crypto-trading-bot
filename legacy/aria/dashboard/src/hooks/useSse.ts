"use client";
import { useEffect, useRef, useCallback } from "react";
import type { ApiEvent } from "@/lib/api";

type EventHandler = (event: ApiEvent) => void;

export function useSse(url: string, onEvent: EventHandler) {
  const handlerRef = useRef(onEvent);
  handlerRef.current = onEvent;

  useEffect(() => {
    let es: EventSource | null = null;
    let retryTimeout: ReturnType<typeof setTimeout>;
    let attempts = 0;

    const connect = () => {
      es = new EventSource(url);

      const handleMsg = (e: MessageEvent) => {
        try {
          const parsed: ApiEvent = JSON.parse(e.data);
          handlerRef.current(parsed);
        } catch {
          // ignore malformed
        }
      };

      const eventTypes = [
        "signal", "fill", "close", "partial",
        "sl_moved", "survival", "equity", "screening", "error",
      ];
      eventTypes.forEach((type) => {
        es!.addEventListener(type, handleMsg);
      });

      es.onopen = () => {
        attempts = 0;
      };

      es.onerror = () => {
        es?.close();
        const delay = Math.min(1000 * 2 ** attempts, 30_000);
        attempts++;
        retryTimeout = setTimeout(connect, delay);
      };
    };

    connect();

    return () => {
      clearTimeout(retryTimeout);
      es?.close();
    };
  }, [url]);
}

export function useSseStatus(url: string) {
  const connected = useRef(false);
  return connected;
}
