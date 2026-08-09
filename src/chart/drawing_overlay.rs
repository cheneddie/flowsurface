use std::{
    cell::{Cell, RefCell},
    fs,
};

use data::chart::kline::KlineDataPoint;
use data::{
    chart::{Basis, PlotData},
    drawing::{
        DrawingId, DrawingKind, DrawingPoint, DrawingStore, DrawingX, MagnetMode,
        fibonacci_extension_prices, fibonacci_retracement_prices, snap_to_ohlc,
    },
};
use exchange::{TickerInfo, UnixMs, unit::Price};
use iced::{
    Alignment, Point, Rectangle, Size, keyboard, mouse,
    theme::palette::Extended,
    widget::canvas::{self, Event, Path, Stroke},
};

use super::{Message, ViewState};

const TOOLBAR_X: f32 = 8.0;
const TOOLBAR_Y: f32 = 8.0;
const TOOLBAR_W: f32 = 46.0;
const TOOLBAR_H: f32 = 24.0;
const TOOLBAR_GAP: f32 = 3.0;
const HIT_RADIUS_PX: f32 = 8.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DrawingTool {
    #[default]
    Select,
    TrendLine,
    HorizontalLine,
    HorizontalRay,
    VerticalLine,
    Ray,
    Rectangle,
    ParallelChannel,
    FibRetracement,
    FibExtension,
    Text,
    Brush,
}

