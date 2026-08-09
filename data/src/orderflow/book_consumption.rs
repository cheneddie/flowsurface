use exchange::{
    Trade, UnixMs,
    depth::Depth,
    unit::{Price, Qty},
};

use super::{BookConsumptionEvent, BookSide};

/// Configuration for converting L2 depth reductions into *confirmed* book
/// consumption events.
///
/// A depth reduction is not considered consumed liquidity by itself because it
/// may simply be a cancellation. The detector requires same-side aggressive
/// trade volume at the exact price within a short matching window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConsumptionConfig {
    /// How far back trade prints may be matched to a book reduction.
    pub match_window_ms: u64,
    /// Minimum fraction of removed depth that must be explained by matching
    /// aggressive trade volume. Clamped to [0, 1].
    pub min_trade_match_ratio: f64,
}

impl Default for ConsumptionConfig {
    fn default() -> Self {
        Self {
            match_window_ms: 750,
            min_trade_match_ratio: 0.50,
        }
    }
}

impl ConsumptionConfig {
    pub fn new(match_window_ms: u64, min_trade_match_ratio: f64) -> Self {
        Self {
            match_window_ms: match_window_ms.max(1),
            min_trade_match_ratio: min_trade_match_ratio.clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MatchableTrade {
    time: UnixMs,
    is_sell: bool,
    price: Price,
    remaining: Qty,
}

impl From<&Trade> for MatchableTrade {
    fn from(trade: &Trade) -> Self {
        Self {
            time: trade.time,
            is_sell: trade.is_sell,
            price: trade.price,
            remaining: trade.qty,
        }
    }
}

/// Conservative L2 book-consumption detector.
///
/// The detector intentionally prefers false negatives over false positives:
/// - Ask reduction requires aggressive BUY prints (`Trade::is_sell == false`).
/// - Bid reduction requires aggressive SELL prints (`Trade::is_sell == true`).
/// - The prints must occur at the exact price level.
/// - Matched trade quantity is consumed once and cannot explain later depth
///   reductions again.
/// - A completely removed level emits `levels = 1`; a partial reduction emits
///   `levels = 0` while still reporting matched quantity.
///
/// This makes the downstream `BookSpeedEngine` safer than treating every L2
/// delete/update as execution pressure.
pub struct BookConsumptionDetector {
    config: ConsumptionConfig,
    previous: Option<Depth>,
    trades: Vec<MatchableTrade>,
}

impl BookConsumptionDetector {
    pub fn new(config: ConsumptionConfig) -> Self {
        Self {
            config,
            previous: None,
            trades: Vec::new(),
        }
    }

    pub fn config(&self) -> ConsumptionConfig {
        self.config
    }

    pub fn set_config(&mut self, config: ConsumptionConfig) {
        self.config = ConsumptionConfig::new(config.match_window_ms, config.min_trade_match_ratio);
    }

    pub fn clear(&mut self) {
        self.previous = None;
        self.trades.clear();
    }

    /// Add trade prints that can later confirm L2 reductions.
    pub fn record_trades(&mut self, trades: &[Trade]) {
        if trades.is_empty() {
            return;
        }

        let mut ordered = trades.to_vec();
        ordered.sort_by_key(|trade| trade.time);
        self.trades.extend(ordered.iter().map(MatchableTrade::from));

        if let Some(latest) = ordered.last().map(|trade| trade.time) {
            self.prune(latest);
        }
    }

    /// Compare a new depth snapshot/state with the previous state and return
    /// only reductions that are sufficiently explained by aggressive trades.
    pub fn process_depth(&mut self, now: UnixMs, depth: &Depth) -> Vec<BookConsumptionEvent> {
        self.prune(now);

        let Some(previous) = self.previous.take() else {
            self.previous = Some(depth.clone());
            return Vec::new();
        };

        let mut events = Vec::new();

        self.detect_side(
            now,
            &previous.asks,
            &depth.asks,
            BookSide::Ask,
            false,
            &mut events,
        );
        self.detect_side(
            now,
            &previous.bids,
            &depth.bids,
            BookSide::Bid,
            true,
            &mut events,
        );

        self.previous = Some(depth.clone());
        events
    }

    fn detect_side(
        &mut self,
        now: UnixMs,
        previous: &std::collections::BTreeMap<Price, Qty>,
        current: &std::collections::BTreeMap<Price, Qty>,
        side: BookSide,
        matching_is_sell: bool,
        out: &mut Vec<BookConsumptionEvent>,
    ) {
        for (&price, &old_qty) in previous {
            let new_qty = current.get(&price).copied().unwrap_or(Qty::ZERO);
            if new_qty >= old_qty {
                continue;
            }

            let removed = old_qty - new_qty;
            if removed.is_zero() {
                continue;
            }

            let available = self.matching_trade_qty(now, price, matching_is_sell);
            if available.is_zero() {
                continue;
            }

            let ratio = available.to_f64() / removed.to_scale_or_one();
            if ratio + f64::EPSILON < self.config.min_trade_match_ratio {
                continue;
            }

            let matched = std::cmp::min(removed, available);
            self.consume_matching_trades(now, price, matching_is_sell, matched);

            out.push(BookConsumptionEvent {
                time: now,
                side,
                levels: u32::from(new_qty.is_zero()),
                qty: matched,
            });
        }
    }

    fn matching_trade_qty(&self, now: UnixMs, price: Price, is_sell: bool) -> Qty {
        let earliest = now.saturating_sub(self.config.match_window_ms);

        self.trades
            .iter()
            .filter(|trade| {
                trade.time >= earliest
                    && trade.time <= now
                    && trade.price == price
                    && trade.is_sell == is_sell
                    && !trade.remaining.is_zero()
            })
            .fold(Qty::ZERO, |acc, trade| acc + trade.remaining)
    }

    fn consume_matching_trades(&mut self, now: UnixMs, price: Price, is_sell: bool, mut qty: Qty) {
        if qty.is_zero() {
            return;
        }

        let earliest = now.saturating_sub(self.config.match_window_ms);

        for trade in &mut self.trades {
            if qty.is_zero() {
                break;
            }

            if trade.time < earliest
                || trade.time > now
                || trade.price != price
                || trade.is_sell != is_sell
                || trade.remaining.is_zero()
            {
                continue;
            }

            let used = std::cmp::min(qty, trade.remaining);
            trade.remaining -= used;
            qty -= used;
        }
    }

    fn prune(&mut self, now: UnixMs) {
        let earliest = now.saturating_sub(self.config.match_window_ms);
        self.trades
            .retain(|trade| trade.time >= earliest && !trade.remaining.is_zero());
    }
}

impl Default for BookConsumptionDetector {
    fn default() -> Self {
        Self::new(ConsumptionConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(v: f64) -> Price {
        Price::from_f64(v)
    }

    fn qty(v: f64) -> Qty {
        Qty::from_f64(v)
    }

    fn trade(ms: u64, is_sell: bool, px: f64, size: f64) -> Trade {
        Trade {
            time: UnixMs::new(ms),
            is_sell,
            price: price(px),
            qty: qty(size),
        }
    }

    fn depth(bids: &[(f64, f64)], asks: &[(f64, f64)]) -> Depth {
        let mut depth = Depth::default();
        for &(px, size) in bids {
            depth.bids.insert(price(px), qty(size));
        }
        for &(px, size) in asks {
            depth.asks.insert(price(px), qty(size));
        }
        depth
    }

    #[test]
    fn cancellation_without_trade_is_not_consumption() {
        let mut detector = BookConsumptionDetector::default();
        let before = depth(&[(99.0, 10.0)], &[(101.0, 10.0)]);
        let after = depth(&[(99.0, 5.0)], &[(101.0, 4.0)]);

        assert!(
            detector
                .process_depth(UnixMs::new(1_000), &before)
                .is_empty()
        );
        assert!(
            detector
                .process_depth(UnixMs::new(1_100), &after)
                .is_empty()
        );
    }

    #[test]
    fn aggressive_buy_confirms_ask_consumption() {
        let mut detector = BookConsumptionDetector::default();
        let before = depth(&[], &[(101.0, 10.0)]);
        let after = depth(&[], &[(101.0, 4.0)]);

        detector.process_depth(UnixMs::new(1_000), &before);
        detector.record_trades(&[trade(1_050, false, 101.0, 6.0)]);
        let events = detector.process_depth(UnixMs::new(1_100), &after);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].side, BookSide::Ask);
        assert_eq!(events[0].levels, 0);
        assert_eq!(events[0].qty, qty(6.0));
    }

    #[test]
    fn aggressive_sell_confirms_bid_level_consumed() {
        let mut detector = BookConsumptionDetector::default();
        let before = depth(&[(99.0, 3.0)], &[]);
        let after = depth(&[], &[]);

        detector.process_depth(UnixMs::new(2_000), &before);
        detector.record_trades(&[trade(2_050, true, 99.0, 3.0)]);
        let events = detector.process_depth(UnixMs::new(2_100), &after);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].side, BookSide::Bid);
        assert_eq!(events[0].levels, 1);
        assert_eq!(events[0].qty, qty(3.0));
    }

    #[test]
    fn wrong_aggressor_side_does_not_confirm_reduction() {
        let mut detector = BookConsumptionDetector::default();
        let before = depth(&[], &[(101.0, 5.0)]);
        let after = depth(&[], &[(101.0, 1.0)]);

        detector.process_depth(UnixMs::new(3_000), &before);
        detector.record_trades(&[trade(3_050, true, 101.0, 5.0)]);
        assert!(
            detector
                .process_depth(UnixMs::new(3_100), &after)
                .is_empty()
        );
    }

    #[test]
    fn matched_trade_volume_cannot_be_reused() {
        let mut detector = BookConsumptionDetector::new(ConsumptionConfig::new(1_000, 0.5));
        let first = depth(&[], &[(101.0, 10.0)]);
        let second = depth(&[], &[(101.0, 6.0)]);
        let third = depth(&[], &[(101.0, 2.0)]);

        detector.process_depth(UnixMs::new(4_000), &first);
        detector.record_trades(&[trade(4_050, false, 101.0, 4.0)]);

        let first_events = detector.process_depth(UnixMs::new(4_100), &second);
        assert_eq!(first_events.len(), 1);

        // The same four units were consumed by the first match and cannot
        // explain the next four-unit depth reduction.
        let second_events = detector.process_depth(UnixMs::new(4_200), &third);
        assert!(second_events.is_empty());
    }

    #[test]
    fn insufficient_trade_match_is_rejected() {
        let mut detector = BookConsumptionDetector::new(ConsumptionConfig::new(1_000, 0.75));
        let before = depth(&[], &[(101.0, 10.0)]);
        let after = depth(&[], &[(101.0, 2.0)]); // removed 8

        detector.process_depth(UnixMs::new(5_000), &before);
        detector.record_trades(&[trade(5_050, false, 101.0, 4.0)]); // only 50%
        assert!(
            detector
                .process_depth(UnixMs::new(5_100), &after)
                .is_empty()
        );
    }
}
