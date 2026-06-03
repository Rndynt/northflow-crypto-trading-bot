//! Position tracker — manages open positions, trailing stops, PnL.
//!
//! Enhanced for HFT quant:
//! - ATR-based trailing stop (activates at 1R, trails at 0.5× ATR)
//! - Time-based exit (close positions open > max_hold_secs)
//! - Partial take-profit (close 50% at first TP, rest trails)
//! - Breakeven stop (move SL to entry after 0.5R profit)
//!
//! `check_exits` now returns `Vec<PositionAction>` so the execution agent
//! can distinguish between a full close, a partial reduce, and an SL move.
//! Partial TP no longer fires `PositionClosed` — it fires `PositionReduced`.
//! SL moves (breakeven, trailing) are returned as `MoveSL` so the execution
//! agent can cancel the old broker SL order and replace it at the new level.

use crate::data::Side;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub client_id: String,
    /// Signal ID that originated this trade (e.g. "S-00001").
    #[serde(default)]
    pub signal_id: String,
    pub symbol: String,
    pub side: Side,
    pub size: f64,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub opened_at: DateTime<Utc>,
    pub trailing_activated: bool,
    pub peak_price: f64,
    pub trough_price: f64,
    /// ATR at entry — used for trailing stop distance.
    #[serde(default)]
    pub atr_at_entry: f64,
    /// Whether partial TP (50%) has been taken.
    #[serde(default)]
    pub partial_taken: bool,
    /// Whether SL has been moved to breakeven.
    #[serde(default)]
    pub breakeven_activated: bool,
    /// Cumulative realized PnL from partial TP closes on this position.
    /// Populated by check_exits when Reduce fires so cmd_positions can display it.
    #[serde(default)]
    pub partial_realized_pnl: f64,
    /// Strategy name that opened this position.
    #[serde(default)]
    pub strategy: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PositionExitReason {
    StopLoss,
    TakeProfit,
    Trailing,
    TimeExit,
    Manual,
    Breakeven,
    PartialTP,
}

impl PositionExitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StopLoss => "SL",
            Self::TakeProfit => "TP",
            Self::Trailing => "TRAILING",
            Self::TimeExit => "TIME",
            Self::Manual => "MANUAL",
            Self::Breakeven => "BE",
            Self::PartialTP => "PARTIAL_TP",
        }
    }
}

/// Configuration for position management behavior.
#[derive(Debug, Clone)]
pub struct PositionConfig {
    /// Maximum time to hold a position (seconds). 0 = no limit.
    pub max_hold_secs: i64,
    /// ATR multiplier for trailing stop distance (e.g. 0.5 = half ATR).
    pub trail_atr_mult: f64,
    /// Profit threshold (in R-multiples) to activate trailing.
    pub trail_activate_r: f64,
    /// Profit threshold (in R-multiples) to move SL to breakeven.
    pub breakeven_r: f64,
    /// Whether to take partial TP (50% at first TP target).
    pub partial_tp_enabled: bool,
    /// Profit threshold (in R-multiples) to take partial TP.
    pub partial_tp_r: f64,
}

impl Default for PositionConfig {
    fn default() -> Self {
        Self {
            max_hold_secs: 1800, // 30 minutes
            trail_atr_mult: 0.5,
            trail_activate_r: 1.0,
            // Disabled by default: moving SL to entry before TP1 creates
            // many fee/slippage "scratch" losses in high-leverage scalps.
            // Partial TP still moves the runner SL to entry after 1R.
            breakeven_r: 0.0,
            partial_tp_enabled: true,
            partial_tp_r: 1.0,
        }
    }
}

