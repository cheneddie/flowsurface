use crate::{
    market::InstrumentId,
    orderflow::{OrderFlowPipeline, OrderFlowPipelineConfig, OrderFlowPipelineError, OrderFlowPipelineUpdate},
    replay::{ReplayArchive, ReplayEngine, ReplayEvent},
};
use exchange::UnixMs;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReplayState {
    #[default]
    Paused,
    Playing,
    Finished,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayDispatch {
    pub event: ReplayEvent,
    pub update: OrderFlowPipelineUpdate,
}

/// Desktop-ready replay session that drives the exact same `OrderFlowPipeline`
/// as live normalized market events.
///
/// The session owns playback state, seeking and speed control. UI code only
/// needs to call `play/pause/seek/set_rate/advance` and render the resulting
/// updates; no replay-specific order-flow calculations are duplicated here.
pub struct ReplaySession {
    replay: ReplayEngine,
    pipeline: OrderFlowPipeline,
    state: ReplayState,
}

impl ReplaySession {
    pub fn new(
        instrument: InstrumentId,
        archive: ReplayArchive,
        config: OrderFlowPipelineConfig,
    ) -> Self {
        Self {
            replay: ReplayEngine::new(archive),
            pipeline: OrderFlowPipeline::new(instrument, config),
            state: ReplayState::Paused,
        }
    }

    pub fn state(&self) -> ReplayState {
        self.state
    }

    pub fn instrument(&self) -> &InstrumentId {
        self.pipeline.instrument()
    }

    pub fn playback_rate(&self) -> f64 {
        self.replay.playback_rate()
    }

    pub fn remaining(&self) -> usize {
        self.replay.remaining()
    }

    pub fn archive(&self) -> &ReplayArchive {
        self.replay.archive()
    }

    pub fn play(&mut self) {
        if self.replay.is_finished() {
            self.replay.reset();
            self.pipeline.clear();
        }
        self.replay.set_rate(self.replay.playback_rate());
        self.state = ReplayState::Playing;
    }

    pub fn pause(&mut self) {
        if self.state != ReplayState::Finished {
            self.state = ReplayState::Paused;
        }
    }

    pub fn set_rate(&mut self, rate: f64) {
        self.replay.set_rate(rate);
    }

    /// Rebuild pipeline state deterministically from the start of the archive to
    /// the requested market time. This is intentionally O(n): correctness and
    /// identical state after seek are more important than a stale partial cache.
    /// Checkpointing can be added later without changing this contract.
    pub fn seek(&mut self, time: UnixMs) -> Result<Vec<ReplayDispatch>, OrderFlowPipelineError> {
        self.pipeline.clear();
        self.replay.reset();
        let events = self.replay.drain_until(time);
        let mut dispatched = self.dispatch(events)?;
        self.replay.seek(time.saturating_add(1));
        self.state = ReplayState::Paused;
        // Keep the rebuilt updates available to callers that need to refresh HUDs.
        dispatched.shrink_to_fit();
        Ok(dispatched)
    }

    pub fn reset(&mut self) {
        self.replay.reset();
        self.pipeline.clear();
        self.state = ReplayState::Paused;
    }

    pub fn advance(
        &mut self,
        wall_elapsed_ms: u64,
    ) -> Result<Vec<ReplayDispatch>, OrderFlowPipelineError> {
        if self.state != ReplayState::Playing {
            return Ok(Vec::new());
        }

        let events = self.replay.advance(wall_elapsed_ms);
        let dispatched = self.dispatch(events)?;
        if self.replay.is_finished() {
            self.state = ReplayState::Finished;
        }
        Ok(dispatched)
    }

    fn dispatch(
        &mut self,
        events: Vec<ReplayEvent>,
    ) -> Result<Vec<ReplayDispatch>, OrderFlowPipelineError> {
        let mut out = Vec::with_capacity(events.len());
        for event in events {
            let update = self.pipeline.process(&event)?;
            out.push(ReplayDispatch { event, update });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::{AggressorSide, AssetClass, MarketVenue, NormalizedMarketEvent, NormalizedTrade};
    use exchange::unit::{Price, Qty};

    fn instrument() -> InstrumentId {
        InstrumentId::new(MarketVenue::Taifex, "TXF", AssetClass::Futures)
    }

    fn trade(ms: u64, qty: f64) -> ReplayEvent {
        ReplayEvent::Market(NormalizedMarketEvent::Trade(NormalizedTrade {
            instrument: instrument(),
            time: UnixMs::new(ms),
            price: Price::from_f64(20_000.0),
            qty: Qty::from_f64(qty),
            aggressor: AggressorSide::Buy,
            sequence: None,
        }))
    }

    #[test]
    fn paused_session_does_not_emit_events() {
        let archive = ReplayArchive::new(vec![trade(1_000, 1.0)]);
        let mut session = ReplaySession::new(instrument(), archive, OrderFlowPipelineConfig::default());
        assert!(session.advance(5_000).unwrap().is_empty());
    }

    #[test]
    fn play_advances_same_orderflow_pipeline() {
        let archive = ReplayArchive::new(vec![trade(1_000, 2.0), trade(2_000, 3.0)]);
        let mut session = ReplaySession::new(instrument(), archive, OrderFlowPipelineConfig::default());
        session.play();
        let batch = session.advance(1_000).unwrap();
        assert_eq!(batch.len(), 2);
        let last = batch.last().unwrap().update.trade_speed.expect("speed");
        assert!(last.buy_volume_rate > 0.0);
        assert_eq!(session.state(), ReplayState::Finished);
    }

    #[test]
    fn seek_rebuilds_pipeline_state_from_start() {
        let archive = ReplayArchive::new(vec![trade(1_000, 1.0), trade(2_000, 2.0), trade(3_000, 3.0)]);
        let mut session = ReplaySession::new(instrument(), archive, OrderFlowPipelineConfig::default());
        let rebuilt = session.seek(UnixMs::new(2_000)).unwrap();
        assert_eq!(rebuilt.len(), 2);
        assert_eq!(session.remaining(), 1);
        assert_eq!(session.state(), ReplayState::Paused);
    }

    #[test]
    fn finished_play_restarts_from_beginning() {
        let archive = ReplayArchive::new(vec![trade(1_000, 1.0)]);
        let mut session = ReplaySession::new(instrument(), archive, OrderFlowPipelineConfig::default());
        session.play();
        session.advance(10).unwrap();
        assert_eq!(session.state(), ReplayState::Finished);
        session.play();
        assert_eq!(session.state(), ReplayState::Playing);
        assert_eq!(session.remaining(), 1);
    }
}
