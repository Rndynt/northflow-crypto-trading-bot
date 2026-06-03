# Data Download Guide

Northflow needs historical **1m OHLCV CSV** data in this format:

```csv
timestamp,open,high,low,close,volume
```

The easiest source for initial research is Binance Public Data.

For USDT perpetual futures such as `BTCUSDT`, `ETHUSDT`, and `SOLUSDT`, use Binance **USD-M Futures** data:

```text
data/futures/um/monthly/klines/<SYMBOL>/1m/
```

Do not use `futures/cm` for `BTCUSDT` research data. `cm` is COIN-M Futures and uses symbols such as `BTCUSD_PERP`.

---

## Prerequisites

The helper script requires:

- `curl`
- `unzip`

Check them:

```bash
which curl
which unzip
```

If `unzip` is missing on a Debian/Ubuntu/Replit container with `apt`:

```bash
sudo apt-get update && sudo apt-get install -y unzip
```

If `apt-get` is unavailable in Replit, install `unzip` from Replit System Dependencies, or add it to `replit.nix`:

```nix
{ pkgs }: {
  deps = [
    pkgs.curl
    pkgs.unzip
  ];
}
```

Then restart the Repl.

---

## Helper script

The repository includes:

```text
scripts/download_binance_klines.sh
```

Make it executable:

```bash
chmod +x scripts/download_binance_klines.sh
```

Script format:

```bash
./scripts/download_binance_klines.sh SYMBOL YEAR START_MONTH END_MONTH [MARKET] [INTERVAL]
```

Arguments:

| Argument | Example | Meaning |
|---|---:|---|
| `SYMBOL` | `BTCUSDT` | Binance trading pair |
| `YEAR` | `2024` | Year to download |
| `START_MONTH` | `01` | First month |
| `END_MONTH` | `12` | Last month |
| `MARKET` | `um` | Optional. Default: `um` = USD-M Futures |
| `INTERVAL` | `1m` | Optional. Default: `1m` |

For Northflow research, keep `INTERVAL = 1m`. The engine builds 5m and 15m candles internally from 1m data.

---

## Download BTCUSDT 1m for 12 months

This downloads January through December 2024:

```bash
./scripts/download_binance_klines.sh BTCUSDT 2024 01 12
```

Meaning:

```text
BTCUSDT = symbol
2024    = year
01      = start month, January
12      = end month, December
```

Output ZIP files:

```text
data/raw/BTCUSDT-1m-2024-01.zip
data/raw/BTCUSDT-1m-2024-02.zip
...
data/raw/BTCUSDT-1m-2024-12.zip
```

Final Northflow CSV:

```text
data/historical/BTCUSDT.csv
```

---

## Download specific months

January 2024 only:

```bash
./scripts/download_binance_klines.sh BTCUSDT 2024 01 01
```

April to June 2024:

```bash
./scripts/download_binance_klines.sh BTCUSDT 2024 04 06
```

March to September 2024:

```bash
./scripts/download_binance_klines.sh BTCUSDT 2024 03 09
```

---

## Download other symbols

ETHUSDT full year 2024:

```bash
./scripts/download_binance_klines.sh ETHUSDT 2024 01 12
```

SOLUSDT full year 2024:

```bash
./scripts/download_binance_klines.sh SOLUSDT 2024 01 12
```

After downloading a different symbol, make sure `config/research.toml` includes that symbol.

Example:

```toml
[pairs]
symbols = ["ETHUSDT"]
```

---

## Verify the converted CSV

Check that the historical file exists:

```bash
ls -lh data/historical/BTCUSDT.csv
head -5 data/historical/BTCUSDT.csv
```

Expected header:

```csv
timestamp,open,high,low,close,volume
```

Expected rows look like:

```csv
1704067200000,42150.0,42800.0,41900.0,42600.0,1234.5
```

---

## Run Northflow research/backtest

After the CSV exists:

```bash
cargo run -- research --config config/research.toml
```

### Release mode for large datasets

For large datasets such as 12 months of BTCUSDT 1m candles (~527k candles),
use release mode:

```bash
cargo run --release -- research --config config/research.toml
```

- Debug mode can be significantly slower on 500k+ candles.
- Release mode is recommended for all real backtests.
- If the CLI appears idle, progress logs now print every 50,000 candles:

```text
Running backtest replay...
  Backtest progress: 50000/527040 1m candles (9.5%)
  Backtest progress: 100000/527040 1m candles (19.0%)
  ...
  Backtest complete: X trades, final equity Y
```

Expected report files:

```text
reports/backtest_summary.json
reports/trades.csv
reports/equity_curve.csv
reports/attribution_summary.json
reports/attribution_by_regime.csv
reports/attribution_by_exit_reason.csv
reports/attribution_by_side.csv
reports/attribution_by_filter.csv
reports/audit_report.json
reports/report_manifest.json
reports/risk_rejections.csv
reports/signal_flow_summary.json
```

For large datasets, if the strategy opens no trades, inspect:

- `reports/signal_flow_summary.json`
- `reports/risk_rejections.csv`

These files show whether signals were rejected by initial risk checks or actual-entry re-risking.

---

## Common errors

### `bash: unzip: command not found`

Install `unzip` first:

```bash
sudo apt-get update && sudo apt-get install -y unzip
```

Or install it from Replit System Dependencies.

### `curl: (22) The requested URL returned error: 404`

Usually this means one of these is wrong:

- symbol does not exist for that market
- month is not available
- using `cm` instead of `um`
- interval is wrong

For `BTCUSDT`, `ETHUSDT`, and `SOLUSDT`, use the default market:

```bash
./scripts/download_binance_klines.sh BTCUSDT 2024 01 12
```

### No file in `data/historical/`

The conversion step probably failed. Check:

```bash
ls -lh data/raw
which unzip
```

Then rerun:

```bash
./scripts/download_binance_klines.sh BTCUSDT 2024 01 12
```

---

## Important notes

- Northflow expects **1m source data**.
- Do not download 5m or 15m data for the current research engine.
- 5m and 15m candles are built internally from 1m candles.
- Do not mix spot, USD-M futures, and COIN-M futures in one historical file.
- Keep one symbol per CSV file.