impl DrawingTool {
    const ALL: [Self; 12] = [
        Self::Select,
        Self::TrendLine,
        Self::HorizontalLine,
        Self::HorizontalRay,
        Self::VerticalLine,
        Self::Ray,
        Self::Rectangle,
        Self::ParallelChannel,
        Self::FibRetracement,
        Self::FibExtension,
        Self::Text,
        Self::Brush,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Select => "SEL",
            Self::TrendLine => "TL",
            Self::HorizontalLine => "H",
            Self::HorizontalRay => "HRAY",
            Self::VerticalLine => "V",
            Self::Ray => "RAY",
            Self::Rectangle => "BOX",
            Self::ParallelChannel => "CH",
            Self::FibRetracement => "FIB",
            Self::FibExtension => "EXT",
            Self::Text => "TXT",
            Self::Brush => "BR",
        }
    }

    fn points_required(self) -> usize {
        match self {
            Self::Select | Self::Brush => 0,
            Self::HorizontalLine | Self::HorizontalRay | Self::VerticalLine | Self::Text => 1,
            Self::TrendLine | Self::Ray | Self::Rectangle | Self::FibRetracement => 2,
            Self::ParallelChannel | Self::FibExtension => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolbarAction {
    Tool(DrawingTool),
    Undo,
    Redo,
    Delete,
    Lock,
    Hide,
    Magnet,
}

impl ToolbarAction {
    fn label(self, overlay: &DrawingOverlay) -> String {
        match self {
            Self::Tool(tool) => tool.label().to_owned(),
            Self::Undo => "UNDO".to_owned(),
            Self::Redo => "REDO".to_owned(),
            Self::Delete => "DEL".to_owned(),
            Self::Lock => "LOCK".to_owned(),
            Self::Hide => "HIDE".to_owned(),
            Self::Magnet => match overlay.magnet.get() {
                MagnetMode::Off => "MAG0".to_owned(),
                MagnetMode::Weak => "MAG1".to_owned(),
                MagnetMode::Strong => "MAG2".to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone)]
enum Draft {
    Points {
        tool: DrawingTool,
        points: Vec<DrawingPoint>,
    },
    Brush(Vec<DrawingPoint>),
    Text {
        at: DrawingPoint,
        text: String,
    },
}

#[derive(Debug, Clone)]
struct DragPreview {
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

pub struct DrawingOverlay {
    store: RefCell<DrawingStore>,
    tool: Cell<DrawingTool>,
    magnet: Cell<MagnetMode>,
    draft: RefCell<Option<Draft>>,
    drag: RefCell<Option<DragPreview>>,
    resize: RefCell<Option<ResizePreview>>,
    storage_file: String,
}

impl DrawingOverlay {
    pub fn new(ticker_info: TickerInfo, basis: Basis) -> Self {
        let storage_file = storage_file(ticker_info, basis);
        let mut store = fs::read_to_string(data::data_path(Some(&storage_file)))
            .ok()
            .and_then(|json| serde_json::from_str::<DrawingStore>(&json).ok())
            .unwrap_or_default();
        store.normalize_after_load();

        Self {
            store: RefCell::new(store),
            tool: Cell::new(DrawingTool::Select),
            magnet: Cell::new(MagnetMode::Off),
            draft: RefCell::new(None),
            drag: RefCell::new(None),
            resize: RefCell::new(None),
            storage_file,
        }
    }

    pub fn set_tool(&self, tool: DrawingTool) {
        self.tool.set(tool);
        self.draft.borrow_mut().take();
        self.drag.borrow_mut().take();
        self.resize.borrow_mut().take();
    }

    pub fn update(
        &self,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        chart: &ViewState,
        source: &PlotData<KlineDataPoint>,
    ) -> Option<canvas::Action<Message>> {
        let cursor_pos = cursor.position_in(bounds);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor_pos?;
                if let Some(action) = self.toolbar_action_at(pos) {
                    self.apply_toolbar_action(action);
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                let point = self.market_point(pos, bounds, chart, source);
                match self.tool.get() {
                    DrawingTool::Select => {
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
                    }
                    DrawingTool::Brush => {
                        *self.draft.borrow_mut() = Some(Draft::Brush(vec![point]));
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    tool => {
                        self.add_draft_point(tool, point);
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let pos = cursor_pos?;
                let point = self.market_point(pos, bounds, chart, source);

                if let Some(resize) = self.resize.borrow_mut().as_mut() {
                    resize.current = point;
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(drag) = self.drag.borrow_mut().as_mut() {
                    drag.current = point;
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(Draft::Brush(points)) = self.draft.borrow_mut().as_mut() {
                    points.push(point);
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                None
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
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

                if let Some(drag) = self.drag.borrow_mut().take() {
                    let shifted = translate_kind(&drag.original, drag.start, drag.current);
                    self.store.borrow_mut().replace_kind(drag.id, shifted);
                    self.persist();
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                if let Some(Draft::Brush(points)) = self.draft.borrow_mut().take() {
                    if points.len() >= 2 {
                        self.store.borrow_mut().add(DrawingKind::Brush { points });
                        self.persist();
                    }
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                None
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if self.handle_text_key(key.as_ref(), *modifiers) {
                    return Some(canvas::Action::request_redraw().and_capture());
                }

                match key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::Escape) => {
                        self.set_tool(DrawingTool::Select);
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    keyboard::Key::Named(keyboard::key::Named::Delete)
                    | keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                        self.delete_selected();
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    keyboard::Key::Character("z") if modifiers.control() => {
                        if self.store.borrow_mut().undo() {
                            self.persist();
                        }
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    keyboard::Key::Character("y") if modifiers.control() => {
                        if self.store.borrow_mut().redo() {
                            self.persist();
                        }
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub fn draw(
        &self,
        frame: &mut canvas::Frame,
        chart: &ViewState,
        source: &PlotData<KlineDataPoint>,
        palette: &Extended,
        region: Rectangle,
    ) {
        let base_color = palette.background.base.text.scale_alpha(0.72);
        let selected_color = palette.primary.base.color;
        let store = self.store.borrow();
        let selected = store.selected();

        for drawing in store.visible_drawings() {
            let color = if selected == Some(drawing.id) {
                selected_color
            } else {
                base_color
            };
            self.draw_kind(frame, chart, &drawing.kind, color, region);
        }
        drop(store);

        if let Some(resize) = self.resize.borrow().as_ref() {
            let preview =
                replace_control_point(&resize.original, resize.control_index, resize.current);
            self.draw_kind(frame, chart, &preview, selected_color, region);
        }

        if let Some(drag) = self.drag.borrow().as_ref() {
            let preview = translate_kind(&drag.original, drag.start, drag.current);
            self.draw_kind(frame, chart, &preview, selected_color, region);
        }

        if let Some(draft) = self.draft.borrow().as_ref() {
            match draft {
                Draft::Points { tool, points } => {
                    if let Some(kind) = kind_from_points(*tool, points.clone()) {
                        self.draw_kind(
                            frame,
                            chart,
                            &kind,
                            selected_color.scale_alpha(0.6),
                            region,
                        );
                    }
                }
                Draft::Brush(points) if points.len() >= 2 => {
                    self.draw_kind(
                        frame,
                        chart,
                        &DrawingKind::Brush {
                            points: points.clone(),
                        },
                        selected_color.scale_alpha(0.6),
                        region,
                    );
                }
                Draft::Brush(_) => {}
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

        self.draw_toolbar(frame, chart, palette, region);

        // Make the data dependency explicit: snapping/rendering must always be
        // driven by the same source as the candles/footprint.
        let _ = source;
    }

    fn add_draft_point(&self, tool: DrawingTool, point: DrawingPoint) {
        if tool == DrawingTool::Text {
            *self.draft.borrow_mut() = Some(Draft::Text {
                at: point,
                text: String::new(),
            });
            return;
        }

        let required = tool.points_required();
        if required == 1 {
            if let Some(kind) = kind_from_points(tool, vec![point]) {
                self.store.borrow_mut().add(kind);
                self.persist();
            }
            return;
        }

        let mut draft = self.draft.borrow_mut();
        match draft.as_mut() {
            Some(Draft::Points {
                tool: existing,
                points,
            }) if *existing == tool => {
                points.push(point);
                if points.len() >= required {
                    let points = points.clone();
                    *draft = None;
                    if let Some(kind) = kind_from_points(tool, points) {
                        self.store.borrow_mut().add(kind);
                        self.persist();
                    }
                }
            }
            _ => {
                *draft = Some(Draft::Points {
                    tool,
                    points: vec![point],
                });
            }
        }
    }

    fn handle_text_key(&self, key: keyboard::Key<&str>, modifiers: keyboard::Modifiers) -> bool {
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

    fn market_point(
        &self,
        pos: Point,
        bounds: Rectangle,
        chart: &ViewState,
        source: &PlotData<KlineDataPoint>,
    ) -> DrawingPoint {
        let world_x = (pos.x - bounds.width / 2.0) / chart.scaling - chart.translation.x;
        let world_y = (pos.y - bounds.height / 2.0) / chart.scaling - chart.translation.y;
        let interval = chart.x_to_interval(world_x);
        let mut price = chart.y_to_price(world_y);

        if !matches!(self.magnet.get(), MagnetMode::Off)
            && let Some(kline) = kline_at(source, chart.basis, interval)
        {
            price = snap_to_ohlc(
                price,
                kline.open,
                kline.high,
                kline.low,
                kline.close,
                chart.tick_size,
                self.magnet.get(),
            );
        }

        let x = match chart.basis {
            Basis::Time(_) => DrawingX::Time(UnixMs::new(interval)),
            Basis::Tick(_) => DrawingX::Tick(interval),
        };
        DrawingPoint { x, price }
    }

    fn toolbar_action_at(&self, pos: Point) -> Option<ToolbarAction> {
        self.toolbar_actions()
            .into_iter()
            .enumerate()
            .find_map(|(index, action)| {
                let rect = toolbar_rect(index);
                rect.contains(pos).then_some(action)
            })
    }

    fn toolbar_actions(&self) -> Vec<ToolbarAction> {
        DrawingTool::ALL
            .into_iter()
            .map(ToolbarAction::Tool)
            .chain([
                ToolbarAction::Undo,
                ToolbarAction::Redo,
                ToolbarAction::Delete,
                ToolbarAction::Lock,
                ToolbarAction::Hide,
                ToolbarAction::Magnet,
            ])
            .collect()
    }

    fn apply_toolbar_action(&self, action: ToolbarAction) {
        match action {
            ToolbarAction::Tool(tool) => self.set_tool(tool),
            ToolbarAction::Undo => {
                if self.store.borrow_mut().undo() {
                    self.persist();
                }
            }
            ToolbarAction::Redo => {
                if self.store.borrow_mut().redo() {
                    self.persist();
                }
            }
            ToolbarAction::Delete => self.delete_selected(),
            ToolbarAction::Lock => {
                if let Some(id) = self.store.borrow().selected() {
                    let locked = self.store.borrow().get(id).is_some_and(|d| d.locked);
                    if self.store.borrow_mut().set_locked(id, !locked) {
                        self.persist();
                    }
                }
            }
            ToolbarAction::Hide => {
                if let Some(id) = self.store.borrow().selected() {
                    let hidden = self.store.borrow().get(id).is_some_and(|d| d.hidden);
                    if self.store.borrow_mut().set_hidden(id, !hidden) {
                        self.persist();
                    }
                }
            }
            ToolbarAction::Magnet => {
                self.magnet.set(match self.magnet.get() {
                    MagnetMode::Off => MagnetMode::Weak,
                    MagnetMode::Weak => MagnetMode::Strong,
                    MagnetMode::Strong => MagnetMode::Off,
                });
            }
        }
    }

    fn delete_selected(&self) {
        if let Some(id) = self.store.borrow().selected()
            && self.store.borrow_mut().delete(id)
        {
            self.persist();
        }
    }

    fn persist(&self) {
        if let Ok(json) = serde_json::to_string(&*self.store.borrow()) {
            let _ = data::write_json_to_file(&json, &self.storage_file);
        }
    }

    fn hit_test(&self, pos: Point, bounds: Rectangle, chart: &ViewState) -> Option<DrawingId> {
        self.store
            .borrow()
            .visible_drawings()
            .filter_map(|drawing| {
                let dist = drawing_distance_px(&drawing.kind, pos, bounds, chart);
                (dist <= HIT_RADIUS_PX).then_some((drawing.id, dist))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    fn draw_toolbar(
        &self,
        frame: &mut canvas::Frame,
        chart: &ViewState,
        palette: &Extended,
        region: Rectangle,
    ) {
        let actions = self.toolbar_actions();
        let scale = chart.scaling.max(0.001);
        let x = region.x + TOOLBAR_X / scale;
        let y = region.y + TOOLBAR_Y / scale;
        let width = TOOLBAR_W / scale;
        let height = TOOLBAR_H / scale;
        let gap = TOOLBAR_GAP / scale;
        let text_size = crate::style::text_size::TINY / scale;

        for (index, action) in actions.into_iter().enumerate() {
            let top = y + index as f32 * (height + gap);
            let active = matches!(action, ToolbarAction::Tool(tool) if tool == self.tool.get());
            let bg = if active {
                palette.primary.weak.color.scale_alpha(0.82)
            } else {
                palette.background.strong.color.scale_alpha(0.86)
            };
            frame.fill_rectangle(Point::new(x, top), Size::new(width, height), bg);
            frame.fill_text(canvas::Text {
                content: action.label(self),
                position: Point::new(x + width / 2.0, top + height / 2.0),
                size: iced::Pixels(text_size),
                color: palette.background.base.text,
                font: crate::style::AZERET_MONO,
                align_x: Alignment::Center.into(),
                align_y: Alignment::Center.into(),
                ..canvas::Text::default()
            });
        }
    }

    fn draw_kind(
        &self,
        frame: &mut canvas::Frame,
        chart: &ViewState,
        kind: &DrawingKind,
        color: iced::Color,
        region: Rectangle,
    ) {
        let stroke = Stroke::with_color(
            Stroke {
                width: 1.4 / chart.scaling.max(0.001),
                ..Stroke::default()
            },
            color,
        );
        let p = |point: DrawingPoint| world_point(point, chart);
        let x = |value: DrawingX| world_x(value, chart);
        let y = |price: Price| chart.price_to_y(price);

        match kind {
            DrawingKind::TrendLine { start, end } => {
                frame.stroke(&Path::line(p(*start), p(*end)), stroke);
            }
            DrawingKind::Ray { start, through } => {
                let a = p(*start);
                let b = p(*through);
                let dx = b.x - a.x;
                let target_x = if dx >= 0.0 {
                    region.x + region.width
                } else {
                    region.x
                };
                let target_y = if dx.abs() <= f32::EPSILON {
                    b.y
                } else {
                    a.y + (b.y - a.y) * ((target_x - a.x) / dx)
                };
                frame.stroke(&Path::line(a, Point::new(target_x, target_y)), stroke);
            }
            DrawingKind::HorizontalLine { price } => {
                frame.stroke(
                    &Path::line(
                        Point::new(region.x, y(*price)),
                        Point::new(region.x + region.width, y(*price)),
                    ),
                    stroke,
                );
            }
            DrawingKind::HorizontalRay { start, price } => {
                frame.stroke(
                    &Path::line(
                        Point::new(x(*start), y(*price)),
                        Point::new(region.x + region.width, y(*price)),
                    ),
                    stroke,
                );
            }
            DrawingKind::VerticalLine { x: at } => {
                let px = x(*at);
                frame.stroke(
                    &Path::line(
                        Point::new(px, region.y),
                        Point::new(px, region.y + region.height),
                    ),
                    stroke,
                );
            }
            DrawingKind::Rectangle { start, end } => {
                let a = p(*start);
                let b = p(*end);
                frame.stroke(
                    &Path::rectangle(
                        Point::new(a.x.min(b.x), a.y.min(b.y)),
                        Size::new((a.x - b.x).abs(), (a.y - b.y).abs()),
                    ),
                    stroke,
                );
            }
            DrawingKind::ParallelChannel { start, end, offset } => {
                let a = p(*start);
                let b = p(*end);
                let c = p(*offset);
                let d = Point::new(c.x + (b.x - a.x), c.y + (b.y - a.y));
                frame.stroke(&Path::line(a, b), stroke);
                frame.stroke(&Path::line(c, d), stroke);
            }
            DrawingKind::FibRetracement { start, end } => {
                let x1 = x(start.x).min(x(end.x));
                let x2 = x(start.x).max(x(end.x));
                for (ratio, price) in fibonacci_retracement_prices(start.price, end.price) {
                    let py = y(price);
                    frame.stroke(&Path::line(Point::new(x1, py), Point::new(x2, py)), stroke);
                    self.draw_label(frame, chart, color, x2, py, &format!("{ratio:.3}"));
                }
            }
            DrawingKind::FibExtension {
                start,
                end,
                projection,
            } => {
                let x1 = x(projection.x);
                let x2 = region.x + region.width;
                for (ratio, price) in
                    fibonacci_extension_prices(start.price, end.price, projection.price)
                {
                    let py = y(price);
                    frame.stroke(&Path::line(Point::new(x1, py), Point::new(x2, py)), stroke);
                    self.draw_label(frame, chart, color, x1, py, &format!("{ratio:.3}"));
                }
            }
            DrawingKind::Text { at, text } => {
                let pos = p(*at);
                self.draw_label(frame, chart, color, pos.x, pos.y, text);
            }
            DrawingKind::Brush { points } => {
                for pair in points.windows(2) {
                    frame.stroke(&Path::line(p(pair[0]), p(pair[1])), stroke);
                }
            }
        }
    }

    fn draw_label(
        &self,
        frame: &mut canvas::Frame,
        chart: &ViewState,
        color: iced::Color,
        x: f32,
        y: f32,
        text: &str,
    ) {
        frame.fill_text(canvas::Text {
            content: text.to_owned(),
            position: Point::new(x + 3.0 / chart.scaling, y - 2.0 / chart.scaling),
            size: iced::Pixels(crate::style::text_size::TINY / chart.scaling),
            color,
            font: crate::style::AZERET_MONO,
            align_x: Alignment::Start.into(),
            align_y: Alignment::End.into(),
            ..canvas::Text::default()
        });
    }
}

fn toolbar_rect(index: usize) -> Rectangle {
    Rectangle {
        x: TOOLBAR_X,
        y: TOOLBAR_Y + index as f32 * (TOOLBAR_H + TOOLBAR_GAP),
        width: TOOLBAR_W,
        height: TOOLBAR_H,
    }
}

fn storage_file(ticker_info: TickerInfo, basis: Basis) -> String {
    let raw = format!(
        "{}-{}",
        ticker_info.ticker.symbol_and_exchange_string(),
        basis
    );
    let safe: String = raw
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    format!("drawings/{safe}.json")
}

fn kline_at(
    source: &PlotData<KlineDataPoint>,
    basis: Basis,
    interval: u64,
) -> Option<exchange::Kline> {
    match (source, basis) {
        (PlotData::TimeBased(series), Basis::Time(_)) => series
            .datapoints
            .get(&UnixMs::new(interval))
            .map(|dp| dp.kline),
        (PlotData::TickBased(series), Basis::Tick(_)) => {
            series.datapoints.get(interval as usize).map(|dp| dp.kline)
        }
        _ => None,
    }
}

fn world_x(x: DrawingX, chart: &ViewState) -> f32 {
    match x {
        DrawingX::Time(time) => chart.interval_to_x(time.as_u64()),
        DrawingX::Tick(index) => chart.interval_to_x(index),
    }
}

fn world_point(point: DrawingPoint, chart: &ViewState) -> Point {
    Point::new(world_x(point.x, chart), chart.price_to_y(point.price))
}

fn screen_point(point: DrawingPoint, bounds: Rectangle, chart: &ViewState) -> Point {
    let world = world_point(point, chart);
    Point::new(
        (world.x + chart.translation.x) * chart.scaling + bounds.width / 2.0,
        (world.y + chart.translation.y) * chart.scaling + bounds.height / 2.0,
    )
}

fn drawing_distance_px(
    kind: &DrawingKind,
    pos: Point,
    bounds: Rectangle,
    chart: &ViewState,
) -> f32 {
    let sp = |point| screen_point(point, bounds, chart);
    match kind {
        DrawingKind::TrendLine { start, end }
        | DrawingKind::Ray {
            start,
            through: end,
        } => segment_distance(pos, sp(*start), sp(*end)),
        DrawingKind::HorizontalLine { price } => {
            let y = screen_point(
                DrawingPoint {
                    x: match chart.basis {
                        Basis::Time(_) => DrawingX::Time(UnixMs::new(chart.latest_x)),
                        Basis::Tick(_) => DrawingX::Tick(chart.latest_x),
                    },
                    price: *price,
                },
                bounds,
                chart,
            )
            .y;
            (pos.y - y).abs()
        }
        DrawingKind::HorizontalRay { start, price } => {
            let a = screen_point(
                DrawingPoint {
                    x: *start,
                    price: *price,
                },
                bounds,
                chart,
            );
            if pos.x + HIT_RADIUS_PX < a.x {
                f32::INFINITY
            } else {
                (pos.y - a.y).abs()
            }
        }
        DrawingKind::VerticalLine { x } => {
            let px = screen_x(*x, bounds, chart);
            (pos.x - px).abs()
        }
        DrawingKind::Rectangle { start, end } => {
            let a = sp(*start);
            let b = sp(*end);
            let left = a.x.min(b.x);
            let right = a.x.max(b.x);
            let top = a.y.min(b.y);
            let bottom = a.y.max(b.y);
            [
                segment_distance(pos, Point::new(left, top), Point::new(right, top)),
                segment_distance(pos, Point::new(right, top), Point::new(right, bottom)),
                segment_distance(pos, Point::new(right, bottom), Point::new(left, bottom)),
                segment_distance(pos, Point::new(left, bottom), Point::new(left, top)),
            ]
            .into_iter()
            .fold(f32::INFINITY, f32::min)
        }
        DrawingKind::ParallelChannel { start, end, offset } => {
            let a = sp(*start);
            let b = sp(*end);
            let c = sp(*offset);
            let d = Point::new(c.x + (b.x - a.x), c.y + (b.y - a.y));
            segment_distance(pos, a, b).min(segment_distance(pos, c, d))
        }
        DrawingKind::FibRetracement { start, end } => segment_distance(pos, sp(*start), sp(*end)),
        DrawingKind::FibExtension {
            start,
            end,
            projection,
        } => segment_distance(pos, sp(*start), sp(*end)).min(segment_distance(
            pos,
            sp(*end),
            sp(*projection),
        )),
        DrawingKind::Text { at, .. } => point_distance(pos, sp(*at)),
        DrawingKind::Brush { points } => points
            .windows(2)
            .map(|pair| segment_distance(pos, sp(pair[0]), sp(pair[1])))
            .fold(f32::INFINITY, f32::min),
    }
}

fn screen_x(x: DrawingX, bounds: Rectangle, chart: &ViewState) -> f32 {
    (world_x(x, chart) + chart.translation.x) * chart.scaling + bounds.width / 2.0
}

fn point_distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 <= f32::EPSILON {
        return point_distance(p, a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    point_distance(p, Point::new(a.x + t * dx, a.y + t * dy))
}

fn kind_from_points(tool: DrawingTool, points: Vec<DrawingPoint>) -> Option<DrawingKind> {
    match tool {
        DrawingTool::Select | DrawingTool::Brush => None,
        DrawingTool::HorizontalLine => Some(DrawingKind::HorizontalLine {
            price: points.first()?.price,
        }),
        DrawingTool::HorizontalRay => Some(DrawingKind::HorizontalRay {
            start: points.first()?.x,
            price: points.first()?.price,
        }),
        DrawingTool::VerticalLine => Some(DrawingKind::VerticalLine {
            x: points.first()?.x,
        }),
        DrawingTool::Text => None,
        DrawingTool::TrendLine => Some(DrawingKind::TrendLine {
            start: *points.first()?,
            end: *points.get(1)?,
        }),
        DrawingTool::Ray => Some(DrawingKind::Ray {
            start: *points.first()?,
            through: *points.get(1)?,
        }),
        DrawingTool::Rectangle => Some(DrawingKind::Rectangle {
            start: *points.first()?,
            end: *points.get(1)?,
        }),
        DrawingTool::ParallelChannel => Some(DrawingKind::ParallelChannel {
            start: *points.first()?,
            end: *points.get(1)?,
            offset: *points.get(2)?,
        }),
        DrawingTool::FibRetracement => Some(DrawingKind::FibRetracement {
            start: *points.first()?,
            end: *points.get(1)?,
        }),
        DrawingTool::FibExtension => Some(DrawingKind::FibExtension {
            start: *points.first()?,
            end: *points.get(1)?,
            projection: *points.get(2)?,
        }),
    }
}

fn control_points(kind: &DrawingKind) -> Vec<DrawingPoint> {
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

fn translate_kind(kind: &DrawingKind, start: DrawingPoint, current: DrawingPoint) -> DrawingKind {
    let price_delta = current.price.units.saturating_sub(start.price.units);
    let x_delta = drawing_x_delta(start.x, current.x);
    let shift_point = |point: DrawingPoint| DrawingPoint {
        x: shift_x(point.x, x_delta),
        price: Price {
            units: point.price.units.saturating_add(price_delta),
        },
    };

    match kind {
        DrawingKind::TrendLine { start, end } => DrawingKind::TrendLine {
            start: shift_point(*start),
            end: shift_point(*end),
        },
        DrawingKind::Ray { start, through } => DrawingKind::Ray {
            start: shift_point(*start),
            through: shift_point(*through),
        },
        DrawingKind::HorizontalLine { price } => DrawingKind::HorizontalLine {
            price: Price {
                units: price.units.saturating_add(price_delta),
            },
        },
        DrawingKind::HorizontalRay { start, price } => DrawingKind::HorizontalRay {
            start: shift_x(*start, x_delta),
            price: Price {
                units: price.units.saturating_add(price_delta),
            },
        },
        DrawingKind::VerticalLine { x } => DrawingKind::VerticalLine {
            x: shift_x(*x, x_delta),
        },
        DrawingKind::Rectangle { start, end } => DrawingKind::Rectangle {
            start: shift_point(*start),
            end: shift_point(*end),
        },
        DrawingKind::ParallelChannel { start, end, offset } => DrawingKind::ParallelChannel {
            start: shift_point(*start),
            end: shift_point(*end),
            offset: shift_point(*offset),
        },
        DrawingKind::FibRetracement { start, end } => DrawingKind::FibRetracement {
            start: shift_point(*start),
            end: shift_point(*end),
        },
        DrawingKind::FibExtension {
            start,
            end,
            projection,
        } => DrawingKind::FibExtension {
            start: shift_point(*start),
            end: shift_point(*end),
            projection: shift_point(*projection),
        },
        DrawingKind::Text { at, text } => DrawingKind::Text {
            at: shift_point(*at),
            text: text.clone(),
        },
        DrawingKind::Brush { points } => DrawingKind::Brush {
            points: points.iter().copied().map(shift_point).collect(),
        },
    }
}

fn drawing_x_delta(start: DrawingX, current: DrawingX) -> i64 {
    let (a, b) = match (start, current) {
        (DrawingX::Time(a), DrawingX::Time(b)) => (a.as_u64(), b.as_u64()),
        (DrawingX::Tick(a), DrawingX::Tick(b)) => (a, b),
        _ => return 0,
    };
    let delta = i128::from(b) - i128::from(a);
    delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn shift_x(x: DrawingX, delta: i64) -> DrawingX {
    match x {
        DrawingX::Time(time) => DrawingX::Time(time.saturating_add_signed(delta)),
        DrawingX::Tick(index) => DrawingX::Tick(if delta >= 0 {
            index.saturating_add(delta as u64)
        } else {
            index.saturating_sub(delta.unsigned_abs())
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: f64) -> Price {
        Price::from_f64(v)
    }
    fn pt(t: u64, v: f64) -> DrawingPoint {
        DrawingPoint::time(UnixMs::new(t), p(v))
    }

    #[test]
    fn translate_moves_both_time_and_price() {
        let kind = DrawingKind::TrendLine {
            start: pt(1000, 100.0),
            end: pt(2000, 101.0),
        };
        let moved = translate_kind(&kind, pt(5000, 100.0), pt(5500, 102.0));
        let DrawingKind::TrendLine { start, end } = moved else {
            panic!("trend")
        };
        assert_eq!(start.x, DrawingX::Time(UnixMs::new(1500)));
        assert_eq!(end.x, DrawingX::Time(UnixMs::new(2500)));
        assert_eq!(start.price, p(102.0));
        assert_eq!(end.price, p(103.0));
    }

    #[test]
    fn segment_distance_hits_middle_of_line() {
        assert!(
            segment_distance(
                Point::new(5.0, 1.0),
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0)
            ) < 1.1
        );
    }

    #[test]
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

    #[test]
    fn two_points_build_fibonacci_retracement() {
        let kind = kind_from_points(
            DrawingTool::FibRetracement,
            vec![pt(1, 100.0), pt(2, 110.0)],
        );
        assert!(matches!(kind, Some(DrawingKind::FibRetracement { .. })));
    }
}
