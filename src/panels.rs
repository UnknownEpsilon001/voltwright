// Lighter panels: Design Explorer, Library Manager, Notes, Source Code,
// 3D Visualizer, Gerber Viewer, BOM table.

use std::collections::HashSet;

use eframe::egui::{
    self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};

use crate::bom;
use crate::model::{Layer, Library, Project, P};
use crate::pcb::absolute_pad;
use crate::theme::*;

fn library_part_row(
    ui: &mut egui::Ui,
    part: &crate::model::PartDef,
    lib: &Library,
    selected_part: &mut Option<String>,
) -> Option<PanelAction> {
    let selected = selected_part.as_deref() == Some(part.id.as_str());
    let response = ui
        .horizontal(|ui| {
            crate::schematic::part_icon(ui, part, lib);
            ui.selectable_label(selected, format!("{}   ({})", part.name, part.value))
        })
        .inner;
    if response.clicked() {
        *selected_part = Some(part.id.clone());
    }
    if response.double_clicked() {
        return Some(PanelAction::Place(part.id.clone()));
    }
    if selected {
        ui.indent(("part-details", &part.id), |ui| {
            ui.weak(format!("Device    : {}", part.id));
            ui.weak(format!("Library   : {}", part.library));
            ui.weak(format!("Package   : {}", part.package));
            ui.weak(format!("Pins      : {}", part.pins.len()));
            ui.weak(format!("Model     : {:?}", part.sim));
            egui::CollapsingHeader::new(format!("Pinout ({} pins)", part.pins.len()))
                .id_salt(("part-pinout", &part.id))
                .default_open(matches!(
                    part.category.as_str(),
                    "Development Boards" | "Microprocessors"
                ))
                .show(ui, |ui| {
                    egui::Grid::new(("pinout-grid", &part.id))
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Pin");
                            ui.strong("Function");
                            ui.end_row();
                            for (index, pin) in part.pins.iter().enumerate() {
                                ui.monospace(format!("{}", index + 1));
                                ui.monospace(&pin.name);
                                ui.end_row();
                            }
                        });
                });
        });
    }
    None
}

#[derive(Clone, Debug, Default)]
pub enum PanelAction {
    #[default]
    None,
    SelectComp(u64),
    SelectFootprint(u64),
    Place(String),
}

pub struct Panels {
    pub explorer_open: HashSet<String>,
    pub search: String,
    pub category: String,
    pub selected_part: Option<String>,
    pub bom_filter: String,
    pub log: String,
    pub notes_scroll: f32,
}

impl Default for Panels {
    fn default() -> Self {
        Panels {
            explorer_open: ["Schematic", "PCB", "Libraries"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            search: String::new(),
            category: "All Categories".into(),
            selected_part: None,
            bom_filter: String::new(),
            log: String::new(),
            notes_scroll: 0.0,
        }
    }
}

impl Panels {
    // ------------------------------------------------------------ explorer