/// Action the execution agent should take for a position.
///
/// **P0-4 fix**: `check_exits` now returns `Vec<PositionAction>`.
/// - `Close`   → full position close; emit `PositionClosed`
/// - `Reduce`  → partial close (PartialTP); emit `PositionReduced` only
/// - `MoveSL`  → SL moved (breakeven/trailing); cancel+replace broker order
/// - `None`    → nothing to do (should not appear in the returned vector)
#[derive(Debug, Clone)]
pub enum PositionAction {
    /// Close the entire position.
    Close(Position, PositionExitReason),
    /// Partial close: (position snapshot, reduce_size, reason).
    /// `position.size` is already the reduced partial size to trade out of.
    /// The `reduce_size` field equals `position.size`.
    Reduce(Position, f64, PositionExitReason),
    /// Update SL on the broker — new_stop_loss is the target price.
    MoveSL(Position, f64),
    /// No action needed.
    None,
}

const POSITION_FILE: &str = "data/positions.json";

#[derive(Default)]
pub struct PositionBook {
    inner: Arc<Mutex<HashMap<String, Position>>>,
}

impl PositionBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load persisted positions from disk (paper mode persistence).
    pub fn load_from_disk(&self) {
        if let Ok(data) = std::fs::read_to_string(POSITION_FILE) {
            if let Ok(positions) = serde_json::from_str::<Vec<Position>>(&data) {
                let count = positions.len();
                let mut book = self.inner.lock();
                for p in positions {
                    book.insert(p.client_id.clone(), p);
                }
                if count > 0 {
                    tracing::info!(count, "loaded persisted positions from disk");
                }
            }
        }
    }

    /// Save current positions to disk.
    fn save_to_disk(&self) {
        let positions: Vec<Position> = self.inner.lock().values().cloned().collect();
        if let Some(parent) = std::path::Path::new(POSITION_FILE).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(&positions) {
            let _ = std::fs::write(POSITION_FILE, data);
        }
    }

    /// Public save for graceful exit (SIGTERM handler).
    pub fn save_to_disk_on_exit(&self) {
        self.save_to_disk();
    }

    pub fn open(&self, p: Position) {
        self.inner.lock().insert(p.client_id.clone(), p);
        self.save_to_disk();
    }

    pub fn close(&self, client_id: &str) -> Option<Position> {
        let result = self.inner.lock().remove(client_id);
        if result.is_some() {
            self.save_to_disk();
        }
        result
    }

    pub fn get(&self, client_id: &str) -> Option<Position> {
        self.inner.lock().get(client_id).cloned()
    }

    pub fn all(&self) -> Vec<Position> {
        self.inner.lock().values().cloned().collect()
    }

    pub fn snapshot(&self) -> Vec<Position> {
        self.all()
    }

    pub fn close_by_id(&self, client_id: &str) -> Option<Position> {
        self.close(client_id)
    }

    pub fn reconcile(&self, positions: Vec<Position>) {
        let mut book = self.inner.lock();
        book.clear();
        for p in positions {
            book.insert(p.client_id.clone(), p);
        }
        drop(book);
        self.save_to_disk();
    }

    pub fn update_price(&self, symbol: &str, price: f64) {
        for p in self.inner.lock().values_mut() {
            if p.symbol != symbol {
                continue;
            }
            if price > p.peak_price {
                p.peak_price = price;
            }
            if price < p.trough_price {
                p.trough_price = price;
            }
        }
    }

    /// Enhanced exit check with ATR trailing, breakeven, partial TP,
    /// and time-based exits.
    ///
    /// Returns a list of `PositionAction`s for the execution agent:
    /// - `Close`   → full position close (SL/TP/trailing/time-exit hit)
    /// - `Reduce`  → partial TP: close half, remainder stays open at breakeven SL
    /// - `MoveSL`  → breakeven or trailing SL moved; broker order must be updated
    ///
    /// **P0-4**: `PartialTP` is now returned as `Reduce`, not `Close`.
    /// **P0-5**: Breakeven and trailing SL moves are returned as `MoveSL` so the
    ///           execution agent can cancel the stale broker SL order and replace it.
    pub fn check_exits(
        &self,
        symbol: &str,
        price: f64,
        cfg: &PositionConfig,
    ) -> Vec<PositionAction> {
        let mut out: Vec<PositionAction> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();
        let mut book = self.inner.lock();
        let now = Utc::now();

        for (id, p) in book.iter_mut() {
            if p.symbol != symbol {
                continue;
            }

            let r = (p.entry_price - p.stop_loss).abs();

            // Time-based exit (always check, even when r == 0)
            if cfg.max_hold_secs > 0 {
                let held = (now - p.opened_at).num_seconds();
                if held > cfg.max_hold_secs {
                    out.push(PositionAction::Close(
                        p.clone(),
                        PositionExitReason::TimeExit,
                    ));
                    to_remove.push(id.clone());
                    continue;
                }
            }

            // Hard SL/TP checks (work even when r == 0 / breakeven)
            match p.side {
                Side::Long => {
                    if p.stop_loss > 0.0 && price <= p.stop_loss {
                        let reason = if is_breakeven_stop(p) {
                            PositionExitReason::Breakeven
                        } else if p.trailing_activated {
                            // BUG FIX: trailing SL is stored in p.stop_loss just like a hard SL.
                            // Hard SL check fires first and would label this as StopLoss.
                            // When trailing was active the correct label is Trailing.
                            PositionExitReason::Trailing
                        } else {
                            PositionExitReason::StopLoss
                        };
                        out.push(PositionAction::Close(p.clone(), reason));
                        to_remove.push(id.clone());
                        continue;
                    }
                    if p.take_profit > 0.0 && price >= p.take_profit {
                        out.push(PositionAction::Close(
                            p.clone(),
                            PositionExitReason::TakeProfit,
                        ));
                        to_remove.push(id.clone());
                        continue;
                    }
                }
                Side::Short => {
                    if p.stop_loss > 0.0 && price >= p.stop_loss {
                        let reason = if is_breakeven_stop(p) {
                            PositionExitReason::Breakeven
                        } else if p.trailing_activated {
                            // BUG FIX: same as Long — trailing SL stored in p.stop_loss,
                            // hard check fires first and would mislabel it as StopLoss.
                            PositionExitReason::Trailing
                        } else {
                            PositionExitReason::StopLoss
                        };
                        out.push(PositionAction::Close(p.clone(), reason));
                        to_remove.push(id.clone());
                        continue;
                    }
                    if p.take_profit > 0.0 && price <= p.take_profit {
                        out.push(PositionAction::Close(
                            p.clone(),
                            PositionExitReason::TakeProfit,
                        ));
                        to_remove.push(id.clone());
                        continue;
                    }
                }
            }

            // Skip R-multiple logic when r == 0 (breakeven with no distance)
            if r <= 0.0 {
                continue;
            }

            match p.side {
                Side::Long => {
                    let profit_r = (price - p.entry_price) / r;

                    // Partial TP: close 50% at 1R profit, move SL to breakeven.
                    // P0-4: return Reduce (not Close) so PositionReduced fires instead of PositionClosed.
                    if cfg.partial_tp_enabled && !p.partial_taken && profit_r >= cfg.partial_tp_r {
                        p.partial_taken = true;
                        let reduce_size = p.size * 0.5;
                        let new_sl = p.entry_price;
                        let reduce_pos = Position {
                            size: reduce_size,
                            ..p.clone()
                        };
                        // Accumulate realized PnL from this partial close on the position
                        // so cmd_positions can show "already realized: $X" while the runner is open.
                        let partial_pnl = pnl_usd(&reduce_pos, price);
                        p.partial_realized_pnl += partial_pnl;
                        // Reduce remaining size in book
                        p.size -= reduce_size;
                        // Move SL to breakeven in book
                        let old_sl = p.stop_loss;
                        p.stop_loss = new_sl;

                        // Emit the partial close action
                        out.push(PositionAction::Reduce(
                            reduce_pos,
                            reduce_size,
                            PositionExitReason::PartialTP,
                        ));
                        // Emit the SL move so execution agent can update the broker order
                        if (new_sl - old_sl).abs() > f64::EPSILON {
                            out.push(PositionAction::MoveSL(p.clone(), new_sl));
                        }

                        if p.size <= 0.0 {
                            to_remove.push(id.clone());
                            continue;
                        }
                    }

                    // Optional pre-TP breakeven: disabled when breakeven_r == 0.
                    if cfg.breakeven_r > 0.0
                        && !p.breakeven_activated
                        && !p.partial_taken
                        && profit_r >= cfg.breakeven_r
                    {
                        p.breakeven_activated = true;
                        let new_sl = p.entry_price;
                        if new_sl > p.stop_loss {
                            p.stop_loss = new_sl;
                            out.push(PositionAction::MoveSL(p.clone(), new_sl));
                        }
                    }

                    // Trailing stop: activate at 1R profit, trail at 0.5× ATR
                    if !p.trailing_activated && profit_r >= cfg.trail_activate_r {
                        p.trailing_activated = true;
                    }
                    if p.trailing_activated {
                        let trail_dist = if p.atr_at_entry > 0.0 {
                            p.atr_at_entry * cfg.trail_atr_mult
                        } else {
                            (price - p.entry_price) * 0.5
                        };
                        let trail_stop = price - trail_dist;
                        if trail_stop > p.stop_loss {
                            p.stop_loss = trail_stop;
                            out.push(PositionAction::MoveSL(p.clone(), trail_stop));
                        }
                        // Check if trailing stop hit
                        if price <= p.stop_loss {
                            out.push(PositionAction::Close(
                                p.clone(),
                                PositionExitReason::Trailing,
                            ));
                            to_remove.push(id.clone());
                            continue;
                        }
                    }
                }
                Side::Short => {
                    let profit_r = (p.entry_price - price) / r;

                    // Partial TP: close 50% at 1R profit, move SL to breakeven.
                    if cfg.partial_tp_enabled && !p.partial_taken && profit_r >= cfg.partial_tp_r {
                        p.partial_taken = true;
                        let reduce_size = p.size * 0.5;
                        let new_sl = p.entry_price;
                        let reduce_pos = Position {
                            size: reduce_size,
                            ..p.clone()
                        };
                        let partial_pnl = pnl_usd(&reduce_pos, price);
                        p.partial_realized_pnl += partial_pnl;
                        p.size -= reduce_size;
                        let old_sl = p.stop_loss;
                        p.stop_loss = new_sl;

                        out.push(PositionAction::Reduce(
                            reduce_pos,
                            reduce_size,
                            PositionExitReason::PartialTP,
                        ));
                        if (new_sl - old_sl).abs() > f64::EPSILON {
                            out.push(PositionAction::MoveSL(p.clone(), new_sl));
                        }

                        if p.size <= 0.0 {
                            to_remove.push(id.clone());
                            continue;
                        }
                    }

                    if cfg.breakeven_r > 0.0
                        && !p.breakeven_activated
                        && !p.partial_taken
                        && profit_r >= cfg.breakeven_r
                    {
                        p.breakeven_activated = true;
                        let new_sl = p.entry_price;
                        if new_sl < p.stop_loss {
                            p.stop_loss = new_sl;
                            out.push(PositionAction::MoveSL(p.clone(), new_sl));
                        }
                    }

                    if !p.trailing_activated && profit_r >= cfg.trail_activate_r {
                        p.trailing_activated = true;
                    }
                    if p.trailing_activated {
                        let trail_dist = if p.atr_at_entry > 0.0 {
                            p.atr_at_entry * cfg.trail_atr_mult
                        } else {
                            (p.entry_price - price) * 0.5
                        };
                        let trail_stop = price + trail_dist;
                        if trail_stop < p.stop_loss {
                            p.stop_loss = trail_stop;
                            out.push(PositionAction::MoveSL(p.clone(), trail_stop));
                        }
                        if price >= p.stop_loss {
                            out.push(PositionAction::Close(
                                p.clone(),
                                PositionExitReason::Trailing,
                            ));
                            to_remove.push(id.clone());
                            continue;
                        }
                    }
                }
            }
        }

        let changed = !out.is_empty() || !to_remove.is_empty();
        for id in to_remove {
            book.remove(&id);
        }
        drop(book);
        if changed {
            #[cfg(not(test))]
            self.save_to_disk();
        }
        out
    }
}

