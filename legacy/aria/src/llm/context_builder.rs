//! Build the Market Context Packet from TA + external feeds.

use crate::feeds::ExternalSnapshot;
use crate::strategy::Regime;
use crate::strategy::state::{PreSignal, SymbolState};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// Snapshot handed to the LLM. Cloneable so it can be logged after the call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketContext {
    pub symbol: String,
    pub current_price: f64,
    pub pre_signal_direction: String,
    pub ta_confidence: u8,
    pub regime: String,
    pub strategy: String,
    pub proposed_entry: f64,
    pub proposed_sl: f64,
    pub proposed_tp: f64,
    pub rsi: Option<f64>,
    pub adx: Option<f64>,
    pub di_plus: Option<f64>,
    pub di_minus: Option<f64>,
    pub atr: Option<f64>,
    pub vwap: Option<f64>,
    pub vwap_slope: Option<f64>,
    pub choppiness: Option<f64>,
    pub ema_8: Option<f64>,
    pub ema_21: Option<f64>,
    pub ema_50: Option<f64>,
    pub ema_200: Option<f64>,
    pub bb_upper: Option<f64>,
    pub bb_lower: Option<f64>,
    pub bb_mid: Option<f64>,
    pub spread_pct: Option<f64>,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub external: ExternalSnapshot,
    /// Optional historical-performance summary fed by the learning system.
    /// Empty string when the journal is cold.
    #[serde(default)]
    pub historical_summary: String,
    /// Strategy performance data for smarter decisions
    #[serde(default)]
    pub strategy_win_rate: f64,
    #[serde(default)]
    pub strategy_total_trades: u64,
    #[serde(default)]
    pub strategy_recent_pnl: f64,
    #[serde(default)]
    pub strategy_loss_streak: u64,
    #[serde(default)]
    pub overall_win_rate: f64,
    #[serde(default)]
    pub overall_total_trades: u64,
    #[serde(default)]
    pub recent_trade_pnl: f64,
    #[serde(default)]
    pub ofi: Option<f64>,
    #[serde(default)]
    pub vpin: Option<f64>,
    #[serde(default)]
    pub vpin_abnormal: bool,
}

pub struct ContextBuilder;

impl ContextBuilder {
    pub fn build(
        state: &SymbolState,
        regime: Regime,
        signal: &PreSignal,
        external: ExternalSnapshot,
    ) -> MarketContext {
        let price = state
            .order_book
            .best_bid()
            .zip(state.order_book.best_ask())
            .map(|(b, a)| (b + a) / 2.0)
            .or_else(|| state.last_candle().map(|c| c.close))
            .unwrap_or(0.0);
        MarketContext {
            symbol: state.symbol.clone(),
            current_price: price,
            pre_signal_direction: signal.side.as_str().to_string(),
            ta_confidence: signal.ta_confidence,
            regime: regime.as_str().to_string(),
            strategy: signal.strategy.as_str().to_string(),
            proposed_entry: signal.entry,
            proposed_sl: signal.stop_loss,
            proposed_tp: signal.take_profit,
            rsi: state.last_rsi,
            adx: state.last_adx,
            di_plus: state.last_di_plus,
            di_minus: state.last_di_minus,
            atr: state.last_atr,
            vwap: state.last_vwap,
            vwap_slope: state.last_vwap_slope,
            choppiness: state.last_choppiness,
            ema_8: state.ema_8.value(),
            ema_21: state.ema_21.value(),
            ema_50: state.ema_50.value(),
            ema_200: state.ema_200.value(),
            bb_upper: state.last_bb.map(|b| b.upper),
            bb_lower: state.last_bb.map(|b| b.lower),
            bb_mid: state.last_bb.map(|b| b.mid),
            spread_pct: state.order_book.spread_pct(),
            best_bid: state.order_book.best_bid(),
            best_ask: state.order_book.best_ask(),
            external,
            historical_summary: String::new(),
            strategy_win_rate: 0.0,
            strategy_total_trades: 0,
            strategy_recent_pnl: 0.0,
            strategy_loss_streak: 0,
            overall_win_rate: 0.0,
            overall_total_trades: 0,
            recent_trade_pnl: 0.0,
            ofi: state.last_ofi,
            vpin: state.last_vpin,
            vpin_abnormal: state.vpin_abnormal,
        }
    }
}

impl MarketContext {
    /// Serialize as the human-readable "Market Context Packet" from the blueprint.
    pub fn build_prompt(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "=== MARKET CONTEXT PACKET ===");
        let _ = writeln!(s);
        let _ = writeln!(s, "[ASSET INFO]");
        let _ = writeln!(s, "  Symbol        : {}", self.symbol);
        let _ = writeln!(s, "  Current Price : {:.2}", self.current_price);

