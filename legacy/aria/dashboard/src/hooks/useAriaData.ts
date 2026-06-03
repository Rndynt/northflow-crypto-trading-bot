"use client";
import useSWR from "swr";
import { api } from "@/lib/api";

const REFRESH = 3000;
const SLOW_REFRESH = 10_000;

export function useStatus() {
  return useSWR("status", api.status, { refreshInterval: REFRESH });
}

export function usePositions() {
  return useSWR("positions", api.positions, { refreshInterval: REFRESH });
}

export function useTrades(page = 1, perPage = 50) {
  return useSWR(
    ["trades", page, perPage],
    () => api.trades(page, perPage),
    { refreshInterval: SLOW_REFRESH }
  );
}

export function useSignals() {
  return useSWR("signals", api.signals, { refreshInterval: REFRESH });
}

export function useScreening() {
  return useSWR("screening", api.screening, { refreshInterval: SLOW_REFRESH });
}

export function useSurvival() {
  return useSWR("survival", api.survival, { refreshInterval: REFRESH });
}

export function useLessons() {
  return useSWR("lessons", api.lessons, { refreshInterval: SLOW_REFRESH });
}

export function useConfig() {
  return useSWR("config", api.config, { refreshInterval: 60_000 });
}

export function useHealth() {
  return useSWR("healthz", api.healthz, { refreshInterval: 5000 });
}
