from pathlib import Path

# data/src/chart/indicator.rs
path = Path("data/src/chart/indicator.rs")
text = path.read_text(encoding="utf-8")
text = text.replace(
"""    CumulativeDelta,
    TradeSpeed,
    OpenInterest,
""",
"""    CumulativeDelta,
    TradeSpeed250ms,
    TradeSpeed500ms,
    TradeSpeed,
    TradeSpeed2s,
    TradeSpeed5s,
    TradeSpeed10s,
    OpenInterest,
""",
1,
)
text = text.replace(
"""    const FOR_SPOT: [KlineIndicator; 4] = [
        KlineIndicator::Volume,
        KlineIndicator::BarAnalysis,
        KlineIndicator::CumulativeDelta,
        KlineIndicator::TradeSpeed,
    ];
""",
"""    const FOR_SPOT: [KlineIndicator; 9] = [
        KlineIndicator::Volume,
        KlineIndicator::BarAnalysis,
        KlineIndicator::CumulativeDelta,
        KlineIndicator::TradeSpeed250ms,
        KlineIndicator::TradeSpeed500ms,
        KlineIndicator::TradeSpeed,
        KlineIndicator::TradeSpeed2s,
        KlineIndicator::TradeSpeed5s,
        KlineIndicator::TradeSpeed10s,
    ];
""",
1,
)
text = text.replace(
"""    const FOR_PERPS: [KlineIndicator; 5] = [
        KlineIndicator::Volume,
        KlineIndicator::BarAnalysis,
        KlineIndicator::CumulativeDelta,
        KlineIndicator::TradeSpeed,
        KlineIndicator::OpenInterest,
    ];
""",
"""    const FOR_PERPS: [KlineIndicator; 10] = [
        KlineIndicator::Volume,
        KlineIndicator::BarAnalysis,
        KlineIndicator::CumulativeDelta,
        KlineIndicator::TradeSpeed250ms,
        KlineIndicator::TradeSpeed500ms,
        KlineIndicator::TradeSpeed,
        KlineIndicator::TradeSpeed2s,
        KlineIndicator::TradeSpeed5s,
        KlineIndicator::TradeSpeed10s,
        KlineIndicator::OpenInterest,
    ];
""",
1,
)
text = text.replace(
"""            KlineIndicator::CumulativeDelta => write!(f, "CVD"),
            KlineIndicator::TradeSpeed => write!(f, "Trade Speed"),
            KlineIndicator::OpenInterest => write!(f, "Open Interest"),
""",
"""            KlineIndicator::CumulativeDelta => write!(f, "CVD"),
            KlineIndicator::TradeSpeed250ms => write!(f, "Trade Speed 250ms"),
            KlineIndicator::TradeSpeed500ms => write!(f, "Trade Speed 500ms"),
            KlineIndicator::TradeSpeed => write!(f, "Trade Speed 1s"),
            KlineIndicator::TradeSpeed2s => write!(f, "Trade Speed 2s"),
            KlineIndicator::TradeSpeed5s => write!(f, "Trade Speed 5s"),
            KlineIndicator::TradeSpeed10s => write!(f, "Trade Speed 10s"),
            KlineIndicator::OpenInterest => write!(f, "Open Interest"),
""",
1,
)
path.write_text(text, encoding="utf-8")

# src/chart/indicator/kline.rs
path = Path("src/chart/indicator/kline.rs")
text = path.read_text(encoding="utf-8")
if "use data::orderflow::SpeedWindow;" not in text:
    text = text.replace(
        "use data::chart::{BasisSeries, PlotData};\n",
        "use data::chart::{BasisSeries, PlotData};\nuse data::orderflow::SpeedWindow;\n",
        1,
    )
old = """        KlineIndicator::TradeSpeed => {
            Box::new(super::kline::trade_speed::TradeSpeedIndicator::new())
        }
"""
new = """        KlineIndicator::TradeSpeed250ms => Box::new(
            super::kline::trade_speed::TradeSpeedIndicator::with_window(SpeedWindow::Ms250),
        ),
        KlineIndicator::TradeSpeed500ms => Box::new(
            super::kline::trade_speed::TradeSpeedIndicator::with_window(SpeedWindow::Ms500),
        ),
        KlineIndicator::TradeSpeed => Box::new(
            super::kline::trade_speed::TradeSpeedIndicator::with_window(SpeedWindow::S1),
        ),
        KlineIndicator::TradeSpeed2s => Box::new(
            super::kline::trade_speed::TradeSpeedIndicator::with_window(SpeedWindow::S2),
        ),
        KlineIndicator::TradeSpeed5s => Box::new(
            super::kline::trade_speed::TradeSpeedIndicator::with_window(SpeedWindow::S5),
        ),
        KlineIndicator::TradeSpeed10s => Box::new(
            super::kline::trade_speed::TradeSpeedIndicator::with_window(SpeedWindow::S10),
        ),
"""
if old in text:
    text = text.replace(old, new, 1)
path.write_text(text, encoding="utf-8")

# trade_speed.rs
path = Path("src/chart/indicator/kline/trade_speed.rs")
text = path.read_text(encoding="utf-8")
text = text.replace(
    "orderflow::{SpeedConfig, TradeSpeedEngine, TradeSpeedSnapshot}",
    "orderflow::{SpeedWindow, TradeSpeedEngine, TradeSpeedSnapshot}",
    1,
)
text = text.replace("const DEFAULT_WINDOW_MS: u64 = 1_000;\n\n", "", 1)
old = """    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            engine: TradeSpeedEngine::new(SpeedConfig::new(DEFAULT_WINDOW_MS)),
            data: BTreeMap::new(),
            interval: None,
            availability: IndicatorAvailability::Unknown,
        }
    }
"""
new = """    pub fn new() -> Self {
        Self::with_window(SpeedWindow::S1)
    }

    pub fn with_window(window: SpeedWindow) -> Self {
        Self {
            cache: Caches::default(),
            engine: TradeSpeedEngine::new(window.into()),
            data: BTreeMap::new(),
            interval: None,
            availability: IndicatorAvailability::Unknown,
        }
    }
"""
if old in text:
    text = text.replace(old, new, 1)

marker = """    #[test]
    fn tooltip_contains_core_speed_metrics() {"""
extra = """    #[test]
    fn selectable_windows_configure_engine() {
        for window in SpeedWindow::ALL {
            let indicator = TradeSpeedIndicator::with_window(window);
            assert_eq!(indicator.engine.config().window_ms, window.millis());
        }
    }

    #[test]
    fn tooltip_contains_core_speed_metrics() {"""
if marker in text and "selectable_windows_configure_engine" not in text:
    text = text.replace(marker, extra, 1)
path.write_text(text, encoding="utf-8")

checks = [
    (Path("data/src/chart/indicator.rs"), "TradeSpeed250ms"),
    (Path("src/chart/indicator/kline.rs"), "SpeedWindow::S10"),
    (Path("src/chart/indicator/kline/trade_speed.rs"), "with_window(window: SpeedWindow)"),
]
for file, needle in checks:
    if needle not in file.read_text(encoding="utf-8"):
        raise SystemExit(f"speed-window integration missing {needle} in {file}")
