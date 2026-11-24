// PCB Layout editor: layers, footprints, routing, vias, DRC markers.

use std::collections::HashSet;

use eframe::egui::{
    self, Color32, FontFamily, FontId, MouseWheelUnit, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Vec2,
};

use crate::model::{
    point_seg_dist, Board, Footprint, Layer, Pad, PadShape, Project, Track, Via, P,
};
use crate::theme::*;

const ZOOM_MIN: f32 = 0.08;
const ZOOM_MAX: f32 = 6.0;
const WHEEL_LINE_FACTOR: f32 = 0.18;
const WHEEL_POINTS_PER_LINE: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcbTool {
    Select,
    Track,
    Via,
    Board,
    Delete,
}

impl PcbTool {
    pub fn label(self) -> &'static str {
        match self {
            PcbTool::Select => "Select",
            PcbTool::Track => "Track",
            PcbTool::Via => "Via",
            PcbTool::Board => "Board Outline",
            PcbTool::Delete => "Delete",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PcbView {
    pub zoom: f32,
    pub off: Vec2,
}

impl Default for PcbView {
    fn default() -> Self {
        PcbView {
            zoom: 0.35,
            off: Vec2::new(90.0, 60.0),
        }
    }
}

impl PcbView {
    pub fn to_screen(&self, p: P) -> Pos2 {
        Pos2::new(self.off.x + p.x * self.zoom, self.off.y + p.y * self.zoom)
    }
    pub fn to_world(&self, s: Pos2) -> P {
        P::new(
            (s.x - self.off.x) / self.zoom,
            (s.y - self.off.y) / self.zoom,
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum PcbDrag {
    MoveFootprint { id: u64, last: P },
    ZoomBox { from: P },
}

pub struct Pcb {
    /// Fit the board into the view on the next paint.
    pub needs_fit: bool,
    pub view: PcbView,
    pub tool: PcbTool,
    pub active_layer: Layer,
    pub visible: HashSet<Layer>,
    pub sel_fp: HashSet<u64>,
    pub sel_track: HashSet<u64>,
    pub draft: Vec<P>,
    pub mouse: P,
    pub hint: String,
    pub show_ratsnest: bool,
    pub show_drc: bool,
    drag: Option<PcbDrag>,
    draft_start: Option<P>,
    pub pan_mode: bool,
    pan_last: Option<Pos2>,
}

impl Default for Pcb {
    fn default() -> Self {
        let mut visible = HashSet::new();
        for l in Layer::all() {
            visible.insert(l);
        }
        Pcb {
            needs_fit: true,
            view: PcbView::default(),
            tool: PcbTool::Select,
            active_layer: Layer::TopCopper,
            visible,
            sel_fp: HashSet::new(),
            sel_track: HashSet::new(),
            draft: Vec::new(),
            mouse: P::ZERO,
            hint: "Ready".into(),
            show_ratsnest: true,
            show_drc: true,
            drag: None,
            draft_start: None,
            pan_mode: false,
            pan_last: None,
        }
    }
}

impl Pcb {
    pub fn clear_selection(&mut self) {
        self.sel_fp.clear();
        self.sel_track.clear();
    }

    pub fn selected_count(&self) -> usize {
        self.sel_fp.len() + self.sel_track.len()
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

    fn zoom_to_fit(&mut self, rect: Rect, board: &Board) {
        let margin = 80.0;
        let zoom = (rect.width() / (board.width + margin))
            .min(rect.height() / (board.height + margin))
            .clamp(ZOOM_MIN, ZOOM_MAX);
        let centre = board
            .origin
            .add(P::new(board.width * 0.5, board.height * 0.5));
        self.view.zoom = zoom;
        self.view.off = Vec2::new(
            rect.center().x - centre.x * zoom,
            rect.center().y - centre.y * zoom,
        );
    }

    fn zoom_to_world_rect(&mut self, rect: Rect, a: P, b: P) {
        let width = (b.x - a.x).abs().max(1.0);
        let height = (b.y - a.y).abs().max(1.0);
        let zoom = (rect.width() / width)
            .min(rect.height() / height)
            .clamp(ZOOM_MIN, ZOOM_MAX);
        self.view.zoom = zoom;
        self.view.off = Vec2::new(
            rect.center().x - (a.x + b.x) * 0.5 * zoom,
            rect.center().y - (a.y + b.y) * 0.5 * zoom,
        );
        self.hint = format!("Zoom {:.0}%", zoom * 100.0);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, proj: &mut Project) -> String {
        let desired = ui.available_size_before_wrap();
        let (rect, resp) = ui.allocate_exact_size(desired, Sense::click_and_drag());
        if rect.width() < 10.0 || rect.height() < 10.0 {
            return self.hint.clone();
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, CANVAS);

        let hover = resp.hover_pos();
        if let Some(h) = hover {
            self.mouse = self.view.to_world(h);
            if !self.pan_mode {
                use egui::CursorIcon as C;
                ui.output_mut(|o| {
                    o.cursor_icon = match self.tool {
                        PcbTool::Select => C::Default,
                        PcbTool::Track | PcbTool::Board => C::Crosshair,
                        PcbTool::Via => C::Cell,
                        PcbTool::Delete => C::NotAllowed,
                    }
                });
            }
        }

        // Match the schematic canvas: use raw wheel events for instant,
        // cursor-centred zoom and Shift+wheel for horizontal pan.
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
                    self.zoom_step(h, factor);
                }
            }
        }

        // A middle click enters track-pan mode; middle-drag and Ctrl-drag pan
        // temporarily without changing the active PCB tool.
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
                || (resp.dragged_by(egui::PointerButton::Primary) && ctrl);
            if panning {
                self.view.off += resp.drag_delta();
            }
        }

        let step = proj.grid.step();
        let snap_on = proj.grid.snap_on;
        let snap = |p: P| {
            if snap_on {
                p.snap(step)
            } else {
                p
            }
        };
        self.mouse = snap(self.mouse);

        let mut esc = false;
        let mut del = false;
        let mut rot = false;
        let mut fit = false;
        let mut zoom_in = false;
        let mut zoom_out = false;
        let mut centre = false;
        let typing = ui.ctx().egui_wants_keyboard_input();
        ui.input(|i| {
            if typing {
                return;
            }
            esc = i.key_pressed(egui::Key::Escape);
            del = i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace);
            rot = i.key_pressed(egui::Key::R);
            fit = i.key_pressed(egui::Key::F8);
            zoom_in = i.key_pressed(egui::Key::F6);
            zoom_out = i.key_pressed(egui::Key::F7);
            centre = i.key_pressed(egui::Key::F5);
        });
        if esc {
            self.draft.clear();
            self.draft_start = None;
            if self.tool != PcbTool::Select {
                self.tool = PcbTool::Select;
                self.hint = "Tool: Select".into();
            }
        }

        if fit || self.needs_fit {
            self.needs_fit = false;
            self.zoom_to_fit(rect, &proj.board);
            self.hint = "Zoom to whole board".into();
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

        if resp.clicked() && !self.pan_mode {
            if let Some(h) = hover {
                let w = snap(self.view.to_world(h));
                match self.tool {
                    PcbTool::Track => {
                        if self.draft.is_empty() {
                            self.draft.push(w);
                        } else {
                            let last = *self.draft.last().unwrap();
                            if last.dist(w) > 1.0 {
                                self.draft.push(w);
                            }
                        }
                    }
                    PcbTool::Via => {
                        let id = proj.alloc_id();
                        let net = self.net_near(proj, w);
                        proj.vias.push(Via {
                            id,
                            pos: w,
                            size: 60.0,
                            drill: 30.0,
                            net,
                        });
                        proj.dirty = true;
                        self.hint = "Via placed".into();
                    }
                    PcbTool::Board => {
                        if self.draft_start.is_none() {
                            self.draft_start = Some(w);
                            self.hint = "Board outline: click the opposite corner".into();
                        } else {
                            let a = self.draft_start.unwrap();
                            let b = w;
                            proj.board = Board {
                                origin: P::new(a.x.min(b.x), a.y.min(b.y)),
                                width: (b.x - a.x).abs().max(100.0),
                                height: (b.y - a.y).abs().max(100.0),
                            };
                            proj.dirty = true;
                            self.draft_start = None;
                            self.hint = "Board outline updated".into();
                        }
                    }
                    PcbTool::Delete => {
                        if let Some(t) = self.hit_track(proj, w) {
                            proj.tracks.retain(|x| x.id != t);
                            proj.dirty = true;
                            self.hint = "Track deleted".into();
                        } else if let Some(f) = self.hit_fp(proj, w) {
                            proj.fps.retain(|x| x.id != f);
                            proj.dirty = true;
                            self.hint = "Footprint deleted".into();
                        } else if let Some(v) = self.hit_via(proj, w) {
                            proj.vias.retain(|x| x.id != v);
                            proj.dirty = true;
                            self.hint = "Via deleted".into();
                        }
                    }
                    PcbTool::Select => {
                        if !ui.input(|i| i.modifiers.shift) {
                            self.clear_selection();
                        }
                        if let Some(t) = self.hit_track(proj, w) {
                            self.sel_track.insert(t);
                        } else if let Some(f) = self.hit_fp(proj, w) {
                            self.sel_fp.insert(f);
                        }
                    }
                }
            }
        }

        if resp.double_clicked() && self.tool == PcbTool::Track && self.draft.len() >= 2 {
            self.commit_track(proj);
        }
        if resp.secondary_clicked() && self.tool == PcbTool::Track {
            if self.draft.len() >= 2 {
                self.commit_track(proj);
            } else {
                self.draft.clear();
            }
        }

        if self.tool == PcbTool::Track && self.draft.len() == 1 {
            if let Some(h) = hover {
                let w = snap(self.view.to_world(h));
                let last = *self.draft.last().unwrap();
                let d = w.sub(last);
                // orthogonal hint
                let guided = if d.x.abs() > d.y.abs() {
                    P::new(w.x, last.y)
                } else {
                    P::new(last.x, w.y)
                };
                self.mouse = guided;
            }
        }

        if self.tool == PcbTool::Select
            && !self.pan_mode
            && !ctrl
            && resp.drag_started_by(egui::PointerButton::Primary)
        {
            if let Some(h) = hover {
                let w = snap(self.view.to_world(h));
                if shift {
                    self.drag = Some(PcbDrag::ZoomBox { from: w });
                    self.hint = "Zoom box: release to zoom in".into();
                } else if self.hit_fp(proj, w).is_none() {
                    self.clear_selection();
                } else if let Some(f) = self.hit_fp(proj, w) {
                    self.sel_fp.insert(f);
                    self.drag = Some(PcbDrag::MoveFootprint { id: f, last: w });
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let (Some(PcbDrag::MoveFootprint { id, last }), Some(h)) = (self.drag, hover) {
                let cur = snap(self.view.to_world(h));
                let delta = cur.sub(last);
                if let Some(f) = proj.fps.iter_mut().find(|f| f.id == id) {
                    let np = P::new(f.x, f.y).add(delta).snap(step);
                    f.x = np.x;
                    f.y = np.y;
                }
                if delta.x != 0.0 || delta.y != 0.0 {
                    proj.dirty = true;
                    self.drag = Some(PcbDrag::MoveFootprint { id, last: cur });
                }
            }
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some(PcbDrag::ZoomBox { from }) = self.drag {
                let to = hover
                    .map(|h| snap(self.view.to_world(h)))
                    .unwrap_or(self.mouse);
                if (to.x - from.x).abs() > 2.0 || (to.y - from.y).abs() > 2.0 {
                    self.zoom_to_world_rect(rect, from, to);
                }
            }
            self.drag = None;
        }

        if rot && !self.sel_fp.is_empty() {
            for id in self.sel_fp.clone() {
                if let Some(f) = proj.fps.iter_mut().find(|f| f.id == id) {
                    f.rot = (f.rot + 90).rem_euclid(360);
                }
            }
            proj.dirty = true;
            self.hint = "Footprint rotated".into();
        }
        if del && self.selected_count() > 0 {
            let f = self.sel_fp.clone();
            let t = self.sel_track.clone();
            proj.fps.retain(|x| !f.contains(&x.id));
            proj.tracks.retain(|x| !t.contains(&x.id));
            if !f.is_empty() || !t.is_empty() {
                proj.dirty = true;
            }
            self.clear_selection();
            self.hint = "Deleted selection".into();
        }

        // ------------------------------------------------------------ draw
        let v = self.view;
        let scr = |p: P| v.to_screen(p);

        // grid
        // dot_spacing() returns screen pixels. Stepping by it in world units
        // made a zoomed-out board allocate millions of shapes per frame and
        // freeze the UI. Convert to world units and cap the count, as the
        // schematic view already does.
        let spacing_px = proj.grid.dot_spacing(v.zoom);
        if proj.grid.show_dots && spacing_px >= 6.0 {
            let world_step = spacing_px / v.zoom;
            let tl = v.to_world(rect.left_top());
            let br = v.to_world(rect.right_bottom());
            let cols = ((br.x - tl.x) / world_step).ceil() as usize;
            let rows = ((br.y - tl.y) / world_step).ceil() as usize;
            if cols > 0 && rows > 0 && cols.saturating_mul(rows) <= 14000 {
                let mut shapes: Vec<Shape> = Vec::with_capacity(cols * rows);
                let mut x = (tl.x / world_step).floor() * world_step;
                while x <= br.x {
                    let mut y = (tl.y / world_step).floor() * world_step;
                    while y <= br.y {
                        shapes.push(Shape::circle_filled(scr(P::new(x, y)), 1.0, GRID_DOT));
                        y += world_step;
                    }
                    x += world_step;
                }
                painter.add(Shape::Vec(shapes));
            }
        }

        // board outline
        let (o, e) = proj.board.rect();
        let br = Rect::from_two_pos(scr(o), scr(e));
        if self.visible.contains(&Layer::Board) {
            painter.rect_stroke(br, 0.0, Stroke::new(2.0, BOARD_EDGE), StrokeKind::Middle);
        }

        // ratsnest
        if self.show_ratsnest {
            self.draw_ratsnest(&painter, proj, &scr);
        }

        // tracks
        for t in &proj.tracks {
            if !self.visible.contains(&t.layer) {
                continue;
            }
            let sel = self.sel_track.contains(&t.id);
            let col = if sel { SELECTION } else { layer_color(t.layer) };
            let w = (t.width * v.zoom).max(1.0);
            painter.add(Shape::line(vec![scr(t.a), scr(t.b)], Stroke::new(w, col)));
        }

        // vias
        for via in &proj.vias {
            let c = scr(via.pos);
            let r = (via.size * 0.5 * v.zoom).max(2.0);
            painter.circle_filled(c, r, VIA_COLOR);
            painter.circle_filled(c, (via.drill * 0.5 * v.zoom).max(1.0), CANVAS);
        }

        // footprints
        for f in &proj.fps {
            self.draw_footprint(&painter, proj, f, scr, v);
        }

        // draft track
        if self.draft.len() >= 1 {
            let mut pts: Vec<Pos2> = self.draft.iter().map(|p| scr(*p)).collect();
            pts.push(scr(self.mouse));
            painter.add(Shape::line(pts, Stroke::new(2.0, SELECTION)));
            for p in &self.draft {
                painter.circle_filled(scr(*p), 3.0, SELECTION);
            }
            self.hint = format!(
                "Track: {} point(s) on {} - double-click or Enter to finish",
                self.draft.len(),
                self.active_layer.name()
            );
        }
        if let Some(a) = self.draft_start {
            let r = Rect::from_two_pos(scr(a), scr(self.mouse));
            painter.rect_stroke(r, 0.0, Stroke::new(2.0, BOARD_EDGE), StrokeKind::Middle);
        }

        if let Some(PcbDrag::ZoomBox { from }) = self.drag {
            if let Some(h) = hover {
                let r = Rect::from_two_pos(scr(from), h);
                painter.rect_filled(r, 0.0, Color32::from_rgba_premultiplied(60, 180, 120, 55));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, OK), StrokeKind::Middle);
            }
        }

        // DRC markers
        if self.show_drc {
            let mut n = 0;
            for v in proj.drc() {
                let c = scr(v.pos);
                painter.add(Shape::convex_polygon(
                    vec![
                        c + Vec2::new(0.0, -7.0),
                        c + Vec2::new(7.0, 6.0),
                        c + Vec2::new(-7.0, 6.0),
                    ],
                    Color32::from_rgba_premultiplied(255, 40, 40, 200),
                    Stroke::new(1.0, Color32::WHITE),
                ));
                n += 1;
            }
            if n > 0 && self.hint == "Ready" {
                self.hint = format!("{n} design rule violation(s)");
            }
        }

        // coordinate readout
        if hover.is_some() {
            let txt = format!(
                "X: {}   Y: {}   {}",
                crate::theme::format_th(self.mouse.x),
                crate::theme::format_th(self.mouse.y),
                self.active_layer.name()
            );
            let pos = Pos2::new(rect.left() + 8.0, rect.bottom() - 20.0);
            painter.rect_filled(
                Rect::from_min_size(pos - Vec2::new(4.0, 2.0), Vec2::new(260.0, 18.0)),
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

        self.hint.clone()
    }

    fn net_near(&self, proj: &Project, w: P) -> Option<String> {
        let mut best: Option<(f32, String)> = None;
        for f in &proj.fps {
            for pad in &f.pads {
                let d = pad.pos.add(P::new(f.x, f.y)).dist(w);
                if let Some(net) = &pad.net {
                    if d < 200.0 && best.as_ref().map(|(bd, _)| d < *bd).unwrap_or(true) {
                        best = Some((d, net.clone()));
                    }
                }
            }
        }
        best.map(|(_, n)| n)
    }

    fn hit_track(&self, proj: &Project, w: P) -> Option<u64> {
        let mut best: Option<(u64, f32)> = None;
        for t in &proj.tracks {
            let d = point_seg_dist(w, t.a, t.b) - t.width * 0.5;
            if d < 25.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((t.id, d));
            }
        }
        best.map(|(id, _)| id)
    }

    fn hit_fp(&self, proj: &Project, w: P) -> Option<u64> {
        let mut best: Option<(u64, f32)> = None;
        for f in &proj.fps {
            for pad in &f.pads {
                let wp = absolute_pad(f, pad);
                let d = wp.dist(w) - pad.size * 0.5;
                if d < 30.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((f.id, d));
                }
            }
            if P::new(f.x, f.y).dist(w) < 60.0 {
                best = Some((f.id, 0.0));
            }
        }
        best.map(|(id, _)| id)
    }

    fn hit_via(&self, proj: &Project, w: P) -> Option<u64> {
        proj.vias
            .iter()
            .filter(|v| v.pos.dist(w) < 45.0)
            .map(|v| v.id)
            .next()
    }

    fn commit_track(&mut self, proj: &mut Project) {
        if self.draft.len() >= 2 {
            let pts = self.draft.clone();
            let net = self.net_near(proj, pts[0]);
            for seg in pts.windows(2) {
                if seg[0].dist(seg[1]) < 1.0 {
                    continue;
                }
                let id = proj.alloc_id();
                proj.tracks.push(Track {
                    id,
                    a: seg[0],
                    b: seg[1],
                    width: 25.0,
                    layer: self.active_layer,
                    net: net.clone(),
                });
            }
            proj.dirty = true;
            self.hint = "Track routed".into();
        }
        self.draft.clear();
    }

    fn draw_ratsnest(&self, painter: &egui::Painter, proj: &Project, scr: &impl Fn(P) -> Pos2) {
        // Only connections that copper does not yet make.
        let shapes: Vec<Shape> = crate::autoroute::open_links(proj)
            .into_iter()
            .map(|(a, b)| {
                Shape::line(
                    vec![scr(a), scr(b)],
                    Stroke::new(1.0, Color32::from_rgba_premultiplied(120, 200, 120, 90)),
                )
            })
            .collect();
        painter.add(Shape::Vec(shapes));
    }

    fn draw_footprint(
        &self,
        painter: &egui::Painter,
        proj: &Project,
        f: &Footprint,
        scr: impl Fn(P) -> Pos2,
        _v: PcbView,
    ) {
        let selected = self.sel_fp.contains(&f.id);
        let rot = f.rot;
        let origin = P::new(f.x, f.y);

        // silk body
        if self.visible.contains(&Layer::TopSilk) {
            let pads: Vec<P> = f.pads.iter().map(|p| origin.add(p.pos.rot(rot))).collect();
            if let Some((lo, hi)) = crate::model::bbox(&pads, 30.0) {
                let a = scr(lo);
                let b = scr(hi);
                let rr = Rect::from_two_pos(a, b);
                painter.rect_stroke(
                    rr,
                    0.0,
                    Stroke::new(1.0, if selected { SELECTION } else { SILK_TOP }),
                    StrokeKind::Middle,
                );
            }
            let ref_pos = scr(origin.add(P::new(0.0, -150.0).rot(rot)));
            painter.text(
                ref_pos,
                egui::Align2::CENTER_BOTTOM,
                f.refdes.clone(),
                FontId::new(13.0, FontFamily::Proportional),
                if selected { SELECTION } else { SILK_TOP },
            );
        }

        for pad in &f.pads {
            let wp = absolute_pad(f, pad);
            let c = scr(wp);
            let r = (pad.size * 0.5 * _v.zoom).max(2.0);
            let copper = if self.visible.contains(&Layer::TopCopper) {
                layer_color(Layer::TopCopper)
            } else {
                CANVAS
            };
            match pad.shape {
                PadShape::Circle => {
                    painter.circle_filled(c, r, copper);
                }
                PadShape::Square => {
                    painter.rect_filled(
                        Rect::from_center_size(c, Vec2::new(r * 2.0, r * 2.0)),
                        0.0,
                        copper,
                    );
                }
                PadShape::Octagon => {
                    let mut pts = Vec::new();
                    for i in 0..8 {
                        let a = std::f32::consts::PI / 8.0 + i as f32 * std::f32::consts::PI / 4.0;
                        pts.push(c + Vec2::new(a.cos() * r * 1.08, a.sin() * r * 1.08));
                    }
                    painter.add(Shape::convex_polygon(pts, copper, Stroke::NONE));
                }
            }
            let drill = (pad.drill * 0.5 * _v.zoom).max(0.0);
            if drill > 0.5 {
                painter.circle_filled(c, drill, CANVAS);
            }
            if selected {
                painter.circle_stroke(c, r + 2.0, Stroke::new(1.0, SELECTION));
            }
        }

        // value text on bottom silk
        if self.visible.contains(&Layer::BottomSilk) {
            let _ = proj;
        }
    }
}

pub fn absolute_pad(f: &Footprint, pad: &Pad) -> P {
    P::new(f.x, f.y).add(pad.pos.rot(f.rot))
}

pub fn layer_color(l: Layer) -> Color32 {
    match l {
        Layer::TopCopper => COPPER_TOP,
        Layer::BottomCopper => COPPER_BOTTOM,
        Layer::TopSilk => SILK_TOP,
        Layer::BottomSilk => SILK_BOTTOM,
        Layer::TopMask => MASK_TOP,
        Layer::BottomMask => MASK_BOTTOM,
        Layer::Board => BOARD_EDGE,
        Layer::Drill => DRILL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcb_zoom_keeps_pointer_over_same_board_position() {
        let mut pcb = Pcb::default();
        let pointer = Pos2::new(480.0, 320.0);
        let before = pcb.view.to_world(pointer);

        pcb.zoom_step(pointer, 1.25);

        let after = pcb.view.to_world(pointer);
        assert!((after.x - before.x).abs() < 0.001);
        assert!((after.y - before.y).abs() < 0.001);
    }

    #[test]
    fn pcb_fit_centres_the_board_in_the_canvas() {
        let mut pcb = Pcb::default();
        let board = Board {
            origin: P::new(100.0, 200.0),
            width: 2000.0,
            height: 1500.0,
        };
        let canvas = Rect::from_min_size(Pos2::new(20.0, 30.0), Vec2::new(1000.0, 700.0));

        pcb.zoom_to_fit(canvas, &board);

        let centre = board
            .origin
            .add(P::new(board.width * 0.5, board.height * 0.5));
        let screen = pcb.view.to_screen(centre);
        assert!((screen.x - canvas.center().x).abs() < 0.001);
        assert!((screen.y - canvas.center().y).abs() < 0.001);
    }
}
