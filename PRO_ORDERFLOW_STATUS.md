# Pro Order Flow Platform — Development Checkpoint

This branch extends Flowsurface as a native Rust/WGPU professional order-flow workstation.

## Implemented core

- Trade Speed / Buy Speed / Sell Speed / Volume Speed / Delta Speed
- Speed acceleration and selectable 250ms / 500ms / 1s / 2s / 5s / 10s rolling windows
- Native Trade Speed Kline indicators with buffered-trade seeding
- Conservative trade-confirmed Book Consumption detector
- Book Speed aggregation and composed Book Flow engine
- Live Heatmap Book Speed integration and HUD
- TradingView-style drawing data model, persistence, magnet snapping, Fibonacci and undo/redo
- Native Kline drawing overlay with toolbar, selection, move, lock, hide, delete and persistence
- Provider-neutral market protocol for crypto, stocks, futures and options
- TWSE / TAIFEX instrument and gateway boundaries
- Versioned JSONL local gateway protocol for broker SDK / external feed bridges
- Sequence-gap detection for external market data adapters
- MBO order/queue model and queue-ahead estimates
- Iceberg candidate detection from stable MBO order IDs
- Absorption, exhaustion, liquidity stack/pull candidate, stop-run and large-order detectors
- Replay archive, recorder, playback clock, seek and playback-rate control
- Instrument-bound ReplaySession with play/pause/rate/seek/reset
- Unified source-neutral OrderFlowPipeline shared by live and replay events

## Validation gate

The branch is not considered complete until all of the following pass on the current head:

1. `cargo fmt --all -- --check`
2. `cargo test --workspace --all-targets --all-features`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`

The current validation cycle compiles Drawing Overlay, Heatmap Book Speed HUD, selectable Trade Speed indicators, ReplaySession, JSONL Market Gateway and all core order-flow engines together.
