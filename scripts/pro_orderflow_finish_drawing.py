from pathlib import Path

path = Path("src/chart/drawing_overlay.rs")
text = path.read_text(encoding="utf-8")

# 1) Tool variants / toolbar entry points.
text = text.replace(
"""    HorizontalLine,
    VerticalLine,
    Ray,
    Rectangle,
    FibRetracement,""",
"""    HorizontalLine,
    HorizontalRay,
    VerticalLine,
    Ray,
    Rectangle,
    ParallelChannel,
    FibRetracement,""",
1)
text = text.replace(
"""    const ALL: [Self; 10] = [
        Self::Select,
        Self::TrendLine,
        Self::HorizontalLine,
        Self::VerticalLine,
        Self::Ray,
        Self::Rectangle,
        Self::FibRetracement,""",
"""    const ALL: [Self; 12] = [
        Self::Select,
        Self::TrendLine,
        Self::HorizontalLine,
        Self::HorizontalRay,
        Self::VerticalLine,
        Self::Ray,
        Self::Rectangle,
        Self::ParallelChannel,
        Self::FibRetracement,""",
1)
text = text.replace(
"""            Self::HorizontalLine => "H",
            Self::VerticalLine => "V",
            Self::Ray => "RAY",
            Self::Rectangle => "BOX",
            Self::FibRetracement => "FIB",""",
"""            Self::HorizontalLine => "H",
            Self::HorizontalRay => "HRAY",
            Self::VerticalLine => "V",
            Self::Ray => "RAY",
            Self::Rectangle => "BOX",
            Self::ParallelChannel => "CH",
            Self::FibRetracement => "FIB",""",
1)
text = text.replace(
"""            Self::Select | Self::Brush => 0,
            Self::HorizontalLine | Self::VerticalLine | Self::Text => 1,
            Self::TrendLine | Self::Ray | Self::Rectangle | Self::FibRetracement => 2,
            Self::FibExtension => 3,""",
"""            Self::Select | Self::Brush => 0,
            Self::HorizontalLine | Self::HorizontalRay | Self::VerticalLine | Self::Text => 1,
            Self::TrendLine | Self::Ray | Self::Rectangle | Self::FibRetracement => 2,
            Self::ParallelChannel | Self::FibExtension => 3,""",
1)

# 2) Draft text editor and resize preview state.
text = text.replace(
"""    },
    Brush(Vec<DrawingPoint>),
}

#[derive(Debug, Clone)]
struct DragPreview {""",
"""    },
    Brush(Vec<DrawingPoint>),
    Text {
        at: DrawingPoint,
        text: String,
    },
}

#[derive(Debug, Clone)]
struct DragPreview {""",
1)
text = text.replace(
"""struct DragPreview {
    id: DrawingId,
    start: DrawingPoint,
    current: DrawingPoint,
    original: DrawingKind,
}

pub struct DrawingOverlay {""",
"""struct DragPreview {
    id: DrawingId,
    start: DrawingPoint,
    current: DrawingPoint,
    original: DrawingKind,
}

#[derive(Debug, Clone)]
struct ResizePreview {
    id: DrawingId,
    control_index: usize,
    current: DrawingPoint,
    original: DrawingKind,
}

pub struct DrawingOverlay {""",
1)
text = text.replace(
"""    draft: RefCell<Option<Draft>>,
    drag: RefCell<Option<DragPreview>>,
    storage_file: String,""",
"""    draft: RefCell<Option<Draft>>,
    drag: RefCell<Option<DragPreview>>,
    resize: RefCell<Option<ResizePreview>>,
    storage_file: String,""",
1)
text = text.replace(
"""            draft: RefCell::new(None),
            drag: RefCell::new(None),
            storage_file,""",
"""            draft: RefCell::new(None),
            drag: RefCell::new(None),
            resize: RefCell::new(None),
            storage_file,""",
1)
text = text.replace(
"""        self.draft.borrow_mut().take();
        self.drag.borrow_mut().take();
    }""",
"""        self.draft.borrow_mut().take();
        self.drag.borrow_mut().take();
        self.resize.borrow_mut().take();
    }""",
1)

# 3) Selection: prefer selected control point for resize, otherwise normal drag.
old = """                    DrawingTool::Select => {
                        if let Some(id) = self.hit_test(pos, bounds, chart) {
                            self.store.borrow_mut().select(Some(id));
                            if let Some(drawing) = self.store.borrow().get(id).cloned()
                                && !drawing.locked
                            {
                                *self.drag.borrow_mut() = Some(DragPreview {
                                    id,
                                    start: point,
                                    current: point,
                                    original: drawing.kind,
                                });
                            }
                        } else {
                            self.store.borrow_mut().select(None);
                        }
                        Some(canvas::Action::request_redraw().and_capture())
                    }"""
