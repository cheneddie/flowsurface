pub mod book_consumption;
pub mod speed;
pub mod window;

pub use book_consumption::{BookConsumptionDetector, ConsumptionConfig};
pub use speed::{
    BookConsumptionEvent, BookSide, BookSpeedEngine, BookSpeedSnapshot, SpeedConfig,
    TradeSpeedEngine, TradeSpeedSnapshot,
};
pub use window::SpeedWindow;
