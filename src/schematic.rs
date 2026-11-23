// Schematic Capture editor: pan/zoom canvas, tools, symbol drawing, net hints.

use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use eframe::egui::{
    self, Color32, FontFamily, FontId, MouseWheelUnit, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Vec2,
};

use crate::model::{Component, Label, LabelKind, Library, Netlist, Project, Symbol, Wire, P};
use crate::sim::Simulator;
use crate::theme::*;
use crate::util::line_distance;

pub const SHEET_W: f32 = 1600.0;
pub const SHEET_H: f32 = 1200.0;

/// Zoom limits. The old 0.04 floor made the grid draw tens of thousands of
/// dots per frame at full zoom-out; 0.08 keeps the whole sheet usable.
pub const ZOOM_MIN: f32 = 0.08;
pub const ZOOM_MAX: f32 = 6.0;

/// One wheel notch is treated as `exp(0.18)`, roughly 20% per click.
/// egui 0.37 reports wheel notches as line deltas and trackpads as points.
const WHEEL_LINE_FACTOR: f32 = 0.18;
const WHEEL_POINTS_PER_LINE: f32 = 40.0;

/// Clearance the wire router keeps around component bodies. The router
/// refuses a diagonal when no rectilinear route is clear.
const WIRE_CLEARANCE: f32 = 12.0;
const WIRE_BEND_COST: f32 = 8.0;
const WIRE_STUB: f32 = 16.0;
const WIRE_ROUTE_MARGIN: f32 = 20.0;
const WIRE_AVOID: f32 = 8.0;

/// Pointer targets stay about this large on screen at every zoom level.  Grid
/// snapping is applied to the result of a move, never to the point being hit.
const COMPONENT_HIT_PAD_PX: f32 = 10.0;

type PinKey = (u64, usize);

/// Commands the canvas asks the application to perform (dialogs it cannot open).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    PickParts,
    EditComponent(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Wire,
    Part,
    Label,
    Text,
    Probe,
    Delete,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Wire => "Wire",
            Tool::Part => "Place Part",
            Tool::Label => "Net Label",
            Tool::Text => "Text",
            Tool::Probe => "Voltage Probe",
            Tool::Delete => "Delete",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub zoom: f32,
    pub off: Vec2,
}

impl Default for View {
    fn default() -> Self {
        View {
            zoom: 0.42,
            off: Vec2::new(70.0, 50.0),
        }
    }
}

impl View {
    pub fn to_screen(&self, p: P) -> Pos2 {
        Pos2::new(self.off.x + p.x * self.zoom, self.off.y + p.y * self.zoom)
    }
    pub fn to_world(&self, s: Pos2) -> P {
        P::new(
            (s.x - self.off.x) / self.zoom,
            (s.y - self.off.y) / self.zoom,
        )
    }
    pub fn len_to_screen(&self, v: f32) -> f32 {
        v * self.zoom
    }
}

#[derive(Clone, Debug)]
enum Drag {
    Move {
        start: P,
        comps: Vec<(u64, P)>,
        labels: Vec<(u64, P)>,
    },
    Box {
        from: P,
    },
    ZoomBox {
        from: P,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PinRef {
    comp: u64,
    index: usize,
    pos: P,
    horizontal: bool,
}

impl PinRef {
    fn key(self) -> PinKey {
        (self.comp, self.index)
    }
}

#[derive(Clone, Copy, Debug)]
struct RouteNode {
    cost: f32,
    xi: usize,
    yi: usize,
    dir: u8,
}

impl PartialEq for RouteNode {
    fn eq(&self, other: &Self) -> bool {
        self.cost.total_cmp(&other.cost) == Ordering::Equal
            && self.xi == other.xi
            && self.yi == other.yi
            && self.dir == other.dir
    }
}

impl Eq for RouteNode {}

impl PartialOrd for RouteNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RouteNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cost
            .total_cmp(&other.cost)
            .then(self.xi.cmp(&other.xi))
            .then(self.yi.cmp(&other.yi))
            .then(self.dir.cmp(&other.dir))
            .reverse()
    }
}

pub struct Schematic {
    pub view: View,
    pub tool: Tool,
    pub pending: Option<String>,
    /// Live rotation of the part preview, changed with `+`/`-` while placing.
    pending_rot: i32,
    pub sel: HashSet<u64>,
    pub sel_wire: HashSet<u64>,
    pub sel_label: HashSet<u64>,
    /// Anchors/vertices of the wire currently being placed.
    pub draft: Vec<P>,
    /// The pin the wire started on, if any.
    pub draft_pin: Option<PinRef>,
    /// True when Ctrl was held for this wire, bypassing follow-me routing.
    wire_free: bool,
    /// Pin under the cursor, drawn as the green "hot" cursor.
    pub hot_pin: Option<PinRef>,
    /// Component under the cursor. Hover never changes selection or state.
    pub hot_comp: Option<u64>,
    pub mouse: P,
    pub hint: String,
    /// 0 = dots, 1 = lines, 2 = off (cycled with G).
    pub grid_mode: u8,
    /// Manual origin placed with O.
    false_origin: Option<P>,
    /// X cursor lines through the pointer, toggled with X.
    x_cursor: bool,
    /// Middle-button track pan mode.
    pub pan_mode: bool,
    /// Last hovered screen position, used to move the sheet with the mouse.
    pan_last: Option<egui::Pos2>,
    /// Last placed wire, for double-click auto-repeat.
    last_wire: Option<Wire>,
    /// Manual switch state used when the simulator cannot be rebuilt.
    switch_overrides: HashMap<String, bool>,
    pub action: Option<EditorAction>,
    drag: Option<Drag>,
    /// Temporary wire shapes shown while components move. Keeping these out
    /// of the project prevents junction and routing state changing every frame.
    drag_wire_paths: HashMap<u64, Vec<P>>,
    /// Junction topology changes only when wire geometry changes. Live
    /// simulation repaints many times per second, so recomputing the quadratic
    /// segment/terminal scan on every animation frame wastes most of the UI
    /// budget on larger drawings.
    junction_cache_key: u64,
    junction_cache: Vec<P>,
    /// Electrical connectivity and the set of connected pins are stable while
    /// the drawing geometry is unchanged. Cache both instead of rebuilding
    /// them on every animated simulation frame.
    electrical_cache_key: u64,
    netlist_cache: Netlist,
    connected_pin_cache: HashSet<PinKey>,
    follow: bool,
    fit_on_next_frame: bool,
}

impl Default for Schematic {
    fn default() -> Self {
        Schematic {
            view: View::default(),
            tool: Tool::Select,
            pending: None,
            pending_rot: 0,
            sel: HashSet::new(),
            sel_wire: HashSet::new(),
            sel_label: HashSet::new(),
            draft: Vec::new(),
            draft_pin: None,
            wire_free: false,
            hot_pin: None,
            hot_comp: None,
            mouse: P::ZERO,
            hint: "Ready".into(),
            grid_mode: 0,
            false_origin: None,
            x_cursor: false,
            pan_mode: false,
            pan_last: None,
            last_wire: None,
            switch_overrides: HashMap::new(),
            action: None,
            drag: None,
            drag_wire_paths: HashMap::new(),
            junction_cache_key: u64::MAX,
            junction_cache: Vec::new(),
            electrical_cache_key: u64::MAX,
            netlist_cache: Netlist::default(),
            connected_pin_cache: HashSet::new(),
            follow: true,
            fit_on_next_frame: true,
        }
    }
}

impl Schematic {
    pub fn clear_selection(&mut self) {
        self.sel.clear();
        self.sel_wire.clear();
        self.sel_label.clear();
    }

    pub fn request_fit(&mut self) {
        self.fit_on_next_frame = true;
    }

    /// Highlight a set of components as the visible result of Search and Tag.
    /// There is no separate tag flag, so the existing selection colour
    /// is reused for tagged parts; the dialog keeps its own id list for AND/OR
    /// refinement between searches.
    pub fn set_selection(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.clear_selection();
        self.sel.extend(ids);
    }

    pub fn selected_count(&self) -> usize {
        self.sel.len() + self.sel_wire.len() + self.sel_label.len()
    }

    fn hit_component(&self, proj: &Project, lib: &Library, w: P) -> Option<u64> {
        let pad = (COMPONENT_HIT_PAD_PX / self.view.zoom).clamp(3.0, 80.0);
        hit_component_with_pad(proj, lib, w, pad)
    }

