use exchange::{UnixMs, unit::{Price, PriceStep}};
use serde::{Deserialize, Serialize};

pub type DrawingId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrawingX {
    Time(UnixMs),
    Tick(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingPoint {
    pub x: DrawingX,
    pub price: Price,
}

impl DrawingPoint {
    pub fn time(time: UnixMs, price: Price) -> Self {
        Self {
            x: DrawingX::Time(time),
            price,
        }
    }

    pub fn tick(index: u64, price: Price) -> Self {
        Self {
            x: DrawingX::Tick(index),
            price,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrawingKind {
    TrendLine { start: DrawingPoint, end: DrawingPoint },
    Ray { start: DrawingPoint, through: DrawingPoint },
    HorizontalLine { price: Price },
    HorizontalRay { start: DrawingX, price: Price },
    VerticalLine { x: DrawingX },
    Rectangle { start: DrawingPoint, end: DrawingPoint },
    ParallelChannel {
        start: DrawingPoint,
        end: DrawingPoint,
        offset: DrawingPoint,
    },
    FibRetracement { start: DrawingPoint, end: DrawingPoint },
    FibExtension {
        start: DrawingPoint,
        end: DrawingPoint,
        projection: DrawingPoint,
    },
    Text { at: DrawingPoint, text: String },
    Brush { points: Vec<DrawingPoint> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: DrawingId,
    pub kind: DrawingKind,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct StoreSnapshot {
    drawings: Vec<Drawing>,
    selected: Option<DrawingId>,
    next_id: DrawingId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingStore {
    #[serde(default)]
    drawings: Vec<Drawing>,
    #[serde(default)]
    selected: Option<DrawingId>,
    #[serde(default = "default_next_id")]
    next_id: DrawingId,
    #[serde(skip)]
    undo_stack: Vec<StoreSnapshot>,
    #[serde(skip)]
    redo_stack: Vec<StoreSnapshot>,
}

const fn default_next_id() -> DrawingId {
    1
}

impl Default for DrawingStore {
    fn default() -> Self {
        Self {
            drawings: Vec::new(),
            selected: None,
            next_id: default_next_id(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl DrawingStore {
    pub fn drawings(&self) -> &[Drawing] {
        &self.drawings
    }

    pub fn visible_drawings(&self) -> impl Iterator<Item = &Drawing> {
        self.drawings.iter().filter(|drawing| !drawing.hidden)
    }

    pub fn selected(&self) -> Option<DrawingId> {
        self.selected
    }

    pub fn get(&self, id: DrawingId) -> Option<&Drawing> {
        self.drawings.iter().find(|drawing| drawing.id == id)
    }

    pub fn add(&mut self, kind: DrawingKind) -> DrawingId {
        self.record_history();

        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        self.drawings.push(Drawing {
            id,
            kind,
            locked: false,
            hidden: false,
        });
        self.selected = Some(id);
        id
    }

    pub fn replace_kind(&mut self, id: DrawingId, kind: DrawingKind) -> bool {
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        if self.drawings[index].locked {
            return false;
        }

        self.record_history();
        self.drawings[index].kind = kind;
        true
    }

    pub fn delete(&mut self, id: DrawingId) -> bool {
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };

        self.record_history();
        self.drawings.remove(index);
        if self.selected == Some(id) {
            self.selected = None;
        }
        true
    }

    pub fn clear(&mut self) {
        if self.drawings.is_empty() {
            return;
        }
        self.record_history();
        self.drawings.clear();
        self.selected = None;
    }

    pub fn select(&mut self, id: Option<DrawingId>) -> bool {
        if let Some(id) = id
            && self.get(id).is_none()
        {
            return false;
        }
        self.selected = id;
        true
    }

    pub fn set_locked(&mut self, id: DrawingId, locked: bool) -> bool {
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        if self.drawings[index].locked == locked {
            return true;
        }

        self.record_history();
        self.drawings[index].locked = locked;
        true
    }

    pub fn set_hidden(&mut self, id: DrawingId, hidden: bool) -> bool {
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        if self.drawings[index].hidden == hidden {
            return true;
        }

        self.record_history();
        self.drawings[index].hidden = hidden;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };

        self.redo_stack.push(self.snapshot());
        self.restore(previous);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };

        self.undo_stack.push(self.snapshot());
        self.restore(next);
        true
    }

    /// Call after loading a persisted store. It removes invalid selections and
    /// guarantees future drawing IDs cannot collide with persisted drawings.
    pub fn normalize_after_load(&mut self) {
        let max_id = self.drawings.iter().map(|drawing| drawing.id).max().unwrap_or(0);
        self.next_id = self.next_id.max(max_id.saturating_add(1)).max(1);

        if self.selected.is_some_and(|id| self.get(id).is_none()) {
            self.selected = None;
        }

        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    fn snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            drawings: self.drawings.clone(),
            selected: self.selected,
            next_id: self.next_id,
        }
    }

    fn restore(&mut self, snapshot: StoreSnapshot) {
        self.drawings = snapshot.drawings;
        self.selected = snapshot.selected;
        self.next_id = snapshot.next_id;
    }

    fn record_history(&mut self) {
        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MagnetMode {
    #[default]
    Off,
    Weak,
    Strong,
}

/// Snap a candidate price to the nearest supplied market price.
///
/// Weak mode snaps only within three ticks. Strong mode always snaps to the
/// nearest candidate. Passing an empty candidate list leaves the price intact.
pub fn snap_price(
    target: Price,
    candidates: &[Price],
    step: PriceStep,
    mode: MagnetMode,
) -> Price {
    if matches!(mode, MagnetMode::Off) || candidates.is_empty() {
        return target;
    }

    let Some(nearest) = candidates
        .iter()
        .copied()
        .min_by_key(|candidate| target.units.abs_diff(candidate.units))
    else {
        return target;
    };

    match mode {
        MagnetMode::Off => target,
        MagnetMode::Strong => nearest,
        MagnetMode::Weak => {
            let tick_units = step.units.unsigned_abs().max(1);
            let threshold = tick_units.saturating_mul(3);
            if target.units.abs_diff(nearest.units) <= threshold {
                nearest
            } else {
                target
            }
        }
    }
}

pub fn snap_to_ohlc(
    target: Price,
    open: Price,
    high: Price,
    low: Price,
    close: Price,
    step: PriceStep,
    mode: MagnetMode,
) -> Price {
    snap_price(target, &[open, high, low, close], step, mode)
}

pub const FIB_RETRACEMENT_RATIOS: [f64; 7] = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
pub const FIB_EXTENSION_RATIOS: [f64; 8] = [0.0, 0.618, 1.0, 1.272, 1.618, 2.0, 2.618, 4.236];

pub fn fibonacci_retracement_prices(start: Price, end: Price) -> Vec<(f64, Price)> {
    let start_f = start.to_f64();
    let span = end.to_f64() - start_f;

    FIB_RETRACEMENT_RATIOS
        .into_iter()
        .map(|ratio| (ratio, Price::from_f64(start_f + span * ratio)))
        .collect()
}

pub fn fibonacci_extension_prices(
    start: Price,
    end: Price,
    projection: Price,
) -> Vec<(f64, Price)> {
    let move_size = end.to_f64() - start.to_f64();
    let base = projection.to_f64();

    FIB_EXTENSION_RATIOS
        .into_iter()
        .map(|ratio| (ratio, Price::from_f64(base + move_size * ratio)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(value: f64) -> Price {
        Price::from_f64(value)
    }

    fn point(value: f64) -> DrawingPoint {
        DrawingPoint::time(UnixMs::new(1_000), p(value))
    }

    #[test]
    fn add_delete_undo_redo_round_trip() {
        let mut store = DrawingStore::default();
        let id = store.add(DrawingKind::HorizontalLine { price: p(100.0) });
        assert_eq!(store.drawings().len(), 1);

        assert!(store.delete(id));
        assert!(store.drawings().is_empty());

        assert!(store.undo());
        assert_eq!(store.drawings().len(), 1);
        assert_eq!(store.drawings()[0].id, id);

        assert!(store.redo());
        assert!(store.drawings().is_empty());
    }

    #[test]
    fn locked_drawing_rejects_geometry_changes() {
        let mut store = DrawingStore::default();
        let id = store.add(DrawingKind::TrendLine {
            start: point(100.0),
            end: point(101.0),
        });

        assert!(store.set_locked(id, true));
        assert!(!store.replace_kind(
            id,
            DrawingKind::HorizontalLine { price: p(99.0) }
        ));

        assert!(matches!(store.get(id).unwrap().kind, DrawingKind::TrendLine { .. }));
    }

    #[test]
    fn hidden_drawings_are_filtered_from_visible_iterator() {
        let mut store = DrawingStore::default();
        let a = store.add(DrawingKind::HorizontalLine { price: p(100.0) });
        store.add(DrawingKind::HorizontalLine { price: p(101.0) });
        assert!(store.set_hidden(a, true));

        assert_eq!(store.visible_drawings().count(), 1);
    }

    #[test]
    fn weak_magnet_snaps_only_nearby_prices() {
        let step = PriceStep { units: p(0.5).units };
        let candidates = [p(100.0), p(101.0)];

        assert_eq!(
            snap_price(p(100.4), &candidates, step, MagnetMode::Weak),
            p(100.0)
        );
        assert_eq!(
            snap_price(p(110.0), &candidates, step, MagnetMode::Weak),
            p(110.0)
        );
        assert_eq!(
            snap_price(p(110.0), &candidates, step, MagnetMode::Strong),
            p(101.0)
        );
    }

    #[test]
    fn retracement_contains_halfway_price() {
        let levels = fibonacci_retracement_prices(p(100.0), p(110.0));
        let (_, halfway) = levels
            .iter()
            .find(|(ratio, _)| (*ratio - 0.5).abs() < f64::EPSILON)
            .expect("50% level");
        assert_eq!(*halfway, p(105.0));
    }

    #[test]
    fn extension_projects_original_move_from_third_point() {
        let levels = fibonacci_extension_prices(p(100.0), p(110.0), p(105.0));
        let (_, one_x) = levels
            .iter()
            .find(|(ratio, _)| (*ratio - 1.0).abs() < f64::EPSILON)
            .expect("100% extension");
        assert_eq!(*one_x, p(115.0));
    }

    #[test]
    fn persisted_store_recovers_next_id() {
        let mut store = DrawingStore::default();
        let id = store.add(DrawingKind::HorizontalLine { price: p(100.0) });
        let json = serde_json::to_string(&store).unwrap();
        let mut loaded: DrawingStore = serde_json::from_str(&json).unwrap();
        loaded.normalize_after_load();

        let next = loaded.add(DrawingKind::HorizontalLine { price: p(101.0) });
        assert!(next > id);
    }
}
