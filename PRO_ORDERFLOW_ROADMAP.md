# Flowsurface Pro Order Flow — Development Roadmap

Base: `flowsurface-rs/flowsurface`
Development branch: `feature/pro-orderflow-platform`

## Phase 1 — Speed Engine
Status: IN PROGRESS

Core completed:
- Trade rate / second
- Buy trade rate / second
- Sell trade rate / second
- Total volume speed
- Buy volume speed
- Sell volume speed
- Delta volume speed
- Raw acceleration
- Percentage acceleration
- Directional delta-share
- Book Speed event/aggregator abstraction
- Unit tests for trade/book speed core

Next integration:
1. Add `KlineIndicator::TradeSpeed`
2. Add configurable windows: 250ms / 500ms / 1s / 2s / 5s / 10s
3. Render Buy/Sell/Delta Speed in the indicator pane
4. Feed the same engine into Heatmap HUD
5. Persist indicator configuration

## Phase 2 — Book Consumption Detector
- Consume L2 depth diffs + trade stream
- Distinguish likely consumed liquidity from cancellations
- Emit `BookConsumptionEvent`
- Bid levels consumed / sec
- Ask levels consumed / sec
- Level-speed acceleration
- Quantity-speed acceleration

## Phase 3 — TradingView-style Drawing Engine
- Trend line
- Horizontal / vertical line
- Ray
- Rectangle
- Fibonacci retracement
- Fibonacci extension
- Text / annotation
- Magnet snapping
- Selection / drag
- Lock / hide / delete
- Undo / redo
- Persistent drawings by symbol + timeframe

## Phase 4 — Market Adapter Generalization
Current crypto-only assumptions must be generalized:
- Venue / AssetClass separation
- Stock
- Futures
- Options
- Crypto spot/perps

Then add Taiwan adapters:
- TWSE
- TAIFEX
- Broker/data-feed adapter interface

## Phase 5 — Advanced Order Flow
- Absorption
- Exhaustion
- Liquidity pull / stack
- Stop-run detector
- Large order tracker
- Iceberg heuristics
- MBO event model and queue-aware extensions

## Phase 6 — Replay / Research
- Unified live + replay event bus
- Tick / L2 recording
- Historical replay
- Strategy event export
- Performance profiling and benchmark suite