    /// Snapshot movable object anchors. Wires are intentionally excluded: they
    /// may be selected or deleted, but users cannot reposition them directly.
    fn move_drag(&self, proj: &Project, start: P) -> Drag {
        let comps = self
            .sel
            .iter()
            .filter_map(|id| proj.comp(*id).map(|c| (*id, c.pos())))
            .collect::<Vec<_>>();
        let labels = if comps.is_empty() {
            self.sel_label
                .iter()
                .filter_map(|id| {
                    proj.labels
                        .iter()
                        .find(|l| l.id == *id)
                        .map(|l| (*id, P::new(l.x, l.y)))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        Drag::Move {
            start,
            comps,
            labels,
        }
    }

    /// Pin under a world point. Pins are the only valid wire endpoints.
    fn hit_pin(proj: &Project, lib: &Library, w: P) -> Option<PinRef> {
        let mut best: Option<(f32, PinRef)> = None;
        for c in &proj.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            for (i, pd) in def.pins.iter().enumerate() {
                let local = pd.pos.rot(c.rot);
                let pos = c.pos().add(local);
                let d = pos.dist(w);
                if d < 18.0 && best.as_ref().map(|(bd, _)| d < *bd).unwrap_or(true) {
                    best = Some((
                        d,
                        PinRef {
                            comp: c.id,
                            index: i,
                            pos,
                            horizontal: local.x.abs() >= local.y.abs(),
                        },
                    ));
                }
            }
        }
        best.map(|(_, pin)| pin)
    }

    /// Closest wire segment to a world point: (wire id, segment index, point).
    fn hit_wire(proj: &Project, w: P) -> Option<(u64, usize, P)> {
        let mut best: Option<(f32, u64, usize, P)> = None;
        for wire in &proj.wires {
            for (i, seg) in wire.pts.windows(2).enumerate() {
                let a = Pos2::new(seg[0].x, seg[0].y);
                let b = Pos2::new(seg[1].x, seg[1].y);
                let d = line_distance(Pos2::new(w.x, w.y), a, b);
                if d < 12.0 && best.as_ref().map(|(bd, _, _, _)| d < *bd).unwrap_or(true) {
                    best = Some((d, wire.id, i, Self::closest_on_seg(w, seg[0], seg[1])));
                }
            }
        }
        best.map(|(_, id, seg, p)| (id, seg, p))
    }

    fn closest_on_seg(p: P, a: P, b: P) -> P {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len2 = dx * dx + dy * dy;
        if len2 <= 0.0 {
            return a;
        }
        let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
        P::new(a.x + dx * t.clamp(0.0, 1.0), a.y + dy * t.clamp(0.0, 1.0))
    }

    /// Insert a vertex into a wire so a junction dot is drawn where a new wire lands.
    fn insert_wire_point(proj: &mut Project, id: u64, at: P) {
        if let Some(wire) = proj.wires.iter_mut().find(|w| w.id == id) {
            let key = |p: P| ((p.x * 4.0).round() as i64, (p.y * 4.0).round() as i64);
            if !wire.pts.iter().any(|p| key(*p) == key(at)) {
                let mut best = (usize::MAX, f32::INFINITY);
                for i in 0..wire.pts.len().saturating_sub(1) {
                    let d = line_distance(
                        Pos2::new(at.x, at.y),
                        Pos2::new(wire.pts[i].x, wire.pts[i].y),
                        Pos2::new(wire.pts[i + 1].x, wire.pts[i + 1].y),
                    );
                    if d < best.1 {
                        best = (i, d);
                    }
                }
                if best.0 != usize::MAX {
                    wire.pts.insert(best.0 + 1, at);
                    proj.dirty = true;
                }
            }
        }
    }

    /// Bodies are the pin bounding box with the pin stubs trimmed off. Pins
    /// sit outside this rectangle, so a wire may leave a pin freely but may
    /// not cut through the symbol body.
    fn component_obstacles(proj: &Project, lib: &Library) -> Vec<Rect> {
        let mut out = Vec::new();
        for c in &proj.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            let pins: Vec<P> = def
                .pins
                .iter()
                .map(|pd| c.pos().add(pd.pos.rot(c.rot)))
                .collect();
            let Some(base) = bbox_of(&pins, 0.0) else {
                continue;
            };
            // A thin pin layout does not imply a thin symbol. Capacitors,
            // lamps and rotated two-pin parts extend beyond the pin centreline.
            let w = (base.width() - 36.0).max(72.0);
            let h = (base.height() - 36.0).max(72.0);
            out.push(Rect::from_center_size(base.center(), Vec2::new(w, h)));
        }
        out
    }

    fn segment_crosses_rect(a: P, b: P, r: Rect) -> bool {
        const EPS: f32 = 0.5;
        if (a.y - b.y).abs() < EPS {
            if a.y <= r.min.y + EPS || a.y >= r.max.y - EPS {
                return false;
            }
            let lo = a.x.min(b.x);
            let hi = a.x.max(b.x);
            return hi > r.min.x + EPS && lo < r.max.x - EPS;
        }
        if (a.x - b.x).abs() < EPS {
            if a.x <= r.min.x + EPS || a.x >= r.max.x - EPS {
                return false;
            }
            let lo = a.y.min(b.y);
            let hi = a.y.max(b.y);
            return hi > r.min.y + EPS && lo < r.max.y - EPS;
        }
        false
    }

    fn segments_clear(a: P, b: P, obstacles: &[Rect]) -> bool {
        !obstacles
            .iter()
            .any(|r| Self::segment_crosses_rect(a, b, *r))
    }

    fn simplify_route(points: Vec<P>) -> Vec<P> {
        let mut out: Vec<P> = Vec::new();
        for p in points {
            if out.last().map(|q| q.dist(p) < 0.5).unwrap_or(false) {
                continue;
            }
            out.push(p);
        }
        loop {
            let mut remove = None;
            for i in 1..out.len().saturating_sub(1) {
                let ab = out[i].sub(out[i - 1]);
                let bc = out[i + 1].sub(out[i]);
                if (ab.x * bc.y - ab.y * bc.x).abs() < 0.5 && ab.x * bc.x + ab.y * bc.y > 0.0 {
                    remove = Some(i);
                    break;
                }
            }
            match remove {
                Some(i) => {
                    out.remove(i);
                }
                None => break,
            }
        }
        out
    }

    fn nearest_index(values: &[f32], value: f32) -> usize {
        values
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (*a - value).abs().total_cmp(&(*b - value).abs()))
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// A short straight stub out of a pin keeps the first wire segment aligned
    /// with the pin even when the next bend is far away.
    fn pin_stub(proj: &Project, lib: &Library, pin: PinKey) -> Option<P> {
        let p = Self::pin_position(proj, lib, pin)?;
        let c = proj.comp(pin.0)?;
        let pd = lib.get(&c.part)?.pins.get(pin.1)?;
        let step = P::new(WIRE_STUB, 0.0).rot(pd.dir + c.rot);
        Some(p.add(step))
    }

    fn wire_segments(proj: &Project, skip: Option<u64>) -> Vec<(P, P)> {
        proj.wires
            .iter()
            .filter(|w| Some(w.id) != skip)
            .flat_map(|w| w.pts.windows(2).map(|s| (s[0], s[1])).collect::<Vec<_>>())
            .collect()
    }

    fn overlap_length(a: P, b: P, segments: &[(P, P)]) -> f32 {
        let mut total = 0.0;
        for &(c, d) in segments {
            let horizontal = (a.y - b.y).abs() < 0.5 && (c.y - d.y).abs() < 0.5;
            let vertical = (a.x - b.x).abs() < 0.5 && (c.x - d.x).abs() < 0.5;
            if horizontal && (a.y - c.y).abs() < 2.5 {
                let lo = a.x.min(b.x).max(c.x.min(d.x));
                let hi = a.x.max(b.x).min(c.x.max(d.x));
                if hi > lo {
                    total += hi - lo;
                }
            } else if vertical && (a.x - c.x).abs() < 2.5 {
                let lo = a.y.min(b.y).max(c.y.min(d.y));
                let hi = a.y.max(b.y).min(c.y.max(d.y));
                if hi > lo {
                    total += hi - lo;
                }
            }
        }
        total
    }

    fn route_cost(route: &[P], obstacles: &[Rect], wires: &[(P, P)]) -> f32 {
        let mut length = 0.0;
        let mut bends = 0usize;
        let mut overlap = 0.0;
        for pair in route.windows(2) {
            length += pair[0].dist(pair[1]);
            overlap += Self::overlap_length(pair[0], pair[1], wires);
        }
        for triple in route.windows(3) {
            let ab = triple[1].sub(triple[0]);
            let bc = triple[2].sub(triple[1]);
            if (ab.x * bc.y - ab.y * bc.x).abs() > 0.5 {
                bends += 1;
            }
        }
        let blocked = route
            .windows(2)
            .any(|seg| !Self::segments_clear(seg[0], seg[1], obstacles))
            as usize;
        length + bends as f32 * WIRE_BEND_COST + overlap * 4.0 + blocked as f32 * 100_000.0
    }

    fn route_is_clear(route: &[P], obstacles: &[Rect], wires: &[(P, P)]) -> bool {
        route
            .windows(2)
            .all(|seg| Self::segments_clear(seg[0], seg[1], obstacles))
            && route
                .windows(2)
                .all(|seg| Self::overlap_length(seg[0], seg[1], wires) < 0.5)
    }

    fn orthogonal_route(
        proj: &Project,
        lib: &Library,
        start: P,
        end: P,
        horizontal_first: bool,
    ) -> Vec<P> {
        Self::route_with_options(proj, lib, start, end, horizontal_first, None)
    }

    fn route_with_options(
        proj: &Project,
        lib: &Library,
        start: P,
        end: P,
        horizontal_first: bool,
        skip_wire: Option<u64>,
    ) -> Vec<P> {
        if start.dist(end) < 0.5 {
            return vec![start];
        }
        let obstacles = Self::component_obstacles(proj, lib);
        let wires = Self::wire_segments(proj, skip_wire);
        let simple = Self::simplify_route(vec![
            start,
            if horizontal_first {
                P::new(end.x, start.y)
            } else {
                P::new(start.x, end.y)
            },
            end,
        ]);
        if Self::route_is_clear(&simple, &obstacles, &wires) {
            return simple;
        }

        let pad = WIRE_ROUTE_MARGIN;
        let min_x = start.x.min(end.x) - pad;
        let max_x = start.x.max(end.x) + pad;
        let min_y = start.y.min(end.y) - pad;
        let max_y = start.y.max(end.y) + pad;
        let mut candidates = vec![simple];
        for &y in &[min_y, max_y] {
            candidates.push(Self::simplify_route(vec![
                start,
                P::new(start.x, y),
                P::new(end.x, y),
                end,
            ]));
        }
        for &x in &[min_x, max_x] {
            candidates.push(Self::simplify_route(vec![
                start,
                P::new(x, start.y),
                P::new(x, end.y),
                end,
            ]));
        }
        candidates.sort_by(|a, b| {
            Self::route_cost(a, &obstacles, &wires)
                .total_cmp(&Self::route_cost(b, &obstacles, &wires))
        });
        if let Some(best) = candidates
            .iter()
            .find(|candidate| Self::route_is_clear(candidate, &obstacles, &wires))
        {
            return best.clone();
        }
        Self::orthogonal_route_astar(proj, lib, start, end, horizontal_first, skip_wire)
    }

    /// Compressed-grid A* fallback used when the L and box detours are blocked.
    fn orthogonal_route_astar(
        proj: &Project,
        lib: &Library,
        start: P,
        end: P,
        horizontal_first: bool,
        skip_wire: Option<u64>,
    ) -> Vec<P> {
        if start.dist(end) < 0.5 {
            return vec![start];
        }
        let obstacles = Self::component_obstacles(proj, lib);
        let wires = Self::wire_segments(proj, skip_wire);
        let mid = if horizontal_first {
            P::new(end.x, start.y)
        } else {
            P::new(start.x, end.y)
        };
        let simple = Self::simplify_route(vec![start, mid, end]);
        if Self::route_is_clear(&simple, &obstacles, &wires) {
            return simple;
        }

        let mut xs = vec![start.x, end.x];
        let mut ys = vec![start.y, end.y];
        for r in &obstacles {
            xs.push(r.min.x - WIRE_CLEARANCE);
            xs.push(r.max.x + WIRE_CLEARANCE);
            ys.push(r.min.y - WIRE_CLEARANCE);
            ys.push(r.max.y + WIRE_CLEARANCE);
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        ys.sort_by(|a, b| a.total_cmp(b));
        xs.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        ys.dedup_by(|a, b| (*a - *b).abs() < 0.5);

        let sx = Self::nearest_index(&xs, start.x);
        let sy = Self::nearest_index(&ys, start.y);
        let tx = Self::nearest_index(&xs, end.x);
        let ty = Self::nearest_index(&ys, end.y);
        let heuristic = |xi: usize, yi: usize| (xs[xi] - end.x).abs() + (ys[yi] - end.y).abs();

        let mut best: HashMap<(usize, usize, u8), f32> = HashMap::new();
        let mut parent: HashMap<(usize, usize, u8), (usize, usize, u8)> = HashMap::new();
        let mut open = BinaryHeap::new();
        let start_key = (sx, sy, 0);
        best.insert(start_key, 0.0);
        open.push(RouteNode {
            cost: heuristic(sx, sy),
            xi: sx,
            yi: sy,
            dir: 0,
        });

        let mut goal = None;
        let mut visited = 0usize;
        while let Some(cur) = open.pop() {
            let key = (cur.xi, cur.yi, cur.dir);
            let Some(&g) = best.get(&key) else {
                continue;
            };
            if cur.cost > g + heuristic(cur.xi, cur.yi) + 0.01 {
                continue;
            }
            if cur.xi == tx && cur.yi == ty {
                goal = Some(key);
                break;
            }
            visited += 1;
            if visited > 40_000 {
                break;
            }
            for (dx, dy, dir) in [(1isize, 0isize, 1u8), (-1, 0, 1), (0, 1, 2), (0, -1, 2)] {
                let ni = cur.xi as isize + dx;
                let nj = cur.yi as isize + dy;
                if ni < 0 || nj < 0 || ni >= xs.len() as isize || nj >= ys.len() as isize {
                    continue;
                }
                let ni = ni as usize;
                let nj = nj as usize;
                let a = P::new(xs[cur.xi], ys[cur.yi]);
                let b = P::new(xs[ni], ys[nj]);
                if !Self::segments_clear(a, b, &obstacles) {
                    continue;
                }
                let bend = if cur.dir != 0 && cur.dir != dir {
                    WIRE_BEND_COST
                } else {
                    0.0
                };
                let ng = g + a.dist(b) + bend + Self::overlap_length(a, b, &wires) * 4.0;
                let nkey = (ni, nj, dir);
                if best.get(&nkey).map(|v| ng < *v).unwrap_or(true) {
                    best.insert(nkey, ng);
                    parent.insert(nkey, key);
                    open.push(RouteNode {
                        cost: ng + heuristic(ni, nj),
                        xi: ni,
                        yi: nj,
                        dir,
                    });
                }
            }
        }

        let Some(mut key) = goal else {
            return simple;
        };
        let mut path = Vec::new();
        path.push(P::new(xs[key.0], ys[key.1]));
        while let Some(&prev) = parent.get(&key) {
            key = prev;
            path.push(P::new(xs[key.0], ys[key.1]));
        }
        path.reverse();
        if let Some(first) = path.first_mut() {
            *first = start;
        }
        if let Some(last) = path.last_mut() {
            *last = end;
        }
        Self::simplify_route(path)
    }

    /// Connect each group of pins with routed wires, chaining every pin to
    /// its nearest unconnected neighbour. Returns the number of wires added.
    pub(crate) fn wire_nets(proj: &mut Project, lib: &Library, nets: &[Vec<(u64, usize)>]) -> usize {
        let mut wired = 0;
        for net in nets {
            let mut pins = net.clone();
            pins.dedup();
            if pins.len() < 2 {
                continue;
            }
            let pos = |p: &(u64, usize), proj: &Project| proj.pin_pos(lib, p.0, p.1);
            let mut chain = vec![pins.remove(0)];
            while !pins.is_empty() {
                let Some(last) = pos(chain.last().unwrap(), proj) else { break };
                let (i, _) = pins
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| pos(p, proj).map(|q| (i, q.dist(last))))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap_or((0, 0.0));
                chain.push(pins.remove(i));
            }
            for pair in chain.windows(2) {
                let (Some(a), Some(b)) = (pos(&pair[0], proj), pos(&pair[1], proj)) else {
                    continue;
                };
                let horizontal = proj.pin_is_horizontal(lib, pair[0].0, pair[0].1);
                let route = Self::route_anchored(proj, lib, &[a, b], horizontal);
                if route.len() >= 2 {
                    let id = proj.alloc_id();
                    proj.wires.push(Wire {
                        id,
                        pts: route,
                        start_pin: Some(pair[0]),
                        end_pin: Some(pair[1]),
                    });
                    wired += 1;
                }
            }
        }
        wired
    }

    pub(crate) fn route_anchored(
        proj: &Project,
        lib: &Library,
        points: &[P],
        horizontal_first: bool,
    ) -> Vec<P> {
        Self::route_anchored_skipping(proj, lib, points, horizontal_first, None)
    }

    fn route_anchored_skipping(
        proj: &Project,
        lib: &Library,
        points: &[P],
        horizontal_first: bool,
        skip_wire: Option<u64>,
    ) -> Vec<P> {
        let mut out: Vec<P> = Vec::new();
        for (i, pair) in points.windows(2).enumerate() {
            let h = if i == 0 {
                horizontal_first
            } else {
                !horizontal_first
            };
            let seg = Self::route_with_options(proj, lib, pair[0], pair[1], h, skip_wire);
            if out.is_empty() {
                out.extend(seg);
            } else {
                out.extend(seg.into_iter().skip(1));
            }
        }
        out
    }

    fn pin_position(proj: &Project, lib: &Library, pin: PinKey) -> Option<P> {
        proj.pin_pos(lib, pin.0, pin.1)
    }

    fn pin_horizontal(proj: &Project, lib: &Library, pin: PinKey) -> bool {
        proj.pin_is_horizontal(lib, pin.0, pin.1)
    }

    fn pin_at(proj: &Project, lib: &Library, p: P) -> Option<PinKey> {
        let mut best = None;
        for c in &proj.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            for (i, pd) in def.pins.iter().enumerate() {
                let wp = c.pos().add(pd.pos.rot(c.rot));
                let d = wp.dist(p);
                if d < 3.0 && best.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best = Some((d, (c.id, i)));
                }
            }
        }
        best.map(|(_, key)| key)
    }

    /// Interior points where another wire actually terminates. Two wires can
    /// share generated bends near the same pin; those bends are not junctions
    /// and retaining them makes a dragged part leave a trail of corners.
    fn wire_junctions(proj: &Project, id: u64, pts: &[P]) -> Vec<P> {
        let mut out: Vec<P> = Vec::new();
        for p in pts.iter().skip(1).take(pts.len().saturating_sub(2)) {
            let shared = proj
                .wires
                .iter()
                .filter(|other| other.id != id)
                .any(|other| {
                    other
                        .pts
                        .first()
                        .into_iter()
                        .chain(other.pts.last())
                        .any(|q| q.dist(*p) < 0.5)
                });
            if shared && !out.iter().any(|q| q.dist(*p) < 0.5) {
                out.push(*p);
            }
        }
        out
    }

    fn refresh_attached_wires(proj: &mut Project, lib: &Library, moved: &HashSet<u64>) {
        if moved.is_empty() {
            return;
        }
        let plans: Vec<(u64, Vec<P>)> = proj
            .wires
            .iter()
            .filter_map(|w| {
                let attached = w.start_pin.map(|p| moved.contains(&p.0)).unwrap_or(false)
                    || w.end_pin.map(|p| moved.contains(&p.0)).unwrap_or(false);
                if !attached || w.pts.len() < 2 {
                    return None;
                }
                let old = w.pts.clone();
                let start_pin = w
                    .start_pin
                    .and_then(|p| Self::pin_position(proj, lib, p))
                    .unwrap_or(old[0]);
                let end_pin = w
                    .end_pin
                    .and_then(|p| Self::pin_position(proj, lib, p))
                    .unwrap_or(*old.last().unwrap());
                let start = w
                    .start_pin
                    .and_then(|p| Self::pin_stub(proj, lib, p))
                    .unwrap_or(start_pin);
                let end = w
                    .end_pin
                    .and_then(|p| Self::pin_stub(proj, lib, p))
                    .unwrap_or(end_pin);
                let horizontal = match w.start_pin {
                    Some(pin) => Self::pin_horizontal(proj, lib, pin),
                    None => (old[1].x - old[0].x).abs() >= (old[1].y - old[0].y).abs(),
                };
                let mut points = vec![start_pin];
                if start.dist(start_pin) > 0.5 {
                    points.push(start);
                }
                points.extend(Self::wire_junctions(proj, w.id, &old));
                if end.dist(end_pin) > 0.5 {
                    points.push(end);
                }
                points.push(end_pin);
                Some((
                    w.id,
                    Self::route_anchored_skipping(proj, lib, &points, horizontal, Some(w.id)),
                ))
            })
            .collect();
        for (id, pts) in plans {
            if let Some(w) = proj.wires.iter_mut().find(|w| w.id == id) {
                if w.pts != pts && pts.len() >= 2 {
                    w.pts = pts;
                    proj.dirty = true;
                }
            }
        }
    }

    /// During a drag, keep attached wires visually stable with a simple
    /// orthogonal preview. The obstacle-aware router runs once on release;
    /// repeatedly invoking it while the pointer moves makes equally valid
    /// routes alternate from frame to frame.
    fn attached_wire_previews(
        proj: &Project,
        lib: &Library,
        moved: &HashSet<u64>,
    ) -> HashMap<u64, Vec<P>> {
        if moved.is_empty() {
            return HashMap::new();
        }
        proj.wires
            .iter()
            .filter_map(|wire| {
                let attached = wire
                    .start_pin
                    .map(|pin| moved.contains(&pin.0))
                    .unwrap_or(false)
                    || wire
                        .end_pin
                        .map(|pin| moved.contains(&pin.0))
                        .unwrap_or(false);
                if !attached || wire.pts.len() < 2 {
                    return None;
                }
                let start = wire
                    .start_pin
                    .and_then(|pin| Self::pin_position(proj, lib, pin))
                    .unwrap_or(wire.pts[0]);
                let end = wire
                    .end_pin
                    .and_then(|pin| Self::pin_position(proj, lib, pin))
                    .unwrap_or(*wire.pts.last().unwrap());
                let horizontal = wire
                    .start_pin
                    .map(|pin| Self::pin_horizontal(proj, lib, pin))
                    .unwrap_or(true);
                let corner = if horizontal {
                    P::new(end.x, start.y)
                } else {
                    P::new(start.x, end.y)
                };
                let mut pts = vec![start];
                if corner.dist(start) > 0.5 && corner.dist(end) > 0.5 {
                    pts.push(corner);
                }
                pts.push(end);
                Some((wire.id, pts))
            })
            .collect()
    }

    /// Re-route wires that a newly placed or moved component landed on.
    /// Attached wires are refreshed separately; this handles unrelated wires
    /// that previously remained visible through the symbol body.
    fn repair_obstructed_wires(proj: &mut Project, lib: &Library) {
        let obstacles = Self::component_obstacles(proj, lib);
        let blocked: Vec<u64> = proj
            .wires
            .iter()
            .filter(|wire| {
                wire.pts
                    .windows(2)
                    .any(|seg| !Self::segments_clear(seg[0], seg[1], &obstacles))
            })
            .map(|wire| wire.id)
            .collect();

        for id in blocked {
            let Some(wire) = proj.wires.iter().find(|wire| wire.id == id).cloned() else {
                continue;
            };
            if wire.pts.len() < 2 {
                continue;
            }
            let old = wire.pts.clone();
            let start_pin = wire
                .start_pin
                .and_then(|pin| Self::pin_position(proj, lib, pin))
                .unwrap_or(old[0]);
            let end_pin = wire
                .end_pin
                .and_then(|pin| Self::pin_position(proj, lib, pin))
                .unwrap_or(*old.last().unwrap());
            let start = wire
                .start_pin
                .and_then(|pin| Self::pin_stub(proj, lib, pin))
                .unwrap_or(start_pin);
            let end = wire
                .end_pin
                .and_then(|pin| Self::pin_stub(proj, lib, pin))
                .unwrap_or(end_pin);
            let horizontal = wire
                .start_pin
                .map(|pin| Self::pin_horizontal(proj, lib, pin))
                .unwrap_or_else(|| (old[1].x - old[0].x).abs() >= (old[1].y - old[0].y).abs());
            let mut anchors = vec![start_pin];
            if start.dist(start_pin) > 0.5 {
                anchors.push(start);
            }
            anchors.extend(Self::wire_junctions(proj, id, &old));
            if end.dist(end_pin) > 0.5 {
                anchors.push(end);
            }
            anchors.push(end_pin);
            let route = Self::route_anchored_skipping(proj, lib, &anchors, horizontal, Some(id));
            if route.len() >= 2 && route != old {
                if let Some(target) = proj.wires.iter_mut().find(|wire| wire.id == id) {
                    target.pts = route;
                    proj.dirty = true;
                }
            }
        }
    }

    fn remove_components(proj: &mut Project, _lib: &Library, ids: &HashSet<u64>) {
        if ids.is_empty() {
            return;
        }
        // Deleting an object also deletes wires connected to it. Keeping
        // their old geometry creates misleading floating nets after deletion.
        proj.wires.retain(|w| !w.uses_any(ids));
        proj.comps.retain(|c| !ids.contains(&c.id));
        proj.fps.retain(|f| !ids.contains(&f.comp));
        proj.dirty = true;
    }

    fn rotate_components(proj: &mut Project, lib: &Library, ids: &HashSet<u64>, step: i32) {
        for c in proj.comps.iter_mut().filter(|c| ids.contains(&c.id)) {
            c.rot = (c.rot + step).rem_euclid(360);
        }
        Self::refresh_attached_wires(proj, lib, ids);
        Self::repair_obstructed_wires(proj, lib);
        proj.dirty = true;
    }

    fn finish_wire_to(
        &mut self,
        proj: &mut Project,
        lib: &Library,
        end: P,
        end_pin: Option<PinKey>,
    ) {
        if self.draft.len() >= 1 {
            let start_pin = self.draft_pin.map(|p| p.key());
            let start_h = self.draft_pin.map(|p| p.horizontal).unwrap_or(true);
            let mut points = self.draft.clone();
            points.push(end);
            let route = if self.wire_free {
                points
            } else {
                Self::route_anchored(proj, lib, &points, start_h)
            };
            if route.len() >= 2 {
                let id = proj.alloc_id();
                let wire = Wire {
                    id,
                    pts: route,
                    start_pin,
                    end_pin,
                };
                self.last_wire = Some(wire.clone());
                proj.wires.push(wire);
                proj.dirty = true;
                self.hint = "Wire connected".into();
            }
        }
        self.draft.clear();
        self.draft_pin = None;
        self.wire_free = false;
    }

    /// Auto-repeat the previous wire from an aligned green pin.
    fn repeat_last_wire(&mut self, proj: &mut Project, lib: &Library, pin: PinRef) {
        let Some(last) = self.last_wire.clone() else {
            self.hint = "Place a wire first, then double-click an aligned pin".into();
            return;
        };
        if last.pts.len() < 2 {
            return;
        }
        let first_h =
            (last.pts[1].x - last.pts[0].x).abs() >= (last.pts[1].y - last.pts[0].y).abs();
        if first_h != pin.horizontal {
            self.hint = "Double-click repeat needs an aligned pin".into();
            return;
        }
        let delta = pin.pos.sub(last.pts[0]);
        let pts: Vec<P> = last.pts.iter().map(|p| p.add(delta)).collect();
        let end_pin = last
            .end_pin
            .and_then(|_| Self::pin_at(proj, lib, *pts.last().unwrap()));
        let id = proj.alloc_id();
        proj.wires.push(Wire {
            id,
            pts,
            start_pin: Some(pin.key()),
            end_pin,
        });
        self.last_wire = Some(proj.wires.last().unwrap().clone());
        proj.dirty = true;
        self.hint = "Wire repeated".into();
    }

    fn zoom_step(&mut self, anchor: Pos2, factor: f32) {
        let before = self.view.to_world(anchor);
        self.view.zoom = (self.view.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let after = self.view.to_world(anchor);
        self.view.off += Vec2::new(
            (after.x - before.x) * self.view.zoom,
            (after.y - before.y) * self.view.zoom,
        );
        self.hint = format!("Zoom {:.0}%", self.view.zoom * 100.0);
    }

    fn zoom_to_fit(&mut self, rect: Rect) {
        let zoom = (rect.width() / (SHEET_W + 80.0))
            .min(rect.height() / (SHEET_H + 80.0))
            .clamp(ZOOM_MIN, ZOOM_MAX);
        self.view.zoom = zoom;
        self.view.off = Vec2::new(
            rect.center().x - SHEET_W * 0.5 * zoom,
            rect.center().y - SHEET_H * 0.5 * zoom,
        );
    }

    /// Keep a small part of the sheet frame on-screen after panning. This
    /// leaves room to inspect the canvas while preventing the page from being
    /// lost completely with no obvious way back.
    fn keep_sheet_reachable(&mut self, rect: Rect) {
        const VISIBLE_EDGE: f32 = 24.0;
        let sheet = Rect::from_two_pos(
            self.view.to_screen(P::new(0.0, 0.0)),
            self.view.to_screen(P::new(SHEET_W, SHEET_H)),
        );

        if sheet.right() < rect.left() + VISIBLE_EDGE {
            self.view.off.x += rect.left() + VISIBLE_EDGE - sheet.right();
        } else if sheet.left() > rect.right() - VISIBLE_EDGE {
            self.view.off.x -= sheet.left() - (rect.right() - VISIBLE_EDGE);
        }
        if sheet.bottom() < rect.top() + VISIBLE_EDGE {
            self.view.off.y += rect.top() + VISIBLE_EDGE - sheet.bottom();
        } else if sheet.top() > rect.bottom() - VISIBLE_EDGE {
            self.view.off.y -= sheet.top() - (rect.bottom() - VISIBLE_EDGE);
        }
    }

    /// Shift+drag zoom: fit the world-space box into the canvas.
    fn zoom_to_world_rect(&mut self, rect: Rect, a: P, b: P) {
        let w = (b.x - a.x).abs().max(1.0);
        let h = (b.y - a.y).abs().max(1.0);
        let zoom = (rect.width() / w)
            .min(rect.height() / h)
            .clamp(ZOOM_MIN, ZOOM_MAX);
        self.view.zoom = zoom;
        self.view.off = Vec2::new(
            rect.center().x - (a.x + b.x) * 0.5 * zoom,
            rect.center().y - (a.y + b.y) * 0.5 * zoom,
        );
        self.hint = format!("Zoom {:.0}%", zoom * 100.0);
    }

    fn context_menu(&mut self, ui: &mut egui::Ui, proj: &mut Project, lib: &Library, w: P) {
        let pin = Self::hit_pin(proj, lib, w);
        let comp = pin.map(|p| p.comp).or_else(|| hit_component(proj, lib, w));
        let wire = Self::hit_wire(proj, w).map(|(id, _, _)| id);
        let label = proj
            .labels
            .iter()
            .find(|l| P::new(l.x, l.y).dist(w) < 40.0)
            .map(|l| l.id);

        if let Some(id) = comp {
            if ui.button("Edit Properties (E)").clicked() {
                self.action = Some(EditorAction::EditComponent(id));
            }
            if ui.button("Rotate 90° (R)").clicked() {
                let mut ids = HashSet::new();
                ids.insert(id);
                Self::rotate_components(proj, lib, &ids, 90);
            }
            if ui.button("Delete (Del)").clicked() {
                let mut ids = HashSet::new();
                ids.insert(id);
                Self::remove_components(proj, lib, &ids);
                self.sel.remove(&id);
            }
            ui.separator();
        }
        if let Some(id) = wire {
            if ui.button("Delete Wire").clicked() {
                proj.wires.retain(|x| x.id != id);
                self.sel_wire.remove(&id);
                proj.dirty = true;
            }
            ui.separator();
        }
        if let Some(id) = label {
            if ui.button("Delete Label").clicked() {
                proj.labels.retain(|l| l.id != id);
                proj.dirty = true;
            }
            ui.separator();
        }
        if ui.button("Place Component (P)").clicked() {
            self.action = Some(EditorAction::PickParts);
        }
        if ui.button("Start Wire (W)").clicked() {
            self.tool = Tool::Wire;
            self.hint = "Wire tool: click a pin to start".into();
        }
        if ui.button("Net Label").clicked() {
            self.tool = Tool::Label;
        }
        if ui.button("Clear Selection").clicked() {
            self.clear_selection();
        }
        if ui.button("Zoom to Fit (F8)").clicked() {
            // The rectangle is recomputed by the caller on the next frame.
            self.hint = "Press F8 to fit the drawing".into();
        }
    }

    /// Show the sheet. Returns a status string for the status bar.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        proj: &mut Project,
        lib: &Library,
        mut sim: Option<&mut Simulator>,
        label_text: &str,
        probe_text: &str,
    ) -> String {
        let desired = ui.available_size_before_wrap();
        let (rect, resp) = ui.allocate_exact_size(desired, Sense::click_and_drag());
        if rect.width() < 10.0 || rect.height() < 10.0 {
            return self.hint.clone();
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, CANVAS);

        if self.fit_on_next_frame {
            self.zoom_to_fit(rect);
            self.fit_on_next_frame = false;
        }

        // ---- input
        let hover = resp.hover_pos();
        if let Some(h) = hover {
            self.mouse = self.view.to_world(h);
        }

        // Instant, cursor-centred zoom. egui's smooth_scroll_delta is filtered
        // over several frames, which caused the reported zoom delay. Read the
        // raw MouseWheel events instead.
        let mut scroll_lines = 0.0;
        let mut wheel_zoom = 0.0;
        let mut wheel_x = 0.0;
        let (shift, ctrl) = ui.input(|i| {
            for event in &i.events {
                if let egui::Event::MouseWheel { delta, unit, .. } = event {
                    let scale = match unit {
                        MouseWheelUnit::Line => 1.0,
                        MouseWheelUnit::Point => 1.0 / WHEEL_POINTS_PER_LINE,
                        MouseWheelUnit::Page => 5.0,
                    };
                    scroll_lines -= delta.y * scale;
                    // egui reports wheel-up as positive. Zoom follows that sign;
                    // shift-wheel pan keeps the previous scroll direction.
                    wheel_zoom += delta.y * scale;
                    wheel_x -= delta.x * scale;
                }
            }
            (i.modifiers.shift, i.modifiers.ctrl || i.modifiers.command)
        });
        if resp.hovered() && (wheel_zoom.abs() > 0.0 || wheel_x.abs() > 0.0) {
            if shift {
                let dx = if wheel_x.abs() > 0.0 {
                    wheel_x
                } else {
                    scroll_lines
                } * 80.0;
                self.view.off.x -= dx;
                self.hint = "Horizontal pan".into();
            } else if wheel_zoom.abs() > 0.0 {
                let factor = (wheel_zoom * WHEEL_LINE_FACTOR).exp();
                if let Some(h) = hover {
                    let before = self.view.to_world(h);
                    self.view.zoom = (self.view.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
                    let after = self.view.to_world(h);
                    self.view.off += Vec2::new(
                        (after.x - before.x) * self.view.zoom,
                        (after.y - before.y) * self.view.zoom,
                    );
                    self.hint = format!("Zoom {:.0}%", self.view.zoom * 100.0);
                }
            }
        }

        // Middle click enters track-pan; move the mouse, click again to drop.
        if resp.clicked_by(egui::PointerButton::Middle) {
            self.pan_mode = !self.pan_mode;
            self.pan_last = if self.pan_mode { hover } else { None };
            self.hint = if self.pan_mode {
                "Track pan: move the mouse, click to drop".into()
            } else {
                "Track pan off".into()
            };
        }
        if self.pan_mode {
            if let (Some(now), Some(last)) = (hover, self.pan_last) {
                self.view.off += now - last;
            }
            self.pan_last = hover;
        } else {
            let panning = resp.dragged_by(egui::PointerButton::Middle)
                || (resp.dragged_by(egui::PointerButton::Primary)
                    && ui.input(|i| i.modifiers.command || i.key_pressed(egui::Key::Space)));
            if panning {
                self.view.off += resp.drag_delta();
            }
        }

        let snapped = self.snap(proj, self.mouse);
        self.mouse = snapped;

        // Green "hot" cursor: a pin is close enough to start or finish a wire.
        let hover_world = hover.map(|h| self.view.to_world(h));
        self.hot_pin = hover_world.and_then(|w| Self::hit_pin(proj, lib, w));
        self.hot_comp = hover_world.and_then(|w| self.hit_component(proj, lib, w));
        if self.tool == Tool::Select && self.drag.is_none() && self.hot_comp.is_some() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grab);
        }

        // A right click is consumed by the active gesture, so remember which
        // gesture was live before the click handlers change it.
        let wire_active = !self.draft.is_empty();
        let placement_active = self.pending.is_some();

        // ---- keyboard
        let mut rotate = false;
        let mut rotate_ccw = false;
        let mut del = false;
        let mut esc = false;
        let mut edit = false;
        let mut grid_toggle = false;
        let mut fit = false;
        let mut zoom_in = false;
        let mut zoom_out = false;
        let mut centre = false;
        let mut false_origin_toggle = false;
        let mut x_cursor_toggle = false;
        let mut snap: Option<usize> = None;
        let mut redraw = false;
        let placing = self.pending.is_some();
        // Keys typed into a text field (a dialog, the label box) belong to
        // that field, not to the canvas shortcuts.
        let typing = ui.ctx().egui_wants_keyboard_input();
        ui.input(|i| {
            if typing {
                return;
            }
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            let r_key = i.key_pressed(egui::Key::R);
            rotate = r_key || i.key_pressed(egui::Key::Plus);
            redraw = r_key && !placing;
            rotate_ccw = i.key_pressed(egui::Key::Minus);
            del = i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace);
            esc = i.key_pressed(egui::Key::Escape);
            edit = i.key_pressed(egui::Key::Enter);
            grid_toggle = i.key_pressed(egui::Key::G);
            false_origin_toggle = i.key_pressed(egui::Key::O);
            x_cursor_toggle = i.key_pressed(egui::Key::X);
            fit = i.key_pressed(egui::Key::F8);
            zoom_in = i.key_pressed(egui::Key::F6);
            zoom_out = i.key_pressed(egui::Key::F7);
            centre = i.key_pressed(egui::Key::F5);
            if ctrl && i.key_pressed(egui::Key::F1) {
                snap = Some(0);
            } else if i.key_pressed(egui::Key::F2) {
                snap = Some(1);
            } else if i.key_pressed(egui::Key::F3) {
                snap = Some(2);
            } else if i.key_pressed(egui::Key::F4) {
                snap = Some(3);
            }
        });

        if esc {
            if !self.draft.is_empty() {
                self.draft.clear();
                self.draft_pin = None;
                self.wire_free = false;
                self.hint = "Wire cancelled".into();
            } else if self.pending.is_some() {
                self.pending = None;
                self.pending_rot = 0;
                self.tool = Tool::Select;
                self.hint = "Placement cancelled".into();
            } else {
                self.clear_selection();
                if self.tool != Tool::Select {
                    self.tool = Tool::Select;
                    self.hint = "Tool: Select".into();
                }
            }
            self.drag = None;
            self.drag_wire_paths.clear();
        }
        if (rotate || rotate_ccw) && self.pending.is_some() {
            self.pending_rot =
                (self.pending_rot + if rotate_ccw { -90 } else { 90 }).rem_euclid(360);
            self.hint = format!("Placement rotation {}°", self.pending_rot);
        }
        if grid_toggle {
            self.grid_mode = (self.grid_mode + 1) % 3;
            self.hint = match self.grid_mode {
                0 => "Grid: dots".into(),
                1 => "Grid: lines".into(),
                _ => "Grid: off".into(),
            };
        }
        if let Some(index) = snap {
            proj.grid.active = index.min(proj.grid.snaps.len() - 1);
            self.hint = format!("Grid snap {} set", proj.grid.step());
        }
        if false_origin_toggle {
            if self.false_origin.take().is_some() {
                self.hint = "False origin cleared".into();
            } else {
                let w = self.snap(proj, self.mouse);
                self.false_origin = Some(w);
                self.hint = format!("False origin at {:.0}, {:.0}", w.x, w.y);
            }
        }
        if x_cursor_toggle {
            self.x_cursor = !self.x_cursor;
            self.hint = if self.x_cursor {
                "X cursor on".into()
            } else {
                "X cursor off".into()
            };
        }
        if redraw {
            self.hint = "Display redrawn".into();
        }
        if fit {
            self.zoom_to_fit(rect);
            self.hint = "Zoom to whole drawing".into();
        }
        if zoom_in {
            self.zoom_step(hover.unwrap_or(rect.center()), 1.25);
        }
        if zoom_out {
            self.zoom_step(hover.unwrap_or(rect.center()), 1.0 / 1.25);
        }
        if centre {
            self.view.off = Vec2::new(rect.width() * 0.5, rect.height() * 0.5)
                - Vec2::new(self.mouse.x * self.view.zoom, self.mouse.y * self.view.zoom);
            self.hint = "Centred on cursor".into();
        }

        // ---- mouse actions
        if resp.clicked() && !resp.double_clicked() && !self.pan_mode {
            if let Some(pos) = hover {
                let raw = self.view.to_world(pos);
                let w = self.snap(proj, raw);
                if !self.draft.is_empty() {
                    // Modeless wiring: click a pin or wire to finish, click empty
                    // space to add a follow-me anchor (follow-me wiring).
                    let same_pin = self
                        .draft_pin
                        .and_then(|p| self.hot_pin.map(|h| h.key() == p.key()))
                        .unwrap_or(false);
                    if let Some(target) = self.hot_pin {
                        if !same_pin {
                            self.finish_wire_to(proj, lib, target.pos, Some(target.key()));
                        }
                    } else if !ctrl {
                        if let Some((wid, _seg, on)) = Self::hit_wire(proj, w) {
                            self.finish_wire_to(proj, lib, on, None);
                            Self::insert_wire_point(proj, wid, on);
                        } else if self.draft.last().map(|p| p.dist(w) > 4.0).unwrap_or(true) {
                            self.draft.push(w);
                            self.hint = format!(
                                "Wire: {} anchor(s) - click a pin to connect",
                                self.draft.len()
                            );
                        }
                    } else {
                        self.wire_free = true;
                        self.draft.push(w);
                        self.hint = "Wire: free route (Ctrl held)".into();
                    }
                } else {
                    match self.tool {
                        Tool::Part => {
                            if let Some(pid) = self.pending.clone() {
                                if let Some(id) = proj.add_component(lib, &pid, w) {
                                    if let Some(c) = proj.comp_mut(id) {
                                        c.rot = self.pending_rot;
                                    }
                                    Self::repair_obstructed_wires(proj, lib);
                                    self.pending_rot = 0;
                                    self.hint = format!(
                                        "Placed {} - right click or Esc to stop",
                                        proj.comp(id).map(|c| c.refdes.clone()).unwrap_or_default()
                                    );
                                }
                            }
                        }
                        Tool::Select | Tool::Wire => {
                            if let Some(pin) = self.hot_pin {
                                // Hot pin: start a wire without a dedicated mode.
                                self.draft = vec![pin.pos];
                                self.draft_pin = Some(pin);
                                self.wire_free = false;
                                self.hint = "Wire: move to a pin and click to connect".into();
                            } else if self.tool == Tool::Wire {
                                self.draft.push(w);
                                self.draft_pin = None;
                                self.wire_free = ctrl;
                                self.hint = "Wire: click to add anchors".into();
                            } else {
                                self.click_select(proj, lib, raw, ui.input(|i| i.modifiers.shift));
                                if let Some(id) = self.hit_component(proj, lib, raw) {
                                    let is_switch = proj
                                        .comp(id)
                                        .and_then(|c| lib.get(&c.part))
                                        .map(|d| {
                                            matches!(d.symbol, Symbol::Switch | Symbol::Button)
                                        })
                                        .unwrap_or(false);
                                    if is_switch {
                                        if let Some(c) = proj.comp(id) {
                                            let rd = c.refdes.clone();
                                            let on = sim
                                                .as_deref()
                                                .map(|sm| sm.switch_state(&rd))
                                                .or_else(|| self.switch_overrides.get(&rd).copied())
                                                .unwrap_or(false);
                                            let next = !on;
                                            if let Some(sm) = sim.as_deref_mut() {
                                                sm.set_switch(&rd, next);
                                            }
                                            self.switch_overrides.insert(rd.clone(), next);
                                            self.hint = format!(
                                                "{rd} -> {}",
                                                if next { "closed" } else { "open" }
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Tool::Label | Tool::Probe | Tool::Text => {
                            let kind = match self.tool {
                                Tool::Text => LabelKind::Text,
                                _ => LabelKind::Net,
                            };
                            let mut text = match self.tool {
                                Tool::Text => label_text.to_string(),
                                Tool::Probe => probe_text.to_string(),
                                _ => label_text.to_string(),
                            };
                            if text.trim().is_empty() {
                                text = if self.tool == Tool::Probe {
                                    "PROBE".into()
                                } else {
                                    "NET".into()
                                };
                            }
                            let id = proj.alloc_id();
                            let probe = self.tool == Tool::Probe;
                            proj.labels.push(Label {
                                id,
                                kind,
                                text: text.trim().to_string(),
                                x: w.x,
                                y: w.y,
                                rot: 0,
                                probe,
                            });
                            proj.dirty = true;
                            self.hint = format!("Added label {}", text.trim());
                        }
                        Tool::Delete => {
                            Self::delete_at(proj, lib, w);
                            self.hint = "Deleted".into();
                        }
                    }
                }
            }
        }

        if resp.double_clicked() {
            let same_start = self
                .draft_pin
                .and_then(|p| self.hot_pin.map(|h| h.key() == p.key()))
                .unwrap_or(false);
            if !self.draft.is_empty() && !same_start {
                if let Some(pin) = self.hot_pin {
                    self.finish_wire_to(proj, lib, pin.pos, Some(pin.key()));
                } else {
                    self.finish_wire_to(proj, lib, self.mouse, None);
                }
            } else if let Some(pin) = self.hot_pin {
                // Help: double-clicking a green pin repeats the last wire when
                // the destination pin is aligned with the previous one.
                self.draft.clear();
                self.draft_pin = None;
                self.wire_free = false;
                self.repeat_last_wire(proj, lib, pin);
            } else if let Some(raw) = hover_world {
                // Help: double-clicking a component opens its properties.
                if let Some(id) = self.hit_component(proj, lib, raw) {
                    self.action = Some(EditorAction::EditComponent(id));
                }
            }
        }

        if resp.secondary_clicked() && !self.pan_mode {
            if wire_active {
                // Terminate Wire: finish on the target pin, wire or cursor.
                let same_start = self
                    .draft_pin
                    .and_then(|p| self.hot_pin.map(|h| h.key() == p.key()))
                    .unwrap_or(false);
                match self.hot_pin {
                    Some(target) if !same_start => {
                        self.finish_wire_to(proj, lib, target.pos, Some(target.key()));
                    }
                    _ => {
                        let at = Self::hit_wire(proj, self.mouse)
                            .map(|(_, _, on)| on)
                            .unwrap_or(self.mouse);
                        self.finish_wire_to(proj, lib, at, None);
                    }
                }
                self.draft.clear();
                self.draft_pin = None;
                self.wire_free = false;
                self.hint = "Wire finished".into();
            } else if placement_active {
                self.pending = None;
                self.pending_rot = 0;
                self.tool = Tool::Select;
                self.hint = "Placement finished".into();
            }
        }

        // Dragging moves components and labels or draws a tag box. Wires remain
        // selectable but locked against direct movement.
        if self.tool == Tool::Select
            && self.draft.is_empty()
            && resp.drag_started_by(egui::PointerButton::Primary)
        {
            self.drag_wire_paths.clear();
            if let Some(h) = hover {
                let raw = self.view.to_world(h);
                let w = self.snap(proj, raw);
                if ui.input(|i| i.modifiers.shift) {
                    self.drag = Some(Drag::ZoomBox { from: w });
                    self.hint = "Zoom box: release to zoom in".into();
                } else if let Some(cid) = self.hit_component(proj, lib, raw) {
                    if !self.sel.contains(&cid) {
                        self.clear_selection();
                        self.sel.insert(cid);
                    }
                    self.drag = Some(self.move_drag(proj, w));
                    self.hint = "Moving component".into();
                } else if let Some((wid, _, _)) = Self::hit_wire(proj, w) {
                    self.clear_selection();
                    self.sel_wire.insert(wid);
                    self.drag = None;
                    self.hint = "Wire selected (movement locked)".into();
                } else {
                    if self.selected_count() == 0 {
                        self.click_select(proj, lib, raw, false);
                    }
                    if self.selected_count() > 0 {
                        self.drag = Some(self.move_drag(proj, w));
                    } else {
                        self.drag = Some(Drag::Box { from: w });
                    }
                }
            }
        }
        if let Some(d) = &self.drag {
            if resp.dragged_by(egui::PointerButton::Primary) {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grabbing);
                let d = d.clone();
                if let Some(h) = hover {
                    let cur = self.snap(proj, self.view.to_world(h));
                    let step = self.snap_step(proj);
                    match d {
                        Drag::Move {
                            start,
                            comps,
                            labels,
                        } => {
                            let delta = cur.sub(start);
                            let moved: HashSet<u64> = comps.iter().map(|(id, _)| *id).collect();
                            for (id, p0) in comps {
                                if let Some(c) = proj.comp_mut(id) {
                                    let np = p0.add(delta).snap(step);
                                    c.x = np.x;
                                    c.y = np.y;
                                }
                            }
                            for (id, p0) in labels {
                                if let Some(l) = proj.labels.iter_mut().find(|l| l.id == id) {
                                    let np = p0.add(delta).snap(step);
                                    l.x = np.x;
                                    l.y = np.y;
                                }
                            }
                            if !moved.is_empty() {
                                self.drag_wire_paths =
                                    Self::attached_wire_previews(proj, lib, &moved);
                            }
                            if delta.x != 0.0 || delta.y != 0.0 {
                                proj.dirty = true;
                            }
                        }
                        Drag::Box { .. } | Drag::ZoomBox { .. } => {}
                    }
                }
            }
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            let moved = match &self.drag {
                Some(Drag::Move { comps, .. }) => {
                    comps.iter().map(|(id, _)| *id).collect::<HashSet<_>>()
                }
                _ => HashSet::new(),
            };
            if let Some(Drag::ZoomBox { from }) = self.drag {
                let to = hover
                    .map(|h| self.snap(proj, self.view.to_world(h)))
                    .unwrap_or(self.mouse);
                if (to.x - from.x).abs() > 2.0 || (to.y - from.y).abs() > 2.0 {
                    self.zoom_to_world_rect(rect, from, to);
                }
            }
            if !moved.is_empty() {
                Self::refresh_attached_wires(proj, lib, &moved);
                Self::repair_obstructed_wires(proj, lib);
            }
            self.drag_wire_paths.clear();
            self.drag = None;
        }

        if self.pending.is_none() && (rotate || rotate_ccw) && !self.sel.is_empty() {
            let step = if rotate_ccw { -90 } else { 90 };
            Self::rotate_components(proj, lib, &self.sel, step);
            self.hint = format!("Rotated {step}°");
        }
        if del {
            if !self.draft.is_empty() {
                self.draft.clear();
                self.draft_pin = None;
                self.wire_free = false;
                self.hint = "Wire cancelled".into();
            } else if self.selected_count() > 0 {
                self.delete_selection(proj, lib);
                self.hint = "Deleted selection".into();
            }
        }
        if edit && !self.draft.is_empty() {
            if let Some(pin) = self.hot_pin {
                self.finish_wire_to(proj, lib, pin.pos, Some(pin.key()));
            } else {
                self.finish_wire_to(proj, lib, self.mouse, None);
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::E)) {
            if let Some(&id) = self.sel.iter().next() {
                self.action = Some(EditorAction::EditComponent(id));
            }
        }
        // Right-click context menu. Wiring and placement consume the right
        // click, so they must not also open the menu in the same frame.
        if !wire_active && !placement_active {
            let menu_w = self.mouse;
            resp.context_menu(|ui| {
                self.context_menu(ui, proj, lib, menu_w);
            });
        }

        // ---- drawing
        self.keep_sheet_reachable(rect);
        let sim = sim.as_deref();
        self.draw_sheet(&painter, rect);
        self.draw_grid(&painter, rect, proj);
        self.draw_cursor_overlays(&painter, rect);
        self.draw_wires(&painter, proj, lib, sim);
        self.draw_junctions(&painter, proj);
        self.draw_components(&painter, proj, lib, sim);
        self.draw_labels(&painter, proj, sim);
        self.draw_pins(&painter, proj, lib, sim);
        self.draw_simulation_status(&painter, rect, sim);

        // Draft wire: follow-me orthogonal route, with a green target pin.
        if let Some(pin) = self.hot_pin {
            let s = self.view.to_screen(pin.pos);
            painter.circle_stroke(s, 6.0, Stroke::new(2.0, BODY_GREEN));
        }
        if self.draft.len() >= 1 {
            let mut world = self.draft.clone();
            world.push(self.mouse);
            let first_h = self.draft_pin.map(|p| p.horizontal).unwrap_or(true);
            let route = if self.wire_free {
                world
            } else {
                Self::route_anchored(proj, lib, &world, first_h)
            };
            let pts: Vec<Pos2> = route.iter().map(|p| self.view.to_screen(*p)).collect();
            painter.add(Shape::line(pts, Stroke::new(2.0, WIRE_HOT)));
            for p in &self.draft {
                let s = self.view.to_screen(*p);
                painter.circle_filled(s, 3.0, WIRE_HOT);
            }
            self.hint = format!(
                "Wire: {} anchor(s) - click a pin or wire to connect, right click to finish, Esc to cancel",
                self.draft.len()
            );
        }

        if !wire_active
            && !placement_active
            && self.draft.is_empty()
            && self.tool == Tool::Select
            && self.drag.is_none()
        {
            if let Some(id) = self.hot_comp {
                if let Some(c) = proj.comp(id) {
                    self.hint = format!("{} = {} - double-click to edit", c.refdes, c.value);
                }
            }
        }

        // ghost part
        if self.tool == Tool::Part {
            if let Some(pid) = &self.pending {
                if let Some(def) = lib.get(pid) {
                    let ghost = Component {
                        id: u64::MAX,
                        part: def.id.clone(),
                        refdes: String::new(),
                        value: def.value.clone(),
                        x: self.mouse.x,
                        y: self.mouse.y,
                        rot: self.pending_rot,
                        props: Vec::new(),
                        hidden_props: Vec::new(),
                        text_size: None,
                    };
                    let mut a = painter.clone();
                    a.set_opacity(0.55);
                    draw_component(&a, &self.view, &ghost, def, sim, lib, None);
                }
            }
        }

        // Shift+drag zoom preview.
        if let Some(Drag::ZoomBox { from }) = &self.drag {
            if let Some(h) = hover {
                let a = self.view.to_screen(*from);
                let b = self.view.to_screen(self.view.to_world(h));
                let r = Rect::from_two_pos(a, b);
                painter.rect_filled(r, 0.0, Color32::from_rgba_premultiplied(60, 180, 120, 55));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, OK), StrokeKind::Middle);
            }
        }

        // selection rubber band
        if let Some(Drag::Box { from }) = &self.drag {
            if let Some(h) = hover {
                let a = self.view.to_screen(*from);
                let b = self.view.to_screen(self.view.to_world(h));
                let r = Rect::from_two_pos(a, b);
                painter.rect_filled(r, 0.0, Color32::from_rgba_premultiplied(60, 120, 200, 60));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, GHOST), StrokeKind::Middle);
                let w0 = self.view.to_world(Pos2::new(a.x.min(b.x), a.y.min(b.y)));
                let w1 = self.view.to_world(Pos2::new(a.x.max(b.x), a.y.max(b.y)));
                self.sel.clear();
                for c in &proj.comps {
                    let p = c.pos();
                    if p.x >= w0.x && p.x <= w1.x && p.y >= w0.y && p.y <= w1.y {
                        self.sel.insert(c.id);
                    }
                }
            }
        }

        // status line under the cursor
        if hover.is_some() {
            let txt = format!(
                "X: {}   Y: {}",
                format_th(self.mouse.x),
                format_th(self.mouse.y)
            );
            let pos = Pos2::new(rect.left() + 8.0, rect.bottom() - 20.0);
            painter.rect_filled(
                Rect::from_min_size(pos - Vec2::new(4.0, 2.0), Vec2::new(150.0, 18.0)),
                0.0,
                Color32::from_rgba_premultiplied(0, 0, 0, 160),
            );
            painter.text(
                pos,
                egui::Align2::LEFT_TOP,
                txt,
                FontId::new(12.0, FontFamily::Monospace),
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
            );
        }

        if !self.follow {
            self.follow = true;
        }
        self.hint.clone()
    }

    fn snap_step(&self, proj: &Project) -> f32 {
        if proj.grid.snap_on {
            proj.grid.step()
        } else {
            1.0
        }
    }

    fn snap(&self, proj: &Project, p: P) -> P {
        p.snap(self.snap_step(proj))
    }

    fn delete_at(proj: &mut Project, lib: &Library, w: P) {
        // wires first (closest hit), then labels, then components
        let mut best: Option<(usize, f32)> = None;
        for (i, wire) in proj.wires.iter().enumerate() {
            for seg in wire.pts.windows(2) {
                let d = crate::model::point_seg_dist(w, seg[0], seg[1]);
                if d < 14.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((i, d));
                }
            }
        }
        if let Some((i, _)) = best {
            proj.wires.remove(i);
            proj.dirty = true;
            return;
        }
        let mut best_l: Option<(usize, f32)> = None;
        for (i, l) in proj.labels.iter().enumerate() {
            let d = P::new(l.x, l.y).dist(w);
            if d < 40.0 && best_l.map(|(_, bd)| d < bd).unwrap_or(true) {
                best_l = Some((i, d));
            }
        }
        if let Some((i, _)) = best_l {
            proj.labels.remove(i);
            proj.dirty = true;
            return;
        }
        if let Some(id) = hit_component(proj, lib, w) {
            let mut ids = HashSet::new();
            ids.insert(id);
            Self::remove_components(proj, lib, &ids);
        }
    }

    fn delete_selection(&mut self, proj: &mut Project, lib: &Library) {
        let ids = self.sel.clone();
        Self::remove_components(proj, lib, &ids);
        let wsel = self.sel_wire.clone();
        proj.wires.retain(|w| !wsel.contains(&w.id));
        let lsel = self.sel_label.clone();
        proj.labels.retain(|l| !lsel.contains(&l.id));
        if !ids.is_empty() || !wsel.is_empty() || !lsel.is_empty() {
            proj.dirty = true;
        }
        self.clear_selection();
    }

    fn click_select(&mut self, proj: &Project, lib: &Library, w: P, add: bool) {
        if !add {
            self.clear_selection();
        }
        if let Some(id) = hit_component(proj, lib, w) {
            if add && self.sel.contains(&id) {
                self.sel.remove(&id);
            } else {
                self.sel.insert(id);
            }
            return;
        }
        for wire in &proj.wires {
            for seg in wire.pts.windows(2) {
                if crate::model::point_seg_dist(w, seg[0], seg[1]) < 12.0 {
                    if add && self.sel_wire.contains(&wire.id) {
                        self.sel_wire.remove(&wire.id);
                    } else {
                        self.sel_wire.insert(wire.id);
                    }
                    return;
                }
            }
        }
        for l in &proj.labels {
            if P::new(l.x, l.y).dist(w) < 40.0 {
                if add && self.sel_label.contains(&l.id) {
                    self.sel_label.remove(&l.id);
                } else {
                    self.sel_label.insert(l.id);
                }
                return;
            }
        }
    }

    // ------------------------------------------------------------ drawing

    fn draw_sheet(&self, p: &egui::Painter, rect: Rect) {
        let a = self.view.to_screen(P::new(0.0, 0.0));
        let b = self.view.to_screen(P::new(SHEET_W, SHEET_H));
        let r = Rect::from_two_pos(a, b);
        if r.intersects(rect) {
            p.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(0x50, 0x54, 0x58)),
                StrokeKind::Middle,
            );
        }
    }

    /// Manual origin and X cursor overlays.
    fn draw_cursor_overlays(&self, p: &egui::Painter, rect: Rect) {
        let color = Color32::from_rgb(0x4a, 0x80, 0xb8);
        if let Some(origin) = self.false_origin {
            let c = self.view.to_screen(origin);
            p.line_segment(
                [Pos2::new(c.x - 10.0, c.y), Pos2::new(c.x + 10.0, c.y)],
                Stroke::new(1.4, color),
            );
            p.line_segment(
                [Pos2::new(c.x, c.y - 10.0), Pos2::new(c.x, c.y + 10.0)],
                Stroke::new(1.4, color),
            );
            p.circle_stroke(c, 5.0, Stroke::new(1.2, color));
        }
        if self.x_cursor {
            let c = self.view.to_screen(self.mouse);
            p.line_segment(
                [Pos2::new(c.x, rect.top()), Pos2::new(c.x, rect.bottom())],
                Stroke::new(1.0, color),
            );
            p.line_segment(
                [Pos2::new(rect.left(), c.y), Pos2::new(rect.right(), c.y)],
                Stroke::new(1.0, color),
            );
        }
    }

    fn draw_grid(&self, p: &egui::Painter, rect: Rect, proj: &Project) {
        if self.grid_mode == 2 || !proj.grid.show_dots {
            return;
        }
        // dot_spacing is in screen pixels; convert to world units before
        // stepping through the sheet. Using pixels as world units made the
        // grid explode into ~30k shapes at full zoom-out (the reported lag).
        let spacing_px = proj.grid.dot_spacing(self.view.zoom);
        let world_step = spacing_px / self.view.zoom;
        let tl = self.view.to_world(rect.left_top());
        let br = self.view.to_world(rect.right_bottom());
        let cols = ((br.x - tl.x) / world_step).ceil() as usize;
        let rows = ((br.y - tl.y) / world_step).ceil() as usize;
        if cols == 0 || rows == 0 || cols.saturating_mul(rows) > 14000 {
            return;
        }
        let major = proj.grid.step() * 5.0;
        let mut shapes: Vec<Shape> = Vec::with_capacity(cols * rows);
        if self.grid_mode == 1 {
            let mut x = (tl.x / world_step).floor() * world_step;
            while x <= br.x {
                let s = self.view.to_screen(P::new(x, tl.y));
                let e = self.view.to_screen(P::new(x, br.y));
                shapes.push(Shape::line_segment([s, e], Stroke::new(1.0, GRID_DOT)));
                x += world_step;
            }
            let mut y = (tl.y / world_step).floor() * world_step;
            while y <= br.y {
                let s = self.view.to_screen(P::new(tl.x, y));
                let e = self.view.to_screen(P::new(br.x, y));
                shapes.push(Shape::line_segment([s, e], Stroke::new(1.0, GRID_DOT)));
                y += world_step;
            }
        } else {
            let mut x = (tl.x / world_step).floor() * world_step;
            while x <= br.x {
                let mut y = (tl.y / world_step).floor() * world_step;
                while y <= br.y {
                    let s = self.view.to_screen(P::new(x, y));
                    let is_major = ((x / major).round() - x / major).abs() < 1e-3
                        && ((y / major).round() - y / major).abs() < 1e-3;
                    shapes.push(Shape::circle_filled(
                        s,
                        if is_major { 1.6 } else { 1.1 },
                        if is_major { GRID_DOT_MAJOR } else { GRID_DOT },
                    ));
                    y += world_step;
                }
                x += world_step;
            }
        }
        p.add(Shape::Vec(shapes));
    }

    fn draw_wires(
        &mut self,
        p: &egui::Painter,
        proj: &Project,
        lib: &Library,
        sim: Option<&Simulator>,
    ) {
        // Net construction walks every component and wire. Build it once for
        // this paint instead of once for every wire without a simulated part
        // directly attached to an endpoint.
        let running_sim = sim.filter(|sm| sm.running);
        self.refresh_electrical_cache(proj, lib);
        let netlist = running_sim.map(|_| &self.netlist_cache);
        for w in &proj.wires {
            let world_points = self
                .drag_wire_paths
                .get(&w.id)
                .map(Vec::as_slice)
                .unwrap_or(&w.pts);
            let pts: Vec<Pos2> = world_points
                .iter()
                .map(|q| self.view.to_screen(*q))
                .collect();
            if pts.len() < 2 {
                continue;
            }
            let sel = self.sel_wire.contains(&w.id);
            let current = running_sim
                .map(|sm| wire_current_with_netlist(proj, w, sm, netlist))
                .unwrap_or(0.0);
            let flowing = current.abs() > 1.0e-9;
            if flowing && !sel {
                p.add(Shape::line(
                    pts.clone(),
                    Stroke::new(6.0, Color32::from_rgba_premultiplied(0x30, 0xC8, 0xFF, 45)),
                ));
            }
            p.add(Shape::line(
                pts.clone(),
                Stroke::new(
                    if sel { 3.0 } else { 1.8 },
                    if sel {
                        SELECTION
                    } else if flowing {
                        Color32::from_rgb(0x16, 0xC8, 0xFF)
                    } else {
                        WIRE
                    },
                ),
            ));
            if flowing {
                // Animate from the paint clock, not sim.t: the solver only
                // advances 20 times a second, which made the arrows jump.
                let clock = p.ctx().input(|i| i.time);
                let length = polyline_length(&pts);
                let spacing = 42.0;
                let speed = 130.0 + (current.abs() as f32 * 5_000.0).min(130.0);
                let mut phase = ((clock * speed as f64) % spacing as f64) as f32;
                phase = (phase + (w.id % 17) as f32 * 7.0) % spacing;
                if current < 0.0 {
                    phase = spacing - phase;
                }
                let strength = (current.abs() as f32 * 1_000.0 / 20.0).clamp(0.25, 1.0);
                let mut at = phase;
                while at < length {
                    if let Some((dot, mut direction)) = point_and_direction_along_polyline(&pts, at)
                    {
                        if current < 0.0 {
                            direction = -direction;
                        }
                        p.circle_filled(
                            dot,
                            5.5,
                            Color32::from_rgba_premultiplied(0x20, 0xC8, 0xFF, 55),
                        );
                        let side = Vec2::new(-direction.y, direction.x);
                        p.add(Shape::convex_polygon(
                            vec![
                                dot + direction * (4.8 + strength),
                                dot - direction * 3.2 + side * 3.2,
                                dot - direction * 3.2 - side * 3.2,
                            ],
                            Color32::from_rgb(0xD8, 0xFA, 0xFF),
                            Stroke::NONE,
                        ));
                    }
                    at += spacing;
                }
            }
            if sel {
                for s in &pts {
                    p.circle_filled(*s, 2.5, SELECTION);
                }
            }
        }
    }

    fn draw_simulation_status(&self, p: &egui::Painter, canvas: Rect, sim: Option<&Simulator>) {
        let Some(sim) = sim.filter(|sm| sm.running) else {
            return;
        };
        let badge = Rect::from_min_size(
            canvas.left_top() + Vec2::new(12.0, 12.0),
            Vec2::new(430.0, 34.0),
        );
        p.rect_filled(badge, 5.0, Color32::from_rgba_premultiplied(4, 30, 24, 235));
        p.rect_stroke(
            badge,
            5.0,
            Stroke::new(1.0, Color32::from_rgb(0x22, 0xD6, 0x83)),
            StrokeKind::Middle,
        );
        p.circle_filled(
            Pos2::new(badge.left() + 17.0, badge.center().y),
            4.5,
            Color32::from_rgb(0x2B, 0xF0, 0x91),
        );
        p.text(
            Pos2::new(badge.left() + 30.0, badge.center().y),
            egui::Align2::LEFT_CENTER,
            format!(
                "LIVE  |  conventional current: VCC -> load -> GND  |  t = {:.3} s",
                sim.t
            ),
            FontId::new(13.0, FontFamily::Monospace),
            Color32::from_rgb(0xD8, 0xFF, 0xEC),
        );
    }

    /// Points where the schematic should draw a junction dot. A dot marks a
    /// wire terminal landing on another wire, or terminals that meet there.
    /// Interior crossings and generated bends stay bare.
    fn junction_points(proj: &Project) -> Vec<P> {
        let key = |pt: P| ((pt.x * 4.0).round() as i64, (pt.y * 4.0).round() as i64);
        let segments: Vec<(u64, P, P)> = proj
            .wires
            .iter()
            .flat_map(|w| {
                w.pts
                    .windows(2)
                    .map(|s| (w.id, s[0], s[1]))
                    .collect::<Vec<_>>()
            })
            .collect();
        let terminals: Vec<(u64, P)> = proj
            .wires
            .iter()
            .filter_map(|w| {
                let first = *w.pts.first()?;
                let last = *w.pts.last()?;
                Some(vec![(w.id, first), (w.id, last)])
            })
            .flatten()
            .collect();

        let mut candidates: HashMap<(i64, i64), P> = HashMap::new();
        for (wid, at) in &terminals {
            for (oid, a, b) in &segments {
                if oid != wid && crate::model::point_on_segment(*at, *a, *b, 1.0) {
                    candidates.entry(key(*at)).or_insert(*at);
                }
            }
        }

        let direction = |from: P, to: P| -> Option<(i32, i32)> {
            let dx = to.x - from.x;
            let dy = to.y - from.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 0.5 {
                return None;
            }
            Some((
                (dx / len * 4.0).round() as i32,
                (dy / len * 4.0).round() as i32,
            ))
        };

        let mut out = Vec::new();
        for at in candidates.values() {
            let mut dirs: Vec<(i32, i32)> = Vec::new();
            for (_, a, b) in &segments {
                if !crate::model::point_on_segment(*at, *a, *b, 1.0) {
                    continue;
                }
                if let Some(d) = direction(*at, *a) {
                    dirs.push(d);
                }
                if let Some(d) = direction(*at, *b) {
                    dirs.push(d);
                }
            }
            dirs.sort_unstable();
            dirs.dedup();
            if dirs.len() >= 3 {
                out.push(*at);
            }
        }
        out.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        out
    }

    fn wire_geometry_key(proj: &Project) -> u64 {
        let mut key = proj.wires.len() as u64;
        for wire in &proj.wires {
            key = key.wrapping_mul(0x100_0000_01b3).wrapping_add(wire.id);
            for point in &wire.pts {
                key = key
                    .wrapping_mul(0x100_0000_01b3)
                    .wrapping_add(point.x.to_bits() as u64);
                key = key
                    .wrapping_mul(0x100_0000_01b3)
                    .wrapping_add(point.y.to_bits() as u64);
            }
        }
        key
    }

    fn electrical_geometry_key(proj: &Project) -> u64 {
        let mut h = DefaultHasher::new();
        proj.comps.len().hash(&mut h);
        for c in &proj.comps {
            c.id.hash(&mut h);
            c.part.hash(&mut h);
            c.x.to_bits().hash(&mut h);
            c.y.to_bits().hash(&mut h);
            c.rot.hash(&mut h);
        }
        proj.wires.len().hash(&mut h);
        for wire in &proj.wires {
            wire.id.hash(&mut h);
            wire.start_pin.hash(&mut h);
            wire.end_pin.hash(&mut h);
            for point in &wire.pts {
                point.x.to_bits().hash(&mut h);
                point.y.to_bits().hash(&mut h);
            }
        }
        proj.labels.len().hash(&mut h);
        for label in &proj.labels {
            label.text.hash(&mut h);
            label.x.to_bits().hash(&mut h);
            label.y.to_bits().hash(&mut h);
            label.probe.hash(&mut h);
        }
        h.finish()
    }

    fn refresh_electrical_cache(&mut self, proj: &Project, lib: &Library) {
        let key = Self::electrical_geometry_key(proj);
        if self.electrical_cache_key == key {
            return;
        }

        self.netlist_cache = proj.netlist(lib);
        let pins: Vec<(PinKey, P)> = proj
            .comps
            .iter()
            .flat_map(|component| {
                lib.get(&component.part)
                    .into_iter()
                    .flat_map(move |definition| {
                        definition.pins.iter().enumerate().map(move |(index, pin)| {
                            (
                                (component.id, index),
                                component.pos().add(pin.pos.rot(component.rot)),
                            )
                        })
                    })
            })
            .collect();
        let segments: Vec<(P, P)> = proj
            .wires
            .iter()
            .flat_map(|wire| wire.pts.windows(2).map(|segment| (segment[0], segment[1])))
            .collect();

        self.connected_pin_cache.clear();
        for (index, (pin_key, pin_pos)) in pins.iter().enumerate() {
            let on_wire = segments
                .iter()
                .any(|(a, b)| crate::model::point_on_segment(*pin_pos, *a, *b, 3.0));
            let touches_pin = pins
                .iter()
                .enumerate()
                .any(|(other_index, (_, other_pos))| {
                    other_index != index && other_pos.dist(*pin_pos) < 3.0
                });
            if on_wire || touches_pin {
                self.connected_pin_cache.insert(*pin_key);
            }
        }
        self.electrical_cache_key = key;
    }

    fn draw_junctions(&mut self, p: &egui::Painter, proj: &Project) {
        let key = Self::wire_geometry_key(proj);
        if self.junction_cache_key != key {
            self.junction_cache = Self::junction_points(proj);
            self.junction_cache_key = key;
        }
        for at in &self.junction_cache {
            let s = self.view.to_screen(*at);
            p.circle_filled(s, 4.0, WIRE);
        }
    }

    fn draw_components(
        &self,
        p: &egui::Painter,
        proj: &Project,
        lib: &Library,
        sim: Option<&Simulator>,
    ) {
        for c in &proj.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            let selected = self.sel.contains(&c.id);
            let hovered = self.hot_comp == Some(c.id) && !selected;
            if selected || hovered {
                let mut pins = proj.pins_of(lib, c);
                // One-pin symbols draw their body on one side of the pin; box
                // the drawing, not a square centred on the pin.
                let body: &[P] = match def.symbol {
                    Symbol::Rail => &[P { x: -14.0, y: -58.0 }, P { x: 14.0, y: 0.0 }],
                    Symbol::Ground => &[P { x: -34.0, y: 0.0 }, P { x: 34.0, y: 46.0 }],
                    _ => &[],
                };
                pins.extend(body.iter().map(|q| c.pos().add(q.rot(c.rot))));
                let pad = if body.is_empty() { 40.0 } else { 8.0 };
                if let Some(r) = bbox_of(&pins, pad) {
                    let a = self.view.to_screen(P::new(r.min.x, r.min.y));
                    let b = self.view.to_screen(P::new(r.max.x, r.max.y));
                    let rr = Rect::from_two_pos(a, b);
                    let color = if selected { SELECTION } else { HOVER };
                    p.rect_stroke(rr, 0.0, Stroke::new(1.0, color), StrokeKind::Middle);
                }
            }
            draw_component(
                p,
                &self.view,
                c,
                def,
                sim,
                lib,
                self.switch_overrides.get(&c.refdes).copied(),
            );
        }
    }

    fn draw_labels(&self, p: &egui::Painter, proj: &Project, sim: Option<&Simulator>) {
        for l in &proj.labels {
            let selected = self.sel_label.contains(&l.id);
            let color = match l.kind {
                LabelKind::Text => TEXT_DIM,
                LabelKind::Power => BODY_GREEN,
                LabelKind::Net => {
                    if l.probe {
                        Color32::from_rgb(0xFF, 0xC0, 0x40)
                    } else {
                        WIRE_HOT
                    }
                }
            };
            let mut text = l.text.clone();
            if l.probe || l.kind == LabelKind::Net {
                if let Some(sim) = sim {
                    if let Some(v) = sim.voltage(&l.text) {
                        text = format!("{} = {:.3} V", l.text, v);
                    }
                }
            }
            let pos = self.view.to_screen(P::new(l.x, l.y));
            let font = FontId::new(12.0, FontFamily::Proportional);
            let size = p.layout_no_wrap(text.clone(), font.clone(), color);
            let origin = match l.rot.rem_euclid(360) {
                90 => pos + Vec2::new(-size.size().y, 0.0),
                180 => pos - size.size(),
                270 => pos + Vec2::new(0.0, -size.size().x),
                _ => pos,
            };
            p.rect_filled(
                Rect::from_min_size(
                    origin - Vec2::new(2.0, 1.0),
                    size.size() + Vec2::new(4.0, 2.0),
                ),
                0.0,
                Color32::from_rgba_premultiplied(0, 0, 0, 150),
            );
            p.text(origin, egui::Align2::LEFT_TOP, text, font, color);
            if selected {
                p.rect_stroke(
                    Rect::from_min_size(
                        origin - Vec2::new(3.0, 2.0),
                        size.size() + Vec2::new(6.0, 4.0),
                    ),
                    0.0,
                    Stroke::new(1.0, SELECTION),
                    StrokeKind::Middle,
                );
            }
            if l.probe {
                p.circle_stroke(
                    pos,
                    5.0,
                    Stroke::new(1.5, Color32::from_rgb(0xFF, 0xC0, 0x40)),
                );
            }
        }
    }

    fn draw_pins(
        &mut self,
        p: &egui::Painter,
        proj: &Project,
        lib: &Library,
        sim: Option<&Simulator>,
    ) {
        let show_all = matches!(self.tool, Tool::Wire | Tool::Select | Tool::Part);
        if !show_all {
            return;
        }
        self.refresh_electrical_cache(proj, lib);
        for c in &proj.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            for (pin_index, pd) in def.pins.iter().enumerate() {
                let wp = c.pos().add(pd.pos.rot(c.rot));
                let s = self.view.to_screen(wp);
                let connected = self.connected_pin_cache.contains(&(c.id, pin_index));
                if connected {
                    p.circle_filled(s, 2.6, WIRE);
                } else {
                    p.circle_stroke(s, 2.6, Stroke::new(1.0, PIN));
                }
                if self.tool == Tool::Wire && !connected {
                    let _ = sim;
                }
            }
        }
    }
}