        let _ = writeln!(s, "\n[TECHNICAL SNAPSHOT]");
        if let (Some(e8), Some(e21), Some(e50)) = (self.ema_8, self.ema_21, self.ema_50) {
            let _ = writeln!(s, "  EMA 8/21/50   : {e8:.2} / {e21:.2} / {e50:.2}");
        }
        if let Some(e) = self.ema_200 {
            let _ = writeln!(s, "  EMA 200       : {e:.2}");
        }
        if let Some(r) = self.rsi {
            let _ = writeln!(s, "  RSI (14)      : {r:.2}");
        }
        if let (Some(l), Some(m), Some(u)) = (self.bb_lower, self.bb_mid, self.bb_upper) {
            let _ = writeln!(s, "  BB (20,2)     : L:{l:.2} M:{m:.2} U:{u:.2}");
        }
        if let Some(v) = self.vwap {
            let _ = writeln!(s, "  VWAP          : {v:.2}");
        }
        if let Some(v) = self.vwap_slope {
            let _ = writeln!(s, "  VWAP slope    : {v:.6}");
        }
        if let Some(v) = self.atr {
            let _ = writeln!(s, "  ATR (14)      : {v:.2}");
        }
        if let (Some(a), Some(p), Some(m)) = (self.adx, self.di_plus, self.di_minus) {
            let _ = writeln!(s, "  ADX / DI±     : {a:.2} / {p:.2} / {m:.2}");
        }
        if let Some(c) = self.choppiness {
            let _ = writeln!(s, "  Choppiness    : {c:.2}");
        }
        let _ = writeln!(s, "  Regime        : {}", self.regime);
        let _ = writeln!(s, "  Strategy      : {}", self.strategy);
        let _ = writeln!(s, "  Pre-signal    : {}", self.pre_signal_direction);
        let _ = writeln!(s, "  TA Confidence : {}/100", self.ta_confidence);
        let _ = writeln!(s, "  Proposed Entry: {:.2}", self.proposed_entry);
        let _ = writeln!(s, "  Proposed SL   : {:.2}", self.proposed_sl);
        let _ = writeln!(s, "  Proposed TP   : {:.2}", self.proposed_tp);

        let _ = writeln!(s, "\n[ORDER BOOK]");
        if let (Some(b), Some(a)) = (self.best_bid, self.best_ask) {
            let _ = writeln!(s, "  Best bid/ask  : {b:.2} / {a:.2}");
        }
        if let Some(sp) = self.spread_pct {
            let _ = writeln!(s, "  Spread %      : {sp:.4}");
        }

        // Microstructure signals — OFI and VPIN
        let _ = writeln!(s, "\n[MICROSTRUCTURE]");
        if let Some(ofi) = self.ofi {
            let pressure = if ofi > 0.0 {
                "BUY"
            } else if ofi < 0.0 {
                "SELL"
            } else {
                "NEUTRAL"
            };
            let _ = writeln!(s, "  OFI           : {ofi:.2} ({pressure} pressure)");
        } else {
            let _ = writeln!(s, "  OFI           : N/A (waiting for data)");
        }
        if let Some(vpin) = self.vpin {
            let abnormal_flag = if self.vpin_abnormal {
                " 🚨 ABNORMAL — above 95th percentile"
            } else {
                ""
            };
            let risk = if self.vpin_abnormal {
                "🚨 EXTREME"
            } else if vpin > 0.5 {
                "⚠️ HIGH"
            } else if vpin > 0.3 {
                "MODERATE"
            } else {
                "LOW"
            };
            let _ = writeln!(
                s,
                "  VPIN          : {vpin:.3} ({risk} adverse selection risk){abnormal_flag}"
            );
        } else {
            let _ = writeln!(s, "  VPIN          : N/A (warming up, need 50 buckets)");
        }

        if let Some(fg) = &self.external.fear_greed {
            let _ = writeln!(s, "\n[FEAR & GREED]");
            let _ = writeln!(s, "  Value         : {} ({})", fg.value, fg.label.as_str());
            if let Some(a) = fg.avg_7d {
                let _ = writeln!(s, "  7-day average : {a}");
            }
        }

        if let Some(f) = &self.external.funding {
            let _ = writeln!(s, "\n[FUNDING]");
            let _ = writeln!(s, "  Rate          : {}", f.rate);
            if let Some(oi) = f.open_interest {
                let _ = writeln!(s, "  Open Interest : {oi}");
            }
        }

