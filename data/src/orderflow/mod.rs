pub mod advanced;
pub mod book_consumption;
pub mod book_flow;
pub mod mbo;
pub mod speed;
pub mod window;

pub use advanced::{
    AbsorptionConfig, AbsorptionSignal, AggressiveSide, ExhaustionConfig, ExhaustionSignal,
    LargeRestingOrder, LargeTradeSignal, LiquidityChangeKind, LiquidityChangeSignal,
    StopRunConfig, StopRunObservation, StopRunSignal, detect_absorption, detect_exhaustion,
    detect_large_resting_orders, detect_large_trade, detect_liquidity_changes, detect_stop_run,
};
pub use book_consumption::{BookConsumptionDetector, ConsumptionConfig};
pub use book_flow::{BookFlowEngine, BookFlowUpdate};
pub use mbo::{
    IcebergCandidate, IcebergConfig, MboAction, MboBook, MboEvent, MboOrder, MboSide, OrderId,
};
pub use speed::{
    BookConsumptionEvent, BookSide, BookSpeedEngine, BookSpeedSnapshot, SpeedConfig,
    TradeSpeedEngine, TradeSpeedSnapshot,
};
pub use window::SpeedWindow;