fn is_breakeven_stop(p: &Position) -> bool {
    (p.breakeven_activated || p.partial_taken)
        && (p.stop_loss - p.entry_price).abs() <= p.entry_price.abs().max(1.0) * 1e-9
}

pub fn pnl_usd(p: &Position, exit_price: f64) -> f64 {
    match p.side {
        Side::Long => (exit_price - p.entry_price) * p.size,
        Side::Short => (p.entry_price - exit_price) * p.size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// Helper: open a fresh PositionBook with one position, return the book.
    fn book_with(pos: Position) -> PositionBook {
        let b = PositionBook::new();
        b.inner.lock().insert(pos.client_id.clone(), pos);
        b
    }

    fn base_long() -> Position {
        Position {
            client_id: "test-long".into(),
            symbol: "BTCUSDT".into(),
            side: Side::Long,
            entry_price: 100.0,
            stop_loss: 90.0, // R = 10
            take_profit: 120.0,
            size: 1.0,
            atr_at_entry: 10.0,
            opened_at: Utc::now(),
            strategy: "vwap_scalp".into(),
            partial_taken: false,
            breakeven_activated: false,
            trailing_activated: false,
            partial_realized_pnl: 0.0,
            peak_price: 100.0,
            trough_price: 100.0,
        }
    }

    fn base_short() -> Position {
        Position {
            client_id: "test-short".into(),
            symbol: "BTCUSDT".into(),
            side: Side::Short,
            entry_price: 100.0,
            stop_loss: 110.0, // R = 10
            take_profit: 80.0,
            size: 1.0,
            atr_at_entry: 10.0,
            opened_at: Utc::now(),
            strategy: "vwap_scalp".into(),
            partial_taken: false,
            breakeven_activated: false,
            trailing_activated: false,
            partial_realized_pnl: 0.0,
            peak_price: 100.0,
            trough_price: 100.0,
        }
    }

    fn cfg_partial_only() -> PositionConfig {
        PositionConfig {
            max_hold_secs: 0, // disable time exit
            partial_tp_enabled: true,
            partial_tp_r: 1.0,
            breakeven_r: 0.5,
            trail_activate_r: 1.5, // high — won't trigger in partial tests
            trail_atr_mult: 0.5,
        }
    }

    // ── Partial TP — Long ─────────────────────────────────────────────────────

    #[test]
    fn test_partial_tp_long_emits_reduce_and_move_sl() {
        let pos = base_long();
        let book = book_with(pos);
        let cfg = cfg_partial_only();

        // Price exactly at 1R profit: entry=100, R=10, price=110
        let actions = book.check_exits("BTCUSDT", 110.0, &cfg);

        // Must have Reduce + MoveSL
        assert_eq!(
            actions.len(),
            2,
            "expected Reduce + MoveSL, got: {:?}",
            actions.len()
        );

        let has_reduce = actions.iter().any(|a| {
            matches!(
                a,
                PositionAction::Reduce(_, _, PositionExitReason::PartialTP)
            )
        });
        let has_move_sl = actions
            .iter()
            .any(|a| matches!(a, PositionAction::MoveSL(_, _)));
        assert!(has_reduce, "Reduce(PartialTP) not emitted");
        assert!(has_move_sl, "MoveSL not emitted after partial TP");
    }

    #[test]
    fn test_partial_tp_long_halves_size_and_sets_sl_to_entry() {
        let pos = base_long();
        let book = book_with(pos);
        let cfg = cfg_partial_only();

        let actions = book.check_exits("BTCUSDT", 110.0, &cfg);

        // Reduce action should have size 0.5 (half of 1.0)
        for a in &actions {
            if let PositionAction::Reduce(p, reduce_size, _) = a {
                assert!(
                    (reduce_size - 0.5).abs() < 1e-9,
                    "reduce_size should be 0.5, got {}",
                    reduce_size
                );
                assert!(
                    (p.size - 0.5).abs() < 1e-9,
                    "snapshot size should be 0.5, got {}",
                    p.size
                );
            }
            if let PositionAction::MoveSL(p, new_sl) = a {
                // SL should be moved to entry price (100.0)
                assert!(
                    (new_sl - 100.0).abs() < 1e-9,
                    "new SL should be entry 100.0, got {}",
                    new_sl
                );
                let _ = p;
            }
        }

        // Book should still have the position with half size
        let book_inner = book.inner.lock();
        let remaining = book_inner
            .get("test-long")
            .expect("position should still be in book");
        assert!(
            (remaining.size - 0.5).abs() < 1e-9,
            "remaining size should be 0.5, got {}",
            remaining.size
        );
        assert!(
            (remaining.stop_loss - 100.0).abs() < 1e-9,
            "SL should be moved to entry"
        );
        assert!(remaining.partial_taken, "partial_taken flag should be set");
    }

    #[test]
    fn test_partial_tp_long_not_triggered_below_1r() {
        let pos = base_long();
        let book = book_with(pos);
        let cfg = cfg_partial_only();

        // Price at 0.9R — should NOT trigger partial TP
        let actions = book.check_exits("BTCUSDT", 109.0, &cfg);
        let has_reduce = actions
            .iter()
            .any(|a| matches!(a, PositionAction::Reduce(_, _, _)));
        assert!(!has_reduce, "Reduce should not fire at 0.9R profit");
    }

    // ── Partial TP — Short ────────────────────────────────────────────────────

    #[test]
    fn test_partial_tp_short_emits_reduce_and_move_sl() {
        let pos = base_short();
        let book = book_with(pos);
        let cfg = cfg_partial_only();

        // Price at 1R profit: entry=100, R=10, price=90
        let actions = book.check_exits("BTCUSDT", 90.0, &cfg);

        let has_reduce = actions.iter().any(|a| {
            matches!(
                a,
                PositionAction::Reduce(_, _, PositionExitReason::PartialTP)
            )
        });
        let has_move_sl = actions
            .iter()
            .any(|a| matches!(a, PositionAction::MoveSL(_, _)));
        assert!(has_reduce, "Reduce(PartialTP) not emitted for short");
        assert!(has_move_sl, "MoveSL not emitted after short partial TP");
    }

    #[test]
    fn test_partial_tp_short_sl_moves_to_entry() {
        let pos = base_short();
        let book = book_with(pos);
        let cfg = cfg_partial_only();

        let actions = book.check_exits("BTCUSDT", 90.0, &cfg);
        for a in &actions {
            if let PositionAction::MoveSL(_, new_sl) = a {
                assert!(
                    (new_sl - 100.0).abs() < 1e-9,
                    "short SL should move to entry 100.0, got {}",
                    new_sl
                );
            }
        }
    }

    // ── Breakeven — Long ──────────────────────────────────────────────────────

    #[test]
    fn test_breakeven_long_moves_sl_at_0_5r() {
        let pos = base_long();
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            partial_tp_enabled: false, // disable partial TP so only breakeven fires
            partial_tp_r: 1.0,
            breakeven_r: 0.5,
            trail_activate_r: 1.5,
            trail_atr_mult: 0.5,
        };

        // Price at 0.5R: entry=100, R=10, price=105
        let actions = book.check_exits("BTCUSDT", 105.0, &cfg);

        let move_sl = actions
            .iter()
            .find(|a| matches!(a, PositionAction::MoveSL(_, _)));
        assert!(move_sl.is_some(), "MoveSL should fire at breakeven (0.5R)");

        if let Some(PositionAction::MoveSL(_, new_sl)) = move_sl {
            assert!(
                (new_sl - 100.0).abs() < 1e-9,
                "breakeven SL should be entry 100.0, got {}",
                new_sl
            );
        }
    }

    #[test]
    fn test_breakeven_does_not_fire_twice() {
        let pos = base_long();
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            partial_tp_enabled: false,
            partial_tp_r: 1.0,
            breakeven_r: 0.5,
            trail_activate_r: 1.5,
            trail_atr_mult: 0.5,
        };

        // First call at 0.5R — should set breakeven
        let _ = book.check_exits("BTCUSDT", 105.0, &cfg);
        // Second call at same price — breakeven_activated is set, should not emit MoveSL again
        let actions2 = book.check_exits("BTCUSDT", 105.0, &cfg);
        let move_count = actions2
            .iter()
            .filter(|a| matches!(a, PositionAction::MoveSL(_, _)))
            .count();
        assert_eq!(
            move_count, 0,
            "MoveSL should not fire a second time after breakeven_activated=true"
        );
    }

    #[test]
    fn test_breakeven_zero_disables_pre_tp_move() {
        let pos = base_short();
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            partial_tp_enabled: false,
            partial_tp_r: 1.0,
            breakeven_r: 0.0,
            trail_activate_r: 1.5,
            trail_atr_mult: 0.5,
        };

        // Even after a favorable move, breakeven_r=0 means "disabled",
        // not "move SL immediately".
        let actions = book.check_exits("BTCUSDT", 95.0, &cfg);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, PositionAction::MoveSL(_, _))),
            "breakeven_r=0 must disable pre-TP MoveSL"
        );
        let remaining = book.inner.lock().get("test-short").cloned().unwrap();
        assert!((remaining.stop_loss - 110.0).abs() < 1e-9);
        assert!(!remaining.breakeven_activated);
    }

    #[test]
    fn test_entry_stop_after_breakeven_reports_breakeven_not_stop_loss() {
        let mut pos = base_short();
        pos.stop_loss = pos.entry_price;
        pos.breakeven_activated = true;
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            partial_tp_enabled: false,
            partial_tp_r: 1.0,
            breakeven_r: 0.5,
            trail_activate_r: 1.5,
            trail_atr_mult: 0.5,
        };

        let actions = book.check_exits("BTCUSDT", 100.0, &cfg);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, PositionAction::Close(_, PositionExitReason::Breakeven)))
        );
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, PositionAction::Close(_, PositionExitReason::StopLoss)))
        );
    }

    // ── MoveSL — trailing ─────────────────────────────────────────────────────

    #[test]
    fn test_trailing_stop_activates_and_moves_sl() {
        // Use high breakeven_r (1.5) so breakeven does NOT trigger at 1R,
        // keeping R = 10 (stop_loss=90, entry=100) so the trailing logic
        // can compute trail_stop = price - atr*mult = 110 - 5 = 105.
        let pos = base_long(); // stop_loss=90, entry=100 → R=10
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            partial_tp_enabled: false,
            partial_tp_r: 1.0,
            breakeven_r: 1.5,      // high — won't fire at 1R
            trail_activate_r: 1.0, // activate at 1R
            trail_atr_mult: 0.5,
        };

        // Price at exactly 1R: entry=100, R=10, price=110
        // trail_dist = atr_at_entry(10) * 0.5 = 5; trail_stop = 110 - 5 = 105
        // trail_stop (105) > stop_loss (90) → MoveSL fires
        let actions = book.check_exits("BTCUSDT", 110.0, &cfg);
        let move_sl = actions.iter().find(|a| {
            if let PositionAction::MoveSL(_, sl) = a {
                *sl > 100.0
            } else {
                false
            }
        });
        assert!(
            move_sl.is_some(),
            "trailing MoveSL should fire at 1R profit"
        );

        if let Some(PositionAction::MoveSL(_, new_sl)) = move_sl {
            assert!(
                (new_sl - 105.0).abs() < 1e-9,
                "trailing SL should be 105.0, got {}",
                new_sl
            );
        }
    }

    #[test]
    fn test_trailing_stop_not_triggered_below_activate_r() {
        let pos = base_long(); // stop_loss=90, entry=100 → R=10
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            partial_tp_enabled: false,
            partial_tp_r: 1.0,
            breakeven_r: 1.5,      // high — won't fire at 0.9R
            trail_activate_r: 1.0, // needs full 1R to activate
            trail_atr_mult: 0.5,
        };

        // Price at 0.9R (109) — trailing should NOT activate
        let actions = book.check_exits("BTCUSDT", 109.0, &cfg);
        // Only a breakeven/trailing MoveSL above entry would count; there should be none
        let has_trailing = actions.iter().any(|a| {
            if let PositionAction::MoveSL(_, sl) = a {
                *sl > 90.0
            } else {
                false
            }
        });
        assert!(
            !has_trailing,
            "trailing should not fire below trail_activate_r"
        );
    }

    // ── Hard SL/TP ────────────────────────────────────────────────────────────

    #[test]
    fn test_hard_stop_loss_long() {
        let pos = base_long();
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            ..Default::default()
        };

        // Price at or below SL (90.0) → Close(StopLoss)
        let actions = book.check_exits("BTCUSDT", 90.0, &cfg);
        let closed = actions
            .iter()
            .any(|a| matches!(a, PositionAction::Close(_, PositionExitReason::StopLoss)));
        assert!(closed, "hard SL should close the long position");
    }

    #[test]
    fn test_hard_take_profit_long() {
        let pos = base_long();
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            ..Default::default()
        };

        // Price at or above TP (120.0) → Close(TakeProfit)
        let actions = book.check_exits("BTCUSDT", 120.0, &cfg);
        let tped = actions
            .iter()
            .any(|a| matches!(a, PositionAction::Close(_, PositionExitReason::TakeProfit)));
        assert!(tped, "hard TP should close the long position");
    }

    #[test]
    fn test_hard_stop_loss_short() {
        let pos = base_short();
        let book = book_with(pos);
        let cfg = PositionConfig {
            max_hold_secs: 0,
            ..Default::default()
        };

        // Price at or above SL (110.0) → Close(StopLoss)
        let actions = book.check_exits("BTCUSDT", 110.0, &cfg);
        let closed = actions
            .iter()
            .any(|a| matches!(a, PositionAction::Close(_, PositionExitReason::StopLoss)));
        assert!(closed, "hard SL should close the short position");
    }

    // ── pnl_usd helper ───────────────────────────────────────────────────────

    #[test]
    fn test_pnl_usd_long_profit() {
        let pos = base_long();
        let pnl = pnl_usd(&pos, 110.0);
        assert!((pnl - 10.0).abs() < 1e-9, "expected pnl=10.0, got {}", pnl);
    }

    #[test]
    fn test_pnl_usd_long_loss() {
        let pos = base_long();
        let pnl = pnl_usd(&pos, 95.0);
        assert!(
            (pnl - (-5.0)).abs() < 1e-9,
            "expected pnl=-5.0, got {}",
            pnl
        );
    }

    #[test]
    fn test_pnl_usd_short_profit() {
        let pos = base_short();
        let pnl = pnl_usd(&pos, 90.0);
        assert!((pnl - 10.0).abs() < 1e-9, "expected pnl=10.0, got {}", pnl);
    }
}
