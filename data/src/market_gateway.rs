use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::market::{MarketAdapterError, NormalizedMarketEvent};

pub const MARKET_GATEWAY_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayFrame {
    pub version: u16,
    pub event: NormalizedMarketEvent,
}

impl GatewayFrame {
    pub fn new(event: NormalizedMarketEvent) -> Self {
        Self {
            version: MARKET_GATEWAY_SCHEMA_VERSION,
            event,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON frame: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported gateway schema version {received}; expected {expected}")]
    Version { expected: u16, received: u16 },
    #[error("market adapter error: {0}")]
    Adapter(#[from] MarketAdapterError),
}

/// Write one normalized market event as a newline-delimited JSON frame.
///
/// This deliberately uses a tiny, language-neutral protocol so native broker
/// SDK bridges can be written in C#, Python, C++, Rust or Java without linking
/// against the Flowsurface process itself.
pub fn write_jsonl_event(
    writer: &mut impl Write,
    event: &NormalizedMarketEvent,
) -> Result<(), GatewayError> {
    serde_json::to_writer(&mut *writer, &GatewayFrame::new(event.clone()))?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Streaming decoder for local gateway pipes/TCP streams/files.
/// Empty lines are ignored. Every non-empty frame is schema-version checked.
pub struct JsonlGatewayReader<R> {
    reader: R,
    line: String,
}

impl<R: BufRead> JsonlGatewayReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            line: String::new(),
        }
    }

    pub fn next_event(&mut self) -> Result<Option<NormalizedMarketEvent>, GatewayError> {
        loop {
            self.line.clear();
            let bytes = self.reader.read_line(&mut self.line)?;
            if bytes == 0 {
                return Ok(None);
            }
            if self.line.trim().is_empty() {
                continue;
            }

            let frame: GatewayFrame = serde_json::from_str(self.line.trim_end())?;
            if frame.version != MARKET_GATEWAY_SCHEMA_VERSION {
                return Err(GatewayError::Version {
                    expected: MARKET_GATEWAY_SCHEMA_VERSION,
                    received: frame.version,
                });
            }
            return Ok(Some(frame.event));
        }
    }

    pub fn drain_available(&mut self) -> Result<Vec<NormalizedMarketEvent>, GatewayError> {
        let mut out = Vec::new();
        while let Some(event) = self.next_event()? {
            out.push(event);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use exchange::{
        UnixMs,
        unit::{Price, Qty},
    };

    use super::*;
    use crate::market::{AggressorSide, AssetClass, InstrumentId, MarketVenue, NormalizedTrade};

    fn event(sequence: u64) -> NormalizedMarketEvent {
        NormalizedMarketEvent::Trade(NormalizedTrade {
            instrument: InstrumentId::new(MarketVenue::Taifex, "TXF", AssetClass::Futures),
            time: UnixMs::new(1_000 + sequence),
            price: Price::from_f64(20_000.0),
            qty: Qty::from_f64(1.0),
            aggressor: AggressorSide::Buy,
            sequence: Some(sequence),
        })
    }

    #[test]
    fn jsonl_round_trip_preserves_normalized_event() {
        let expected = event(1);
        let mut bytes = Vec::new();
        write_jsonl_event(&mut bytes, &expected).unwrap();

        let mut reader = JsonlGatewayReader::new(BufReader::new(Cursor::new(bytes)));
        assert_eq!(reader.next_event().unwrap(), Some(expected));
        assert_eq!(reader.next_event().unwrap(), None);
    }

    #[test]
    fn multiple_frames_preserve_wire_order() {
        let mut bytes = Vec::new();
        write_jsonl_event(&mut bytes, &event(10)).unwrap();
        write_jsonl_event(&mut bytes, &event(11)).unwrap();

        let mut reader = JsonlGatewayReader::new(BufReader::new(Cursor::new(bytes)));
        let events = reader.drain_available().unwrap();
        assert_eq!(events, vec![event(10), event(11)]);
    }

    #[test]
    fn incompatible_schema_is_rejected() {
        let frame = GatewayFrame {
            version: MARKET_GATEWAY_SCHEMA_VERSION + 1,
            event: event(1),
        };
        let mut bytes = serde_json::to_vec(&frame).unwrap();
        bytes.push(b'\n');

        let mut reader = JsonlGatewayReader::new(BufReader::new(Cursor::new(bytes)));
        assert!(matches!(
            reader.next_event(),
            Err(GatewayError::Version { .. })
        ));
    }

    #[test]
    fn empty_lines_are_ignored() {
        let mut bytes = b"\n\n".to_vec();
        write_jsonl_event(&mut bytes, &event(1)).unwrap();
        let mut reader = JsonlGatewayReader::new(BufReader::new(Cursor::new(bytes)));
        assert_eq!(reader.next_event().unwrap(), Some(event(1)));
    }
}
