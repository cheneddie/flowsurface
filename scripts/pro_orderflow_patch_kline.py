from pathlib import Path

path = Path("src/chart/kline.rs")
text = path.read_text(encoding="utf-8")

constructor_old = """                    let mut indi = indicator::kline::make_empty(i);\n                    indi.rebuild_from_source(&data_source);\n                    indicators[i] = Some(indi);\n"""
constructor_new = """                    let mut indi = indicator::kline::make_empty(i);\n                    indi.rebuild_from_source(&data_source);\n                    indi.seed_trades(&raw_trades, &data_source);\n                    indicators[i] = Some(indi);\n"""

count = text.count(constructor_old)
if count not in (0, 2):
    raise SystemExit(f"expected 0 or 2 constructor matches, found {count}")
if count == 2:
    text = text.replace(constructor_old, constructor_new)

toggle_old = """            let mut box_indi = indicator::kline::make_empty(indicator);\n            box_indi.rebuild_from_source(&self.data_source);\n            self.indicators[indicator] = Some(box_indi);\n"""
toggle_new = """            let mut box_indi = indicator::kline::make_empty(indicator);\n            box_indi.rebuild_from_source(&self.data_source);\n            box_indi.seed_trades(&self.raw_trades, &self.data_source);\n            self.indicators[indicator] = Some(box_indi);\n"""

count = text.count(toggle_old)
if count not in (0, 1):
    raise SystemExit(f"expected 0 or 1 toggle match, found {count}")
if count == 1:
    text = text.replace(toggle_old, toggle_new)

if text.count("seed_trades(&raw_trades, &data_source)") != 2:
    raise SystemExit("constructor seed integration missing")
if text.count("seed_trades(&self.raw_trades, &self.data_source)") != 1:
    raise SystemExit("toggle seed integration missing")

path.write_text(text, encoding="utf-8")
