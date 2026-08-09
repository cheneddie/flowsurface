use std::collections::VecDeque;

use exchange::{Trade, UnixMs, unit::Qty};

const EPSILON: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedConfig {
    pub window_ms: u64,
}

impl Default for SpeedConfig {
    fn default() -> Self {
        Self { window_ms: 1_000 }
    }
}

impl SpeedConfig {
    pub fn new(window_ms: u64) -> Self {
        Self { window_ms: window_ms.max(1) }
    }

    #[inline]
    fn window_seconds(self) -> f64 {
        self.window_ms as f64 / 1_000.0
    }

    #[inline]
    fn retained_ms(self) -> u64 {
        self.window_ms.saturating_mul(2)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct TradeWindowStats {
    trade_count: u64,
    buy_trade_count: u64,
    sell_trade_count: u64,
    buy_volume: f64,
    sell_volume: f64,
}

impl TradeWindowStats {
    #[inline]
    fn add(&mut self, sample: &TradeSample) {
        self.trade_count = self.trade_count.saturating_add(1);
        let qty = sample.qty.to_f64();
        if sample.is_sell {
            self.sell_trade_count = self.sell_trade_count.saturating_add(1);
            self.sell_volume += qty;
        } else {
            self.buy_trade_count = self.buy_trade_count.saturating_add(1);
            self.buy_volume += qty;
        }
    }

    #[inline]
    fn total_volume(self) -> f64 { self.buy_volume + self.sell_volume }

    #[inline]
    fn delta_volume(self) -> f64 { self.buy_volume - self.sell_volume }
}

#[derive(Debug, Clone, Copy)]
struct TradeSample {
    time: UnixMs,
    is_sell: bool,
    qty: Qty,
}

impl From<&Trade> for TradeSample {
    fn from(value: &Trade) -> Self {
        Self { time: value.time, is_sell: value.is_sell, qty: value.qty }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TradeSpeedSnapshot {
    pub timestamp: UnixMs,
    pub window_ms: u64,
    pub trade_rate: f64,
    pub buy_trade_rate: f64,
    pub sell_trade_rate: f64,
    pub volume_rate: f64,
    pub buy_volume_rate: f64,
    pub sell_volume_rate: f64,
    pub delta_volume_rate: f64,
    pub trade_acceleration: f64,
    pub volume_acceleration: f64,
    pub buy_volume_acceleration: f64,
    pub sell_volume_acceleration: f64,
    pub trade_acceleration_pct: Option<f64>,
    pub volume_acceleration_pct: Option<f64>,
    pub buy_volume_acceleration_pct: Option<f64>,
    pub sell_volume_acceleration_pct: Option<f64>,
    pub delta_share: f64,
}

impl TradeSpeedSnapshot {
    #[inline]
    pub fn buy_dominant(self) -> bool { self.delta_volume_rate > 0.0 }

    #[inline]
    pub fn sell_dominant(self) -> bool { self.delta_volume_rate < 0.0 }
}

pub struct TradeSpeedEngine {
    config: SpeedConfig,
    samples: VecDeque<TradeSample>,
    latest_time: Option<UnixMs>,
}

impl TradeSpeedEngine {
    pub fn new(config: SpeedConfig) -> Self {
        Self { config, samples: VecDeque::new(), latest_time: None }
    }

    pub fn config(&self) -> SpeedConfig { self.config }

    pub fn set_config(&mut self, config: SpeedConfig) {
        self.config = SpeedConfig::new(config.window_ms);
        if let Some(now) = self.latest_time { self.prune(now); }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.latest_time = None;
    }

    pub fn is_empty(&self) -> bool { self.samples.is_empty() }
    pub fn len(&self) -> usize { self.samples.len() }

    pub fn push_trade(&mut self, trade: &Trade) -> bool {
        let sample = TradeSample::from(trade);
        let newest = self.latest_time.map_or(sample.time, |current| current.max(sample.time));
        let cutoff = newest.saturating_sub(self.config.retained_ms());
        if sample.time < cutoff { return false; }
        self.latest_time = Some(newest);
        self.samples.push_back(sample);
        self.prune(newest);
        true
    }

    pub fn push_trades(&mut self, trades: &[Trade]) -> usize {
        trades.iter().filter(|trade| self.push_trade(trade)).count()
    }

    pub fn snapshot(&self) -> Option<TradeSpeedSnapshot> {
        self.latest_time.map(|now| self.snapshot_at(now))
    }

    pub fn snapshot_at(&self, now: UnixMs) -> TradeSpeedSnapshot {
        let window_ms = self.config.window_ms.max(1);
        let window_seconds = self.config.window_seconds();
        let current_start = now.saturating_sub(window_ms);
        let previous_start = current_start.saturating_sub(window_ms);
        let mut current = TradeWindowStats::default();
        let mut previous = TradeWindowStats::default();

        for sample in &self.samples {
            if sample.time > current_start && sample.time <= now {
                current.add(sample);
            } else if sample.time > previous_start && sample.time <= current_start {
                previous.add(sample);
            }
        }

        let trade_rate = current.trade_count as f64 / window_seconds;
        let buy_trade_rate = current.buy_trade_count as f64 / window_seconds;
        let sell_trade_rate = current.sell_trade_count as f64 / window_seconds;
        let volume_rate = current.total_volume() / window_seconds;
        let buy_volume_rate = current.buy_volume / window_seconds;
        let sell_volume_rate = current.sell_volume / window_seconds;
        let delta_volume_rate = current.delta_volume() / window_seconds;
        let previous_trade_rate = previous.trade_count as f64 / window_seconds;
        let previous_volume_rate = previous.total_volume() / window_seconds;
        let previous_buy_volume_rate = previous.buy_volume / window_seconds;
        let previous_sell_volume_rate = previous.sell_volume / window_seconds;
        let trade_acceleration = (trade_rate - previous_trade_rate) / window_seconds;
        let volume_acceleration = (volume_rate - previous_volume_rate) / window_seconds;
        let buy_volume_acceleration = (buy_volume_rate - previous_buy_volume_rate) / window_seconds;
        let sell_volume_acceleration = (sell_volume_rate - previous_sell_volume_rate) / window_seconds;
        let delta_share = if current.total_volume().abs() <= EPSILON {
            0.0
        } else {
            (current.delta_volume() / current.total_volume()).clamp(-1.0, 1.0)
        };

        TradeSpeedSnapshot {
            timestamp: now,
            window_ms,
            trade_rate,
            buy_trade_rate,
            sell_trade_rate,
            volume_rate,
            buy_volume_rate,
            sell_volume_rate,
            delta_volume_rate,
            trade_acceleration,
            volume_acceleration,
            buy_volume_acceleration,
            sell_volume_acceleration,
            trade_acceleration_pct: percent_change(trade_rate, previous_trade_rate),
            volume_acceleration_pct: percent_change(volume_rate, previous_volume_rate),
            buy_volume_acceleration_pct: percent_change(buy_volume_rate, previous_buy_volume_rate),
            sell_volume_acceleration_pct: percent_change(sell_volume_rate, previous_sell_volume_rate),
            delta_share,
        }
    }

    fn prune(&mut self, now: UnixMs) {
        let cutoff = now.saturating_sub(self.config.retained_ms());
        self.samples.retain(|sample| sample.time >= cutoff);
    }
}

impl Default for TradeSpeedEngine {
    fn default() -> Self { Self::new(SpeedConfig::default()) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookSide { Bid, Ask }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BookConsumptionEvent {
    pub time: UnixMs,
    pub side: BookSide,
    pub levels: u32,
    pub qty: Qty,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct BookWindowStats {
    bid_levels: u64,
    ask_levels: u64,
    bid_qty: f64,
    ask_qty: f64,
}

impl BookWindowStats {
    fn add(&mut self, event: &BookConsumptionEvent) {
        match event.side {
            BookSide::Bid => {
                self.bid_levels = self.bid_levels.saturating_add(event.levels as u64);
                self.bid_qty += event.qty.to_f64();
            }
            BookSide::Ask => {
                self.ask_levels = self.ask_levels.saturating_add(event.levels as u64);
                self.ask_qty += event.qty.to_f64();
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BookSpeedSnapshot {
    pub timestamp: UnixMs,
    pub window_ms: u64,
    pub bid_levels_per_sec: f64,
    pub ask_levels_per_sec: f64,
    pub bid_qty_per_sec: f64,
    pub ask_qty_per_sec: f64,
    pub bid_level_acceleration: f64,
    pub ask_level_acceleration: f64,
    pub bid_level_acceleration_pct: Option<f64>,
    pub ask_level_acceleration_pct: Option<f64>,
}

pub struct BookSpeedEngine {
    config: SpeedConfig,
    events: VecDeque<BookConsumptionEvent>,
    latest_time: Option<UnixMs>,
}

impl BookSpeedEngine {
    pub fn new(config: SpeedConfig) -> Self {
        Self { config, events: VecDeque::new(), latest_time: None }
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.latest_time = None;
    }

    pub fn push(&mut self, event: BookConsumptionEvent) -> bool {
        let newest = self.latest_time.map_or(event.time, |current| current.max(event.time));
        let cutoff = newest.saturating_sub(self.config.retained_ms());
        if event.time < cutoff { return false; }
        self.latest_time = Some(newest);
        self.events.push_back(event);
        self.prune(newest);
        true
    }

    pub fn snapshot(&self) -> Option<BookSpeedSnapshot> {
        self.latest_time.map(|now| self.snapshot_at(now))
    }

    pub fn snapshot_at(&self, now: UnixMs) -> BookSpeedSnapshot {
        let window_ms = self.config.window_ms.max(1);
        let seconds = self.config.window_seconds();
        let current_start = now.saturating_sub(window_ms);
        let previous_start = current_start.saturating_sub(window_ms);
        let mut current = BookWindowStats::default();
        let mut previous = BookWindowStats::default();

        for event in &self.events {
            if event.time > current_start && event.time <= now {
                current.add(event);
            } else if event.time > previous_start && event.time <= current_start {
                previous.add(event);
            }
        }

        let bid_levels_per_sec = current.bid_levels as f64 / seconds;
        let ask_levels_per_sec = current.ask_levels as f64 / seconds;
        let bid_qty_per_sec = current.bid_qty / seconds;
        let ask_qty_per_sec = current.ask_qty / seconds;
        let previous_bid_levels_per_sec = previous.bid_levels as f64 / seconds;
        let previous_ask_levels_per_sec = previous.ask_levels as f64 / seconds;

        BookSpeedSnapshot {
            timestamp: now,
            window_ms,
            bid_levels_per_sec,
            ask_levels_per_sec,
            bid_qty_per_sec,
            ask_qty_per_sec,
            bid_level_acceleration: (bid_levels_per_sec - previous_bid_levels_per_sec) / seconds,
            ask_level_acceleration: (ask_levels_per_sec - previous_ask_levels_per_sec) / seconds,
            bid_level_acceleration_pct: percent_change(bid_levels_per_sec, previous_bid_levels_per_sec),
            ask_level_acceleration_pct: percent_change(ask_levels_per_sec, previous_ask_levels_per_sec),
        }
    }

    fn prune(&mut self, now: UnixMs) {
        let cutoff = now.saturating_sub(self.config.retained_ms());
        self.events.retain(|event| event.time >= cutoff);
    }
}

impl Default for BookSpeedEngine {
    fn default() -> Self { Self::new(SpeedConfig::default()) }
}

#[inline]
fn percent_change(current: f64, previous: f64) -> Option<f64> {
    if previous.abs() <= EPSILON { None } else { Some(((current - previous) / previous.abs()) * 100.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange::unit::Price;

    fn trade(ms: u64, is_sell: bool, qty: f64) -> Trade {
        Trade {
            time: UnixMs::new(ms),
            is_sell,
            price: Price::from_f64(100.0),
            qty: Qty::from_f64(qty),
        }
    }

    #[test]
    fn trade_speed_reports_directional_rates() {
        let mut engine = TradeSpeedEngine::new(SpeedConfig::new(1_000));
        engine.push_trade(&trade(100, false, 2.0));
        engine.push_trade(&trade(500, false, 0.5));
        engine.push_trade(&trade(900, true, 1.0));
        let s = engine.snapshot_at(UnixMs::new(1_000));
        assert_eq!(s.trade_rate, 3.0);
        assert_eq!(s.buy_trade_rate, 2.0);
        assert_eq!(s.sell_trade_rate, 1.0);
        assert!((s.volume_rate - 3.5).abs() < EPSILON);
        assert!((s.buy_volume_rate - 2.5).abs() < EPSILON);
        assert!((s.sell_volume_rate - 1.0).abs() < EPSILON);
        assert!((s.delta_volume_rate - 1.5).abs() < EPSILON);
    }

    #[test]
    fn trade_speed_compares_with_previous_window() {
        let mut engine = TradeSpeedEngine::new(SpeedConfig::new(1_000));
        engine.push_trade(&trade(500, false, 1.0));
        engine.push_trade(&trade(1_200, false, 1.0));
        engine.push_trade(&trade(1_500, false, 1.0));
        engine.push_trade(&trade(1_800, true, 1.0));
        let s = engine.snapshot_at(UnixMs::new(2_000));
        assert_eq!(s.trade_rate, 3.0);
        assert_eq!(s.trade_acceleration, 2.0);
        assert_eq!(s.trade_acceleration_pct, Some(200.0));
    }

    #[test]
    fn book_speed_tracks_consumed_levels() {
        let mut engine = BookSpeedEngine::new(SpeedConfig::new(1_000));
        engine.push(BookConsumptionEvent {
            time: UnixMs::new(200),
            side: BookSide::Ask,
            levels: 3,
            qty: Qty::from_f64(10.0),
        });
        engine.push(BookConsumptionEvent {
            time: UnixMs::new(700),
            side: BookSide::Bid,
            levels: 2,
            qty: Qty::from_f64(4.0),
        });
        let s = engine.snapshot_at(UnixMs::new(1_000));
        assert_eq!(s.ask_levels_per_sec, 3.0);
        assert_eq!(s.bid_levels_per_sec, 2.0);
    }
}
