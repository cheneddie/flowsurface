use exchange::{Trade, UnixMs, depth::Depth};

use super::{
    BookConsumptionDetector, BookConsumptionEvent, BookSpeedEngine, BookSpeedSnapshot,
    ConsumptionConfig, SpeedConfig,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookFlowUpdate {
    pub events: Vec<BookConsumptionEvent>,
    pub speed: Option<BookSpeedSnapshot>,
}

/// High-level Book Speed pipeline.
///
/// Input:
/// - normalized aggressive trades
/// - successive L2 book states
///
/// Output:
/// - conservatively confirmed consumption events
/// - rolling Book Speed snapshot
///
/// The pipeline keeps trade matching and speed aggregation in one place so UI
/// panels, heatmaps and replay all share identical semantics.
pub struct BookFlowEngine {
    detector: BookConsumptionDetector,
    speed: BookSpeedEngine,
}

impl BookFlowEngine {
    pub fn new(consumption: ConsumptionConfig, speed: SpeedConfig) -> Self {
        Self {
            detector: BookConsumptionDetector::new(consumption),
            speed: BookSpeedEngine::new(speed),
        }
    }

    pub fn record_trades(&mut self, trades: &[Trade]) {
        self.detector.record_trades(trades);
    }

    pub fn on_depth(&mut self, now: UnixMs, depth: &Depth) -> BookFlowUpdate {
        let events = self.detector.process_depth(now, depth);
        for &event in &events {
            self.speed.push(event);
        }

        BookFlowUpdate {
            events,
            speed: self.speed.snapshot(),
        }
    }

    pub fn clear(&mut self) {
        self.detector.clear();
        self.speed.clear();
    }

    pub fn detector(&self) -> &BookConsumptionDetector {
        &self.detector
    }

    pub fn speed(&self) -> &BookSpeedEngine {
        &self.speed
    }
}

impl Default for BookFlowEngine {
    fn default() -> Self {
        Self::new(ConsumptionConfig::default(), SpeedConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use exchange::unit::{Price, Qty};

    use super::*;
    use crate::orderflow::BookSide;

    fn depth(ask_qty: f64) -> Depth {
        let mut depth = Depth::default();
        if ask_qty > 0.0 {
            depth
                .asks
                .insert(Price::from_f64(101.0), Qty::from_f64(ask_qty));
        }
        depth
    }

    fn buy_trade(ms: u64, qty: f64) -> Trade {
        Trade {
            time: UnixMs::new(ms),
            is_sell: false,
            price: Price::from_f64(101.0),
            qty: Qty::from_f64(qty),
        }
    }

    #[test]
    fn confirmed_consumption_immediately_updates_book_speed() {
        let mut flow = BookFlowEngine::new(
            ConsumptionConfig::new(1_000, 0.5),
            SpeedConfig::new(1_000),
        );

        flow.on_depth(UnixMs::new(1_000), &depth(5.0));
        flow.record_trades(&[buy_trade(1_050, 5.0)]);
        let update = flow.on_depth(UnixMs::new(1_100), &depth(0.0));

        assert_eq!(update.events.len(), 1);
        assert_eq!(update.events[0].side, BookSide::Ask);
        assert_eq!(update.events[0].levels, 1);

        let speed = update.speed.expect("speed snapshot");
        assert_eq!(speed.ask_levels_per_sec, 1.0);
        assert_eq!(speed.ask_qty_per_sec, 5.0);
        assert_eq!(speed.bid_levels_per_sec, 0.0);
    }

    #[test]
    fn cancellation_produces_no_speed_pressure() {
        let mut flow = BookFlowEngine::default();
        flow.on_depth(UnixMs::new(2_000), &depth(5.0));
        let update = flow.on_depth(UnixMs::new(2_100), &depth(0.0));

        assert!(update.events.is_empty());
        assert!(update.speed.is_none());
    }
}
