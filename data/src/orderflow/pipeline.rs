use exchange::depth::Depth;

use crate::{
    market::{
        BookSide as MarketBookSide, DepthEventKind, InstrumentId, NormalizedDepthEvent,
        NormalizedMarketEvent,
    },
    replay::ReplayEvent,
};

use super::{
    BookFlowEngine, BookFlowUpdate, ConsumptionConfig, IcebergCandidate, IcebergConfig, MboBook,
    SpeedConfig, TradeSpeedEngine, TradeSpeedSnapshot,
};

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct OrderFlowPipelineConfig {
    pub trade_speed: SpeedConfig,
    pub book_speed: SpeedConfig,
    pub consumption: ConsumptionConfig,
    pub iceberg: IcebergConfig,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OrderFlowPipelineUpdate {
    pub trade_speed: Option<TradeSpeedSnapshot>,
    pub book_flow: Option<BookFlowUpdate>,
    pub iceberg_candidates: Vec<IcebergCandidate>,
    pub ignored_unknown_aggressor: bool,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum OrderFlowPipelineError {
    #[error("event belongs to {received:?}, pipeline is bound to {expected:?}")]
    InstrumentMismatch {
        expected: Box<InstrumentId>,
        received: Box<InstrumentId>,
    },
}

/// Instrument-bound order-flow state machine shared by live and replay feeds.
///
/// Feed adapters and ReplayEngine both produce `ReplayEvent`-compatible
/// normalized events. This pipeline is intentionally unaware of where an event
/// came from, preventing live/replay behavioral drift.
pub struct OrderFlowPipeline {
    instrument: InstrumentId,
    trade_speed: TradeSpeedEngine,
    book_flow: BookFlowEngine,
    depth: Depth,
    has_depth: bool,
    mbo: MboBook,
    iceberg_config: IcebergConfig,
}

impl OrderFlowPipeline {
    pub fn new(instrument: InstrumentId, config: OrderFlowPipelineConfig) -> Self {
        Self {
            instrument,
            trade_speed: TradeSpeedEngine::new(config.trade_speed),
            book_flow: BookFlowEngine::new(config.consumption, config.book_speed),
            depth: Depth::default(),
            has_depth: false,
            mbo: MboBook::default(),
            iceberg_config: config.iceberg,
        }
    }

    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    pub fn depth(&self) -> &Depth {
        &self.depth
    }

    pub fn mbo(&self) -> &MboBook {
        &self.mbo
    }

    pub fn clear(&mut self) {
        self.trade_speed.clear();
        self.book_flow.clear();
        self.depth = Depth::default();
        self.has_depth = false;
        self.mbo.clear();
    }

    pub fn process(
        &mut self,
        event: &ReplayEvent,
    ) -> Result<OrderFlowPipelineUpdate, OrderFlowPipelineError> {
        self.ensure_instrument(event.instrument())?;

        let mut update = OrderFlowPipelineUpdate::default();
        match event {
            ReplayEvent::Market(NormalizedMarketEvent::Trade(normalized)) => {
                if let Some(trade) = normalized.to_flowsurface_trade() {
                    self.trade_speed.push_trade(&trade);
                    self.book_flow.record_trades(&[trade]);
                    update.trade_speed = self.trade_speed.snapshot();
                } else {
                    update.ignored_unknown_aggressor = true;
                }
            }
            ReplayEvent::Market(NormalizedMarketEvent::Depth(depth_event)) => {
                self.apply_depth(depth_event);
                let book_update = self.book_flow.on_depth(depth_event.time, &self.depth);
                update.book_flow = Some(book_update);
            }
            ReplayEvent::Mbo { event, .. } => {
                self.mbo.apply(*event);
                update.iceberg_candidates = self.mbo.iceberg_candidates(self.iceberg_config);
            }
        }

        Ok(update)
    }

    fn ensure_instrument(&self, received: &InstrumentId) -> Result<(), OrderFlowPipelineError> {
        if received == &self.instrument {
            Ok(())
        } else {
            Err(OrderFlowPipelineError::InstrumentMismatch {
                expected: Box::new(self.instrument.clone()),
                received: Box::new(received.clone()),
            })
        }
    }

    fn apply_depth(&mut self, event: &NormalizedDepthEvent) {
        if matches!(event.kind, DepthEventKind::Snapshot) {
            self.depth.bids.clear();
            self.depth.asks.clear();
            self.has_depth = true;
        } else if !self.has_depth {
            // A diff without an initial image cannot produce a trustworthy book.
            // We still build observed state, but mark the first state as the
            // detector baseline so no fake consumption is emitted.
            self.has_depth = true;
        }

        for level in &event.levels {
            let map = match level.side {
                MarketBookSide::Bid => &mut self.depth.bids,
                MarketBookSide::Ask => &mut self.depth.asks,
            };

            if level.qty.is_zero() {
                map.remove(&level.price);
            } else {
                map.insert(level.price, level.qty);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use exchange::{
        UnixMs,
        unit::{Price, Qty},
    };

    use super::*;
    use crate::{
        market::{AggressorSide, AssetClass, MarketVenue, NormalizedBookLevel, NormalizedTrade},
        orderflow::{MboAction, MboEvent, MboSide},
    };

    fn instrument() -> InstrumentId {
        InstrumentId::new(MarketVenue::Taifex, "TXF", AssetClass::Futures)
    }

    fn other_instrument() -> InstrumentId {
        InstrumentId::new(MarketVenue::Taifex, "MXF", AssetClass::Futures)
    }

    fn trade_event(time: u64, side: AggressorSide, qty: f64) -> ReplayEvent {
        ReplayEvent::Market(NormalizedMarketEvent::Trade(NormalizedTrade {
            instrument: instrument(),
            time: UnixMs::new(time),
            price: Price::from_f64(101.0),
            qty: Qty::from_f64(qty),
            aggressor: side,
            sequence: None,
        }))
    }

    fn depth_event(time: u64, kind: DepthEventKind, ask_qty: f64) -> ReplayEvent {
        ReplayEvent::Market(NormalizedMarketEvent::Depth(NormalizedDepthEvent {
            instrument: instrument(),
            time: UnixMs::new(time),
            kind,
            sequence: None,
            levels: vec![NormalizedBookLevel {
                side: MarketBookSide::Ask,
                price: Price::from_f64(101.0),
                qty: Qty::from_f64(ask_qty),
            }],
        }))
    }

    #[test]
    fn live_or_replay_trade_drives_same_trade_speed_engine() {
        let mut pipeline = OrderFlowPipeline::new(instrument(), OrderFlowPipelineConfig::default());
        let update = pipeline
            .process(&trade_event(1_000, AggressorSide::Buy, 2.0))
            .unwrap();
        let speed = update.trade_speed.expect("trade speed");
        assert_eq!(speed.buy_trade_rate, 1.0);
        assert_eq!(speed.buy_volume_rate, 2.0);
    }

    #[test]
    fn replayed_trade_plus_depth_drives_book_speed() {
        let mut pipeline = OrderFlowPipeline::new(
            instrument(),
            OrderFlowPipelineConfig {
                consumption: ConsumptionConfig::new(1_000, 0.5),
                ..OrderFlowPipelineConfig::default()
            },
        );

        pipeline
            .process(&depth_event(1_000, DepthEventKind::Snapshot, 5.0))
            .unwrap();
        pipeline
            .process(&trade_event(1_050, AggressorSide::Buy, 5.0))
            .unwrap();
        let update = pipeline
            .process(&depth_event(1_100, DepthEventKind::Diff, 0.0))
            .unwrap();

        let book = update.book_flow.expect("book update");
        assert_eq!(book.events.len(), 1);
        let speed = book.speed.expect("book speed");
        assert_eq!(speed.ask_levels_per_sec, 1.0);
        assert_eq!(speed.ask_qty_per_sec, 5.0);
    }

    #[test]
    fn unknown_aggressor_is_ignored_not_guessed() {
        let mut pipeline = OrderFlowPipeline::new(instrument(), OrderFlowPipelineConfig::default());
        let update = pipeline
            .process(&trade_event(1_000, AggressorSide::Unknown, 2.0))
            .unwrap();
        assert!(update.ignored_unknown_aggressor);
        assert!(update.trade_speed.is_none());
    }

    #[test]
    fn instrument_mismatch_is_rejected() {
        let mut pipeline = OrderFlowPipeline::new(instrument(), OrderFlowPipelineConfig::default());
        let event = ReplayEvent::Market(NormalizedMarketEvent::Trade(NormalizedTrade {
            instrument: other_instrument(),
            time: UnixMs::new(1_000),
            price: Price::from_f64(101.0),
            qty: Qty::from_f64(1.0),
            aggressor: AggressorSide::Buy,
            sequence: None,
        }));

        assert!(matches!(
            pipeline.process(&event),
            Err(OrderFlowPipelineError::InstrumentMismatch { .. })
        ));
    }

    #[test]
    fn mbo_reloads_surface_iceberg_candidate() {
        let mut pipeline = OrderFlowPipeline::new(
            instrument(),
            OrderFlowPipelineConfig {
                iceberg: IcebergConfig {
                    min_refresh_count: 2,
                    min_total_filled: Qty::from_f64(10.0),
                },
                ..OrderFlowPipelineConfig::default()
            },
        );

        let make = |time, action, qty| ReplayEvent::Mbo {
            instrument: instrument(),
            event: MboEvent {
                time: UnixMs::new(time),
                order_id: 42,
                side: MboSide::Ask,
                price: Price::from_f64(101.0),
                qty: Qty::from_f64(qty),
                action,
            },
        };

        pipeline.process(&make(1, MboAction::Add, 5.0)).unwrap();
        pipeline.process(&make(2, MboAction::Fill, 5.0)).unwrap();
        pipeline.process(&make(3, MboAction::Add, 5.0)).unwrap();
        pipeline.process(&make(4, MboAction::Fill, 5.0)).unwrap();
        let update = pipeline.process(&make(5, MboAction::Modify, 5.0)).unwrap();

        assert_eq!(update.iceberg_candidates.len(), 1);
        assert_eq!(update.iceberg_candidates[0].order_id, 42);
    }
}
