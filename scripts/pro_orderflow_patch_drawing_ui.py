from pathlib import Path

chart_path = Path("src/chart.rs")
chart = chart_path.read_text(encoding="utf-8")
if "pub mod drawing_overlay;" not in chart:
    chart = chart.replace("pub mod comparison;\n", "pub mod comparison;\npub mod drawing_overlay;\n", 1)
chart_path.write_text(chart, encoding="utf-8")

path = Path("src/chart/kline.rs")
text = path.read_text(encoding="utf-8")

old = """    data_source: PlotData<KlineDataPoint>,\n    raw_trades: Vec<Trade>,\n    indicators: EnumMap<KlineIndicator, Option<Box<dyn KlineIndicatorImpl>>>,\n"""
new = """    data_source: PlotData<KlineDataPoint>,\n    raw_trades: Vec<Trade>,\n    drawing_overlay: super::drawing_overlay::DrawingOverlay,\n    indicators: EnumMap<KlineIndicator, Option<Box<dyn KlineIndicatorImpl>>>,\n"""
if old in text:
    text = text.replace(old, new, 1)

# Two constructors
old = """                    data_source,\n                    raw_trades,\n                    indicators,\n"""
new = """                    data_source,\n                    raw_trades,\n                    drawing_overlay: super::drawing_overlay::DrawingOverlay::new(ticker_info, basis),\n                    indicators,\n"""
count = text.count(old)
if count not in (0, 2):
    raise SystemExit(f"expected 0 or 2 constructor locations, found {count}")
if count == 2:
    text = text.replace(old, new)

old = """    ) -> Option<canvas::Action<Message>> {\n        super::canvas_interaction(self, interaction, event, bounds, cursor)\n    }\n\n    fn draw(\n"""
new = """    ) -> Option<canvas::Action<Message>> {\n        if let Some(action) = self.drawing_overlay.update(\n            event,\n            bounds,\n            cursor,\n            &self.chart,\n            &self.data_source,\n        ) {\n            self.chart.cache.clear_all();\n            return Some(action);\n        }\n        super::canvas_interaction(self, interaction, event, bounds, cursor)\n    }\n\n    fn draw(\n"""
if old in text:
    text = text.replace(old, new, 1)

old = """            chart.draw_last_price_line(frame, palette, region);\n        });\n"""
new = """            self.drawing_overlay\n                .draw(frame, chart, &self.data_source, palette, region);\n            chart.draw_last_price_line(frame, palette, region);\n        });\n"""
if old in text:
    text = text.replace(old, new, 1)

checks = [
    "drawing_overlay: super::drawing_overlay::DrawingOverlay",
    "DrawingOverlay::new(ticker_info, basis)",
    "self.drawing_overlay.update(",
    "self.drawing_overlay\n                .draw(",
]
for needle in checks:
    if needle not in text:
        raise SystemExit(f"drawing UI integration missing: {needle}")

if text.count("DrawingOverlay::new(ticker_info, basis)") != 2:
    raise SystemExit("expected DrawingOverlay in both constructors")

path.write_text(text, encoding="utf-8")
