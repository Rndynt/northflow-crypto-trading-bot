use std::{fs, path::Path};

#[derive(Debug, Clone)]
pub struct ResearchConfig {
    // data
    pub symbols: Vec<String>,
    pub data_dir: String,
    pub reports_dir: String,
    // risk
    pub initial_equity: f64,
    pub risk_per_trade_pct: f64,
    pub max_open_positions: usize,
    pub max_leverage: f64,
    pub min_reward_risk: f64,
    pub max_daily_loss_pct: f64,
    pub max_drawdown_pct: f64,
    // cost
    pub taker_fee_bps: f64,
    pub slippage_bps: f64,
    pub spread_bps: f64,
    pub market_impact_bps: f64,
    // backtest
    pub conservative_intrabar: bool,
    pub min_confidence: u8,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            symbols: vec!["BTCUSDT".to_string()],
            data_dir: "data/historical".to_string(),
            reports_dir: "reports".to_string(),
            initial_equity: 5000.0,
            risk_per_trade_pct: 0.25,
            max_open_positions: 1,
            max_leverage: 3.0,
            min_reward_risk: 1.5,
            max_daily_loss_pct: 1.5,
            max_drawdown_pct: 5.0,
            taker_fee_bps: 4.0,
            slippage_bps: 2.0,
            spread_bps: 1.0,
            market_impact_bps: 1.0,
            conservative_intrabar: true,
            min_confidence: 65,
        }
    }
}

impl ResearchConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let raw = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("failed to read config {}: {e}", path.as_ref().display()))?;
        Ok(Self::parse(&raw))
    }

    pub fn parse(raw: &str) -> Self {
        let mut cfg = Self::default();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "symbols"               => cfg.symbols = parse_string_array(value),
                "data_dir"              => cfg.data_dir = value.to_string(),
                "reports_dir"           => cfg.reports_dir = value.to_string(),
                "initial_equity_usd"    => cfg.initial_equity = parse_f64(value, cfg.initial_equity),
                "risk_per_trade_pct"    => cfg.risk_per_trade_pct = parse_f64(value, cfg.risk_per_trade_pct),
                "max_open_positions"    => cfg.max_open_positions = value.parse().unwrap_or(cfg.max_open_positions),
                "max_leverage"          => cfg.max_leverage = parse_f64(value, cfg.max_leverage),
                "min_reward_risk"       => cfg.min_reward_risk = parse_f64(value, cfg.min_reward_risk),
                "max_daily_loss_pct"    => cfg.max_daily_loss_pct = parse_f64(value, cfg.max_daily_loss_pct),
                "max_drawdown_pct"      => cfg.max_drawdown_pct = parse_f64(value, cfg.max_drawdown_pct),
                "taker_fee_bps"         => cfg.taker_fee_bps = parse_f64(value, cfg.taker_fee_bps),
                "slippage_bps"          => cfg.slippage_bps = parse_f64(value, cfg.slippage_bps),
                "spread_bps"            => cfg.spread_bps = parse_f64(value, cfg.spread_bps),
                "market_impact_bps"     => cfg.market_impact_bps = parse_f64(value, cfg.market_impact_bps),
                "conservative_intrabar" => cfg.conservative_intrabar = value == "true",
                "min_confidence"        => cfg.min_confidence = value.parse().unwrap_or(cfg.min_confidence),
                _ => {}
            }
        }
        cfg
    }
}

fn parse_string_array(value: &str) -> Vec<String> {
    let trimmed = value.trim().trim_start_matches('[').trim_end_matches(']');
    let items: Vec<String> = trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() { vec!["BTCUSDT".to_string()] } else { items }
}

fn parse_f64(value: &str, default: f64) -> f64 {
    value.trim().parse::<f64>().unwrap_or(default)
}
