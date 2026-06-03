# Codebase Review ARIA Crypto Scalper

Tanggal: 2026-05-27.

## Verdict singkat

- **Belum bisa dinyatakan profitable** dari code saja; butuh bukti statistik out-of-sample + biaya real + slippage real.
- **Flow arsitektur sudah benar** untuk pola multi-agent trading (signal → risk → brain → manager → execution → monitor/learning), dengan defense-in-depth yang cukup matang.
- **Ini termasuk quant trading hybrid** (rules + risk math + microstructure + LLM overlay), bukan pure discretionary AI.
- **AI-agent responsibilities sudah cukup rapi**, tapi belum cukup untuk “hidup sendiri” tanpa supervisi karena masih ada gap realism pada paper fill, stale-data gating, exchange constraints, dan governance operasional live.

## Yang sudah kuat

1. Pipeline agent jelas dan typed-event based lewat `MessageBus`.
2. Risk gate berlapis (learning policy, survival gate, funding gate, spread gate, portfolio load, orchestrator multiplier).
3. Execution punya final gate tambahan (survival, risk manager blocked state, policy gate lagi, anti-duplicate position check ke local+exchange).
4. Ada survival logic + freeze/unfreeze + flat-all path + protection orders.

## Celah kritikal / bug-risk prioritas tinggi

1. **Paper fill masih terlalu optimistis**: `PaperExchange` melakukan instant fill di harga request dan tanpa partial fill/latency/queue dynamics.
2. **Equity reconciliation di paper disiasati dengan `fetch_equity_usd -> 0.0`** untuk menghindari overwrite, ini aman untuk workaround internal tapi berisiko masking mismatch jika logic lain bergantung pada equity broker-side.
3. **`unsafe set_var` di dotenv loader**: penggunaan unsafe untuk mutasi env layak ditinjau ulang dan diberi pembatasan call-time yang tegas.
4. **LLM cooldown 45 detik per simbol** menghemat biaya API tetapi bisa membuat signal cepat terlewat di market regime sangat cepat.
5. **Tidak terlihat guard kuat untuk stale market data age** pada jalur pre-trade (book/tick/feed age), berisiko trade pada data basi saat websocket degradasi.
6. **Profitability claim risk**: banyak guard sudah bagus, tetapi belum terlihat bukti integrated net-of-fee expectancy validator sebelum order live.

## Saran peningkatan (urutan implementasi)

1. Realistic paper simulator: latency model, partial fill, maker/taker path, queue rejection, spread-cross penalty.
2. Stale-data hard gate di risk/execution (max age per stream: tick, book, candle, funding/news).
3. Exchange filter cache (tick size, step size, min notional, leverage bracket) + preflight reject reason yang eksplisit.
4. Live liquidation-distance guard berbasis mark price dan leverage efektif.
5. Shadow accounting: catat expected edge vs realized edge per trade (decision-price vs fill-price vs exit-quality).
6. Tambah walk-forward robustness report otomatis harian (rolling OOS), bukan hanya sesekali backtest manual.

## Kesimpulan

Bot ini **sudah punya fondasi quant-agentic yang serius**, dan flow utamanya sudah benar. Tetapi untuk target “bisa hidupi diri sendiri”, masih butuh hardening eksekusi live + realism simulasi + validasi statistik berulang yang disiplin.
