//! ResearchConfig — parsed from config/research.toml.
//!
//! Timeframe roles are explicit; never inferred from array order:
//!   entry_timeframe        = "1m"   (entry & execution)
//!   screening_timeframe    = "15m"  (regime bias)
//!   confirmation_timeframe = "5m"   (confirmation)

use std::{fs, path::Path};

use crate::core::{NorthflowError, Timeframe};

// ── V2Config ──────────────────────────────────────────────────────────────────

/// Configuration for the screened_vwap_scalp_v2 strategy.
///
/// All fields have safe defaults.  Unknown strategy_id must be rejected
/// by `ResearchConfig::validate_strategy_config()` before use.
#[derive(Debug, Clone)]
pub struct V2Config {
    pub require_strict_confirmation: bool,
    pub require_ema_ribbon_alignment: bool,
    pub allow_neutral_confirmation: bool,
    pub min_expected_reward_bps: f64,
    pub min_expected_net_edge_bps: f64,
    pub min_atr_bps: f64,
    pub max_atr_bps: f64,
    pub tp_atr_multiple: f64,
    pub sl_atr_multiple: f64,
    pub min_volume_ratio: f64,
    pub vwap_distance_atr_min: f64,
    pub vwap_distance_atr_max: f64,
    pub cooldown_bars: u64,
    pub enable_long: bool,
    pub enable_short: bool,
}

impl Default for V2Config {
    fn default() -> Self {
        Self {
            require_strict_confirmation: true,
            require_ema_ribbon_alignment: true,
            allow_neutral_confirmation: false,
            min_expected_reward_bps: 20.0,
            min_expected_net_edge_bps: 5.0,
            min_atr_bps: 5.0,
            max_atr_bps: 150.0,
            tp_atr_multiple: 2.0,
            sl_atr_multiple: 1.0,
            min_volume_ratio: 1.0,
            vwap_distance_atr_min: 0.0,
            vwap_distance_atr_max: 2.0,
            cooldown_bars: 0,
            enable_long: true,
            enable_short: true,
        }
    }
}

// ── EtpConfig ─────────────────────────────────────────────────────────────────

/// Configuration for the ema_trend_pullback_v1 strategy.
///
/// All fields have safe defaults.  Unknown strategy_id must be rejected
/// by `ResearchConfig::validate_strategy_config()` before use.
#[derive(Debug, Clone)]
pub struct EtpConfig {
    pub require_strict_15m_trend: bool,
    pub require_strict_5m_confirmation: bool,
    pub require_1m_ema_alignment: bool,
    pub allow_long: bool,
    pub allow_short: bool,
    pub pullback_to: String,
    pub max_pullback_distance_atr: f64,
    pub min_pullback_distance_atr: f64,
    pub reclaim_mode: String,
    pub min_body_ratio: f64,
    pub min_wick_rejection_ratio: f64,
    pub sl_atr_multiple: f64,
    pub tp_atr_multiple: f64,
    pub min_reward_risk: f64,
    pub min_atr_bps: f64,
    pub max_atr_bps: f64,
    pub min_expected_reward_bps: f64,
    pub min_expected_net_edge_bps: f64,
    pub min_volume_ratio: f64,
    pub cooldown_bars: u64,
}

impl Default for EtpConfig {
    fn default() -> Self {
        Self {
            require_strict_15m_trend: true,
            require_strict_5m_confirmation: true,
            require_1m_ema_alignment: true,
            allow_long: true,
            allow_short: true,
            pullback_to: "ema21_or_vwap".to_string(),
            max_pullback_distance_atr: 1.25,
            min_pullback_distance_atr: 0.0,
            reclaim_mode: "close_reclaim".to_string(),
            min_body_ratio: 0.30,
            min_wick_rejection_ratio: 0.20,
            sl_atr_multiple: 1.0,
            tp_atr_multiple: 3.0,
            min_reward_risk: 2.5,
            min_atr_bps: 8.0,
            max_atr_bps: 200.0,
            min_expected_reward_bps: 25.0,
            min_expected_net_edge_bps: 8.0,
            min_volume_ratio: 1.0,
            cooldown_bars: 10,
        }
    }
}

