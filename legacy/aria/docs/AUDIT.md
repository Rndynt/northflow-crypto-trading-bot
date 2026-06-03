# Audit ARIA Crypto Scalper

Tanggal audit: 2026-05-08.

## Ringkasan status

ARIA sudah memiliki fondasi bot quant trading otonom: data agent membangun candle, signal agent memilih strategi, risk agent melakukan gating dan sizing, brain/manager LLM memberi keputusan, execution agent mengirim order, learning agent mencatat hasil, dan survival agent menurunkan risiko atau membekukan trading saat kondisi memburuk.

Namun, ARIA belum boleh dianggap sebagai mesin "cari uang sendiri" yang siap live tanpa pengawasan. Mode overlay agresif masih `paper` dan `dry_run`, paper exchange masih terlalu optimistis karena instant-fill, dan live execution masih perlu guard tambahan untuk filter exchange, margin, liquidation, fill quality, dan post-only maker execution.

## Temuan utama

### 1. Quant engine sudah aktif, tetapi harus dipakai dengan unit risiko yang benar

Quant engine berisi Kelly sizing, volatility targeting, VaR/CVaR cap, IC adjustment, Kalman trend gate, dan correlation penalty. Signal agent meng-update return dan Kalman per candle, learning agent mencatat PnL trade tertutup, lalu risk agent memanggil quant engine sebelum order masuk pipeline eksekusi.

Perbaikan yang sudah dibuat di patch terkait:

- `RiskAgentConfig.base_risk_pct` sekarang menerima `cfg.risk.risk_per_trade_pct`, sehingga overlay aktif tidak diam-diam kembali ke default.
- `base_risk_pct` dikonversi dari percentage points ke fraction sebelum dibandingkan dengan Kelly fraction.
- VaR check memakai risk-per-trade terkonfigurasi untuk estimasi loss aktual, bukan hanya jarak stop terhadap entry.

### 2. Bot sudah otonom secara arsitektur, tetapi belum otonom secara finansial

Otonomi operasional sudah ada: multi-agent runtime, message bus, LLM decision layer, risk layer, execution layer, monitoring, dan learning loop. Tetapi otonomi finansial membutuhkan bukti out-of-sample, paper-fill realistis, biaya real exchange, dan recovery logic yang diuji dalam kondisi buruk.

Syarat minimum sebelum live:

- Backtest walk-forward untuk setiap simbol dan strategi.
- Paper test dengan fill/slippage realistis, bukan instant-fill.
- Batas leverage efektif dan liquidation buffer.
- Circuit breaker yang diuji dengan simulasi gap, API error, websocket stale, dan order rejection.
- Monitoring PnL net of fees, bukan gross PnL.

### 3. Survival system ada dan berguna, tetapi harus dibuat lebih konservatif untuk leverage tinggi

Survival agent sudah menghitung score, mode, death line, cooldown, drawdown, loss streak, ratchet, dan size multiplier. Risk/execution juga menolak entry saat mode `Frozen` atau `Dead`.

Risiko konfigurasi agresif:

- `risk_per_trade_pct = 2.0` terlalu besar untuk modal kecil dan leverage tinggi.
- `max_open_positions = 6` dan `max_position_notional_pct = 500.0` bisa menumpuk exposure korelatif BTC/ETH/SOL.
- `min_reward_risk = 0.3` dapat mengizinkan setup dengan payoff terlalu kecil, sehingga fee/slippage mudah menghapus edge.

Rekomendasi live awal:

- Mulai dari overlay `hft-live.toml`, bukan `aggressive.toml`.
- Risk per trade maksimum 0.1%-0.3% sampai ada minimal 200 trade valid.
- Batasi notional per posisi dan total correlated exposure.
- Jangan aktifkan 100x kecuali ada liquidation-price guard dan exchange filter real-time.

### 4. Strategi scalping terbaik untuk ARIA

Strategi terbaik untuk struktur bot ini adalah hybrid microstructure scalping:

1. Gunakan regime filter untuk memilih trend-following atau mean-reversion.
2. Gunakan VWAP/EMA sebagai directional anchor.
3. Gunakan order-book imbalance, OFI, VPIN/toxicity, spread, dan funding sebagai gate.
4. Prefer maker/post-only limit order saat spread tipis dan queue risk masuk akal.
5. Gunakan taker/market hanya bila net edge setelah fee, slippage, dan market impact tetap positif.
6. Auto-retire strategi yang IC/win-rate/payoff memburuk.

Strategi yang perlu diprioritaskan:

- VWAP pullback scalp saat trend sehat.
- EMA ribbon continuation saat regime trending.
- Mean reversion hanya saat choppy/range dan spread sangat tipis.
- Squeeze breakout hanya saat volatility expansion terkonfirmasi volume/order-flow.

### 5. Bug dan celah berikutnya yang perlu diperbaiki

Prioritas tinggi:

- Tambahkan exchange info cache untuk tick size, step size, min qty, min notional, leverage bracket, dan price precision.
- Tambahkan liquidation price guard sebelum order live.
- Buat paper exchange lebih realistis: latency, partial fill, maker queue, spread crossing, slippage, dan rejection.
- Simpan quant state lintas restart agar Kelly/IC tidak reset setiap boot.
- Log dan tampilkan alasan quant sizing di dashboard/Telegram, bukan dibuang sebagai `_quant_reason`.
- Tambahkan stale-data gate: jangan trade jika websocket/book/candle/feed terlalu tua.
- Tambahkan post-only maker order path dan cancel/replace logic.

Prioritas sedang:

- Bersihkan warning `unused_*` agar CI lebih ketat.
- Tambahkan property tests untuk parser LLM dan sizing.
- Tambahkan shadow backtest untuk setiap signal live/paper.
- Tambahkan risk report harian berisi net PnL, hit rate, payoff, expectancy, max adverse excursion, dan fee drag.

## Kesimpulan

ARIA sudah bergerak ke arah quant-autonomous-survival trading bot, tetapi belum layak dianggap siap mencari uang sendiri di live mode tanpa pengawasan. Perbaikan sizing/parse pada patch sebelumnya penting, tetapi langkah paling besar berikutnya adalah membuat simulasi paper lebih realistis dan menambahkan live-execution guard untuk exchange filters, liquidation, margin, stale data, dan maker-first execution.
