use std::collections::VecDeque;

use exchange::{Trade, UnixMs, unit::{Price, Qty}};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetClass {
    Crypto,
    Stock,
    Futures,
    Options,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketVenue {
    Twse,
    Taifex,
    Cme,
    Nyse,
    Nasdaq,
    Crypto(String),
    Custom(String),
}

impl std::fmt::Display for MarketVenue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Twse => write!(f, "TWSE"),
            Self::Taifex => write!(f, "TAIFEX"),
            Self::Cme => write!(f, "CME"),
            Self::Nyse => write!(f, "NYSE"),
            Self::Nasdaq => write!(f, "NASDAQ"),
            Self::Crypto(name) | Self::Custom(name) => write!(f, "{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrumentId {
    pub venue: MarketVenue,
    pub symbol: String,
    pub asset_class: AssetClass,
}

impl InstrumentId {
    pub fn new(
        venue: MarketVenue,
        symbol: impl Into<String>,
        asset_class: AssetClass,
    ) -> Self {
        Self {
            venue,
            symbol: symbol.into(),
            asset_class,
        }
    }

    pub fn twse(symbol: impl Into<String>) -> Self {
        Self::new(MarketVenue::Twse, symbol, AssetClass::Stock)
    }

    pub fn taifex_future(symbol: impl Into<String>) -> Self {
        Self::new(MarketVenue::Taifex, symbol, AssetClass::Futures)
    }

    pub fn taifex_option(symbol: impl Into<String>) -> Self {
        Self::new(MarketVenue::Taifex, symbol, AssetClass::Options)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggressorSide {
    Buy,
    Sell,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookSide {
    Bid,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepthEventKind {
    Snapshot,
    Diff,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedTrade {
    pub instrument: InstrumentId,
    pub time: UnixMs,
    pub price: Price,
    pub qty: Qty,
    pub aggressor: AggressorSide,
    /// Optional venue/exchange sequence number for gap detection.
    pub sequence: Option<u64>,
}

impl NormalizedTrade {
    /// Bridge into the current Flowsurface trade model.
    ///
    /// Order-flow classification requires a known aggressor. Unknown-side
    /// trades are deliberately rejected rather than guessed here; a provider
    /// adapter may classify them upstream using quote/tick rules if needed.
    pub fn to_flowsurface_trade(&self) -> Option<Trade> {
        let is_sell = match self.aggressor {
            AggressorSide::Buy => false,
            AggressorSide::Sell => true,
            AggressorSide::Unknown => return None,
        };

        Some(Trade {
            time: self.time,
            is_sell,
            price: self.price,
            qty: self.qty,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizedBookLevel {
    pub side: BookSide,
    pub price: Price,
    pub qty: Qty,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedDepthEvent {
    pub instrument: InstrumentId,
    pub time: UnixMs,
    pub kind: DepthEventKind,
    pub sequence: Option<u64>,
    pub levels: Vec<NormalizedBookLevel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NormalizedMarketEvent {
    Trade(NormalizedTrade),
    Depth(NormalizedDepthEvent),
}

impl NormalizedMarketEvent {
    pub fn instrument(&self) -> &InstrumentId {
        match self {
            Self::Trade(event) => &event.instrument,
            Self::Depth(event) => &event.instrument,
        }
    }

    pub fn time(&self) -> UnixMs {
        match self {
            Self::Trade(event) => event.time,
            Self::Depth(event) => event.time,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketCapabilities {
    pub trades: bool,
    pub l1: bool,
    pub l2: bool,
    pub mbo: bool,
    pub historical_trades: bool,
    pub historical_depth: bool,
}

impl MarketCapabilities {
    pub const fn l2_orderflow() -> Self {
        Self {
            trades: true,
            l1: true,
            l2: true,
            mbo: false,
            historical_trades: false,
            historical_depth: false,
        }
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum MarketAdapterError {
    #[error("data source disconnected")]
    Disconnected,
    #[error("sequence gap: expected {expected}, received {received}")]
    SequenceGap { expected: u64, received: u64 },
    #[error("adapter error: {0}")]
    Other(String),
}

/// Provider-neutral adapter boundary.
///
/// Network/API-specific code belongs behind this trait. The rest of Flowsurface
/// consumes only `NormalizedMarketEvent`, which keeps TWSE/TAIFEX/broker APIs
/// separate from chart and order-flow logic.
pub trait MarketDataAdapter {
    fn name(&self) -> &str;
    fn venue(&self) -> &MarketVenue;
    fn capabilities(&self) -> MarketCapabilities;
    fn drain_events(&mut self) -> Result<Vec<NormalizedMarketEvent>, MarketAdapterError>;
}

/// Testable queue-backed adapter useful as the handoff point for broker SDKs,
/// FFI callbacks, WebSocket clients and local gateways.
///
/// A concrete Taiwan broker integration can push its normalized callbacks into
/// this queue without changing chart/order-flow code.
pub struct QueueMarketAdapter {
    name: String,
    venue: MarketVenue,
    capabilities: MarketCapabilities,
    events: VecDeque<NormalizedMarketEvent>,
    last_sequence: Option<u64>,
    enforce_sequence: bool,
}

impl QueueMarketAdapter {
    pub fn new(
        name: impl Into<String>,
        venue: MarketVenue,
        capabilities: MarketCapabilities,
    ) -> Self {
        Self {
            name: name.into(),
            venue,
            capabilities,
            events: VecDeque::new(),
            last_sequence: None,
            enforce_sequence: false,
        }
    }

    pub fn taiwan_stock_gateway(name: impl Into<String>) -> Self {
        Self::new(name, MarketVenue::Twse, MarketCapabilities::l2_orderflow())
    }

    pub fn taiwan_futures_gateway(name: impl Into<String>) -> Self {
        Self::new(name, MarketVenue::Taifex, MarketCapabilities::l2_orderflow())
    }

    pub fn set_sequence_enforcement(&mut self, enabled: bool) {
        self.enforce_sequence = enabled;
        if !enabled {
            self.last_sequence = None;
        }
    }

    pub fn push(&mut self, event: NormalizedMarketEvent) -> Result<(), MarketAdapterError> {
        if self.enforce_sequence
            && let Some(sequence) = event_sequence(&event)
        {
            if let Some(last) = self.last_sequence {
                let expected = last.saturating_add(1);
                if sequence != expected {
                    return Err(MarketAdapterError::SequenceGap {
                        expected,
                        received: sequence,
                    });
                }
            }
            self.last_sequence = Some(sequence);
        }

        self.events.push_back(event);
        Ok(())
    }

    pub fn queued_len(&self) -> usize {
        self.events.len()
    }
}

impl MarketDataAdapter for QueueMarketAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn venue(&self) -> &MarketVenue {
        &self.venue
    }

    fn capabilities(&self) -> MarketCapabilities {
        self.capabilities
    }

    fn drain_events(&mut self) -> Result<Vec<NormalizedMarketEvent>, MarketAdapterError> {
        Ok(self.events.drain(..).collect())
    }
}

fn event_sequence(event: &NormalizedMarketEvent) -> Option<u64> {
    match event {
        NormalizedMarketEvent::Trade(event) => event.sequence,
        NormalizedMarketEvent::Depth(event) => event.sequence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(sequence: Option<u64>, aggressor: AggressorSide) -> NormalizedMarketEvent {
        NormalizedMarketEvent::Trade(NormalizedTrade {
            instrument: InstrumentId::taifex_future("TXF"),
            time: UnixMs::new(1_000),
            price: Price::from_f64(20_000.0),
            qty: Qty::from_f64(2.0),
            aggressor,
            sequence,
        })
    }

    #[test]
    fn taifex_future_has_correct_asset_class() {
        let id = InstrumentId::taifex_future("TXF");
        assert_eq!(id.venue, MarketVenue::Taifex);
        assert_eq!(id.asset_class, AssetClass::Futures);
    }

    #[test]
    fn known_aggressor_converts_to_existing_trade_model() {
        let NormalizedMarketEvent::Trade(event) = trade(None, AggressorSide::Buy) else {
            unreachable!();
        };
        let converted = event.to_flowsurface_trade().expect("known aggressor");
        assert!(!converted.is_sell);
        assert_eq!(converted.qty, Qty::from_f64(2.0));
    }

    #[test]
    fn unknown_aggressor_is_not_silently_guessed() {
        let NormalizedMarketEvent::Trade(event) = trade(None, AggressorSide::Unknown) else {
            unreachable!();
        };
        assert!(event.to_flowsurface_trade().is_none());
    }

    #[test]
    fn queue_adapter_drains_events_in_order() {
        let mut adapter = QueueMarketAdapter::taiwan_futures_gateway("test");
        adapter.push(trade(None, AggressorSide::Buy)).unwrap();
        adapter.push(trade(None, AggressorSide::Sell)).unwrap();

        assert_eq!(adapter.queued_len(), 2);
        let drained = adapter.drain_events().unwrap();
        assert_eq!(drained.len(), 2);
        assert_eq!(adapter.queued_len(), 0);
    }

    #[test]
    fn sequence_gap_is_detected_before_event_is_queued() {
        let mut adapter = QueueMarketAdapter::taiwan_futures_gateway("test");
        adapter.set_sequence_enforcement(true);
        adapter.push(trade(Some(10), AggressorSide::Buy)).unwrap();

        let err = adapter
            .push(trade(Some(12), AggressorSide::Buy))
            .expect_err("gap should fail");

        assert_eq!(
            err,
            MarketAdapterError::SequenceGap {
                expected: 11,
                received: 12,
            }
        );
        assert_eq!(adapter.queued_len(), 1);
    }
}