// ── ResearchConfig ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResearchConfig {
    // pairs
    pub symbols: Vec<String>,
    /// entry_timeframe = "1m"
    pub entry_timeframe: String,
    /// screening_timeframe = "15m"
    pub screening_timeframe: String,
    /// confirmation_timeframe = "5m"
    pub confirmation_timeframe: String,
    // data / output
    pub data_dir: String,
    pub reports_dir: String,
    // strategy selection
    pub strategy_id: String,
    // strategy runner
    pub strategy_run_mode: String,
    pub strategies: Vec<String>,
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
    pub stop_slippage_bps: f64,
    // backtest
    pub conservative_intrabar: bool,
    pub max_bars_held: u32,
    pub min_confidence: u8,
    pub entry_geometry_mode: String,
    // v2 strategy filters
    pub v2_require_strict_confirmation: bool,
    pub v2_require_ema_ribbon_alignment: bool,
    pub v2_allow_neutral_confirmation: bool,
    pub v2_min_expected_reward_bps: f64,
    pub v2_min_expected_net_edge_bps: f64,
    pub v2_min_atr_bps: f64,
    pub v2_max_atr_bps: f64,
    pub v2_tp_atr_multiple: f64,
    pub v2_sl_atr_multiple: f64,
    pub v2_min_volume_ratio: f64,
    pub v2_vwap_distance_atr_min: f64,
    pub v2_vwap_distance_atr_max: f64,
    pub v2_cooldown_bars: u64,
    pub v2_enable_long: bool,
    pub v2_enable_short: bool,
    // etp strategy filters
    pub etp_require_strict_15m_trend: bool,
    pub etp_require_strict_5m_confirmation: bool,
    pub etp_require_1m_ema_alignment: bool,
    pub etp_allow_long: bool,
    pub etp_allow_short: bool,
    pub etp_pullback_to: String,
    pub etp_max_pullback_distance_atr: f64,
    pub etp_min_pullback_distance_atr: f64,
    pub etp_reclaim_mode: String,
    pub etp_min_body_ratio: f64,
    pub etp_min_wick_rejection_ratio: f64,
    pub etp_sl_atr_multiple: f64,
    pub etp_tp_atr_multiple: f64,
    pub etp_min_reward_risk: f64,
    pub etp_min_atr_bps: f64,
    pub etp_max_atr_bps: f64,
    pub etp_min_expected_reward_bps: f64,
    pub etp_min_expected_net_edge_bps: f64,
    pub etp_min_volume_ratio: f64,
    pub etp_cooldown_bars: u64,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            symbols: vec!["BTCUSDT".to_string()],
            entry_timeframe: "1m".to_string(),
            screening_timeframe: "15m".to_string(),
            confirmation_timeframe: "5m".to_string(),
            data_dir: "data/historical".to_string(),
            reports_dir: "reports".to_string(),
            strategy_id: "screened_vwap_scalp".to_string(),
            strategy_run_mode: "single".to_string(),
            strategies: vec![],
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
            stop_slippage_bps: 5.0,
            conservative_intrabar: true,
            max_bars_held: 60,
            min_confidence: 65,
            entry_geometry_mode: "preserve_signal_levels".to_string(),
            v2_require_strict_confirmation: true,
            v2_require_ema_ribbon_alignment: true,
            v2_allow_neutral_confirmation: false,
            v2_min_expected_reward_bps: 20.0,
            v2_min_expected_net_edge_bps: 5.0,
            v2_min_atr_bps: 5.0,
            v2_max_atr_bps: 150.0,
            v2_tp_atr_multiple: 2.0,
            v2_sl_atr_multiple: 1.0,
            v2_min_volume_ratio: 1.0,
            v2_vwap_distance_atr_min: 0.0,
            v2_vwap_distance_atr_max: 2.0,
            v2_cooldown_bars: 0,
            v2_enable_long: true,
            v2_enable_short: true,
            etp_require_strict_15m_trend: true,
            etp_require_strict_5m_confirmation: true,
            etp_require_1m_ema_alignment: true,
            etp_allow_long: true,
            etp_allow_short: true,
            etp_pullback_to: "ema21_or_vwap".to_string(),
            etp_max_pullback_distance_atr: 1.25,
            etp_min_pullback_distance_atr: 0.0,
            etp_reclaim_mode: "close_reclaim".to_string(),
            etp_min_body_ratio: 0.30,
            etp_min_wick_rejection_ratio: 0.20,
            etp_sl_atr_multiple: 1.0,
            etp_tp_atr_multiple: 3.0,
            etp_min_reward_risk: 2.5,
            etp_min_atr_bps: 8.0,
            etp_max_atr_bps: 200.0,
            etp_min_expected_reward_bps: 25.0,
            etp_min_expected_net_edge_bps: 8.0,
            etp_min_volume_ratio: 1.0,
            etp_cooldown_bars: 10,
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
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "symbols" => cfg.symbols = parse_string_array(value),
                "entry_timeframe" => cfg.entry_timeframe = value.to_string(),
                "screening_timeframe" => cfg.screening_timeframe = value.to_string(),
                "confirmation_timeframe" => cfg.confirmation_timeframe = value.to_string(),
                "data_dir" => cfg.data_dir = value.to_string(),
                "reports_dir" => cfg.reports_dir = value.to_string(),
                // Accept both "strategy_id" and legacy "active" key.
                "strategy_id" | "active" => cfg.strategy_id = value.to_string(),
                // Strategy runner fields.
                "strategy_run_mode" => cfg.strategy_run_mode = value.to_string(),
                "strategies" => cfg.strategies = parse_strategies_array(value),
                "initial_equity_usd" => cfg.initial_equity = parse_f64(value, cfg.initial_equity),
                "risk_per_trade_pct" => {
                    cfg.risk_per_trade_pct = parse_f64(value, cfg.risk_per_trade_pct)
                }
                "max_open_positions" => {
                    cfg.max_open_positions = value.parse().unwrap_or(cfg.max_open_positions)
                }
                "max_leverage" => cfg.max_leverage = parse_f64(value, cfg.max_leverage),
                "min_reward_risk" => cfg.min_reward_risk = parse_f64(value, cfg.min_reward_risk),
                "max_daily_loss_pct" => {
                    cfg.max_daily_loss_pct = parse_f64(value, cfg.max_daily_loss_pct)
                }
                "max_drawdown_pct" => cfg.max_drawdown_pct = parse_f64(value, cfg.max_drawdown_pct),
                "taker_fee_bps" => cfg.taker_fee_bps = parse_f64(value, cfg.taker_fee_bps),
                "slippage_bps" => cfg.slippage_bps = parse_f64(value, cfg.slippage_bps),
                "spread_bps" => cfg.spread_bps = parse_f64(value, cfg.spread_bps),
                "market_impact_bps" => {
                    cfg.market_impact_bps = parse_f64(value, cfg.market_impact_bps)
                }
                "stop_slippage_bps" => {
                    cfg.stop_slippage_bps = parse_f64(value, cfg.stop_slippage_bps)
                }
                "conservative_intrabar" => cfg.conservative_intrabar = value == "true",
                "max_bars_held" => cfg.max_bars_held = value.parse().unwrap_or(cfg.max_bars_held),
                "min_confidence" => {
                    cfg.min_confidence = value.parse().unwrap_or(cfg.min_confidence)
                }
                "entry_geometry_mode" => cfg.entry_geometry_mode = value.to_string(),
                // V2 filters
                "v2_require_strict_confirmation" => {
                    cfg.v2_require_strict_confirmation = value == "true"
                }
                "v2_require_ema_ribbon_alignment" => {
                    cfg.v2_require_ema_ribbon_alignment = value == "true"
                }
                "v2_allow_neutral_confirmation" => {
                    cfg.v2_allow_neutral_confirmation = value == "true"
                }
                "v2_min_expected_reward_bps" => {
                    cfg.v2_min_expected_reward_bps =
                        parse_f64(value, cfg.v2_min_expected_reward_bps)
                }
                "v2_min_expected_net_edge_bps" => {
                    cfg.v2_min_expected_net_edge_bps =
                        parse_f64(value, cfg.v2_min_expected_net_edge_bps)
                }
                "v2_min_atr_bps" => cfg.v2_min_atr_bps = parse_f64(value, cfg.v2_min_atr_bps),
                "v2_max_atr_bps" => cfg.v2_max_atr_bps = parse_f64(value, cfg.v2_max_atr_bps),
                "v2_tp_atr_multiple" => {
                    cfg.v2_tp_atr_multiple = parse_f64(value, cfg.v2_tp_atr_multiple)
                }
                "v2_sl_atr_multiple" => {
                    cfg.v2_sl_atr_multiple = parse_f64(value, cfg.v2_sl_atr_multiple)
                }
                "v2_min_volume_ratio" => {
                    cfg.v2_min_volume_ratio = parse_f64(value, cfg.v2_min_volume_ratio)
                }
                "v2_vwap_distance_atr_min" => {
                    cfg.v2_vwap_distance_atr_min = parse_f64(value, cfg.v2_vwap_distance_atr_min)
                }
                "v2_vwap_distance_atr_max" => {
                    cfg.v2_vwap_distance_atr_max = parse_f64(value, cfg.v2_vwap_distance_atr_max)
                }
                "v2_cooldown_bars" => {
                    cfg.v2_cooldown_bars = value.parse().unwrap_or(cfg.v2_cooldown_bars)
                }
                "v2_enable_long" => cfg.v2_enable_long = value == "true",
                "v2_enable_short" => cfg.v2_enable_short = value == "true",
                // ETP filters
                "etp_require_strict_15m_trend" => {
                    cfg.etp_require_strict_15m_trend = value == "true"
                }
                "etp_require_strict_5m_confirmation" => {
                    cfg.etp_require_strict_5m_confirmation = value == "true"
                }
                "etp_require_1m_ema_alignment" => {
                    cfg.etp_require_1m_ema_alignment = value == "true"
                }
                "etp_allow_long" => cfg.etp_allow_long = value == "true",
                "etp_allow_short" => cfg.etp_allow_short = value == "true",
                "etp_pullback_to" => cfg.etp_pullback_to = value.to_string(),
                "etp_max_pullback_distance_atr" => {
                    cfg.etp_max_pullback_distance_atr =
                        parse_f64(value, cfg.etp_max_pullback_distance_atr)
                }
                "etp_min_pullback_distance_atr" => {
                    cfg.etp_min_pullback_distance_atr =
                        parse_f64(value, cfg.etp_min_pullback_distance_atr)
                }
                "etp_reclaim_mode" => cfg.etp_reclaim_mode = value.to_string(),
                "etp_min_body_ratio" => {
                    cfg.etp_min_body_ratio = parse_f64(value, cfg.etp_min_body_ratio)
                }
                "etp_min_wick_rejection_ratio" => {
                    cfg.etp_min_wick_rejection_ratio =
                        parse_f64(value, cfg.etp_min_wick_rejection_ratio)
                }
                "etp_sl_atr_multiple" => {
                    cfg.etp_sl_atr_multiple = parse_f64(value, cfg.etp_sl_atr_multiple)
                }
                "etp_tp_atr_multiple" => {
                    cfg.etp_tp_atr_multiple = parse_f64(value, cfg.etp_tp_atr_multiple)
                }
                "etp_min_reward_risk" => {
                    cfg.etp_min_reward_risk = parse_f64(value, cfg.etp_min_reward_risk)
                }
                "etp_min_atr_bps" => cfg.etp_min_atr_bps = parse_f64(value, cfg.etp_min_atr_bps),
                "etp_max_atr_bps" => cfg.etp_max_atr_bps = parse_f64(value, cfg.etp_max_atr_bps),
                "etp_min_expected_reward_bps" => {
                    cfg.etp_min_expected_reward_bps =
                        parse_f64(value, cfg.etp_min_expected_reward_bps)
                }
                "etp_min_expected_net_edge_bps" => {
                    cfg.etp_min_expected_net_edge_bps =
                        parse_f64(value, cfg.etp_min_expected_net_edge_bps)
                }
                "etp_min_volume_ratio" => {
                    cfg.etp_min_volume_ratio = parse_f64(value, cfg.etp_min_volume_ratio)
                }
                "etp_cooldown_bars" => {
                    cfg.etp_cooldown_bars = value.parse().unwrap_or(cfg.etp_cooldown_bars)
                }
                _ => {}
            }
        }
        cfg
    }

    /// Extract an `EtpConfig` from the etp_* fields of this `ResearchConfig`.
    pub fn etp_config(&self) -> EtpConfig {
        EtpConfig {
            require_strict_15m_trend: self.etp_require_strict_15m_trend,
            require_strict_5m_confirmation: self.etp_require_strict_5m_confirmation,
            require_1m_ema_alignment: self.etp_require_1m_ema_alignment,
            allow_long: self.etp_allow_long,
            allow_short: self.etp_allow_short,
            pullback_to: self.etp_pullback_to.clone(),
            max_pullback_distance_atr: self.etp_max_pullback_distance_atr,
            min_pullback_distance_atr: self.etp_min_pullback_distance_atr,
            reclaim_mode: self.etp_reclaim_mode.clone(),
            min_body_ratio: self.etp_min_body_ratio,
            min_wick_rejection_ratio: self.etp_min_wick_rejection_ratio,
            sl_atr_multiple: self.etp_sl_atr_multiple,
            tp_atr_multiple: self.etp_tp_atr_multiple,
            min_reward_risk: self.etp_min_reward_risk,
            min_atr_bps: self.etp_min_atr_bps,
            max_atr_bps: self.etp_max_atr_bps,
            min_expected_reward_bps: self.etp_min_expected_reward_bps,
            min_expected_net_edge_bps: self.etp_min_expected_net_edge_bps,
            min_volume_ratio: self.etp_min_volume_ratio,
            cooldown_bars: self.etp_cooldown_bars,
        }
    }

    /// Returns the cooldown_bars for the given strategy_id.
    ///
    /// - `screened_vwap_scalp_v2`   → `v2_cooldown_bars`
    /// - `ema_trend_pullback_v1`    → `etp_cooldown_bars`
    /// - `screened_vwap_scalp` / unknown → 0
    pub fn cooldown_bars_for_strategy(&self, strategy_id: &str) -> u64 {
        match strategy_id {
            "screened_vwap_scalp_v2" => self.v2_cooldown_bars,
            "ema_trend_pullback_v1" => self.etp_cooldown_bars,
            _ => 0,
        }
    }

    /// Extract a `V2Config` from the v2_* fields of this `ResearchConfig`.
    pub fn v2_config(&self) -> V2Config {
        V2Config {
            require_strict_confirmation: self.v2_require_strict_confirmation,
            require_ema_ribbon_alignment: self.v2_require_ema_ribbon_alignment,
            allow_neutral_confirmation: self.v2_allow_neutral_confirmation,
            min_expected_reward_bps: self.v2_min_expected_reward_bps,
            min_expected_net_edge_bps: self.v2_min_expected_net_edge_bps,
            min_atr_bps: self.v2_min_atr_bps,
            max_atr_bps: self.v2_max_atr_bps,
            tp_atr_multiple: self.v2_tp_atr_multiple,
            sl_atr_multiple: self.v2_sl_atr_multiple,
            min_volume_ratio: self.v2_min_volume_ratio,
            vwap_distance_atr_min: self.v2_vwap_distance_atr_min,
            vwap_distance_atr_max: self.v2_vwap_distance_atr_max,
            cooldown_bars: self.v2_cooldown_bars,
            enable_long: self.v2_enable_long,
            enable_short: self.v2_enable_short,
        }
    }

    /// Validate strategy_id and per-strategy numeric config.
    ///
    /// Returns `Err` for unknown strategy_id or invalid numeric values.
    /// Must be called before the backtest engine uses the strategy.
    pub fn validate_strategy_config(&self) -> Result<(), NorthflowError> {
        match self.strategy_id.as_str() {
            "screened_vwap_scalp" | "screened_vwap_scalp_v2" => {}
            "ema_trend_pullback_v1" => {
                return self.validate_etp_config();
            }
            other => {
                return Err(NorthflowError::ConfigError(format!(
                    "unknown strategy_id: '{other}'. \
                     Valid values: 'screened_vwap_scalp', 'screened_vwap_scalp_v2', \
                     'ema_trend_pullback_v1'"
                )));
            }
        }

        if !self.v2_tp_atr_multiple.is_finite() || self.v2_tp_atr_multiple <= 0.0 {
            return Err(NorthflowError::ConfigError(
                "v2_tp_atr_multiple must be finite and > 0".to_string(),
            ));
        }
        if !self.v2_sl_atr_multiple.is_finite() || self.v2_sl_atr_multiple <= 0.0 {
            return Err(NorthflowError::ConfigError(
                "v2_sl_atr_multiple must be finite and > 0".to_string(),
            ));
        }
        if !self.v2_min_expected_reward_bps.is_finite() || self.v2_min_expected_reward_bps < 0.0 {
            return Err(NorthflowError::ConfigError(
                "v2_min_expected_reward_bps must be finite and >= 0".to_string(),
            ));
        }
        if !self.v2_min_expected_net_edge_bps.is_finite() || self.v2_min_expected_net_edge_bps < 0.0
        {
            return Err(NorthflowError::ConfigError(
                "v2_min_expected_net_edge_bps must be finite and >= 0".to_string(),
            ));
        }
        if !self.v2_min_atr_bps.is_finite() || self.v2_min_atr_bps < 0.0 {
            return Err(NorthflowError::ConfigError(
                "v2_min_atr_bps must be finite and >= 0".to_string(),
            ));
        }
        if !self.v2_max_atr_bps.is_finite() || self.v2_max_atr_bps <= self.v2_min_atr_bps {
            return Err(NorthflowError::ConfigError(
                "v2_max_atr_bps must be finite and > v2_min_atr_bps".to_string(),
            ));
        }
        if !self.v2_min_volume_ratio.is_finite() || self.v2_min_volume_ratio < 0.0 {
            return Err(NorthflowError::ConfigError(
                "v2_min_volume_ratio must be finite and >= 0".to_string(),
            ));
        }
        if !self.v2_vwap_distance_atr_min.is_finite() || self.v2_vwap_distance_atr_min < 0.0 {
            return Err(NorthflowError::ConfigError(
                "v2_vwap_distance_atr_min must be finite and >= 0".to_string(),
            ));
        }
        if !self.v2_vwap_distance_atr_max.is_finite()
            || self.v2_vwap_distance_atr_max < self.v2_vwap_distance_atr_min
        {
            return Err(NorthflowError::ConfigError(
                "v2_vwap_distance_atr_max must be finite and >= v2_vwap_distance_atr_min"
                    .to_string(),
            ));
        }

        Ok(())
    }

    /// Validate ETP-specific config fields.
    ///
    /// Returns `Err` for unknown pullback_to / reclaim_mode or invalid numerics.
    pub fn validate_etp_config(&self) -> Result<(), NorthflowError> {
        match self.etp_pullback_to.as_str() {
            "ema21" | "ema50" | "vwap" | "ema21_or_vwap" | "ema21_or_ema50_or_vwap" => {}
            other => {
                return Err(NorthflowError::ConfigError(format!(
                    "unknown etp_pullback_to: '{other}'. \
                     Valid values: 'ema21', 'ema50', 'vwap', 'ema21_or_vwap', \
                     'ema21_or_ema50_or_vwap'"
                )));
            }
        }
        match self.etp_reclaim_mode.as_str() {
            "close_reclaim" | "wick_rejection" | "close_reclaim_or_wick" => {}
            other => {
                return Err(NorthflowError::ConfigError(format!(
                    "unknown etp_reclaim_mode: '{other}'. \
                     Valid values: 'close_reclaim', 'wick_rejection', 'close_reclaim_or_wick'"
                )));
            }
        }
        if !self.etp_min_atr_bps.is_finite() || self.etp_min_atr_bps < 0.0 {
            return Err(NorthflowError::ConfigError(
                "etp_min_atr_bps must be finite and >= 0".to_string(),
            ));
        }
        if !self.etp_max_atr_bps.is_finite() || self.etp_max_atr_bps <= self.etp_min_atr_bps {
            return Err(NorthflowError::ConfigError(
                "etp_max_atr_bps must be finite and > etp_min_atr_bps".to_string(),
            ));
        }
        if !self.etp_min_pullback_distance_atr.is_finite()
            || self.etp_min_pullback_distance_atr < 0.0
        {
            return Err(NorthflowError::ConfigError(
                "etp_min_pullback_distance_atr must be finite and >= 0".to_string(),
            ));
        }
        if !self.etp_max_pullback_distance_atr.is_finite()
            || self.etp_max_pullback_distance_atr < self.etp_min_pullback_distance_atr
        {
            return Err(NorthflowError::ConfigError(
                "etp_max_pullback_distance_atr must be finite and >= etp_min_pullback_distance_atr"
                    .to_string(),
            ));
        }
        if !self.etp_min_body_ratio.is_finite()
            || self.etp_min_body_ratio < 0.0
            || self.etp_min_body_ratio > 1.0
        {
            return Err(NorthflowError::ConfigError(
                "etp_min_body_ratio must be finite and in [0, 1]".to_string(),
            ));
        }
        if !self.etp_min_wick_rejection_ratio.is_finite()
            || self.etp_min_wick_rejection_ratio < 0.0
            || self.etp_min_wick_rejection_ratio > 1.0
        {
            return Err(NorthflowError::ConfigError(
                "etp_min_wick_rejection_ratio must be finite and in [0, 1]".to_string(),
            ));
        }
        if !self.etp_sl_atr_multiple.is_finite() || self.etp_sl_atr_multiple <= 0.0 {
            return Err(NorthflowError::ConfigError(
                "etp_sl_atr_multiple must be finite and > 0".to_string(),
            ));
        }
        if !self.etp_tp_atr_multiple.is_finite() || self.etp_tp_atr_multiple <= 0.0 {
            return Err(NorthflowError::ConfigError(
                "etp_tp_atr_multiple must be finite and > 0".to_string(),
            ));
        }
        if !self.etp_min_reward_risk.is_finite() || self.etp_min_reward_risk < 1.0 {
            return Err(NorthflowError::ConfigError(
                "etp_min_reward_risk must be finite and >= 1.0".to_string(),
            ));
        }
        if !self.etp_min_expected_reward_bps.is_finite() || self.etp_min_expected_reward_bps < 0.0 {
            return Err(NorthflowError::ConfigError(
                "etp_min_expected_reward_bps must be finite and >= 0".to_string(),
            ));
        }
        if !self.etp_min_expected_net_edge_bps.is_finite()
            || self.etp_min_expected_net_edge_bps < 0.0
        {
            return Err(NorthflowError::ConfigError(
                "etp_min_expected_net_edge_bps must be finite and >= 0".to_string(),
            ));
        }
        if !self.etp_min_volume_ratio.is_finite() || self.etp_min_volume_ratio < 0.0 {
            return Err(NorthflowError::ConfigError(
                "etp_min_volume_ratio must be finite and >= 0".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate strategy runner config: run mode, strategy list, duplicates, reserved modes.
    ///
    /// Must be called after `validate_strategy_config()`.
    pub fn validate_strategy_runner_config(&self) -> Result<(), NorthflowError> {
        match self.strategy_run_mode.as_str() {
            "multi" => {
                return Err(NorthflowError::ConfigError(
                    "multi-strategy portfolio backtest is not implemented yet; \
                     use strategy_run_mode = \"comparison\""
                        .to_string(),
                ));
            }
            "single" | "comparison" => {}
            other => {
                return Err(NorthflowError::ConfigError(format!(
                    "unknown strategy_run_mode: '{other}'. \
                     Valid values: 'single', 'comparison', 'multi'"
                )));
            }
        }

        // Check for duplicates in strategies list.
        let mut seen = std::collections::HashSet::new();
        for s in &self.strategies {
            if !seen.insert(s.as_str()) {
                return Err(NorthflowError::ConfigError(format!(
                    "duplicate strategy in strategies list: '{s}'"
                )));
            }
        }

        // Validate each strategy ID in strategies list.
        for s in &self.strategies {
            match s.as_str() {
                "screened_vwap_scalp" | "screened_vwap_scalp_v2" | "ema_trend_pullback_v1" => {}
                other => {
                    return Err(NorthflowError::ConfigError(format!(
                        "unknown strategy in strategies list: '{other}'. \
                         Valid values: 'screened_vwap_scalp', 'screened_vwap_scalp_v2', \
                         'ema_trend_pullback_v1'"
                    )));
                }
            }
        }

        // Single mode: strategies list must have 0 or 1 items.
        if self.strategy_run_mode == "single" && self.strategies.len() > 1 {
            return Err(NorthflowError::ConfigError(format!(
                "strategy_run_mode = 'single' requires at most one strategy in strategies, \
                 but got {}: [{}]. Use strategy_run_mode = 'comparison' to run multiple strategies.",
                self.strategies.len(),
                self.strategies.join(", ")
            )));
        }

        // Comparison mode: strategies list must have at least one item.
        if self.strategy_run_mode == "comparison" && self.strategies.is_empty() {
            return Err(NorthflowError::ConfigError(
                "strategy_run_mode = 'comparison' requires at least one strategy \
                 in strategies list"
                    .to_string(),
            ));
        }

        Ok(())
    }

    /// Returns the ordered list of strategies to run, based on run mode and fallback rules.
    ///
    /// - `single`: returns `[strategies[0]]` if strategies is non-empty, else `[strategy_id]`.
    /// - `comparison`: returns `strategies.clone()`.
    /// - `multi`: returns `ConfigError` (not implemented).
    /// - unknown mode: returns `ConfigError`.
    pub fn selected_strategies(&self) -> Result<Vec<String>, NorthflowError> {
        match self.strategy_run_mode.as_str() {
            "single" => {
                if self.strategies.is_empty() {
                    Ok(vec![self.strategy_id.clone()])
                } else {
                    Ok(vec![self.strategies[0].clone()])
                }
            }
            "comparison" => Ok(self.strategies.clone()),
            "multi" => Err(NorthflowError::ConfigError(
                "multi-strategy portfolio backtest is not implemented yet; \
                 use strategy_run_mode = \"comparison\""
                    .to_string(),
            )),
            other => Err(NorthflowError::ConfigError(format!(
                "unknown strategy_run_mode: '{other}'. \
                 Valid values: 'single', 'comparison', 'multi'"
            ))),
        }
    }

    /// Returns a cloned config with `strategy_id` and `reports_dir` overridden.
    ///
    /// Used to run one strategy at a time in comparison mode without mutating the root config.
    pub fn with_strategy_for_run(&self, strategy_id: &str, reports_dir: String) -> Self {
        let mut cfg = self.clone();
        cfg.strategy_id = strategy_id.to_string();
        cfg.reports_dir = reports_dir;
        cfg
    }

    /// Build a RiskConfig from this ResearchConfig.
    pub fn risk_config(&self) -> crate::risk::RiskConfig {
        crate::risk::RiskConfig {
            risk_per_trade_pct: self.risk_per_trade_pct,
            max_open_positions: self.max_open_positions,
            max_leverage: self.max_leverage,
            min_reward_risk: self.min_reward_risk,
            max_daily_loss_pct: self.max_daily_loss_pct,
            max_drawdown_pct: self.max_drawdown_pct,
        }
    }

    /// Build a CostModelConfig from this ResearchConfig.
    pub fn cost_model_config(&self) -> crate::risk::CostModelConfig {
        crate::risk::CostModelConfig {
            taker_fee_bps: self.taker_fee_bps,
            slippage_bps: self.slippage_bps,
            spread_bps: self.spread_bps,
            market_impact_bps: self.market_impact_bps,
            stop_slippage_bps: self.stop_slippage_bps,
        }
    }

    /// Validate that the three explicit timeframe roles match Phase 2 requirements:
    ///   entry_timeframe        = "1m"
    ///   screening_timeframe    = "15m"
    ///   confirmation_timeframe = "5m"
    ///
    /// Returns `Err` if any value is unparseable or assigned to the wrong role.
    pub fn validate_timeframes(&self) -> Result<(), NorthflowError> {
        let entry = Timeframe::from_str(&self.entry_timeframe)
            .map_err(|e| NorthflowError::ConfigError(format!("entry_timeframe invalid: {e}")))?;
        let screening = Timeframe::from_str(&self.screening_timeframe).map_err(|e| {
            NorthflowError::ConfigError(format!("screening_timeframe invalid: {e}"))
        })?;
        let confirmation = Timeframe::from_str(&self.confirmation_timeframe).map_err(|e| {
            NorthflowError::ConfigError(format!("confirmation_timeframe invalid: {e}"))
        })?;

        if entry != Timeframe::OneMinute {
            return Err(NorthflowError::ConfigError(format!(
                "entry_timeframe must be '1m', got '{}'. \
                 Northflow Phase 2 expects: entry=1m, screening=15m, confirmation=5m",
                self.entry_timeframe
            )));
        }
        if screening != Timeframe::FifteenMinute {
            return Err(NorthflowError::ConfigError(format!(
                "screening_timeframe must be '15m', got '{}'. \
                 Northflow Phase 2 expects: entry=1m, screening=15m, confirmation=5m",
                self.screening_timeframe
            )));
        }
        if confirmation != Timeframe::FiveMinute {
            return Err(NorthflowError::ConfigError(format!(
                "confirmation_timeframe must be '5m', got '{}'. \
                 Northflow Phase 2 expects: entry=1m, screening=15m, confirmation=5m",
                self.confirmation_timeframe
            )));
        }

        Ok(())
    }
}

fn parse_string_array(value: &str) -> Vec<String> {
    let trimmed = value.trim().trim_start_matches('[').trim_end_matches(']');
    let items: Vec<String> = trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        vec!["BTCUSDT".to_string()]
    } else {
        items
    }
}

/// Parse a TOML-style string array, returning an empty vec when empty.
/// Unlike `parse_string_array`, does not fall back to a default value.
fn parse_strategies_array(value: &str) -> Vec<String> {
    let trimmed = value.trim().trim_start_matches('[').trim_end_matches(']');
    trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_f64(value: &str, default: f64) -> f64 {
    value.trim().parse::<f64>().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_cfg() -> ResearchConfig {
        ResearchConfig::default()
    }

    #[test]
    fn valid_explicit_timeframe_config_passes() {
        let cfg = default_cfg();
        assert!(cfg.validate_timeframes().is_ok());
    }

    #[test]
    fn invalid_entry_timeframe_string_fails() {
        let mut cfg = default_cfg();
        cfg.entry_timeframe = "4h".to_string();
        let err = cfg.validate_timeframes().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("entry_timeframe"),
            "expected mention of field: {msg}"
        );
    }

    #[test]
    fn invalid_screening_timeframe_string_fails() {
        let mut cfg = default_cfg();
        cfg.screening_timeframe = "badval".to_string();
        assert!(cfg.validate_timeframes().is_err());
    }

    #[test]
    fn wrong_entry_timeframe_role_fails() {
        let mut cfg = default_cfg();
        cfg.entry_timeframe = "15m".to_string();
        let err = cfg.validate_timeframes().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("entry=1m"),
            "error should mention expected roles: {msg}"
        );
    }

    #[test]
    fn wrong_screening_timeframe_role_fails() {
        let mut cfg = default_cfg();
        cfg.screening_timeframe = "1m".to_string();
        let err = cfg.validate_timeframes().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("entry=1m"),
            "error should list expected roles: {msg}"
        );
    }

    #[test]
    fn wrong_confirmation_timeframe_role_fails() {
        let mut cfg = default_cfg();
        cfg.confirmation_timeframe = "1h".to_string();
        assert!(cfg.validate_timeframes().is_err());
    }

    #[test]
    fn config_parses_stop_slippage_bps() {
        let toml = "[cost]\nstop_slippage_bps = 7.5\n";
        let cfg = ResearchConfig::parse(toml);
        assert!((cfg.stop_slippage_bps - 7.5).abs() < 1e-9);
    }

    #[test]
    fn default_stop_slippage_bps_is_positive() {
        let cfg = ResearchConfig::default();
        assert!(cfg.stop_slippage_bps > 0.0);
    }

    #[test]
    fn cost_model_config_from_research_config_contains_stop_slippage() {
        let cfg = ResearchConfig::default();
        let cost = cfg.cost_model_config();
        assert!((cost.stop_slippage_bps - cfg.stop_slippage_bps).abs() < 1e-9);
    }

    // ── Strategy config tests ─────────────────────────────────────────────────

    #[test]
    fn parses_strategy_id_v1() {
        let toml = "[strategy]\nstrategy_id = \"screened_vwap_scalp\"\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(cfg.strategy_id, "screened_vwap_scalp");
        assert!(cfg.validate_strategy_config().is_ok());
    }

    #[test]
    fn parses_strategy_id_v2() {
        let toml = "[strategy]\nstrategy_id = \"screened_vwap_scalp_v2\"\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(cfg.strategy_id, "screened_vwap_scalp_v2");
        assert!(cfg.validate_strategy_config().is_ok());
    }

    #[test]
    fn rejects_unknown_strategy_id() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "bad_strategy_xyz".to_string();
        let err = cfg.validate_strategy_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bad_strategy_xyz"),
            "error must mention the bad id: {msg}"
        );
    }

    #[test]
    fn v2_defaults_are_safe() {
        let cfg = default_cfg();
        assert!(cfg.validate_strategy_config().is_ok());
        let v2 = cfg.v2_config();
        assert!(v2.tp_atr_multiple > 0.0);
        assert!(v2.sl_atr_multiple > 0.0);
        assert!(v2.max_atr_bps > v2.min_atr_bps);
        assert!(v2.vwap_distance_atr_max >= v2.vwap_distance_atr_min);
    }

    #[test]
    fn parses_v2_tp_sl_multipliers() {
        let toml = "[strategy]\nstrategy_id = \"screened_vwap_scalp_v2\"\nv2_tp_atr_multiple = 2.5\nv2_sl_atr_multiple = 1.0\n";
        let cfg = ResearchConfig::parse(toml);
        assert!((cfg.v2_tp_atr_multiple - 2.5).abs() < 1e-9);
        assert!((cfg.v2_sl_atr_multiple - 1.0).abs() < 1e-9);
        assert!(cfg.validate_strategy_config().is_ok());
    }

    #[test]
    fn rejects_invalid_v2_tp_multiplier() {
        let mut cfg = default_cfg();
        cfg.v2_tp_atr_multiple = 0.0;
        assert!(cfg.validate_strategy_config().is_err());

        cfg.v2_tp_atr_multiple = -1.0;
        assert!(cfg.validate_strategy_config().is_err());
    }

    #[test]
    fn rejects_invalid_v2_atr_range() {
        let mut cfg = default_cfg();
        cfg.v2_min_atr_bps = 50.0;
        cfg.v2_max_atr_bps = 30.0;
        assert!(cfg.validate_strategy_config().is_err());
    }

    // ── Strategy runner config tests ──────────────────────────────────────────

    #[test]
    fn parses_strategy_run_mode_single() {
        let toml = "[backtest]\nstrategy_run_mode = \"single\"\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(cfg.strategy_run_mode, "single");
    }

    #[test]
    fn parses_strategy_run_mode_comparison() {
        let toml = "[backtest]\nstrategy_run_mode = \"comparison\"\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(cfg.strategy_run_mode, "comparison");
    }

    #[test]
    fn parses_backtest_strategies_array() {
        let toml =
            "[backtest]\nstrategies = [\"screened_vwap_scalp\", \"screened_vwap_scalp_v2\"]\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(
            cfg.strategies,
            vec!["screened_vwap_scalp", "screened_vwap_scalp_v2"]
        );
    }

    #[test]
    fn default_strategy_run_mode_is_single() {
        let cfg = default_cfg();
        assert_eq!(cfg.strategy_run_mode, "single");
        assert!(cfg.strategies.is_empty());
    }

    #[test]
    fn selected_strategies_falls_back_to_strategy_id() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "screened_vwap_scalp_v2".to_string();
        cfg.strategy_run_mode = "single".to_string();
        cfg.strategies = vec![];
        let strats = cfg.selected_strategies().unwrap();
        assert_eq!(strats, vec!["screened_vwap_scalp_v2"]);
    }

    #[test]
    fn selected_strategies_uses_backtest_strategies() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "single".to_string();
        cfg.strategies = vec!["screened_vwap_scalp_v2".to_string()];
        let strats = cfg.selected_strategies().unwrap();
        assert_eq!(strats, vec!["screened_vwap_scalp_v2"]);
    }

    #[test]
    fn selected_strategies_comparison_returns_all() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "comparison".to_string();
        cfg.strategies = vec![
            "screened_vwap_scalp".to_string(),
            "screened_vwap_scalp_v2".to_string(),
        ];
        let strats = cfg.selected_strategies().unwrap();
        assert_eq!(
            strats,
            vec!["screened_vwap_scalp", "screened_vwap_scalp_v2"]
        );
    }

    #[test]
    fn rejects_unknown_strategy_run_mode() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "turbo_mode".to_string();
        let err = cfg.validate_strategy_runner_config().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("turbo_mode"), "must name the bad mode: {msg}");
    }

    #[test]
    fn rejects_duplicate_strategies() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "comparison".to_string();
        cfg.strategies = vec![
            "screened_vwap_scalp".to_string(),
            "screened_vwap_scalp".to_string(),
        ];
        let err = cfg.validate_strategy_runner_config().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("duplicate"), "must say duplicate: {msg}");
    }

    #[test]
    fn rejects_unknown_strategy_in_strategies() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "comparison".to_string();
        cfg.strategies = vec!["screened_vwap_scalp".to_string(), "bad_strat".to_string()];
        let err = cfg.validate_strategy_runner_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bad_strat"),
            "must name the bad strategy: {msg}"
        );
    }

    #[test]
    fn rejects_multi_mode_as_not_implemented() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "multi".to_string();
        let err = cfg.validate_strategy_runner_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not implemented"),
            "must say not implemented: {msg}"
        );
        assert!(
            msg.contains("comparison"),
            "must suggest comparison mode: {msg}"
        );
    }

    #[test]
    fn single_mode_rejects_multiple_strategies() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "single".to_string();
        cfg.strategies = vec![
            "screened_vwap_scalp".to_string(),
            "screened_vwap_scalp_v2".to_string(),
        ];
        let err = cfg.validate_strategy_runner_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("comparison"),
            "must suggest comparison mode: {msg}"
        );
    }

    #[test]
    fn comparison_mode_requires_at_least_one_strategy() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "comparison".to_string();
        cfg.strategies = vec![];
        let err = cfg.validate_strategy_runner_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("at least one"),
            "must mention at least one: {msg}"
        );
    }

    #[test]
    fn with_strategy_for_run_overrides_strategy_and_reports_dir() {
        let cfg = default_cfg();
        let run_cfg = cfg.with_strategy_for_run("screened_vwap_scalp_v2", "reports/v2".to_string());
        assert_eq!(run_cfg.strategy_id, "screened_vwap_scalp_v2");
        assert_eq!(run_cfg.reports_dir, "reports/v2");
        // Other fields unchanged.
        assert_eq!(run_cfg.initial_equity, cfg.initial_equity);
        assert_eq!(run_cfg.symbols, cfg.symbols);
    }

    #[test]
    fn single_mode_with_empty_strategies_and_strategy_id_passes_validation() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "single".to_string();
        cfg.strategy_id = "screened_vwap_scalp_v2".to_string();
        cfg.strategies = vec![];
        assert!(cfg.validate_strategy_runner_config().is_ok());
        let strats = cfg.selected_strategies().unwrap();
        assert_eq!(strats, vec!["screened_vwap_scalp_v2"]);
    }

    // ── ETP config tests ──────────────────────────────────────────────────────

    #[test]
    fn parses_strategy_id_ema_trend_pullback_v1() {
        let toml = "[strategy]\nstrategy_id = \"ema_trend_pullback_v1\"\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(cfg.strategy_id, "ema_trend_pullback_v1");
        assert!(
            cfg.validate_strategy_config().is_ok(),
            "default etp config must pass validation"
        );
    }

    #[test]
    fn rejects_unknown_strategy_id_still() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "totally_unknown_xyz".to_string();
        let err = cfg.validate_strategy_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("totally_unknown_xyz"),
            "error must name the bad id: {msg}"
        );
    }

    #[test]
    fn parses_etp_defaults() {
        let cfg = ResearchConfig::default();
        let etp = cfg.etp_config();
        assert!(etp.require_strict_15m_trend);
        assert!(etp.require_1m_ema_alignment);
        assert!(etp.allow_long);
        assert!(etp.allow_short);
        assert_eq!(etp.pullback_to, "ema21_or_vwap");
        assert_eq!(etp.reclaim_mode, "close_reclaim");
        assert!(etp.tp_atr_multiple > 0.0);
        assert!(etp.sl_atr_multiple > 0.0);
        assert!(etp.max_atr_bps > etp.min_atr_bps);
    }

    #[test]
    fn parses_etp_pullback_to_ema21_or_vwap() {
        let toml = "[strategy]\netp_pullback_to = \"ema21_or_vwap\"\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(cfg.etp_pullback_to, "ema21_or_vwap");
    }

    #[test]
    fn rejects_unknown_etp_pullback_to() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "ema_trend_pullback_v1".to_string();
        cfg.etp_pullback_to = "fib_retracement".to_string();
        let err = cfg.validate_strategy_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("fib_retracement"),
            "error must name the bad value: {msg}"
        );
    }

    #[test]
    fn parses_etp_reclaim_mode_close_reclaim() {
        let toml = "[strategy]\netp_reclaim_mode = \"close_reclaim\"\n";
        let cfg = ResearchConfig::parse(toml);
        assert_eq!(cfg.etp_reclaim_mode, "close_reclaim");
    }

    #[test]
    fn rejects_unknown_etp_reclaim_mode() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "ema_trend_pullback_v1".to_string();
        cfg.etp_reclaim_mode = "magic_candle".to_string();
        let err = cfg.validate_strategy_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("magic_candle"),
            "error must name the bad value: {msg}"
        );
    }

    #[test]
    fn rejects_invalid_etp_atr_range() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "ema_trend_pullback_v1".to_string();
        cfg.etp_min_atr_bps = 100.0;
        cfg.etp_max_atr_bps = 50.0; // max < min
        let err = cfg.validate_strategy_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("etp_max_atr_bps"),
            "error must mention the field: {msg}"
        );
    }

    #[test]
    fn rejects_invalid_etp_tp_sl_multiple() {
        let mut cfg = default_cfg();
        cfg.strategy_id = "ema_trend_pullback_v1".to_string();
        cfg.etp_tp_atr_multiple = -1.0;
        assert!(cfg.validate_strategy_config().is_err());

        cfg.etp_tp_atr_multiple = 3.0;
        cfg.etp_sl_atr_multiple = 0.0;
        assert!(cfg.validate_strategy_config().is_err());
    }

    #[test]
    fn cooldown_bars_for_strategy_returns_etp_value() {
        let mut cfg = default_cfg();
        cfg.etp_cooldown_bars = 15;
        cfg.v2_cooldown_bars = 5;
        assert_eq!(cfg.cooldown_bars_for_strategy("ema_trend_pullback_v1"), 15);
        assert_eq!(cfg.cooldown_bars_for_strategy("screened_vwap_scalp_v2"), 5);
        assert_eq!(cfg.cooldown_bars_for_strategy("screened_vwap_scalp"), 0);
        assert_eq!(cfg.cooldown_bars_for_strategy("unknown"), 0);
    }

    #[test]
    fn comparison_accepts_ema_trend_pullback_v1_in_strategies() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "comparison".to_string();
        cfg.strategies = vec![
            "screened_vwap_scalp".to_string(),
            "screened_vwap_scalp_v2".to_string(),
            "ema_trend_pullback_v1".to_string(),
        ];
        assert!(cfg.validate_strategy_runner_config().is_ok());
    }

    #[test]
    fn unknown_strategy_validation_still_fails() {
        let mut cfg = default_cfg();
        cfg.strategy_run_mode = "comparison".to_string();
        cfg.strategies = vec!["ema_trend_pullback_v1".to_string(), "bad_strat".to_string()];
        let err = cfg.validate_strategy_runner_config().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bad_strat"),
            "must name the bad strategy: {msg}"
        );
    }
}
