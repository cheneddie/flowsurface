from pathlib import Path

path = Path("src/chart/heatmap.rs")
text = path.read_text(encoding="utf-8")

# Imports
old = """use data::util::abbr_large_numbers;\nuse data::{\n    aggr::time::{DataPoint, TimeSeries},\n    chart::Autoscale,\n};\n"""
new = """use data::util::abbr_large_numbers;\nuse data::{\n    aggr::time::{DataPoint, TimeSeries},\n    chart::Autoscale,\n    orderflow::{BookFlowEngine, BookSpeedSnapshot},\n};\n"""
if old in text:
    text = text.replace(old, new, 1)

# Struct fields
old = """    pause_buffer: Vec<(UnixMs, Box<[Trade]>, Depth)>,\n    heatmap: HistoricalDepth,\n    visual_config: Config,\n"""
new = """    pause_buffer: Vec<(UnixMs, Box<[Trade]>, Depth)>,\n    heatmap: HistoricalDepth,\n    book_flow: BookFlowEngine,\n    latest_book_speed: Option<BookSpeedSnapshot>,\n    visual_config: Config,\n"""
if old in text:
    text = text.replace(old, new, 1)

# Constructor
old = """            pause_buffer: vec![],\n            heatmap,\n            trades: TimeSeries::<HeatmapDataPoint>::new(basis, step),\n            visual_config: config.unwrap_or_default(),\n"""
new = """            pause_buffer: vec![],\n            heatmap,\n            book_flow: BookFlowEngine::default(),\n            latest_book_speed: None,\n            trades: TimeSeries::<HeatmapDataPoint>::new(basis, step),\n            visual_config: config.unwrap_or_default(),\n"""
if old in text:
    text = text.replace(old, new, 1)

# record trades
old = """    pub fn insert_trades(&mut self, buffer: &[Trade], update_t: UnixMs) {\n        let rounded_update_t = self.round_to_basis_time(update_t);\n"""
new = """    pub fn insert_trades(&mut self, buffer: &[Trade], update_t: UnixMs) {\n        self.book_flow.record_trades(buffer);\n        let rounded_update_t = self.round_to_basis_time(update_t);\n"""
if old in text:
    text = text.replace(old, new, 1)

# process book flow on raw time before display pause/rounding
old = """    pub fn insert_depth(&mut self, depth: &Depth, update_t: UnixMs) {\n        let rounded_depth_update = self.round_to_basis_time(update_t);\n\n        let chart = &mut self.chart;\n"""
new = """    pub fn insert_depth(&mut self, depth: &Depth, update_t: UnixMs) {\n        let flow_update = self.book_flow.on_depth(update_t, depth);\n        if let Some(speed) = flow_update.speed {\n            self.latest_book_speed = Some(speed);\n        }\n\n        let rounded_depth_update = self.round_to_basis_time(update_t);\n\n        let chart = &mut self.chart;\n"""
if old in text:
    text = text.replace(old, new, 1)

# reset on basis
old = """        self.trades.datapoints.clear();\n        self.heatmap =\n            HistoricalDepth::new(self.chart.ticker_info.min_qty, self.chart.tick_size, basis);\n\n        let chart = &mut self.chart;\n"""
new = """        self.trades.datapoints.clear();\n        self.heatmap =\n            HistoricalDepth::new(self.chart.ticker_info.min_qty, self.chart.tick_size, basis);\n        self.book_flow.clear();\n        self.latest_book_speed = None;\n\n        let chart = &mut self.chart;\n"""
if old in text:
    text = text.replace(old, new, 1)

# reset on tick size
old = """        self.trades.datapoints.clear();\n        self.heatmap = HistoricalDepth::new(self.chart.ticker_info.min_qty, step, basis);\n    }\n\n    pub fn tick_size(&self) -> PriceStep {\n"""
new = """        self.trades.datapoints.clear();\n        self.heatmap = HistoricalDepth::new(self.chart.ticker_info.min_qty, step, basis);\n        self.book_flow.clear();\n        self.latest_book_speed = None;\n    }\n\n    pub fn latest_book_speed(&self) -> Option<BookSpeedSnapshot> {\n        self.latest_book_speed\n    }\n\n    pub fn tick_size(&self) -> PriceStep {\n"""
if old in text:
    text = text.replace(old, new, 1)

# HUD after current-depth block, before trades loop
marker = """            };\n\n            self.trades\n                .datapoints\n"""
hud = """            };\n\n            if let Some(speed) = self.latest_book_speed {\n                let text_size = crate::style::text_size::TINY / chart.scaling;\n                let text_position = Point::new(\n                    region.x + (8.0 / chart.scaling),\n                    region.y + (8.0 / chart.scaling),\n                );\n                frame.fill_text(canvas::Text {\n                    content: book_speed_hud_text(speed),\n                    position: text_position,\n                    size: iced::Pixels(text_size),\n                    color: palette.background.base.text,\n                    font: style::AZERET_MONO,\n                    align_x: Alignment::Start.into(),\n                    align_y: Alignment::Start.into(),\n                    ..canvas::Text::default()\n                });\n            }\n\n            self.trades\n                .datapoints\n"""
if marker in text:
    text = text.replace(marker, hud, 1)

# helper + test before Program impl
marker = """impl canvas::Program<Message> for HeatmapChart {\n"""
helper = """fn book_speed_hud_text(speed: BookSpeedSnapshot) -> String {\n    format!(\n        \"BOOK SPEED  Buy {:.1}L/s  Sell {:.1}L/s\\nBuy Qty {} /s  Sell Qty {} /s\",\n        speed.ask_levels_per_sec,\n        speed.bid_levels_per_sec,\n        abbr_large_numbers(speed.ask_qty_per_sec),\n        abbr_large_numbers(speed.bid_qty_per_sec),\n    )\n}\n\n#[cfg(test)]\nmod pro_orderflow_tests {\n    use super::*;\n\n    #[test]\n    fn book_speed_hud_maps_ask_consumption_to_buy_pressure() {\n        let speed = BookSpeedSnapshot {\n            ask_levels_per_sec: 3.0,\n            bid_levels_per_sec: 2.0,\n            ask_qty_per_sec: 12.0,\n            bid_qty_per_sec: 7.0,\n            ..BookSpeedSnapshot::default()\n        };\n        let text = book_speed_hud_text(speed);\n        assert!(text.contains(\"Buy 3.0L/s\"));\n        assert!(text.contains(\"Sell 2.0L/s\"));\n    }\n}\n\nimpl canvas::Program<Message> for HeatmapChart {\n"""
if marker in text and "fn book_speed_hud_text" not in text:
    text = text.replace(marker, helper, 1)

required = [
    "book_flow: BookFlowEngine",
    "latest_book_speed: Option<BookSpeedSnapshot>",
    "self.book_flow.record_trades(buffer)",
    "self.book_flow.on_depth(update_t, depth)",
    "fn book_speed_hud_text",
]
for needle in required:
    if needle not in text:
        raise SystemExit(f"heatmap integration missing: {needle}")

path.write_text(text, encoding="utf-8")
