from pathlib import Path

# ---- Drawing overlay fixes ----
path = Path("src/chart/drawing_overlay.rs")
text = path.read_text(encoding="utf-8")
text = text.replace(
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum DrawingTool {\n    Select,",
    "#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]\npub enum DrawingTool {\n    #[default]\n    Select,",
    1,
)
manual_default = """impl Default for DrawingTool {
    fn default() -> Self {
        Self::Select
    }
}

"""
text = text.replace(manual_default, "", 1)
unused_tool = """    pub fn tool(&self) -> DrawingTool {
        self.tool.get()
    }

"""
text = text.replace(unused_tool, "", 1)
text = text.replace(
    """                let Some(pos) = cursor_pos else {
                    return None;
                };
""",
    """                let pos = cursor_pos?;
""",
    1,
)
if "impl Default for DrawingTool" in text:
    raise SystemExit("manual DrawingTool Default still present")
if "pub fn tool(&self)" in text:
    raise SystemExit("unused DrawingOverlay::tool still present")
path.write_text(text, encoding="utf-8")

# ---- Box DrawingOverlay to keep dashboard Content enum compact ----
path = Path("src/chart/kline.rs")
text = path.read_text(encoding="utf-8")
text = text.replace(
    "drawing_overlay: super::drawing_overlay::DrawingOverlay,",
    "drawing_overlay: Box<super::drawing_overlay::DrawingOverlay>,",
    1,
)
text = text.replace(
    "drawing_overlay: super::drawing_overlay::DrawingOverlay::new(\n                        ticker_info, basis,\n                    ),",
    "drawing_overlay: Box::new(super::drawing_overlay::DrawingOverlay::new(\n                        ticker_info, basis,\n                    )),",
)
# rustfmt may use multi-line args
text = text.replace(
    "drawing_overlay: super::drawing_overlay::DrawingOverlay::new(\n                        ticker_info,\n                        basis,\n                    ),",
    "drawing_overlay: Box::new(super::drawing_overlay::DrawingOverlay::new(\n                        ticker_info,\n                        basis,\n                    )),",
)
if "drawing_overlay: super::drawing_overlay::DrawingOverlay," in text:
    raise SystemExit("DrawingOverlay field was not boxed")
if text.count("drawing_overlay: Box::new(super::drawing_overlay::DrawingOverlay::new(") != 2:
    raise SystemExit("expected boxed DrawingOverlay in both Kline constructors")
path.write_text(text, encoding="utf-8")

# ---- Move Heatmap test module after all non-test items ----
path = Path("src/chart/heatmap.rs")
text = path.read_text(encoding="utf-8")
start_token = "#[cfg(test)]\nmod pro_orderflow_tests {"
start = text.find(start_token)
if start != -1:
    brace_start = text.find("{", start)
    depth = 0
    end = None
    for i in range(brace_start, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                end = i + 1
                break
    if end is None:
        raise SystemExit("could not parse Heatmap test module")
    module = text[start:end].strip()
    text = (text[:start].rstrip() + "\n\n" + text[end:].lstrip()).rstrip() + "\n\n" + module + "\n"

if text.find(start_token) < text.find("impl canvas::Program<Message> for HeatmapChart"):
    raise SystemExit("Heatmap tests still precede implementation")
path.write_text(text, encoding="utf-8")