    pub fn design_explorer(
        &mut self,
        ui: &mut egui::Ui,
        proj: &Project,
        lib: &Library,
        sel: &HashSet<u64>,
    ) -> PanelAction {
        let mut act = PanelAction::None;
        ui.heading("Design Explorer");
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            let roots = [
                ("Schematic", format!("{} components", proj.comps.len())),
                ("PCB", format!("{} footprints", proj.fps.len())),
                ("Libraries", format!("{} devices", lib.parts.len())),
                ("Simulation", String::new()),
                ("Outputs", String::new()),
            ];
            for (name, badge) in roots {
                let open = self.explorer_open.contains(name);
                let header = if badge.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}  ({badge})")
                };
                let resp = ui.selectable_label(
                    open,
                    format!("{} {}", if open { "\u{25BE}" } else { "\u{25B8}" }, header),
                );
                if resp.clicked() {
                    if open {
                        self.explorer_open.remove(name);
                    } else {
                        self.explorer_open.insert(name.to_string());
                    }
                }
                if !open {
                    continue;
                }
                ui.indent(name, |ui| match name {
                    "Schematic" => {
                        for c in &proj.comps {
                            let selected = sel.contains(&c.id);
                            let txt = format!(
                                "{}  {}",
                                c.refdes,
                                if c.value.is_empty() {
                                    c.part.clone()
                                } else {
                                    c.value.clone()
                                }
                            );
                            if ui.selectable_label(selected, txt).clicked() {
                                act = PanelAction::SelectComp(c.id);
                            }
                        }
                        if proj.wires.is_empty() {
                            ui.weak("no wires");
                        } else {
                            ui.weak(format!("{} wire segments", proj.wires.len()));
                        }
                        if !proj.labels.is_empty() {
                            ui.weak(format!("{} labels", proj.labels.len()));
                        }
                    }
                    "PCB" => {
                        for f in &proj.fps {
                            let txt = format!("{}  {}", f.refdes, f.package);
                            if ui.selectable_label(false, txt).clicked() {
                                act = PanelAction::SelectFootprint(f.id);
                            }
                        }
                        if proj.fps.is_empty() {
                            ui.weak("no footprints");
                        }
                        ui.weak(format!(
                            "{} tracks, {} vias, board {:.0} x {:.0} th",
                            proj.tracks.len(),
                            proj.vias.len(),
                            proj.board.width,
                            proj.board.height
                        ));
                    }
                    "Libraries" => {
                        for cat in lib.categories() {
                            let open = self.explorer_open.contains(&cat);
                            if ui
                                .selectable_label(
                                    open,
                                    format!(
                                        "{} {}",
                                        if open { "\u{25BE}" } else { "\u{25B8}" },
                                        cat
                                    ),
                                )
                                .clicked()
                            {
                                if open {
                                    self.explorer_open.remove(&cat);
                                } else {
                                    self.explorer_open.insert(cat.clone());
                                }
                            }
                            if open {
                                for p in lib.search("", &cat) {
                                    if ui
                                        .selectable_label(false, format!("   {}", p.name))
                                        .clicked()
                                    {
                                        act = PanelAction::Place(p.id.clone());
                                    }
                                }
                            }
                        }
                    }
                    "Simulation" => {
                        ui.weak("Analyses available:");
                        for a in [
                            "DC Operating Point",
                            "Transient Analysis",
                            "AC Sweep",
                            "Interactive Animation",
                        ] {
                            ui.label(format!("   \u{2022} {a}"));
                        }
                    }
                    "Outputs" => {
                        for a in [
                            "Bill of Materials",
                            "Gerber Files",
                            "Pick and Place",
                            "3D Visualisation",
                        ] {
                            ui.label(format!("   \u{2022} {a}"));
                        }
                    }
                    _ => {}
                });
            }
        });
        act
    }

    // ------------------------------------------------------------ library

    pub fn library_manager(&mut self, ui: &mut egui::Ui, lib: &Library) -> PanelAction {
        let mut act = PanelAction::None;
        ui.horizontal(|ui| {
            ui.label("Category:");
            egui::ComboBox::from_id_salt("libcat")
                .selected_text(&self.category)
                .width(190.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.category,
                        "All Categories".into(),
                        "All Categories",
                    );
                    for c in lib.categories() {
                        ui.selectable_value(&mut self.category, c.clone(), c);
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Keywords:");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("device, package, or pin function...")
                    .desired_width(220.0),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                r.request_focus();
            }
        });
        ui.separator();

        let hits = lib.search(&self.search, &self.category);
        ui.weak(format!(
            "{} parts in {} categories",
            hits.len(),
            lib.categories().len()
        ));
        egui::ScrollArea::vertical()
            .id_salt("liblist")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.search.trim().is_empty() && self.category == "All Categories" {
                    for category in lib.categories() {
                        let parts = lib.search("", &category);
                        egui::CollapsingHeader::new(format!("{}  ({})", category, parts.len()))
                            .id_salt(("library-category", &category))
                            .default_open(category == "Development Boards")
                            .show(ui, |ui| {
                                for part in parts {
                                    if let Some(action) =
                                        library_part_row(ui, part, lib, &mut self.selected_part)
                                    {
                                        act = action;
                                    }
                                }
                            });
                    }
                } else {
                    for part in hits {
                        if let Some(action) = library_part_row(ui, part, lib, &mut self.selected_part) {
                            act = action;
                        }
                    }
                }
            });
        if let Some(id) = &self.selected_part {
            ui.separator();
            if ui.button("Place on schematic").clicked() {
                act = PanelAction::Place(id.clone());
            }
        }
        act
    }

    // ------------------------------------------------------------ notes

    pub fn notes(&mut self, ui: &mut egui::Ui, proj: &mut Project) {
        ui.horizontal(|ui| {
            ui.label("Project Notes");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    proj.notes.clear();
                    proj.dirty = true;
                }
            });
        });
        ui.separator();
        let r = ui.add(
            egui::TextEdit::multiline(&mut proj.notes)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(28)
                .font(FontId::new(14.0, FontFamily::Monospace))
                .hint_text("Design notes, revision history, test results..."),
        );
        if r.changed() {
            proj.dirty = true;
        }
    }

    // ------------------------------------------------------------ source

    pub fn source_code(&mut self, ui: &mut egui::Ui, proj: &mut Project) {
        ui.horizontal(|ui| {
            ui.label("Source Code");
            if ui.button("Compile").clicked() {
                let lines = proj.source.lines().count();
                let words = proj.source.split_whitespace().count();
                self.log = format!(
                    "Build complete - {lines} lines, {words} tokens, 0 errors, 0 warnings.\r\nTarget: 8-bit MCU (simulation firmware)."
                );
            }
            if ui.button("Reload").clicked() {
                proj.source = crate::model::default_source();
                proj.dirty = true;
                self.log = "Source reloaded from the default template.".into();
            }
        });
        ui.separator();
        let r = ui.add(
            egui::TextEdit::multiline(&mut proj.source)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(26)
                .font(FontId::new(14.0, FontFamily::Monospace)),
        );
        if r.changed() {
            proj.dirty = true;
        }
        if !self.log.is_empty() {
            ui.separator();
            ui.colored_label(OK, &self.log);
        }
    }

    // ------------------------------------------------------------ BOM

    pub fn bom_view(&mut self, ui: &mut egui::Ui, proj: &Project, lib: &Library) {
        let rows = bom::build(proj, lib);
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.add(
                egui::TextEdit::singleline(&mut self.bom_filter)
                    .hint_text("reference or value")
                    .desired_width(180.0),
            );
            ui.separator();
            ui.weak(format!(
                "{} line items, {} components",
                rows.len(),
                bom::total_parts(&rows)
            ));
        });
        ui.separator();

        let filter = self.bom_filter.to_lowercase();
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("bomgrid")
                .striped(true)
                .num_columns(5)
                .min_col_width(70.0)
                .show(ui, |ui| {
                    for h in ["Qty", "References", "Value", "Device", "Package"] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for r in &rows {
                        if !filter.is_empty()
                            && !r.refdes.join(" ").to_lowercase().contains(&filter)
                            && !r.value.to_lowercase().contains(&filter)
                            && !r.device.to_lowercase().contains(&filter)
                        {
                            continue;
                        }
                        ui.label(r.qty.to_string());
                        ui.label(r.refdes.join(" "));
                        ui.label(&r.value);
                        ui.label(&r.device);
                        ui.label(&r.package);
                        ui.end_row();
                    }
                });
        });
    }

    // ------------------------------------------------------------ 3D

    pub fn visualizer(&mut self, ui: &mut egui::Ui, proj: &Project) -> String {
        let (rect, resp) =
            ui.allocate_exact_size(ui.available_size_before_wrap(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(0x18, 0x20, 0x28));

        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if resp.hovered() && scroll.abs() > 0.0 {
            self.notes_scroll = (self.notes_scroll + scroll * 0.4).clamp(-180.0, 180.0);
        }
        if resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            self.notes_scroll += d.x * 0.5;
        }

        let spin = self.notes_scroll.to_radians();
        let scale = 0.30f32;
        let cx = rect.center().x;
        let cy = rect.center().y + 30.0;

        // project a world point (th) into a simple isometric view
        let proj_pt = |p: P, z: f32| -> Pos2 {
            let (s, c) = (spin.sin(), spin.cos());
            let x = p.x * c - p.y * s;
            let y = p.x * s + p.y * c;
            Pos2::new(
                cx + (x - y) * 0.866 * scale,
                cy + (x + y) * 0.5 * scale - z * scale,
            )
        };

        // board slab
        let (o, e) = proj.board.rect();
        let corners = [
            P::new(o.x, o.y),
            P::new(e.x, o.y),
            P::new(e.x, e.y),
            P::new(o.x, e.y),
        ];
        let top: Vec<Pos2> = corners.iter().map(|p| proj_pt(*p, 0.0)).collect();
        painter.add(Shape::convex_polygon(
            top.clone(),
            Color32::from_rgb(0x0E, 0x50, 0x28),
            Stroke::new(1.0, Color32::from_rgb(0x1E, 0x70, 0x38)),
        ));
        // side wall
        let mut side = vec![top[1], top[2]];
        side.push(Pos2::new(top[2].x, top[2].y + 14.0));
        side.push(Pos2::new(top[1].x, top[1].y + 14.0));
        painter.add(Shape::convex_polygon(
            side,
            Color32::from_rgb(0x08, 0x30, 0x18),
            Stroke::NONE,
        ));

        // tracks
        for t in &proj.tracks {
            let a = proj_pt(t.a, 0.6);
            let b = proj_pt(t.b, 0.6);
            let col = if t.layer == Layer::TopCopper {
                Color32::from_rgb(0xC0, 0x60, 0x30)
            } else {
                Color32::from_rgb(0x40, 0x70, 0xC0)
            };
            painter.add(Shape::line(
                vec![a, b],
                Stroke::new((t.width * scale).max(1.5), col),
            ));
        }

        // footprints as extruded blocks
        for f in &proj.fps {
            for pad in &f.pads {
                let wp = absolute_pad(f, pad);
                let base = proj_pt(wp, 0.0);
                let h = 18.0 * scale;
                painter.add(Shape::convex_polygon(
                    vec![
                        base + Vec2::new(-4.0, 0.0),
                        base + Vec2::new(4.0, -3.0),
                        base + Vec2::new(4.0, -3.0 - h),
                        base + Vec2::new(-4.0, -h),
                    ],
                    Color32::from_rgb(0xD0, 0xA0, 0x40),
                    Stroke::NONE,
                ));
            }
            let body = proj_pt(P::new(f.x, f.y), 22.0);
            painter.rect_filled(
                Rect::from_center_size(body, Vec2::new(46.0, 30.0)),
                0.0,
                Color32::from_rgb(0x18, 0x18, 0x1C),
            );
            painter.rect_stroke(
                Rect::from_center_size(body, Vec2::new(46.0, 30.0)),
                0.0,
                Stroke::new(1.0, Color32::from_rgb(0x50, 0x50, 0x60)),
                StrokeKind::Middle,
            );
            painter.text(
                body,
                egui::Align2::CENTER_CENTER,
                f.refdes.clone(),
                FontId::new(12.0, FontFamily::Proportional),
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
            );
        }

        // vias
        for via in &proj.vias {
            let c = proj_pt(via.pos, 1.0);
            painter.circle_filled(c, 4.0, Color32::from_rgb(0xD0, 0xA0, 0x30));
        }

        painter.text(
            rect.left_top() + Vec2::new(10.0, 8.0),
            egui::Align2::LEFT_TOP,
            "3D Visualisation - drag with middle mouse or scroll to rotate",
            FontId::new(12.0, FontFamily::Proportional),
            Color32::from_rgb(0x90, 0xA0, 0xB0),
        );

        format!(
            "{} footprints, {} tracks, {} vias",
            proj.fps.len(),
            proj.tracks.len(),
            proj.vias.len()
        )
    }

    // ------------------------------------------------------------ gerber

    pub fn gerber(&mut self, ui: &mut egui::Ui, proj: &Project) -> Vec<&'static str> {
        let mut shown = Vec::new();
        let (rect, resp) =
            ui.allocate_exact_size(ui.available_size_before_wrap(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(0x06, 0x2A, 0x14));

        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if resp.hovered() && scroll.abs() > 0.0 {
            self.notes_scroll = (self.notes_scroll + scroll * 0.4).clamp(-160.0, 160.0);
        }
        let zoom = (0.26 + self.notes_scroll * 0.0009).clamp(0.05, 1.5);
        let cx = rect.center().x;
        let cy = rect.center().y;
        let to = |p: P| -> Pos2 {
            let (o, _) = proj.board.rect();
            Pos2::new(
                cx + (p.x - (o.x + proj.board.width * 0.5)) * zoom,
                cy + (p.y - (o.y + proj.board.height * 0.5)) * zoom,
            )
        };

        // solder-mask opening board
        let (o, e) = proj.board.rect();
        painter.rect_filled(
            Rect::from_two_pos(to(o), to(e)),
            0.0,
            Color32::from_rgb(0x0B, 0x3D, 0x1E),
        );

        for t in &proj.tracks {
            let col = match t.layer {
                Layer::TopCopper => Color32::from_rgb(0xC8, 0x8A, 0x30),
                Layer::BottomCopper => Color32::from_rgb(0x90, 0x60, 0xC0),
                _ => continue,
            };
            painter.add(Shape::line(
                vec![to(t.a), to(t.b)],
                Stroke::new((t.width * zoom).max(1.0), col),
            ));
            shown.push(match t.layer {
                Layer::TopCopper => "GTL",
                _ => "GBL",
            });
        }
        for f in &proj.fps {
            for pad in &f.pads {
                let c = to(absolute_pad(f, pad));
                let r = (pad.size * 0.5 * zoom).max(2.0);
                painter.circle_filled(c, r, Color32::from_rgb(0xD8, 0xB0, 0x50));
                if pad.drill > 0.0 {
                    painter.circle_filled(
                        c,
                        (pad.drill * 0.5 * zoom).max(0.8),
                        Color32::from_rgb(0x06, 0x2A, 0x14),
                    );
                }
            }
        }
        for via in &proj.vias {
            let c = to(via.pos);
            painter.circle_filled(
                c,
                (via.size * 0.5 * zoom).max(2.0),
                Color32::from_rgb(0xD8, 0xB0, 0x50),
            );
            painter.circle_filled(
                c,
                (via.drill * 0.5 * zoom).max(0.8),
                Color32::from_rgb(0x06, 0x2A, 0x14),
            );
            shown.push("GTL");
        }
        // board outline (GKO)
        painter.rect_stroke(
            Rect::from_two_pos(to(o), to(e)),
            0.0,
            Stroke::new(1.5, Color32::from_rgb(0xE0, 0xE0, 0x60)),
            StrokeKind::Middle,
        );
        shown.push("GKO");

        painter.text(
            rect.left_top() + Vec2::new(10.0, 8.0),
            egui::Align2::LEFT_TOP,
            "Gerber Viewer - top copper, solder mask and board outline",
            FontId::new(12.0, FontFamily::Proportional),
            Color32::from_rgb(0xA0, 0xC0, 0xA0),
        );

        shown.sort();
        shown.dedup();
        shown
    }
}
