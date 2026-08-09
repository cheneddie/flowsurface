use serde::{Deserialize, Serialize};

use exchange::UnixMs;

use crate::{
    market::{InstrumentId, NormalizedMarketEvent},
    orderflow::MboEvent,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReplayEvent {
    Market(NormalizedMarketEvent),
    Mbo {
        instrument: InstrumentId,
        event: MboEvent,
    },
}

impl ReplayEvent {
    pub fn time(&self) -> UnixMs {
        match self {
            Self::Market(event) => event.time(),
            Self::Mbo { event, .. } => event.time,
        }
    }

    pub fn instrument(&self) -> &InstrumentId {
        match self {
            Self::Market(event) => event.instrument(),
            Self::Mbo { instrument, .. } => instrument,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplayArchive {
    pub events: Vec<ReplayEvent>,
}

impl ReplayArchive {
    pub fn new(mut events: Vec<ReplayEvent>) -> Self {
        events.sort_by_key(ReplayEvent::time);
        Self { events }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn timerange(&self) -> Option<(UnixMs, UnixMs)> {
        Some((self.events.first()?.time(), self.events.last()?.time()))
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut archive: Self = serde_json::from_str(json)?;
        archive.events.sort_by_key(ReplayEvent::time);
        Ok(archive)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayClock {
    pub playback_rate: f64,
    pub anchor_market_time: UnixMs,
    pub elapsed_wall_ms: u64,
}

impl ReplayClock {
    pub fn new(anchor_market_time: UnixMs) -> Self {
        Self {
            playback_rate: 1.0,
            anchor_market_time,
            elapsed_wall_ms: 0,
        }
    }

    pub fn set_rate(&mut self, rate: f64) {
        self.playback_rate = sanitize_rate(rate);
    }

    pub fn reset(&mut self, anchor_market_time: UnixMs) {
        self.anchor_market_time = anchor_market_time;
        self.elapsed_wall_ms = 0;
    }

    pub fn advance_wall_time(&mut self, elapsed_ms: u64) -> UnixMs {
        self.elapsed_wall_ms = self.elapsed_wall_ms.saturating_add(elapsed_ms);
        self.market_time()
    }

    pub fn market_time(&self) -> UnixMs {
        let scaled = (self.elapsed_wall_ms as f64 * self.playback_rate).round();
        let scaled = if scaled.is_finite() && scaled > 0.0 {
            scaled.min(u64::MAX as f64) as u64
        } else {
            0
        };
        self.anchor_market_time.saturating_add(scaled)
    }
}

fn sanitize_rate(rate: f64) -> f64 {
    if rate.is_finite() {
        rate.clamp(0.05, 100.0)
    } else {
        1.0
    }
}

/// Deterministic historical event source.
///
/// Consumers should send returned events into the exact same downstream event
/// bus/order-flow engines used by live feeds.
pub struct ReplayEngine {
    archive: ReplayArchive,
    cursor: usize,
    clock: Option<ReplayClock>,
}

impl ReplayEngine {
    pub fn new(archive: ReplayArchive) -> Self {
        Self {
            archive,
            cursor: 0,
            clock: None,
        }
    }

    pub fn archive(&self) -> &ReplayArchive {
        &self.archive
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn remaining(&self) -> usize {
        self.archive.events.len().saturating_sub(self.cursor)
    }

    pub fn is_finished(&self) -> bool {
        self.cursor >= self.archive.events.len()
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        self.clock = None;
    }

    /// Seek to the first event whose timestamp is >= `time`.
    pub fn seek(&mut self, time: UnixMs) {
        self.cursor = self
            .archive
            .events
            .partition_point(|event| event.time() < time);
        self.clock = Some(ReplayClock::new(time));
    }

    pub fn start(&mut self, rate: f64) {
        let anchor = self
            .archive
            .events
            .get(self.cursor)
            .map(ReplayEvent::time)
            .or_else(|| self.archive.events.last().map(ReplayEvent::time))
            .unwrap_or(UnixMs::ZERO);
        let mut clock = ReplayClock::new(anchor);
        clock.set_rate(rate);
        self.clock = Some(clock);
    }

    pub fn set_rate(&mut self, rate: f64) {
        if let Some(clock) = &mut self.clock {
            clock.set_rate(rate);
        } else {
            self.start(rate);
        }
    }

    pub fn playback_rate(&self) -> f64 {
        self.clock.map_or(1.0, |clock| clock.playback_rate)
    }

    pub fn next_event(&mut self) -> Option<ReplayEvent> {
        let event = self.archive.events.get(self.cursor)?.clone();
        self.cursor = self.cursor.saturating_add(1);
        Some(event)
    }

    pub fn drain_until(&mut self, time: UnixMs) -> Vec<ReplayEvent> {
        let start = self.cursor;
        while let Some(event) = self.archive.events.get(self.cursor) {
            if event.time() > time {
                break;
            }
            self.cursor += 1;
        }
        self.archive.events[start..self.cursor].to_vec()
    }

    /// Advance replay by wall-clock milliseconds and return all newly ready
    /// market events.
    pub fn advance(&mut self, wall_elapsed_ms: u64) -> Vec<ReplayEvent> {
        if self.clock.is_none() {
            self.start(1.0);
        }
        let target = self
            .clock
            .as_mut()
            .expect("clock initialized")
            .advance_wall_time(wall_elapsed_ms);
        self.drain_until(target)
    }
}

/// Collects live normalized events into a deterministic replay archive.
pub struct ReplayRecorder {
    events: Vec<ReplayEvent>,
    last_seen: Option<UnixMs>,
    out_of_order_events: u64,
}

impl Default for ReplayRecorder {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            last_seen: None,
            out_of_order_events: 0,
        }
    }
}

impl ReplayRecorder {
    pub fn record(&mut self, event: ReplayEvent) {
        let time = event.time();
        if self.last_seen.is_some_and(|last| time < last) {
            self.out_of_order_events = self.out_of_order_events.saturating_add(1);
        }
        self.last_seen = Some(self.last_seen.map_or(time, |last| last.max(time)));
        self.events.push(event);
    }

    pub fn record_many(&mut self, events: impl IntoIterator<Item = ReplayEvent>) {
        for event in events {
            self.record(event);
        }
    }

    pub fn out_of_order_events(&self) -> u64 {
        self.out_of_order_events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn finish(self) -> ReplayArchive {
        ReplayArchive::new(self.events)
    }
}

#[cfg(test)]
mod tests {
    use exchange::unit::{Price, Qty};

    use super::*;
    use crate::market::{AggressorSide, AssetClass, MarketVenue, NormalizedTrade};

    fn instrument() -> InstrumentId {
        InstrumentId::new(MarketVenue::Taifex, "TXF", AssetClass::Futures)
    }

    fn event(ms: u64) -> ReplayEvent {
        ReplayEvent::Market(NormalizedMarketEvent::Trade(NormalizedTrade {
            instrument: instrument(),
            time: UnixMs::new(ms),
            price: Price::from_f64(20_000.0),
            qty: Qty::from_f64(1.0),
            aggressor: AggressorSide::Buy,
            sequence: None,
        }))
    }

    #[test]
    fn archive_sorts_events_chronologically() {
        let archive = ReplayArchive::new(vec![event(300), event(100), event(200)]);
        let times: Vec<_> = archive.events.iter().map(|event| event.time().as_u64()).collect();
        assert_eq!(times, vec![100, 200, 300]);
    }

    #[test]
    fn seek_points_to_first_event_at_or_after_target() {
        let archive = ReplayArchive::new(vec![event(100), event(200), event(300)]);
        let mut replay = ReplayEngine::new(archive);
        replay.seek(UnixMs::new(200));
        assert_eq!(replay.next_event().unwrap().time(), UnixMs::new(200));
    }

    #[test]
    fn drain_until_is_deterministic_and_non_repeating() {
        let archive = ReplayArchive::new(vec![event(100), event(200), event(300)]);
        let mut replay = ReplayEngine::new(archive);

        assert_eq!(replay.drain_until(UnixMs::new(200)).len(), 2);
        assert_eq!(replay.drain_until(UnixMs::new(200)).len(), 0);
        assert_eq!(replay.drain_until(UnixMs::new(500)).len(), 1);
        assert!(replay.is_finished());
    }

    #[test]
    fn playback_rate_scales_market_clock() {
        let archive = ReplayArchive::new(vec![event(1_000), event(2_000), event(3_000)]);
        let mut replay = ReplayEngine::new(archive);
        replay.start(2.0);

        // At 2x speed, 500 ms of wall time advances market time by 1,000 ms.
        let ready = replay.advance(500);
        assert_eq!(ready.len(), 2); // events at 1000 and 2000
        assert_eq!(replay.remaining(), 1);
    }

    #[test]
    fn recorder_counts_out_of_order_input_but_finishes_sorted() {
        let mut recorder = ReplayRecorder::default();
        recorder.record(event(200));
        recorder.record(event(100));
        recorder.record(event(300));
        assert_eq!(recorder.out_of_order_events(), 1);

        let archive = recorder.finish();
        let times: Vec<_> = archive.events.iter().map(|event| event.time().as_u64()).collect();
        assert_eq!(times, vec![100, 200, 300]);
    }

    #[test]
    fn archive_json_round_trip_preserves_events() {
        let archive = ReplayArchive::new(vec![event(100), event(200)]);
        let json = archive.to_json().unwrap();
        let loaded = ReplayArchive::from_json(&json).unwrap();
        assert_eq!(loaded, archive);
    }
}
