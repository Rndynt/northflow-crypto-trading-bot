use crate::core::{Candle, Side, Signal, SimTrade};
use crate::risk::RiskManager;

pub struct SimPosition {
    pub signal: Signal,
    pub qty: f64,
    pub entry_time: i64,
    pub entry_fill: f64,
}

pub struct SimExecutor {
    pub risk: RiskManager,
    pub position: Option<SimPosition>,
    pub trades: Vec<SimTrade>,
    pub conservative_intrabar: bool,
}

impl SimExecutor {
    pub fn new(risk: RiskManager, conservative_intrabar: bool) -> Self {
        Self {
            risk,
            position: None,
            trades: Vec::new(),
            conservative_intrabar,
        }
    }

    pub fn feed(&mut self, candle: Candle, signal: Option<Signal>) {
        // 1. Check stop-loss / take-profit on open position
        if let Some(pos) = &self.position {
            let (sl, tp, side) = (pos.signal.stop_loss, pos.signal.take_profit, pos.signal.side);

            let sl_hit = match side {
                Side::Buy => candle.low <= sl,
                Side::Sell => candle.high >= sl,
            };
            let tp_hit = match side {
                Side::Buy => candle.high >= tp,
                Side::Sell => candle.low <= tp,
            };

            if sl_hit || tp_hit {
                // Conservative: assume worst-case fill within the bar
                let exit_price = if sl_hit { sl } else { tp };
                self.close_position(candle.timestamp, exit_price, if sl_hit { "stop_loss" } else { "take_profit" });
            }
        }

        // 2. Open a new position if no open trade and signal present
        if self.position.is_none() {
            if let Some(sig) = signal {
                if let Some(qty) = self.risk.size_position(&sig) {
                    let fill = fill_price(&sig, candle, &self.risk, self.conservative_intrabar);
                    self.risk.open();
                    self.position = Some(SimPosition {
                        entry_fill: fill,
                        entry_time: candle.timestamp,
                        qty,
                        signal: sig,
                    });
                }
            }
        }
    }

    pub fn force_close_all(&mut self, candle: Candle) {
        if self.position.is_some() {
            self.close_position(candle.timestamp, candle.close, "end_of_data");
        }
    }

    fn close_position(&mut self, exit_time: i64, exit_price: f64, reason: &str) {
        let pos = self.position.take().unwrap();
        let cost_frac = self.risk.params.cost_fraction();
        let raw_pnl = match pos.signal.side {
            Side::Buy => (exit_price - pos.entry_fill) * pos.qty,
            Side::Sell => (pos.entry_fill - exit_price) * pos.qty,
        };
        let fees = (pos.entry_fill + exit_price) * pos.qty * cost_frac;
        let net_pnl = raw_pnl - fees;

        self.risk.close(net_pnl);
        self.trades.push(SimTrade {
            symbol: pos.signal.symbol.clone(),
            strategy: pos.signal.strategy.clone(),
            side: pos.signal.side,
            entry_time: pos.entry_time,
            exit_time,
            entry: pos.entry_fill,
            exit: exit_price,
            qty: pos.qty,
            net_pnl,
            exit_reason: reason.to_string(),
        });
    }
}

fn fill_price(
    signal: &Signal,
    candle: Candle,
    risk: &RiskManager,
    conservative: bool,
) -> f64 {
    let slippage = signal.entry * (risk.params.slippage_bps / 10_000.0);
    if conservative {
        match signal.side {
            Side::Buy => (signal.entry + slippage).max(candle.open),
            Side::Sell => (signal.entry - slippage).min(candle.open),
        }
    } else {
        match signal.side {
            Side::Buy => signal.entry + slippage,
            Side::Sell => signal.entry - slippage,
        }
    }
}
