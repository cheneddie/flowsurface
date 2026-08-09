from pathlib import Path

path = Path("src/chart/drawing_overlay.rs")
text = path.read_text(encoding="utf-8")
text = text.replace("Alignment, Point, Rectangle, Size, Theme, keyboard, mouse,", "Alignment, Point, Rectangle, Size, keyboard, mouse,")
text = text.replace("align_y: Alignment::Bottom.into(),", "align_y: Alignment::End.into(),")
if "Theme, keyboard" in text:
    raise SystemExit("unused Theme import still present")
if "Alignment::Bottom" in text:
    raise SystemExit("unsupported Alignment::Bottom still present")
path.write_text(text, encoding="utf-8")