new = """                    DrawingTool::Select => {
                        if let Some((id, control_index)) =
                            self.hit_selected_control(pos, bounds, chart)
                        {
                            if let Some(drawing) = self.store.borrow().get(id).cloned()
                                && !drawing.locked
                            {
                                *self.resize.borrow_mut() = Some(ResizePreview {
                                    id,
                                    control_index,
                                    current: point,
                                    original: drawing.kind,
                                });
                            }
                        } else if let Some(id) = self.hit_test(pos, bounds, chart) {
                            self.store.borrow_mut().select(Some(id));
                            if let Some(drawing) = self.store.borrow().get(id).cloned()
                                && !drawing.locked
                            {
                                *self.drag.borrow_mut() = Some(DragPreview {
                                    id,
                                    start: point,
                                    current: point,
                                    original: drawing.kind,
                                });
                            }
                        } else {
                            self.store.borrow_mut().select(None);
                        }
                        Some(canvas::Action::request_redraw().and_capture())
                    }"""
if old not in text and "hit_selected_control" not in text:
    raise SystemExit("could not locate Select interaction")
text = text.replace(old, new, 1)

# 4) Cursor move / mouse release resize lifecycle.
text = text.replace(
"""                if let Some(drag) = self.drag.borrow_mut().as_mut() {
                    drag.current = point;
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(Draft::Brush(points))""",
"""                if let Some(resize) = self.resize.borrow_mut().as_mut() {
                    resize.current = point;
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(drag) = self.drag.borrow_mut().as_mut() {
                    drag.current = point;
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(Draft::Brush(points))""",
1)
text = text.replace(
"""            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(drag) = self.drag.borrow_mut().take() {""",
"""            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(resize) = self.resize.borrow_mut().take() {
                    let resized = replace_control_point(
                        &resize.original,
                        resize.control_index,
                        resize.current,
                    );
                    self.store.borrow_mut().replace_kind(resize.id, resized);
                    self.persist();
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(drag) = self.drag.borrow_mut().take() {""",
1)

# 5) Keyboard text editing before generic delete/undo handling.
needle = """            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                match key.as_ref() {"""
replacement = """            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if self.handle_text_key(key.as_ref(), *modifiers) {
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                match key.as_ref() {"""
if needle not in text and "handle_text_key" not in text:
    raise SystemExit("could not locate keyboard handler")
text = text.replace(needle, replacement, 1)

# 6) Draw resize preview, control points, and text draft preview.
text = text.replace(
"""        if let Some(drag) = self.drag.borrow().as_ref() {
            let preview = translate_kind(&drag.original, drag.start, drag.current);
            self.draw_kind(frame, chart, &preview, selected_color, region);
        }

        if let Some(draft)""",
"""        if let Some(resize) = self.resize.borrow().as_ref() {
            let preview = replace_control_point(
                &resize.original,
                resize.control_index,
                resize.current,
            );
            self.draw_kind(frame, chart, &preview, selected_color, region);
        }

        if let Some(drag) = self.drag.borrow().as_ref() {
            let preview = translate_kind(&drag.original, drag.start, drag.current);
            self.draw_kind(frame, chart, &preview, selected_color, region);
        }

        if let Some(draft)""",
1)
text = text.replace(
"""                Draft::Brush(_) => {}
            }
        }

        self.draw_toolbar""",
"""                Draft::Brush(_) => {}
                Draft::Text { at, text } => {
                    let preview = if text.is_empty() { "|" } else { text.as_str() };
                    let pos = world_point(*at, chart);
                    self.draw_label(frame, chart, selected_color, pos.x, pos.y, preview);
                }
            }
        }

        if let Some(id) = selected
            && let Some(drawing) = self.store.borrow().get(id)
            && !drawing.locked
        {
            self.draw_control_points(frame, chart, &drawing.kind, selected_color);
        }

        self.draw_toolbar""",
1)

# 7) Text click starts text draft instead of immediate placeholder drawing.
text = text.replace(
"""    fn add_draft_point(&self, tool: DrawingTool, point: DrawingPoint) {
        let required = tool.points_required();
        if required == 1 {""",
"""    fn add_draft_point(&self, tool: DrawingTool, point: DrawingPoint) {
        if tool == DrawingTool::Text {
            *self.draft.borrow_mut() = Some(Draft::Text {
                at: point,
                text: String::new(),
            });
            return;
        }

        let required = tool.points_required();
        if required == 1 {""",
1)

# 8) Add interaction helper methods inside impl before market_point.
marker = """    fn market_point(
        &self,"""
