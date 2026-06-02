export function formatNum(num: number | null | undefined, decimals = 2): string {
  if (num == null) return "-";
  return num.toLocaleString("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
}

export function formatCurrency(num: number | null | undefined): string {
  if (num == null) return "-";
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(num);
}

export function formatPercent(num: number | null | undefined): string {
  if (num == null) return "-";
  return `${(num * 100).toFixed(2)}%`;
}