fn bbox_of(pts: &[P], pad: f32) -> Option<Rect> {
    if pts.is_empty() {
        return None;
    }
    let mut min = pts[0];
    let mut max = pts[0];
    for p in pts {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    Some(Rect::from_min_max(
        Pos2::new(min.x - pad, min.y - pad),
        Pos2::new(max.x + pad, max.y + pad),
    ))
}

fn polyline_length(pts: &[Pos2]) -> f32 {
    pts.windows(2).map(|pair| pair[0].distance(pair[1])).sum()
}

fn format_current(current: f64) -> String {
    let current = current.abs();
    if !current.is_finite() {
        return "invalid".into();
    }
    if current >= 1.0 {
        format!("{current:.2} A")
    } else if current >= 1.0e-3 {
        format!("{:.1} mA", current * 1_000.0)
    } else if current >= 1.0e-6 {
        format!("{:.1} uA", current * 1_000_000.0)
    } else {
        "0 mA".into()
    }
}

fn point_and_direction_along_polyline(pts: &[Pos2], mut distance: f32) -> Option<(Pos2, Vec2)> {
    for pair in pts.windows(2) {
        let delta = pair[1] - pair[0];
        let length = delta.length();
        if length <= f32::EPSILON {
            continue;
        }
        if distance <= length {
            let t = (distance / length).clamp(0.0, 1.0);
            return Some((pair[0].lerp(pair[1], t), delta / length));
        }
        distance -= length;
    }
    None
}

fn wire_current_with_netlist(
    proj: &Project,
    wire: &Wire,
    sim: &Simulator,
    netlist: Option<&Netlist>,
) -> f64 {
    [(wire.start_pin, true), (wire.end_pin, false)]
        .into_iter()
        .filter_map(|(terminal, at_start)| {
            let (component, pin) = terminal?;
            let component = proj.comp(component)?;
            let element = sim.element(&component.refdes)?;
            let element_current = sim.current_of(element);
            // Element current is positive from pin 0 to pin 1. Convert it to
            // the wire's stored start-to-end direction so a reversed wire does
            // not make conventional current appear to run toward VCC.
            let into_element = if pin == 0 {
                element_current
            } else if pin == 1 {
                -element_current
            } else {
                element_current
            };
            Some(if at_start {
                -into_element
            } else {
                into_element
            })
        })
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or_else(|| {
            let Some(netlist) = netlist else {
                return 0.0;
            };
            let voltage = [wire.start_pin, wire.end_pin]
                .into_iter()
                .flatten()
                .filter_map(|(component, pin)| netlist.net_of(component, pin))
                .filter_map(|net| sim.voltage(net))
                .map(f64::abs)
                .fold(0.0, f64::max);
            if voltage > 0.05 {
                1.0e-6
            } else {
                0.0
            }
        })
}

fn wire_current(proj: &Project, lib: &Library, wire: &Wire, sim: &Simulator) -> f64 {
    let netlist = proj.netlist(lib);
    wire_current_with_netlist(proj, wire, sim, Some(&netlist))
}

pub fn hit_component(proj: &Project, lib: &Library, w: P) -> Option<u64> {
    hit_component_with_pad(proj, lib, w, 20.0)
}

fn hit_component_with_pad(proj: &Project, lib: &Library, w: P, pad: f32) -> Option<u64> {
    for c in proj.comps.iter().rev() {
        let Some(def) = lib.get(&c.part) else {
            continue;
        };
        let pins: Vec<P> = def
            .pins
            .iter()
            .map(|pd| c.pos().add(pd.pos.rot(c.rot)))
            .collect();
        if pins.is_empty() {
            if w.dist(c.pos()) < pad.max(20.0) {
                return Some(c.id);
            }
            continue;
        }
        // Power symbols and terminals have one electrical pin but a visible
        // body extending away from it.  Make the body selectable too, not only
        // the tiny green pin target.
        if pins.len() == 1 && w.dist(c.pos()) < pad.max(60.0) {
            return Some(c.id);
        }
        if pins.iter().any(|q| w.dist(*q) < pad.max(4.0)) {
            return Some(c.id);
        }
        if let Some(r) = bbox_of(&pins, 0.0) {
            let p = Pos2::new(w.x, w.y);
            if r.expand(pad).contains(p) {
                return Some(c.id);
            }
        }
    }
    None
}

// ---------------------------------------------------------------- symbols

fn line(p: &egui::Painter, a: Pos2, b: Pos2, color: Color32, w: f32) {
    p.add(Shape::line(vec![a, b], Stroke::new(w, color)));
}

fn rect(p: &egui::Painter, r: Rect, fill: Option<Color32>, stroke: Color32, w: f32) {
    match fill {
        Some(f) => {
            p.rect_filled(r, 0.0, f);
            p.rect_stroke(r, 0.0, Stroke::new(w, stroke), StrokeKind::Middle);
        }
        None => {
            p.rect_stroke(r, 0.0, Stroke::new(w, stroke), StrokeKind::Middle);
        }
    }
}

fn draw_component(
    p: &egui::Painter,
    v: &View,
    c: &Component,
    def: &crate::model::PartDef,
    sim: Option<&Simulator>,
    lib: &Library,
    switch_override: Option<bool>,
) {
    let o = c.pos();
    let s = |q: P| v.to_screen(o.add(q.rot(c.rot)));
    let sim_element = sim.and_then(|sm| sm.element(&c.refdes));
    let failed_open = sim_element.is_some_and(|el| el.failed_open);
    let overloaded = sim.is_some_and(|sm| sm.is_overloaded(&c.refdes));
    let ink = if failed_open {
        Color32::from_rgb(0xF0, 0x48, 0x48)
    } else if overloaded {
        Color32::from_rgb(0xFF, 0xA0, 0x28)
    } else {
        BODY_GREEN
    };
    let thin = 1.6;

    // Draw only the short inward pin connection. The old code extended every
    // pin away from the component, creating stray green lines on both sides.
    for pd in &def.pins {
        if pd.pos.dist(P::ZERO) < 0.5 {
            continue;
        }
        let a = s(pd.pos);
        let angle = (pd.dir as f32).to_radians();
        let inward = P::new(-angle.cos() * 22.0, angle.sin() * 22.0);
        let b = s(pd.pos.add(inward));
        line(p, a, b, ink, thin);
    }

    match def.symbol {
        Symbol::Resistor | Symbol::Pot | Symbol::Fuse => {
            let a = s(P::new(-38.0, 0.0));
            let b = s(P::new(38.0, 0.0));
            let r = Rect::from_two_pos(a, b);
            let r =
                Rect::from_center_size(r.center(), Vec2::new(r.width().max(6.0), 20.0 * v.zoom));
            if def.symbol == Symbol::Fuse {
                rect(p, r, None, ink, thin);
                line(
                    p,
                    Pos2::new(r.left(), r.center().y),
                    Pos2::new(r.right(), r.center().y),
                    ink,
                    thin,
                );
            } else {
                rect(p, r, None, ink, thin);
            }
            if def.symbol == Symbol::Pot {
                let t = s(P::new(0.0, -46.0));
                let m = s(P::new(0.0, -14.0));
                line(p, t, m, ink, thin);
                let head = v.to_screen(o.add(P::new(0.0, -46.0).rot(c.rot)));
                p.add(Shape::convex_polygon(
                    vec![
                        head,
                        head + Vec2::new(-4.0, 7.0),
                        head + Vec2::new(4.0, 7.0),
                    ],
                    ink,
                    Stroke::NONE,
                ));
            }
            if def.symbol == Symbol::Resistor && (failed_open || overloaded) {
                let state = if failed_open {
                    "BLOWN • OPEN"
                } else {
                    "OVERLOAD"
                };
                let color = if failed_open {
                    Color32::from_rgb(0xF0, 0x48, 0x48)
                } else {
                    Color32::from_rgb(0xFF, 0xA0, 0x28)
                };
                let badge = Rect::from_min_size(s(P::new(-54.0, 28.0)), Vec2::new(118.0, 20.0));
                p.rect_filled(badge, 4.0, Color32::from_rgba_premultiplied(8, 10, 12, 235));
                p.rect_stroke(badge, 4.0, Stroke::new(1.0, color), StrokeKind::Middle);
                p.text(
                    badge.center(),
                    egui::Align2::CENTER_CENTER,
                    state,
                    FontId::new(11.0, FontFamily::Monospace),
                    color,
                );
            }
        }
        Symbol::Capacitor => {
            let a = s(P::new(-8.0, -26.0));
            let b = s(P::new(-8.0, 26.0));
            let c2 = s(P::new(8.0, -26.0));
            let d = s(P::new(8.0, 26.0));
            line(p, a, b, ink, 2.2);
            line(p, c2, d, ink, 2.2);
        }
        Symbol::PolarCap => {
            let a = s(P::new(-8.0, -26.0));
            let b = s(P::new(-8.0, 26.0));
            line(p, a, b, ink, 2.2);
            let c2 = s(P::new(10.0, -26.0));
            let d = s(P::new(10.0, 26.0));
            // curved plate
            let mid = s(P::new(16.0, 0.0));
            p.add(Shape::line(vec![c2, mid, d], Stroke::new(2.2, ink)));
            let plus = s(P::new(-20.0, -14.0));
            line(
                p,
                plus + Vec2::new(-4.0, 0.0),
                plus + Vec2::new(4.0, 0.0),
                ink,
                1.4,
            );
            line(
                p,
                plus + Vec2::new(0.0, -4.0),
                plus + Vec2::new(0.0, 4.0),
                ink,
                1.4,
            );
        }
        Symbol::Inductor => {
            let mut prev = s(P::new(-40.0, 0.0));
            for i in 0..4 {
                let x0 = -40.0 + i as f32 * 20.0;
                let c1 = s(P::new(x0 + 10.0, 0.0));
                let e = s(P::new(x0 + 20.0, 0.0));
                // approximate an arc with a few segments
                let mut pts = vec![prev];
                for k in 1..=6 {
                    let t = k as f32 / 6.0;
                    let ang = std::f32::consts::PI * t;
                    let wx = x0 + 10.0 - 10.0 * ang.cos();
                    let wy = -10.0 * ang.sin();
                    pts.push(s(P::new(wx, wy)));
                }
                pts.push(e);
                p.add(Shape::line(pts, Stroke::new(thin, ink)));
                prev = e;
                let _ = c1;
            }
        }
        Symbol::Diode | Symbol::Led => {
            let running = sim.is_some_and(|sm| sm.running);
            let lit = if def.symbol == Symbol::Led && running {
                sim.map(|sm| sm.led_level(&c.refdes)).unwrap_or(0.0)
            } else {
                0.0
            };
            let led_color = if def.id.contains("GREEN") {
                Color32::from_rgb(0x35, 0xF0, 0x68)
            } else if def.id.contains("YELLOW") {
                Color32::from_rgb(0xFF, 0xD8, 0x3D)
            } else {
                Color32::from_rgb(0xFF, 0x38, 0x28)
            };
            if def.symbol == Symbol::Led && lit > 0.01 {
                let clock = p.ctx().input(|i| i.time);
                let pulse = sim
                    .map(|_| 0.88 + 0.12 * ((clock * 9.0) % std::f64::consts::TAU).sin() as f32)
                    .unwrap_or(1.0);
                let center = s(P::ZERO);
                p.circle_filled(
                    center,
                    48.0 * v.zoom * pulse,
                    Color32::from_rgba_premultiplied(
                        led_color.r(),
                        led_color.g(),
                        led_color.b(),
                        (22.0 + lit * 35.0) as u8,
                    ),
                );
                p.circle_filled(
                    center,
                    33.0 * v.zoom * pulse,
                    Color32::from_rgba_premultiplied(
                        led_color.r(),
                        led_color.g(),
                        led_color.b(),
                        (45.0 + lit * 70.0) as u8,
                    ),
                );
            }
            let tri = vec![
                s(P::new(-24.0, -24.0)),
                s(P::new(-24.0, 24.0)),
                s(P::new(18.0, 0.0)),
            ];
            let diode_fill = if def.symbol == Symbol::Led {
                if failed_open {
                    Color32::from_rgb(0x31, 0x31, 0x34)
                } else if overloaded {
                    Color32::from_rgb(0xB8, 0x62, 0x18)
                } else if lit > 0.01 {
                    led_color
                } else {
                    Color32::from_rgb(0x62, 0x1B, 0x1A)
                }
            } else {
                BODY_RED
            };
            p.add(Shape::convex_polygon(
                tri,
                diode_fill,
                Stroke::new(thin, ink),
            ));
            line(p, s(P::new(18.0, -26.0)), s(P::new(18.0, 26.0)), ink, 2.2);
            if def.symbol == Symbol::Led {
                for d in [0.0, 14.0] {
                    let a = s(P::new(-6.0 + d, -34.0));
                    let b = s(P::new(6.0 + d, -46.0));
                    line(p, a, b, ink, 1.2);
                    p.add(Shape::convex_polygon(
                        vec![b, b + Vec2::new(-4.0, 1.0), b + Vec2::new(-1.0, 4.0)],
                        ink,
                        Stroke::NONE,
                    ));
                }
                if running || failed_open {
                    if let Some(sm) = sim {
                        if let Some(element) = sm.element(&c.refdes) {
                            let current = sm.current_of(element).abs();
                            let va = sm.v.get(element.a()).copied().unwrap_or(0.0);
                            let vb = sm.v.get(element.b()).copied().unwrap_or(0.0);
                            let state = if failed_open {
                                "BLOWN • OPEN"
                            } else if overloaded {
                                "OVERLOAD"
                            } else if lit > 0.03 {
                                "ON"
                            } else {
                                "OFF"
                            };
                            let status_color = if failed_open {
                                Color32::from_rgb(0xF0, 0x48, 0x48)
                            } else if overloaded {
                                Color32::from_rgb(0xFF, 0xA0, 0x28)
                            } else {
                                led_color
                            };
                            let badge =
                                Rect::from_min_size(s(P::new(-54.0, 42.0)), Vec2::new(198.0, 22.0));
                            p.rect_filled(
                                badge,
                                4.0,
                                Color32::from_rgba_premultiplied(8, 10, 12, 235),
                            );
                            p.rect_stroke(
                                badge,
                                4.0,
                                Stroke::new(1.0, status_color),
                                StrokeKind::Middle,
                            );
                            p.text(
                                badge.center(),
                                egui::Align2::CENTER_CENTER,
                                format!(
                                    "{state}  {}  {:.2} V",
                                    format_current(current),
                                    (va - vb).abs()
                                ),
                                FontId::new(11.5, FontFamily::Monospace),
                                status_color,
                            );
                        }
                    }
                }
            }
        }
        Symbol::Npn | Symbol::Pnp => {
            let r = 44.0;
            let a = s(P::new(-r, -r));
            let b = s(P::new(r, r));
            let circ = Rect::from_two_pos(a, b);
            p.circle_stroke(circ.center(), circ.width() * 0.5, Stroke::new(thin, ink));
            line(p, s(P::new(-46.0, 0.0)), s(P::new(-10.0, 0.0)), ink, thin);
            line(p, s(P::new(-10.0, -26.0)), s(P::new(-10.0, 26.0)), ink, 2.0);
            line(
                p,
                s(P::new(-10.0, -12.0)),
                s(P::new(34.0, -44.0)),
                ink,
                thin,
            );
            line(p, s(P::new(-10.0, 12.0)), s(P::new(34.0, 44.0)), ink, thin);
            // arrow on the emitter
            let tip = if def.symbol == Symbol::Npn {
                s(P::new(20.0, 32.0))
            } else {
                s(P::new(4.0, -20.0))
            };
            let dir = if def.symbol == Symbol::Npn { 1.0 } else { -1.0 };
            let arrow = vec![
                tip,
                tip + Vec2::new(-6.0 * v.zoom * 3.0, -8.0 * v.zoom * 3.0 * dir),
                tip + Vec2::new(6.0 * v.zoom * 3.0, 6.0 * v.zoom * 3.0 * dir),
            ];
            p.add(Shape::convex_polygon(arrow, ink, Stroke::NONE));
        }
        Symbol::OpAmp => {
            let tri = vec![
                s(P::new(-60.0, -70.0)),
                s(P::new(-60.0, 70.0)),
                s(P::new(70.0, 0.0)),
            ];
            p.add(Shape::convex_polygon(
                tri,
                Color32::TRANSPARENT,
                Stroke::new(1.8, ink),
            ));
            let plus = s(P::new(-48.0, -40.0));
            line(
                p,
                plus + Vec2::new(-5.0, 0.0),
                plus + Vec2::new(5.0, 0.0),
                ink,
                1.4,
            );
            line(
                p,
                plus + Vec2::new(0.0, -5.0),
                plus + Vec2::new(0.0, 5.0),
                ink,
                1.4,
            );
            let minus = s(P::new(-48.0, 40.0));
            line(
                p,
                minus + Vec2::new(-5.0, 0.0),
                minus + Vec2::new(5.0, 0.0),
                ink,
                1.4,
            );
        }
        Symbol::Ground => {
            line(p, s(P::new(0.0, 0.0)), s(P::new(0.0, 26.0)), ink, 2.0);
            for (i, w) in [34.0, 22.0, 10.0].into_iter().enumerate() {
                let y = 26.0 + i as f32 * 10.0;
                line(p, s(P::new(-w, y)), s(P::new(w, y)), ink, 2.0);
            }
        }
        Symbol::Rail => {
            let tip = s(P::new(0.0, -34.0));
            line(p, s(P::new(0.0, 0.0)), tip, ink, 2.0);
            p.add(Shape::convex_polygon(
                vec![tip, tip + Vec2::new(-6.0, 9.0), tip + Vec2::new(6.0, 9.0)],
                ink,
                Stroke::NONE,
            ));
            let lab = s(P::new(0.0, -44.0));
            p.text(
                lab,
                egui::Align2::CENTER_BOTTOM,
                def.id.clone(),
                FontId::new(12.0, FontFamily::Proportional),
                BODY_GREEN,
            );
        }
        Symbol::Switch | Symbol::Button => {
            line(p, s(P::new(-34.0, 0.0)), s(P::new(-14.0, 0.0)), ink, 2.0);
            line(p, s(P::new(14.0, 0.0)), s(P::new(34.0, 0.0)), ink, 2.0);
            p.circle_filled(s(P::new(-14.0, 0.0)), 3.0 * v.zoom.max(0.4), ink);
            p.circle_filled(s(P::new(14.0, 0.0)), 3.0 * v.zoom.max(0.4), ink);
            let closed = sim
                .map(|sm| sm.switch_state(&c.refdes))
                .or(switch_override)
                .unwrap_or(false);
            let open = !closed;
            if open {
                line(p, s(P::new(-14.0, 0.0)), s(P::new(26.0, -30.0)), ink, 2.0);
            } else {
                line(p, s(P::new(-14.0, 0.0)), s(P::new(14.0, 0.0)), ink, 2.0);
            }
            if def.symbol == Symbol::Button {
                line(p, s(P::new(0.0, -34.0)), s(P::new(0.0, -10.0)), ink, 1.6);
                line(p, s(P::new(-16.0, -34.0)), s(P::new(16.0, -34.0)), ink, 1.6);
            }
        }
        Symbol::Battery => {
            for (i, h) in [30.0, 16.0].into_iter().enumerate() {
                let x = -12.0 + i as f32 * 24.0;
                if i == 0 {
                    line(p, s(P::new(x, -h)), s(P::new(x, h)), ink, 2.4);
                } else {
                    line(p, s(P::new(x, -h * 0.6)), s(P::new(x, h * 0.6)), ink, 2.4);
                }
            }
            let plus = s(P::new(-30.0, -14.0));
            line(
                p,
                plus + Vec2::new(-4.0, 0.0),
                plus + Vec2::new(4.0, 0.0),
                ink,
                1.4,
            );
            line(
                p,
                plus + Vec2::new(0.0, -4.0),
                plus + Vec2::new(0.0, 4.0),
                ink,
                1.4,
            );
        }
        Symbol::Lamp | Symbol::Motor | Symbol::Speaker => {
            let r = 34.0;
            let a = s(P::new(-r, -r));
            let b = s(P::new(r, r));
            let circ = Rect::from_two_pos(a, b);
            p.circle_stroke(circ.center(), circ.width() * 0.5, Stroke::new(thin, ink));
            match def.symbol {
                Symbol::Lamp => {
                    line(p, s(P::new(-24.0, -24.0)), s(P::new(24.0, 24.0)), ink, thin);
                    line(p, s(P::new(-24.0, 24.0)), s(P::new(24.0, -24.0)), ink, thin);
                }
                Symbol::Motor => {
                    p.text(
                        circ.center(),
                        egui::Align2::CENTER_CENTER,
                        "M",
                        FontId::new(24.0, FontFamily::Proportional),
                        ink,
                    );
                }
                _ => {
                    p.text(
                        circ.center(),
                        egui::Align2::CENTER_CENTER,
                        "\u{266A}",
                        FontId::new(22.0, FontFamily::Proportional),
                        ink,
                    );
                }
            }
        }
        Symbol::Probe => {
            let tip = s(P::new(0.0, -30.0));
            p.add(Shape::convex_polygon(
                vec![tip, tip + Vec2::new(-8.0, 14.0), tip + Vec2::new(8.0, 14.0)],
                Color32::from_rgb(0xFF, 0xC0, 0x40),
                Stroke::NONE,
            ));
            line(p, s(P::new(0.0, -16.0)), s(P::new(0.0, 0.0)), ink, 2.0);
        }
        Symbol::Terminal => {
            let r = 16.0;
            let a = s(P::new(-r, -r));
            let b = s(P::new(r, r));
            let circ = Rect::from_two_pos(a, b);
            p.circle_stroke(circ.center(), circ.width() * 0.5, Stroke::new(2.0, ink));
        }
        Symbol::Xtal => {
            line(p, s(P::new(-16.0, -24.0)), s(P::new(-16.0, 24.0)), ink, 2.2);
            line(p, s(P::new(16.0, -24.0)), s(P::new(16.0, 24.0)), ink, 2.2);
            rect(
                p,
                Rect::from_two_pos(s(P::new(-8.0, -22.0)), s(P::new(8.0, 22.0))),
                None,
                ink,
                thin,
            );
        }
        Symbol::Ic => {
            let pins = &def.pins;
            let (mut miny, mut maxy) = (0.0f32, 0.0f32);
            let (mut minx, mut maxx) = (-100.0f32, 100.0f32);
            for pd in pins {
                miny = miny.min(pd.pos.y);
                maxy = maxy.max(pd.pos.y);
                minx = minx.min(pd.pos.x);
                maxx = maxx.max(pd.pos.x);
            }
            let body = Rect::from_two_pos(
                s(P::new(minx + 18.0, miny - 26.0)),
                s(P::new(maxx - 18.0, maxy + 26.0)),
            );
            rect(p, body, Some(Color32::from_rgb(0x10, 0x18, 0x10)), ink, 1.8);
            // notch
            let notch = Rect::from_center_size(
                Pos2::new(body.left() + 6.0 * v.zoom.max(0.3), body.center().y),
                Vec2::new(8.0 * v.zoom.max(0.3), 8.0 * v.zoom.max(0.3)),
            );
            p.circle_stroke(notch.center(), notch.width() * 0.5, Stroke::new(1.2, ink));
            for (index, pd) in pins.iter().enumerate() {
                let nm = s(pd.pos);
                let inward = P::new(pd.pos.x.signum() * -1.0, 0.0).rot(c.rot);
                let edge = s(pd.pos.add(inward.mul(24.0)));
                line(p, nm, edge, ink, 1.2);

                // Pin identity is part of the circuit, not hidden metadata.
                // Keep it visible on IC/board symbols once the view is large
                // enough to read without turning the canvas into a grey blur.
                if v.zoom >= 0.42 {
                    let local_in = if pd.pos.x < 0.0 { 33.0 } else { -33.0 };
                    let label_pos = s(pd.pos.add(P::new(local_in, 0.0)));
                    let align = if label_pos.x < body.center().x {
                        egui::Align2::LEFT_CENTER
                    } else {
                        egui::Align2::RIGHT_CENTER
                    };
                    let number = index + 1;
                    let label = if pd.name == number.to_string() {
                        number.to_string()
                    } else {
                        format!("{number}  {}", pd.name)
                    };
                    p.text(
                        label_pos,
                        align,
                        label,
                        FontId::new((9.5 * v.zoom).clamp(7.5, 12.0), FontFamily::Monospace),
                        Color32::from_rgb(0xB8, 0xD8, 0xB8),
                    );
                }
            }
            p.text(
                body.center(),
                egui::Align2::CENTER_CENTER,
                def.value.clone(),
                FontId::new(13.0, FontFamily::Proportional),
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
            );
        }
        Symbol::Source => {
            let r = 34.0;
            let a = s(P::new(-r, -r));
            let b = s(P::new(r, r));
            let circ = Rect::from_two_pos(a, b);
            p.circle_stroke(circ.center(), circ.width() * 0.5, Stroke::new(1.8, ink));
            p.text(
                circ.center(),
                egui::Align2::CENTER_CENTER,
                "~",
                FontId::new(26.0, FontFamily::Proportional),
                ink,
            );
        }
        Symbol::Frame => {
            let r = Rect::from_two_pos(s(P::new(0.0, 0.0)), s(P::new(SHEET_W, SHEET_H)));
            rect(p, r, None, Color32::from_rgb(0x60, 0x64, 0x68), 1.0);
        }
    }

    // reference designator and value
    let show_text = def.symbol != Symbol::Ground && def.symbol != Symbol::Rail;
    let hidden = |name: &str| {
        c.hidden_props
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(name))
    };
    // TEXT size is stored as integer height and width. egui has
    // one font-size axis, so the height drives the visible size and
    // width remains persisted for round-tripping and future shaped text.
    let text_scale = c
        .text_size
        .map(|(height, _)| (height as f32 / 20.0).clamp(4.0, 96.0));
    if show_text && !hidden("REF") && !hidden("REFERENCE") && !c.refdes.is_empty() {
        let anchor = s(P::new(0.0, -66.0));
        p.text(
            anchor,
            egui::Align2::CENTER_BOTTOM,
            c.refdes.clone(),
            FontId::new(text_scale.unwrap_or(13.0), FontFamily::Proportional),
            Color32::from_rgb(0xE8, 0xE8, 0xE8),
        );
    }
    if show_text && !hidden("VALUE") && !c.value.is_empty() && def.symbol != Symbol::Ic {
        let anchor = s(P::new(0.0, 70.0));
        p.text(
            anchor,
            egui::Align2::CENTER_TOP,
            c.value.clone(),
            FontId::new(text_scale.unwrap_or(12.0), FontFamily::Proportional),
            Color32::from_rgb(0xC8, 0xD8, 0xC8),
        );
    }
    let _ = lib;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(id: u64, pts: Vec<P>) -> Wire {
        Wire {
            id,
            pts,
            start_pin: None,
            end_pin: None,
        }
    }

    fn near(a: P, b: P) -> bool {
        a.dist(b) < 0.5
    }

    #[test]
    fn flow_marker_follows_the_whole_bent_wire() {
        let pts = [
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 0.0),
            Pos2::new(100.0, 50.0),
        ];
        assert!((polyline_length(&pts) - 150.0).abs() < 0.01);
        assert_eq!(
            point_and_direction_along_polyline(&pts, 25.0).map(|(point, _)| point),
            Some(Pos2::new(25.0, 0.0))
        );
        assert_eq!(
            point_and_direction_along_polyline(&pts, 125.0).map(|(point, _)| point),
            Some(Pos2::new(100.0, 25.0))
        );
    }

    #[test]
    fn connected_wire_uses_the_solved_circuit_current() {
        let lib = Library::builtin();
        let mut proj = Project::new("visible flow");
        let battery = proj
            .add_component(&lib, "BATTERY", P::new(-200.0, 0.0))
            .unwrap();
        let resistor = proj
            .add_component(&lib, "RESISTOR", P::new(0.0, 0.0))
            .unwrap();
        proj.comp_mut(resistor).unwrap().value = "100R".into();
        proj.wires.push(Wire {
            id: 1001,
            pts: vec![P::new(-140.0, 0.0), P::new(-60.0, 0.0)],
            start_pin: Some((battery, 1)),
            end_pin: Some((resistor, 0)),
        });
        proj.wires.push(Wire {
            id: 1002,
            pts: vec![
                P::new(60.0, 0.0),
                P::new(60.0, -100.0),
                P::new(-260.0, -100.0),
                P::new(-260.0, 0.0),
            ],
            start_pin: Some((resistor, 1)),
            end_pin: Some((battery, 0)),
        });
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let current = wire_current(&proj, &lib, &proj.wires[0], &sim).abs();
        assert!(
            (current - 0.09).abs() < 1.0e-3,
            "expected 90 mA, got {current}"
        );
    }

    #[test]
    fn crossing_without_a_terminal_has_no_junction() {
        let mut proj = Project::new("cross");
        proj.wires
            .push(wire(1, vec![P::new(0.0, 0.0), P::new(100.0, 0.0)]));
        proj.wires
            .push(wire(2, vec![P::new(50.0, -50.0), P::new(50.0, 50.0)]));
        assert!(Schematic::junction_points(&proj).is_empty());
    }

    #[test]
    fn bend_alone_has_no_junction() {
        let mut proj = Project::new("bend");
        proj.wires.push(wire(
            1,
            vec![P::new(0.0, 0.0), P::new(50.0, 0.0), P::new(50.0, 50.0)],
        ));
        assert!(Schematic::junction_points(&proj).is_empty());
    }

    #[test]
    fn terminal_on_another_wire_gets_one_junction() {
        let mut proj = Project::new("tee");
        proj.wires
            .push(wire(1, vec![P::new(0.0, 0.0), P::new(100.0, 0.0)]));
        proj.wires
            .push(wire(2, vec![P::new(50.0, 0.0), P::new(50.0, 50.0)]));
        let pts = Schematic::junction_points(&proj);
        assert_eq!(pts.len(), 1);
        assert!(near(pts[0], P::new(50.0, 0.0)));
    }

    #[test]
    fn three_terminals_get_one_junction() {
        let mut proj = Project::new("three");
        proj.wires
            .push(wire(1, vec![P::new(0.0, 0.0), P::new(50.0, 0.0)]));
        proj.wires
            .push(wire(2, vec![P::new(50.0, 50.0), P::new(50.0, 0.0)]));
        proj.wires
            .push(wire(3, vec![P::new(100.0, 0.0), P::new(50.0, 0.0)]));
        let pts = Schematic::junction_points(&proj);
        assert_eq!(pts.len(), 1);
        assert!(near(pts[0], P::new(50.0, 0.0)));
    }

    #[test]
    fn component_hit_uses_the_real_pointer_before_grid_snap() {
        let lib = Library::builtin();
        let mut proj = Project::new("hit");
        let id = proj
            .add_component(&lib, "RESISTOR", P::new(100.0, 100.0))
            .unwrap();
        let raw = P::new(100.0, 119.0);
        let snapped = raw.snap(25.0);

        assert_eq!(hit_component_with_pad(&proj, &lib, raw, 20.0), Some(id));
        assert_eq!(hit_component_with_pad(&proj, &lib, snapped, 20.0), None);
    }

    #[test]
    fn panning_cannot_lose_the_sheet_completely() {
        let mut schematic = Schematic::default();
        let canvas = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 600.0));
        schematic.view.zoom = 0.25;
        schematic.view.off = Vec2::new(-5000.0, 4000.0);

        schematic.keep_sheet_reachable(canvas);

        let sheet = Rect::from_two_pos(
            schematic.view.to_screen(P::new(0.0, 0.0)),
            schematic.view.to_screen(P::new(SHEET_W, SHEET_H)),
        );
        assert!(sheet.intersects(canvas));
    }

    #[test]
    fn single_pin_symbol_body_is_easy_to_select() {
        let lib = Library::builtin();
        let mut proj = Project::new("one pin");
        let id = proj
            .add_component(&lib, "GND", P::new(100.0, 100.0))
            .unwrap();

        assert_eq!(
            hit_component_with_pad(&proj, &lib, P::new(100.0, 150.0), 20.0),
            Some(id)
        );
    }

    #[test]
    fn deleting_component_removes_only_its_connected_wires() {
        let lib = Library::builtin();
        let mut proj = Project::new("delete connected wires");
        let removed = proj
            .add_component(&lib, "RESISTOR", P::new(200.0, 200.0))
            .unwrap();
        let kept_a = proj
            .add_component(&lib, "RESISTOR", P::new(500.0, 200.0))
            .unwrap();
        let kept_b = proj
            .add_component(&lib, "RESISTOR", P::new(800.0, 200.0))
            .unwrap();
        proj.wires.push(Wire {
            id: 1001,
            pts: vec![
                proj.pin_pos(&lib, removed, 1).unwrap(),
                proj.pin_pos(&lib, kept_a, 0).unwrap(),
            ],
            start_pin: Some((removed, 1)),
            end_pin: Some((kept_a, 0)),
        });
        proj.wires.push(Wire {
            id: 1002,
            pts: vec![
                proj.pin_pos(&lib, kept_a, 1).unwrap(),
                proj.pin_pos(&lib, kept_b, 0).unwrap(),
            ],
            start_pin: Some((kept_a, 1)),
            end_pin: Some((kept_b, 0)),
        });

        Schematic::remove_components(&mut proj, &lib, &HashSet::from([removed]));

        assert!(proj.comp(removed).is_none());
        assert_eq!(proj.wires.len(), 1);
        assert_eq!(proj.wires[0].id, 1002);
        assert!(proj.dirty);
    }

    #[test]
    fn repeated_component_moves_keep_attached_wire_small_and_orthogonal() {
        let lib = Library::builtin();
        let mut proj = Project::new("move");
        let moving = proj
            .add_component(&lib, "RESISTOR", P::new(200.0, 200.0))
            .unwrap();
        let fixed = proj
            .add_component(&lib, "RESISTOR", P::new(650.0, 200.0))
            .unwrap();
        let a = proj.pin_pos(&lib, moving, 1).unwrap();
        let b = proj.pin_pos(&lib, fixed, 0).unwrap();
        let wire_id = proj.alloc_id();
        proj.wires.push(Wire {
            id: wire_id,
            pts: vec![a, b],
            start_pin: Some((moving, 1)),
            end_pin: Some((fixed, 0)),
        });
        let moved = HashSet::from([moving]);

        for i in 0..120 {
            let c = proj.comp_mut(moving).unwrap();
            c.x = 200.0 + (i % 12) as f32 * 25.0;
            c.y = 200.0 + (i % 7) as f32 * 25.0;
            Schematic::refresh_attached_wires(&mut proj, &lib, &moved);

            let wire = proj.wires.iter().find(|w| w.id == wire_id).unwrap();
            assert!(
                wire.pts.len() <= 20,
                "wire accumulated {} points",
                wire.pts.len()
            );
            assert!(wire.pts.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
            assert!(wire
                .pts
                .windows(2)
                .all(|s| { (s[0].x - s[1].x).abs() < 0.01 || (s[0].y - s[1].y).abs() < 0.01 }));
            assert!(near(wire.pts[0], proj.pin_pos(&lib, moving, 1).unwrap()));
            assert!(near(
                *wire.pts.last().unwrap(),
                proj.pin_pos(&lib, fixed, 0).unwrap()
            ));
        }
    }

    #[test]
    fn drag_preview_is_stable_and_orthogonal() {
        let lib = Library::builtin();
        let mut proj = Project::new("drag preview");
        let moving = proj
            .add_component(&lib, "RESISTOR", P::new(200.0, 200.0))
            .unwrap();
        let fixed = proj
            .add_component(&lib, "RESISTOR", P::new(650.0, 400.0))
            .unwrap();
        proj.wires.push(Wire {
            id: 1001,
            pts: vec![
                proj.pin_pos(&lib, moving, 1).unwrap(),
                proj.pin_pos(&lib, fixed, 0).unwrap(),
            ],
            start_pin: Some((moving, 1)),
            end_pin: Some((fixed, 0)),
        });
        let moved = HashSet::from([moving]);
        let saved_wire = proj.wires[0].pts.clone();

        for i in 0..40 {
            let component = proj.comp_mut(moving).unwrap();
            component.x = 200.0 + i as f32 * 5.0;
            component.y = 200.0 + (i % 5) as f32 * 10.0;
            let previews = Schematic::attached_wire_previews(&proj, &lib, &moved);
            let preview = previews.get(&1001).unwrap();
            assert!(preview.len() <= 3);
            assert!(preview.windows(2).all(|segment| {
                (segment[0].x - segment[1].x).abs() < 0.01
                    || (segment[0].y - segment[1].y).abs() < 0.01
            }));
            assert_eq!(proj.wires[0].pts, saved_wire);
        }
    }

    #[test]
    fn repeated_component_moves_do_not_keep_shared_generated_bends() {
        let lib = Library::builtin();
        let mut proj = Project::new("two wires from one pin");
        let moving = proj
            .add_component(&lib, "RESISTOR", P::new(200.0, 300.0))
            .unwrap();
        let fixed_a = proj
            .add_component(&lib, "RESISTOR", P::new(650.0, 200.0))
            .unwrap();
        let fixed_b = proj
            .add_component(&lib, "RESISTOR", P::new(650.0, 450.0))
            .unwrap();
        let start = proj.pin_pos(&lib, moving, 1).unwrap();
        let shared_bend = P::new(start.x + WIRE_STUB, start.y);

        for (fixed, id) in [(fixed_a, 1001), (fixed_b, 1002)] {
            let end = proj.pin_pos(&lib, fixed, 0).unwrap();
            proj.wires.push(Wire {
                id,
                pts: vec![start, shared_bend, P::new(end.x, shared_bend.y), end],
                start_pin: Some((moving, 1)),
                end_pin: Some((fixed, 0)),
            });
        }
        let moved = HashSet::from([moving]);

        for i in 0..120 {
            let c = proj.comp_mut(moving).unwrap();
            c.x = 200.0 + (i % 12) as f32 * 25.0;
            c.y = 300.0 + (i % 7) as f32 * 25.0;
            Schematic::refresh_attached_wires(&mut proj, &lib, &moved);

            for wire in &proj.wires {
                assert!(
                    wire.pts.len() <= 20,
                    "wire {} accumulated {} points",
                    wire.id,
                    wire.pts.len()
                );
                assert!(wire
                    .pts
                    .windows(2)
                    .all(|s| { (s[0].x - s[1].x).abs() < 0.01 || (s[0].y - s[1].y).abs() < 0.01 }));
                assert!(near(wire.pts[0], proj.pin_pos(&lib, moving, 1).unwrap()));
            }
        }
    }

    #[test]
    fn component_dropped_on_unattached_wire_routes_wire_around_body() {
        let lib = Library::builtin();
        let mut proj = Project::new("avoid moved component");
        proj.add_component(&lib, "CAP-UF", P::new(300.0, 300.0))
            .unwrap();
        proj.wires
            .push(wire(1001, vec![P::new(100.0, 300.0), P::new(500.0, 300.0)]));

        Schematic::repair_obstructed_wires(&mut proj, &lib);

        let obstacles = Schematic::component_obstacles(&proj, &lib);
        let repaired = proj.wires.iter().find(|wire| wire.id == 1001).unwrap();
        assert!(repaired.pts.len() >= 4);
        assert!(repaired
            .pts
            .windows(2)
            .all(|seg| Schematic::segments_clear(seg[0], seg[1], &obstacles)));
    }

    #[test]
    fn router_detours_instead_of_drawing_over_an_existing_wire() {
        let lib = Library::builtin();
        let mut proj = Project::new("avoid overlap");
        proj.wires
            .push(wire(1001, vec![P::new(100.0, 300.0), P::new(500.0, 300.0)]));

        let route = Schematic::route_with_options(
            &proj,
            &lib,
            P::new(100.0, 300.0),
            P::new(500.0, 300.0),
            true,
            None,
        );
        let existing = Schematic::wire_segments(&proj, None);

        assert!(route.len() >= 4);
        assert!(route
            .windows(2)
            .all(|seg| Schematic::overlap_length(seg[0], seg[1], &existing) < 0.5));
    }
}