helpers = r'''    fn handle_text_key(
        &self,
        key: keyboard::Key<&str>,
        modifiers: keyboard::Modifiers,
    ) -> bool {
        let mut draft = self.draft.borrow_mut();
        let Some(Draft::Text { at, text }) = draft.as_mut() else {
            return false;
        };

        match key {
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                let value = text.trim().to_owned();
                let at = *at;
                *draft = None;
                drop(draft);
                if !value.is_empty() {
                    self.store
                        .borrow_mut()
                        .add(DrawingKind::Text { at, text: value });
                    self.persist();
                }
                true
            }
            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                *draft = None;
                true
            }
            keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                text.pop();
                true
            }
            keyboard::Key::Character(chars)
                if !modifiers.control() && !modifiers.logo() && !modifiers.alt() =>
            {
                text.push_str(chars);
                true
            }
            _ => false,
        }
    }

    fn hit_selected_control(
        &self,
        pos: Point,
        bounds: Rectangle,
        chart: &ViewState,
    ) -> Option<(DrawingId, usize)> {
        let store = self.store.borrow();
        let id = store.selected()?;
        let drawing = store.get(id)?;
        if drawing.locked || drawing.hidden {
            return None;
        }

        control_points(&drawing.kind)
            .into_iter()
            .enumerate()
            .filter_map(|(index, point)| {
                let screen = screen_point(point, bounds, chart);
                let distance = point_distance(pos, screen);
                (distance <= HIT_RADIUS_PX * 1.35).then_some((index, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| (id, index))
    }

    fn draw_control_points(
        &self,
        frame: &mut canvas::Frame,
        chart: &ViewState,
        kind: &DrawingKind,
        color: iced::Color,
    ) {
        let radius = 3.5 / chart.scaling.max(0.001);
        for point in control_points(kind) {
            frame.fill(&Path::circle(world_point(point, chart), radius), color);
        }
    }

'''
if marker not in text and "fn handle_text_key" not in text:
    raise SystemExit("could not locate market_point insertion point")
if "fn handle_text_key" not in text:
    text = text.replace(marker, helpers + marker, 1)

# 9) kind_from_points supports the new tools.
text = text.replace(
"""        DrawingTool::HorizontalLine => Some(DrawingKind::HorizontalLine {
            price: points.first()?.price,
        }),
        DrawingTool::VerticalLine => Some(DrawingKind::VerticalLine {""",
"""        DrawingTool::HorizontalLine => Some(DrawingKind::HorizontalLine {
            price: points.first()?.price,
        }),
        DrawingTool::HorizontalRay => Some(DrawingKind::HorizontalRay {
            start: points.first()?.x,
            price: points.first()?.price,
        }),
        DrawingTool::VerticalLine => Some(DrawingKind::VerticalLine {""",
1)
text = text.replace(
"""        DrawingTool::Rectangle => Some(DrawingKind::Rectangle {
            start: *points.first()?,
            end: *points.get(1)?,
        }),
        DrawingTool::FibRetracement =>""",
"""        DrawingTool::Rectangle => Some(DrawingKind::Rectangle {
            start: *points.first()?,
            end: *points.get(1)?,
        }),
        DrawingTool::ParallelChannel => Some(DrawingKind::ParallelChannel {
            start: *points.first()?,
            end: *points.get(1)?,
            offset: *points.get(2)?,
        }),
        DrawingTool::FibRetracement =>""",
1)
# Text is no longer created by point finalization.
text = text.replace(
"""        DrawingTool::Text => Some(DrawingKind::Text {
            at: *points.first()?,
            text: "Text".to_owned(),
        }),""",
"""        DrawingTool::Text => None,""",
1)

