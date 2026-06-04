//! Generates synthetic 1m OHLCV CSV data for backtesting.
//!
//! Output: data/historical/BTCUSDT.csv
//! Candles: 6000 × 1m (~4.2 days, enough to warm up all indicators)
//!
//! Usage: cargo run --bin gen_sample_data

use std::fs;
use std::path::Path;

fn main() {
    let out_path = Path::new("data/historical/BTCUSDT.csv");
    fs::create_dir_all(out_path.parent().unwrap()).expect("cannot create data dir");

    // Deterministic LCG — no external deps, no system time.
    let mut rng = Lcg::new(0xDEAD_BEEF_1234_5678);

    // Start: 2024-01-01 00:00:00 UTC in milliseconds.
    let start_ms: i64 = 1_704_067_200_000;
    let interval_ms: i64 = 60_000; // 1 minute
    let n_candles: usize = 6_000;

    let mut rows: Vec<String> = Vec::with_capacity(n_candles + 1);
    rows.push("timestamp,open,high,low,close,volume".to_string());

    let mut price: f64 = 45_000.0;

    for i in 0..n_candles {
        let ts = start_ms + i as i64 * interval_ms;

        // Trending component: slow uptrend with regime-like swings.
        let trend_phase = (i as f64 / 300.0) * std::f64::consts::TAU;
        let trend = trend_phase.sin() * 800.0;

        // Random walk component.
        let noise = (rng.next_f64() - 0.5) * 120.0;

        let target = 45_000.0 + trend + noise;
        price += (target - price) * 0.05; // mean-revert toward target

        // Candle internals: open = prev close, then build high/low/close.
        let open = price;
        let body = (rng.next_f64() - 0.48) * 60.0; // slight bullish bias
        let close = open + body;
        let wick_up = rng.next_f64() * 40.0;
        let wick_dn = rng.next_f64() * 40.0;
        let high = open.max(close) + wick_up;
        let low = open.min(close) - wick_dn;

        // Volume: 0.3–8 BTC, higher during volatile bars.
        let base_vol = 0.3 + rng.next_f64() * 4.0;
        let vol_spike = if ((rng.next_f64() * 100.0) as u32) < 8 {
            rng.next_f64() * 6.0
        } else {
            0.0
        };
        let volume = base_vol + vol_spike + body.abs() / 30.0;

        price = close;

        rows.push(format!(
            "{},{:.2},{:.2},{:.2},{:.2},{:.4}",
            ts, open, high, low, close, volume
        ));
    }

    let content = rows.join("\n") + "\n";
    fs::write(out_path, &content).expect("cannot write CSV");

    println!(
        "Generated {} 1m candles → {}",
        n_candles,
        out_path.display()
    );
    println!(
        "Price range: ~{:.0} – ~{:.0} USDT",
        44_000.0_f64,
        46_000.0_f64
    );
    println!();
    println!("Run backtest:");
    println!("  cargo run -- research --config config/research.toml");
}

// ── Deterministic LCG ─────────────────────────────────────────────────────────

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        // Knuth's multiplicative LCG.
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn next_f64(&mut self) -> f64 {
        // Map to [0, 1).
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
