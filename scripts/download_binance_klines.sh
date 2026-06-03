#!/usr/bin/env bash
set -euo pipefail

# Download Binance public 1m kline ZIP files and convert them into Northflow CSV format.
#
# Usage:
#   ./scripts/download_binance_klines.sh BTCUSDT 2024 01 12
#   ./scripts/download_binance_klines.sh ETHUSDT 2024 03 06
#
# Output:
#   data/raw/<SYMBOL>-1m-YYYY-MM.zip
#   data/historical/<SYMBOL>.csv
#
# Northflow CSV format:
#   timestamp,open,high,low,close,volume

symbol="${1:-BTCUSDT}"
year="${2:-2024}"
start_month="${3:-01}"
end_month="${4:-12}"
market="${5:-um}" # um = USD-M futures, cm = COIN-M futures
interval="${6:-1m}"

if ! command -v curl >/dev/null 2>&1; then
  echo "error: curl is required but not installed" >&2
  exit 1
fi

if ! command -v unzip >/dev/null 2>&1; then
  cat >&2 <<'EOF'
error: unzip is required but not installed.

Install it first.

For Debian/Ubuntu/Replit containers with apt:
  sudo apt-get update && sudo apt-get install -y unzip

For Nix-based Replit environments:
  add unzip to replit.nix packages, or use the Packages/System Dependencies UI.

Then rerun this script.
EOF
  exit 1
fi

mkdir -p data/raw data/historical

out="data/historical/${symbol}.csv"
echo "timestamp,open,high,low,close,volume" > "$out"

start_num=$((10#$start_month))
end_num=$((10#$end_month))

if (( start_num < 1 || start_num > 12 || end_num < 1 || end_num > 12 || start_num > end_num )); then
  echo "error: invalid month range: ${start_month}..${end_month}" >&2
  exit 1
fi

rows=0

for ((m=start_num; m<=end_num; m++)); do
  month=$(printf "%02d" "$m")
  file="${symbol}-${interval}-${year}-${month}"
  zip_path="data/raw/${file}.zip"
  url="https://data.binance.vision/data/futures/${market}/monthly/klines/${symbol}/${interval}/${file}.zip"

  echo "Downloading ${url}"
  curl -fL "$url" -o "$zip_path"

  echo "Converting ${zip_path} -> ${out}"
  unzip -p "$zip_path" \
    | awk -F',' '
        BEGIN { count = 0 }
        {
          first = $1
          gsub(/^[[:space:]]+|[[:space:]]+$/, "", first)
          lower = tolower(first)
        }
        # Binance monthly CSV files can contain a header row in every ZIP.
        # Skip header rows wherever they appear, not only NR == 1.
        lower == "open_time" || lower == "timestamp" { next }
        NF >= 6 && first ~ /^[0-9]+$/ {
          print $1 "," $2 "," $3 "," $4 "," $5 "," $6
          count++
        }
        END { print count > "/dev/stderr" }
      ' \
    2> .northflow_rows_tmp \
    >> "$out"

  month_rows=$(cat .northflow_rows_tmp || echo 0)
  rm -f .northflow_rows_tmp
  rows=$((rows + month_rows))
  echo "  rows added: ${month_rows}"
done

# Remove duplicate timestamps while preserving first occurrence and dropping any
# accidental non-numeric rows from older/manual conversions.
tmp="${out}.tmp"
awk -F',' '
  NR == 1 { print; next }
  $1 ~ /^[0-9]+$/ && !seen[$1]++ { print }
' "$out" > "$tmp"
mv "$tmp" "$out"

total_rows=$(( $(wc -l < "$out") - 1 ))

echo
echo "Done."
echo "Output: ${out}"
echo "Rows:   ${total_rows}"
echo
echo "Preview:"
head -5 "$out"