# 10) Add generic control-point extraction/replacement helpers before translate_kind.
marker = """fn translate_kind(kind: &DrawingKind, start: DrawingPoint, current: DrawingPoint) -> DrawingKind {"""
geometry = r'''fn control_points(kind: &DrawingKind) -> Vec<DrawingPoint> {
    match kind {
        DrawingKind::TrendLine { start, end }
        | DrawingKind::Rectangle { start, end }
        | DrawingKind::FibRetracement { start, end } => vec![*start, *end],
        DrawingKind::Ray { start, through } => vec![*start, *through],
        DrawingKind::HorizontalRay { start, price } => vec![DrawingPoint {
            x: *start,
            price: *price,
        }],
        DrawingKind::ParallelChannel { start, end, offset } => vec![*start, *end, *offset],
        DrawingKind::FibExtension {
            start,
            end,
            projection,
        } => vec![*start, *end, *projection],
        DrawingKind::Text { at, .. } => vec![*at],
        DrawingKind::Brush { points } if points.len() >= 2 => {
            vec![points[0], *points.last().expect("brush has >= 2 points")]
        }
        DrawingKind::HorizontalLine { .. }
        | DrawingKind::VerticalLine { .. }
        | DrawingKind::Brush { .. } => Vec::new(),
    }
}

fn replace_control_point(kind: &DrawingKind, index: usize, point: DrawingPoint) -> DrawingKind {
    match kind {
        DrawingKind::TrendLine { start, end } => DrawingKind::TrendLine {
            start: if index == 0 { point } else { *start },
            end: if index == 1 { point } else { *end },
        },
        DrawingKind::Ray { start, through } => DrawingKind::Ray {
            start: if index == 0 { point } else { *start },
            through: if index == 1 { point } else { *through },
        },
        DrawingKind::HorizontalRay { start, price } => DrawingKind::HorizontalRay {
            start: if index == 0 { point.x } else { *start },
            price: if index == 0 { point.price } else { *price },
        },
        DrawingKind::Rectangle { start, end } => DrawingKind::Rectangle {
            start: if index == 0 { point } else { *start },
            end: if index == 1 { point } else { *end },
        },
        DrawingKind::ParallelChannel { start, end, offset } => DrawingKind::ParallelChannel {
            start: if index == 0 { point } else { *start },
            end: if index == 1 { point } else { *end },
            offset: if index == 2 { point } else { *offset },
        },
        DrawingKind::FibRetracement { start, end } => DrawingKind::FibRetracement {
            start: if index == 0 { point } else { *start },
            end: if index == 1 { point } else { *end },
        },
        DrawingKind::FibExtension {
            start,
            end,
            projection,
        } => DrawingKind::FibExtension {
            start: if index == 0 { point } else { *start },
            end: if index == 1 { point } else { *end },
            projection: if index == 2 { point } else { *projection },
        },
        DrawingKind::Text { at, text } => DrawingKind::Text {
            at: if index == 0 { point } else { *at },
            text: text.clone(),
        },
        DrawingKind::Brush { points } => {
            let mut points = points.clone();
            if index == 0 && !points.is_empty() {
                points[0] = point;
            } else if index == 1 && points.len() >= 2 {
                let last = points.len() - 1;
                points[last] = point;
            }
            DrawingKind::Brush { points }
        }
        DrawingKind::HorizontalLine { price } => DrawingKind::HorizontalLine { price: *price },
        DrawingKind::VerticalLine { x } => DrawingKind::VerticalLine { x: *x },
    }
}

'''
if marker not in text and "fn control_points(" not in text:
    raise SystemExit("could not locate geometry helper insertion")
if "fn control_points(" not in text:
    text = text.replace(marker, geometry + marker, 1)

# 11) Unit tests for added entry points and resizing.
test_marker = """    #[test]
    fn two_points_build_fibonacci_retracement() {"""
tests = r'''    #[test]
    fn one_point_builds_horizontal_ray() {
        let point = pt(1, 100.0);
        let kind = kind_from_points(DrawingTool::HorizontalRay, vec![point]);
        assert!(matches!(kind, Some(DrawingKind::HorizontalRay { .. })));
    }

    #[test]
    fn three_points_build_parallel_channel() {
        let kind = kind_from_points(
            DrawingTool::ParallelChannel,
            vec![pt(1, 100.0), pt(2, 101.0), pt(1, 102.0)],
        );
        assert!(matches!(kind, Some(DrawingKind::ParallelChannel { .. })));
    }

    #[test]
    fn control_point_resize_changes_only_target_endpoint() {
        let kind = DrawingKind::TrendLine {
            start: pt(1, 100.0),
            end: pt(2, 101.0),
        };
        let resized = replace_control_point(&kind, 1, pt(3, 105.0));
        let DrawingKind::TrendLine { start, end } = resized else {
            panic!("trend")
        };
        assert_eq!(start, pt(1, 100.0));
        assert_eq!(end, pt(3, 105.0));
    }

    #[test]
    fn text_tool_waits_for_keyboard_content() {
        assert!(kind_from_points(DrawingTool::Text, vec![pt(1, 100.0)]).is_none());
    }

'''
if test_marker in text and "control_point_resize_changes_only_target_endpoint" not in text:
    text = text.replace(test_marker, tests + test_marker, 1)

required = [
    "HorizontalRay",
    "ParallelChannel",
    "Draft::Text",
    "ResizePreview",
    "handle_text_key",
    "hit_selected_control",
    "draw_control_points",
    "fn control_points(",
    "fn replace_control_point(",
    "control_point_resize_changes_only_target_endpoint",
]
for needle in required:
    if needle not in text:
        raise SystemExit(f"drawing finish patch missing {needle}")

path.write_text(text, encoding="utf-8")
