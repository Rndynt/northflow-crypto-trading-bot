use crate::config::RiskConfig;

pub struct OrderCost {
    pub quantity: f64,
    pub fee: f64,
    pub slippage: f64,
    pub total_cost: f64,
}

pub fn compute_quantity(
    capital: f64,
    entry_price: f64,
    atr: f64,
    cfg: &RiskConfig,
) -> f64 {
    let risk_amount = capital * cfg.max_position_pct;
    let stop_distance = atr * cfg.stop_loss_atr_mult;
    if stop_distance <= 0.0 || entry_price <= 0.0 {
        return 0.0;
    }
    (risk_amount / stop_distance).min(capital / entry_price)
}

pub fn entry_cost(quantity: f64, price: f64, cfg: &RiskConfig) -> OrderCost {
    let notional = quantity * price;
    let fee = notional * cfg.taker_fee;
    let slippage = notional * (cfg.slippage_bps / 10_000.0);
    OrderCost {
        quantity,
        fee,
        slippage,
        total_cost: notional + fee + slippage,
    }
}

pub fn exit_cost(quantity: f64, price: f64, cfg: &RiskConfig) -> OrderCost {
    let notional = quantity * price;
    let fee = notional * cfg.taker_fee;
    let slippage = notional * (cfg.slippage_bps / 10_000.0);
    OrderCost {
        quantity,
        fee,
        slippage,
        total_cost: notional - fee - slippage,
    }
}

pub fn stop_loss_price(entry: f64, atr: f64, cfg: &RiskConfig, long: bool) -> f64 {
    if long {
        entry - atr * cfg.stop_loss_atr_mult
    } else {
        entry + atr * cfg.stop_loss_atr_mult
    }
}

pub fn take_profit_price(entry: f64, atr: f64, cfg: &RiskConfig, long: bool) -> f64 {
    if long {
        entry + atr * cfg.take_profit_atr_mult
    } else {
        entry - atr * cfg.take_profit_atr_mult
    }
}
