use crate::core::{Candle, Side, Signal, Trade};
use crate::config::ResearchConfig;
use crate::risk::{compute_quantity, entry_cost, exit_cost, stop_loss_price, take_profit_price};

pub struct SimState {
    pub capital: f64,
    pub trades: Vec<Trade>,
}

struct OpenPosition {
    side: Side,
    entry_price: f64,
    quantity: f64,
    entry_time: chrono::DateTime<chrono::Utc>,
    fee_in: f64,
    stop_loss: f64,
    take_profit: f64,
    trade_id: u64,
}

pub fn run_simulation(
    candles: &[Candle],
    signals: &[Signal],
    atr_vals: &[f64],
    cfg: &ResearchConfig,
    symbol: &str,
) -> SimState {
    let mut capital = cfg.initial_capital;
    let mut trades: Vec<Trade> = Vec::new();
    let mut open: Option<OpenPosition> = None;
    let mut next_id = 1u64;

    for (i, candle) in candles.iter().enumerate() {
        let sig = &signals[i];
        let atr = atr_vals[i];

        // Check SL/TP on open position
        if let Some(ref pos) = open {
            let exit_triggered = match pos.side {
                Side::Long => candle.low <= pos.stop_loss || candle.high >= pos.take_profit,
                Side::Short => candle.high >= pos.stop_loss || candle.low <= pos.take_profit,
            };
            if exit_triggered {
                let exit_price = match pos.side {
                    Side::Long => {
                        if candle.low <= pos.stop_loss {
                            pos.stop_loss
                        } else {
                            pos.take_profit
                        }
                    }
                    Side::Short => {
                        if candle.high >= pos.stop_loss {
                            pos.stop_loss
                        } else {
                            pos.take_profit
                        }
                    }
                };
                let cost_out = exit_cost(pos.quantity, exit_price, &cfg.risk);
                let raw_pnl = match pos.side {
                    Side::Long => (exit_price - pos.entry_price) * pos.quantity,
                    Side::Short => (pos.entry_price - exit_price) * pos.quantity,
                };
                let total_fee = pos.fee_in + cost_out.fee;
                let pnl = raw_pnl - total_fee;
                capital += pnl;
                trades.push(Trade {
                    id: pos.trade_id,
                    symbol: symbol.to_string(),
                    side: pos.side.clone(),
                    entry_price: pos.entry_price,
                    exit_price,
                    quantity: pos.quantity,
                    entry_time: pos.entry_time,
                    exit_time: candle.timestamp,
                    fee: total_fee,
                    pnl,
                });
                open = None;
            }
        }

        if open.is_some() || atr.is_nan() || capital <= 0.0 {
            continue;
        }

        match sig {
            Signal::Buy => {
                let entry_price = candle.close;
                let qty = compute_quantity(capital, entry_price, atr, &cfg.risk);
                if qty <= 0.0 {
                    continue;
                }
                let cost = entry_cost(qty, entry_price, &cfg.risk);
                let sl = stop_loss_price(entry_price, atr, &cfg.risk, true);
                let tp = take_profit_price(entry_price, atr, &cfg.risk, true);
                capital -= cost.fee + cost.slippage;
                open = Some(OpenPosition {
                    side: Side::Long,
                    entry_price,
                    quantity: qty,
                    entry_time: candle.timestamp,
                    fee_in: cost.fee,
                    stop_loss: sl,
                    take_profit: tp,
                    trade_id: next_id,
                });
                next_id += 1;
            }
            Signal::Sell => {
                let entry_price = candle.close;
                let qty = compute_quantity(capital, entry_price, atr, &cfg.risk);
                if qty <= 0.0 {
                    continue;
                }
                let cost = entry_cost(qty, entry_price, &cfg.risk);
                let sl = stop_loss_price(entry_price, atr, &cfg.risk, false);
                let tp = take_profit_price(entry_price, atr, &cfg.risk, false);
                capital -= cost.fee + cost.slippage;
                open = Some(OpenPosition {
                    side: Side::Short,
                    entry_price,
                    quantity: qty,
                    entry_time: candle.timestamp,
                    fee_in: cost.fee,
                    stop_loss: sl,
                    take_profit: tp,
                    trade_id: next_id,
                });
                next_id += 1;
            }
            Signal::Hold => {}
        }
    }

    SimState { capital, trades }
}
