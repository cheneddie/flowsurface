use std::collections::BTreeMap;

use exchange::{
    UnixMs,
    unit::{Price, Qty},
};
use serde::{Deserialize, Serialize};

use super::BookSide;

pub type OrderId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MboSide {
    Bid,
    Ask,
}

impl From<MboSide> for BookSide {
    fn from(value: MboSide) -> Self {
        match value {
            MboSide::Bid => BookSide::Bid,
            MboSide::Ask => BookSide::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MboAction {
    /// Add a new order with `qty` as displayed remaining quantity.
    Add,
    /// Replace displayed remaining quantity with `qty`.
    Modify,
    /// Cancel/remove the order. `qty` is ignored.
    Cancel,
    /// Execute `qty` against the order.
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MboEvent {
    pub time: UnixMs,
    pub order_id: OrderId,
    pub side: MboSide,
    pub price: Price,
    pub qty: Qty,
    pub action: MboAction,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MboOrder {
    pub order_id: OrderId,
    pub side: MboSide,
    pub price: Price,
    pub remaining: Qty,
    pub total_filled: Qty,
    pub refresh_count: u32,
    pub sequence: u64,
    pub last_update: UnixMs,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IcebergCandidate {
    pub order_id: OrderId,
    pub side: MboSide,
    pub price: Price,
    pub refresh_count: u32,
    pub total_filled: Qty,
    pub displayed_remaining: Qty,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IcebergConfig {
    pub min_refresh_count: u32,
    pub min_total_filled: Qty,
}

impl Default for IcebergConfig {
    fn default() -> Self {
        Self {
            min_refresh_count: 2,
            min_total_filled: Qty::ZERO,
        }
    }
}

/// Market-by-order state with queue-order preservation.
///
/// This model is only meaningful when the upstream feed exposes stable order
/// IDs. MBP/L2 aggregate data must not be converted into fake MBO events.
pub struct MboBook {
    orders: BTreeMap<OrderId, MboOrder>,
    next_sequence: u64,
}

impl Default for MboBook {
    fn default() -> Self {
        Self {
            orders: BTreeMap::new(),
            next_sequence: 1,
        }
    }
}

impl MboBook {
    pub fn clear(&mut self) {
        self.orders.clear();
        self.next_sequence = 1;
    }

    pub fn get(&self, order_id: OrderId) -> Option<&MboOrder> {
        self.orders.get(&order_id)
    }

    pub fn len(&self) -> usize {
        self.orders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn apply(&mut self, event: MboEvent) {
        match event.action {
            MboAction::Add => self.apply_add(event),
            MboAction::Modify => self.apply_modify(event),
            MboAction::Cancel => {
                self.orders.remove(&event.order_id);
            }
            MboAction::Fill => self.apply_fill(event),
        }
    }

    pub fn apply_many(&mut self, events: &[MboEvent]) {
        let mut ordered = events.to_vec();
        ordered.sort_by_key(|event| event.time);
        for event in ordered {
            self.apply(event);
        }
    }

    /// Displayed quantity ahead of the given order at the same side and price.
    /// This is a queue estimate based on observed MBO arrival sequence.
    pub fn queue_ahead_qty(&self, order_id: OrderId) -> Option<Qty> {
        let target = self.orders.get(&order_id)?;
        Some(
            self.orders
                .values()
                .filter(|order| {
                    order.order_id != target.order_id
                        && order.side == target.side
                        && order.price == target.price
                        && order.sequence < target.sequence
                })
                .fold(Qty::ZERO, |acc, order| acc + order.remaining),
        )
    }

    pub fn level_qty(&self, side: MboSide, price: Price) -> Qty {
        self.orders
            .values()
            .filter(|order| order.side == side && order.price == price)
            .fold(Qty::ZERO, |acc, order| acc + order.remaining)
    }

    pub fn iceberg_candidates(&self, config: IcebergConfig) -> Vec<IcebergCandidate> {
        self.orders
            .values()
            .filter(|order| {
                order.refresh_count >= config.min_refresh_count
                    && order.total_filled >= config.min_total_filled
            })
            .map(|order| IcebergCandidate {
                order_id: order.order_id,
                side: order.side,
                price: order.price,
                refresh_count: order.refresh_count,
                total_filled: order.total_filled,
                displayed_remaining: order.remaining,
            })
            .collect()
    }

    fn apply_add(&mut self, event: MboEvent) {
        if let Some(existing) = self.orders.get_mut(&event.order_id) {
            // A stable order ID reappearing/reloading at the same level after
            // being partially or fully executed is an MBO-observable refresh.
            if existing.side == event.side
                && existing.price == event.price
                && event.qty > existing.remaining
                && !existing.total_filled.is_zero()
            {
                existing.refresh_count = existing.refresh_count.saturating_add(1);
                existing.remaining = event.qty;
                existing.last_update = event.time;
                return;
            }
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.orders.insert(
            event.order_id,
            MboOrder {
                order_id: event.order_id,
                side: event.side,
                price: event.price,
                remaining: event.qty,
                total_filled: Qty::ZERO,
                refresh_count: 0,
                sequence,
                last_update: event.time,
            },
        );
    }

    fn apply_modify(&mut self, event: MboEvent) {
        let Some(order) = self.orders.get_mut(&event.order_id) else {
            // Some feeds can begin mid-session without a complete initial MBO
            // image. Treat an unknown modify as a new observed order rather
            // than fabricating prior queue history.
            self.apply_add(MboEvent {
                action: MboAction::Add,
                ..event
            });
            return;
        };

        if order.side == event.side
            && order.price == event.price
            && event.qty > order.remaining
            && !order.total_filled.is_zero()
        {
            order.refresh_count = order.refresh_count.saturating_add(1);
        }

        // A side/price change loses the original queue position.
        if order.side != event.side || order.price != event.price {
            order.sequence = self.next_sequence;
            self.next_sequence = self.next_sequence.saturating_add(1);
        }

        order.side = event.side;
        order.price = event.price;
        order.remaining = event.qty;
        order.last_update = event.time;
    }

    fn apply_fill(&mut self, event: MboEvent) {
        let Some(order) = self.orders.get_mut(&event.order_id) else {
            return;
        };

        let filled = std::cmp::min(order.remaining, event.qty);
        order.remaining -= filled;
        order.total_filled += filled;
        order.last_update = event.time;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(value: f64) -> Price {
        Price::from_f64(value)
    }

    fn q(value: f64) -> Qty {
        Qty::from_f64(value)
    }

    fn event(t: u64, id: u64, action: MboAction, side: MboSide, price: f64, qty: f64) -> MboEvent {
        MboEvent {
            time: UnixMs::new(t),
            order_id: id,
            side,
            price: p(price),
            qty: q(qty),
            action,
        }
    }

    #[test]
    fn queue_ahead_respects_order_arrival_sequence() {
        let mut book = MboBook::default();
        book.apply(event(1, 10, MboAction::Add, MboSide::Bid, 100.0, 5.0));
        book.apply(event(2, 11, MboAction::Add, MboSide::Bid, 100.0, 3.0));

        assert_eq!(book.queue_ahead_qty(10), Some(Qty::ZERO));
        assert_eq!(book.queue_ahead_qty(11), Some(q(5.0)));
    }

    #[test]
    fn fill_reduces_displayed_quantity_and_accumulates_execution() {
        let mut book = MboBook::default();
        book.apply(event(1, 10, MboAction::Add, MboSide::Ask, 101.0, 10.0));
        book.apply(event(2, 10, MboAction::Fill, MboSide::Ask, 101.0, 4.0));

        let order = book.get(10).unwrap();
        assert_eq!(order.remaining, q(6.0));
        assert_eq!(order.total_filled, q(4.0));
    }

    #[test]
    fn repeated_reload_becomes_iceberg_candidate() {
        let mut book = MboBook::default();
        book.apply(event(1, 42, MboAction::Add, MboSide::Ask, 101.0, 5.0));

        book.apply(event(2, 42, MboAction::Fill, MboSide::Ask, 101.0, 5.0));
        book.apply(event(3, 42, MboAction::Add, MboSide::Ask, 101.0, 5.0));
        book.apply(event(4, 42, MboAction::Fill, MboSide::Ask, 101.0, 5.0));
        book.apply(event(5, 42, MboAction::Modify, MboSide::Ask, 101.0, 5.0));

        let candidates = book.iceberg_candidates(IcebergConfig {
            min_refresh_count: 2,
            min_total_filled: q(10.0),
        });

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].order_id, 42);
        assert_eq!(candidates[0].refresh_count, 2);
        assert_eq!(candidates[0].total_filled, q(10.0));
    }

    #[test]
    fn cancel_removes_order_from_queue() {
        let mut book = MboBook::default();
        book.apply(event(1, 10, MboAction::Add, MboSide::Bid, 100.0, 5.0));
        book.apply(event(2, 10, MboAction::Cancel, MboSide::Bid, 100.0, 0.0));
        assert!(book.get(10).is_none());
    }
}