        if let Some(o) = &self.external.options {
            let _ = writeln!(s, "\n[OPTIONS SKEW]");
            let _ = writeln!(s, "  25d call IV   : {:.2}%", o.call_25d_iv * 100.0);
            let _ = writeln!(s, "  25d put IV    : {:.2}%", o.put_25d_iv * 100.0);
            let _ = writeln!(s, "  ATM IV        : {:.2}%", o.atm_iv * 100.0);
            let _ = writeln!(s, "  Skew bps      : {:+.1}", o.skew_bps());
            let _ = writeln!(s, "  Sentiment     : {:+.2}", o.sentiment_score());
        }

        if let Some(o) = &self.external.onchain {
            let _ = writeln!(s, "\n[ON-CHAIN]");
            if let Some(v) = o.exchange_inflow_24h {
                let _ = writeln!(s, "  Exch inflow 24h : {v}");
            }
            if let Some(v) = o.exchange_outflow_24h {
                let _ = writeln!(s, "  Exch outflow 24h: {v}");
            }
            if let Some(v) = o.whale_tx_1h {
                let _ = writeln!(s, "  Whale tx 1h     : {v}");
            }
            if let Some(v) = o.sopr_1h {
                let _ = writeln!(s, "  SOPR 1h         : {v}");
            }
        }

        if let Some(snt) = &self.external.sentiment {
            let _ = writeln!(s, "\n[SOCIAL SENTIMENT]");
            let _ = writeln!(
                s,
                "  Volume 24h    : {} (+{:.1}%)",
                snt.social_volume, snt.social_volume_change_pct
            );
            let _ = writeln!(s, "  Sentiment     : {:.2}", snt.sentiment);
            if let Some(g) = snt.galaxy_score {
                let _ = writeln!(s, "  Galaxy score  : {g:.2}");
            }
        }

        if !self.historical_summary.is_empty() {
            let _ = writeln!(s, "\n[HISTORICAL PERFORMANCE]");
            for line in self.historical_summary.lines() {
                let _ = writeln!(s, "  {line}");
            }
        }

        // Strategy performance data — critical for smart decisions
        let _ = writeln!(s, "\n[STRATEGY PERFORMANCE]");
        let _ = writeln!(s, "  Strategy        : {}", self.strategy);
        let _ = writeln!(
            s,
            "  Win rate        : {:.1}% ({}/{} trades)",
            self.strategy_win_rate * 100.0,
            (self.strategy_win_rate * self.strategy_total_trades as f64) as u64,
            self.strategy_total_trades
        );
        let _ = writeln!(s, "  Recent PnL      : ${:.2}", self.strategy_recent_pnl);
        let _ = writeln!(s, "  Loss streak     : {}", self.strategy_loss_streak);
        let _ = writeln!(
            s,
            "  Overall WR      : {:.1}% ({}/{} trades)",
            self.overall_win_rate * 100.0,
            (self.overall_win_rate * self.overall_total_trades as f64) as u64,
            self.overall_total_trades
        );
        if self.recent_trade_pnl != 0.0 {
            let _ = writeln!(s, "  Last trade PnL  : ${:.2}", self.recent_trade_pnl);
        }

        // New bot / insufficient data notice
        if self.strategy_total_trades < 10 {
            let _ = writeln!(
                s,
                "  ℹ️ NEW STRATEGY: Only {} trades recorded — WR is statistically meaningless. Judge by TA, OFI and regime ONLY.",
                self.strategy_total_trades
            );
        } else if self.strategy_total_trades >= 3 && self.strategy_win_rate < 0.40 {
            let _ = writeln!(
                s,
                "  ℹ️ LOW WR NOTE: Do not block for WR alone; reduce size only if dollar PnL/flow is bad."
            );
        }
        if self.strategy_loss_streak >= 3 {
            let _ = writeln!(
                s,
                "  ℹ️ LOSS STREAK: {} consecutive losses — prefer smaller probe size, not automatic WAIT.",
                self.strategy_loss_streak
            );
        }

        if let Some(n) = &self.external.news {
            let _ = writeln!(s, "\n[NEWS HEADLINES]");
            for item in n.items.iter().take(8) {
                let _ = writeln!(
                    s,
                    "  [{}] {} ({})",
                    item.impact.as_str(),
                    item.title,
                    item.source
                );
            }
            let _ = writeln!(s, "  Net score     : {:+.2}", n.net_score);
        }

        s
    }
}
