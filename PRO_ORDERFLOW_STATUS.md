# Pro Order Flow Platform — Development Checkpoint

This branch extends Flowsurface as a native Rust/WGPU professional order-flow workstation.

## Implemented core

- Trade Speed / Buy Speed / Sell Speed / Volume Speed / Delta Speed
- Speed acceleration and configurable rolling windows
- Native Trade Speed Kline indicator
- Conservative trade-confirmed Book Consumption detector
- Book Speed aggregation and composed Book Flow engine
- TradingView-style drawing data model, persistence, magnet snapping, Fibonacci and undo/redo
- Provider-neutral market protocol for crypto, stocks, futures and options
- TWSE / TAIFEX instrument and gateway boundaries
- Sequence-gap detection for external market data adapters
- MBO order/queue model and queue-ahead estimates
- Iceberg candidate detection from stable MBO order IDs
- Absorption, exhaustion, liquidity stack/pull candidate, stop-run and large-order detectors
- Replay archive, recorder, playback clock, seek and playback-rate control
- Unified source-neutral OrderFlowPipeline shared by live and replay events

## Validation gate

The branch is not considered complete until all of the following pass on the current head:

1. `cargo fmt --all -- --check`
2. `cargo test --workspace --all-targets --all-features`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`

UI/render integration is tracked separately from the core-engine validation gate and must also be completed before the draft PR is marked ready.
