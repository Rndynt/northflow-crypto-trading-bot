use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchConfig {
    pub symbol: String,
    pub data_path: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub initial_capital: f64,
    pub strategy: StrategyConfig,
    pub risk: RiskConfig,
    pub report: ReportConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub name: String,
    pub ema_fast: usize,
    pub ema_slow: usize,
    pub atr_period: usize,
    pub vwap_period: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub max_position_pct: f64,
    pub stop_loss_atr_mult: f64,
    pub take_profit_atr_mult: f64,
    pub maker_fee: f64,
    pub taker_fee: f64,
    pub slippage_bps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportConfig {
    pub output_dir: String,
    pub csv: bool,
    pub json: bool,
}

impl ResearchConfig {
    pub fn from_toml<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ResearchConfig = toml::from_str(&content)?;
        Ok(config)
    }
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            symbol: "BTCUSDT".to_string(),
            data_path: "data/BTCUSDT_1h.csv".to_string(),
            start_date: None,
            end_date: None,
            initial_capital: 10_000.0,
            strategy: StrategyConfig {
                name: "ema_crossover".to_string(),
                ema_fast: 9,
                ema_slow: 21,
                atr_period: 14,
                vwap_period: 20,
            },
            risk: RiskConfig {
                max_position_pct: 0.02,
                stop_loss_atr_mult: 1.5,
                take_profit_atr_mult: 3.0,
                maker_fee: 0.001,
                taker_fee: 0.001,
                slippage_bps: 5.0,
            },
            report: ReportConfig {
                output_dir: "reports".to_string(),
                csv: true,
                json: true,
            },
        }
    }
}
