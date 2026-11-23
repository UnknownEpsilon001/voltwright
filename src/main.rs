// LEO WAS HERE ;3
mod bom;
mod dialogs;
mod examples;
mod io;
mod ui_text;
mod model;
mod panels;
mod pdsprj;
mod pcb;
mod schematic;
mod sim;
mod theme;
mod util;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use eframe::egui;
use egui::{FontFamily, FontId, RichText, ScrollArea, Sense, Stroke, Vec2};

use dialogs::Dialog;
use model::P;
use panels::PanelAction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Home,
    Schematic,
    Pcb,
    Simulation,
    Source,
    Explorer,
    Library,
    DeviceLibs,
    PackageLibs,
    SymbolLibs,
    Notes,
    Visualizer,
    Gerber,
    Bom,
}

impl Tab {
    // Keep the working views together. Simulation sits
    // beside Schematic and PCB, where a new user will look for it.
    const ALL: [Tab; 14] = [
        Tab::Home,
        Tab::Schematic,
        Tab::Pcb,
        Tab::Simulation,
        Tab::Source,
        Tab::Explorer,
        Tab::Library,
        Tab::DeviceLibs,
        Tab::PackageLibs,
        Tab::SymbolLibs,
        Tab::Notes,
        Tab::Visualizer,
        Tab::Gerber,
        Tab::Bom,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home Page",
            Tab::Schematic => "Schematic Capture",
            Tab::Pcb => "PCB Layout",
            Tab::Simulation => "Simulation",
            Tab::Source => "Source Code",
            Tab::Explorer => "Design Explorer",
            Tab::Library => "Library Manager",
            Tab::DeviceLibs => "Device Library Manager",
            Tab::PackageLibs => "Package Library Manager",
            Tab::SymbolLibs => "Symbol Library Manager",
            Tab::Notes => "Project Notes",
            Tab::Visualizer => "3D Visualizer",
            Tab::Gerber => "Gerber Viewer",
            Tab::Bom => "Bill of Materials",
        }
    }
}

struct AppState {
    lib: model::Library,
    proj: model::Project,
    sim: Option<sim::Simulator>,
    schematic: schematic::Schematic,
    pcb: pcb::Pcb,
    panels: panels::Panels,
    dialogs: dialogs::Dialogs,
    tab: Tab,
    left_library: bool,
    show_left: bool,
    show_status: bool,
    label_text: String,
    probe_text: String,
    status: String,
    sim_error: String,
    sim_stop: f64,
    sim_dt: f64,
    sim_max: usize,
    ac_f1: f64,
    ac_f2: f64,
    ac_dec: usize,
    wave: Option<sim::Waveform>,
    ac: Option<sim::AcResult>,
    shot: Option<(PathBuf, u32, bool, Arc<AtomicBool>)>,
    frames: u32,
    /// Sideways scroll of the module tab strip, in points.
    tab_scroll: f32,
    /// Simple snapshot undo/redo for schematic and PCB edits.
    undo: Vec<model::Project>,
    redo: Vec<model::Project>,
    /// Captured once when an edit gesture starts. Keeping this across drag
    /// frames avoids cloning the complete project for every pointer move.
    edit_snapshot: Option<(model::Project, u64)>,
    drc_cache_key: u64,
    drc_cache_count: usize,
    sim_accumulator: f64,
    sim_step_rx: Option<Receiver<(sim::Simulator, Result<(), String>)>>,
}

impl AppState {
    fn new() -> Self {
        let lib = model::Library::builtin();
        let proj = demo_project(&lib);
        Self {
            lib,
            proj,
            sim: None,
            schematic: schematic::Schematic::default(),
            pcb: pcb::Pcb::default(),
            panels: panels::Panels::default(),
            dialogs: dialogs::Dialogs::default(),
            tab: Tab::Schematic,
            left_library: true,
            show_left: true,
            show_status: true,
            label_text: "NET".into(),
            probe_text: "PROBE".into(),
            status: "Ready. Place parts from the library panel or open a project.".into(),
            sim_error: String::new(),
            sim_stop: 0.01,
            sim_dt: 0.00001,
            sim_max: 4000,
            ac_f1: 1.0,
            ac_f2: 1.0e6,
            ac_dec: 6,
            wave: None,
            ac: None,
            shot: None,
            frames: 0,
            tab_scroll: 0.0,
            undo: Vec::new(),
            redo: Vec::new(),
            edit_snapshot: None,
            drc_cache_key: u64::MAX,
            drc_cache_count: 0,
            sim_accumulator: 0.0,
            sim_step_rx: None,
        }
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    fn reset_after_document_change(&mut self) {
        self.schematic.clear_selection();
        self.schematic.request_fit();
        self.pcb.clear_selection();
        self.pcb.needs_fit = true;
        self.schematic.draft.clear();
        self.pcb.draft.clear();
        self.sim = None;
        self.sim_error.clear();
        self.wave = None;
        self.ac = None;
        self.undo.clear();
        self.redo.clear();
        self.edit_snapshot = None;
        self.drc_cache_key = u64::MAX;
    }

    /// Allocation-free project fingerprint. The old implementation formatted
    /// the whole design into a String on both sides of every mouse event.
    fn edit_key(proj: &model::Project) -> u64 {
        let mut h = DefaultHasher::new();
        proj.comps.len().hash(&mut h);
        for c in &proj.comps {
            c.id.hash(&mut h);
            c.part.hash(&mut h);
            c.refdes.hash(&mut h);
            c.value.hash(&mut h);
            c.x.to_bits().hash(&mut h);
            c.y.to_bits().hash(&mut h);
            c.rot.hash(&mut h);
        }
        proj.wires.len().hash(&mut h);
        for w in &proj.wires {
            w.id.hash(&mut h);
            w.start_pin.hash(&mut h);
            w.end_pin.hash(&mut h);
            for p in &w.pts {
                p.x.to_bits().hash(&mut h);
                p.y.to_bits().hash(&mut h);
            }
        }
        proj.labels.len().hash(&mut h);
        for l in &proj.labels {
            l.id.hash(&mut h);
            l.text.hash(&mut h);
            l.x.to_bits().hash(&mut h);
            l.y.to_bits().hash(&mut h);
            l.rot.hash(&mut h);
            l.probe.hash(&mut h);
        }
        proj.fps.len().hash(&mut h);
        for f in &proj.fps {
            f.id.hash(&mut h);
            f.x.to_bits().hash(&mut h);
            f.y.to_bits().hash(&mut h);
            f.rot.hash(&mut h);
            for pad in &f.pads {
                pad.pos.x.to_bits().hash(&mut h);
                pad.pos.y.to_bits().hash(&mut h);
                pad.size.to_bits().hash(&mut h);
                pad.net.hash(&mut h);
            }
        }
        proj.tracks.len().hash(&mut h);
        for t in &proj.tracks {
            t.id.hash(&mut h);
            t.a.x.to_bits().hash(&mut h);
            t.a.y.to_bits().hash(&mut h);
            t.b.x.to_bits().hash(&mut h);
            t.b.y.to_bits().hash(&mut h);
            t.width.to_bits().hash(&mut h);
            t.layer.hash(&mut h);
            t.net.hash(&mut h);
        }
        proj.vias.len().hash(&mut h);
        for v in &proj.vias {
            v.id.hash(&mut h);
            v.pos.x.to_bits().hash(&mut h);
            v.pos.y.to_bits().hash(&mut h);
            v.size.to_bits().hash(&mut h);
            v.drill.to_bits().hash(&mut h);
            v.net.hash(&mut h);
        }
        proj.board.origin.x.to_bits().hash(&mut h);
        proj.board.origin.y.to_bits().hash(&mut h);
        proj.board.width.to_bits().hash(&mut h);
        proj.board.height.to_bits().hash(&mut h);
        proj.rules.pad_pad.to_bits().hash(&mut h);
        proj.rules.pad_trace.to_bits().hash(&mut h);
        proj.rules.trace_trace.to_bits().hash(&mut h);
        proj.rules.graphic.to_bits().hash(&mut h);
        proj.rules.edge.to_bits().hash(&mut h);
        h.finish()
    }

    fn begin_edit(&mut self) {
        if self.edit_snapshot.is_none() {
            self.edit_snapshot = Some((self.proj.clone(), Self::edit_key(&self.proj)));
        }
    }

    fn finish_edit(&mut self, gesture_active: bool) {
        let Some((_, before_key)) = self.edit_snapshot.as_ref() else {
            return;
        };
        if Self::edit_key(&self.proj) != *before_key {
            let (before, _) = self.edit_snapshot.take().expect("snapshot checked above");
            self.push_undo(before);
            self.drc_cache_key = u64::MAX;
        } else if !gesture_active {
            self.edit_snapshot = None;
        }
    }

    fn push_undo(&mut self, before: model::Project) {
        self.undo.push(before);
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            let current = self.proj.clone();
            self.proj = prev;
            self.redo.push(current);
            self.rebuild_sim();
            self.schematic.clear_selection();
            self.pcb.clear_selection();
            self.edit_snapshot = None;
            self.drc_cache_key = u64::MAX;
            self.status = "Undo".into();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let current = self.proj.clone();
            self.proj = next;
            self.undo.push(current);
            self.rebuild_sim();
            self.schematic.clear_selection();
            self.pcb.clear_selection();
            self.edit_snapshot = None;
            self.drc_cache_key = u64::MAX;
            self.status = "Redo".into();
        }
    }

    fn handle_editor_action(&mut self, action: schematic::EditorAction) {
        match action {
            schematic::EditorAction::PickParts => {
                self.dialogs.pick_search.clear();
                self.dialogs.pick_selected = None;
                self.dialogs.open(Dialog::PickDevices);
                self.status = "Pick Devices: search, then click a part and the sheet.".into();
            }
            schematic::EditorAction::EditComponent(id) => {
                if self.proj.comp(id).is_some() {
                    self.dialogs.edit_comp = Some(id);
                    self.dialogs.open(Dialog::Properties);
                    self.status = "Edit component properties.".into();
                }
            }
        }
    }

    fn rebuild_sim(&mut self) {
        // Dropping the receiver invalidates any older solve that may still be
        // finishing after the circuit changed.
        self.sim_step_rx = None;
        match sim::Simulator::from_project(&self.proj, &self.lib) {
            Ok(mut s) => {
                self.sim_error.clear();
                if let Err(e) = s.solve_dc() {
                    self.sim_error = e;
                }
                self.sim = Some(s);
            }
            Err(e) => {
                self.sim = None;
                self.sim_error = e;
                self.wave = None;
                self.ac = None;
            }
        }
    }

    fn start_simulation(&mut self) {
        self.rebuild_sim();
        if let Some(sim) = self.sim.as_mut() {
            sim.running = true;
            self.sim_accumulator = 0.0;
            self.status =
                "Simulation running. Arrows show conventional current from VCC through the load to GND."
                    .into();
        } else {
            self.status = "Simulation could not start. Check the message in Simulation.".into();
        }
    }

    fn stop_simulation(&mut self) {
        self.sim_step_rx = None;
        if let Some(sim) = self.sim.as_mut() {
            sim.running = false;
            let _ = sim.solve_dc();
            self.status = "Simulation stopped.".into();
        }
        self.sim_accumulator = 0.0;
    }

    fn pump_simulation(&mut self, frame_dt: f64) -> bool {
        self.collect_simulation_step();

        if !self.sim.as_ref().is_some_and(|sim| sim.running) {
            self.sim_accumulator = 0.0;
            return false;
        }

        // Solve at a steady 20 Hz instead of once for every UI paint. This
        // keeps large schematics responsive and drops long frame gaps rather
        // than trying to catch up with a burst of expensive solves.
        const SOLVE_INTERVAL: f64 = 1.0 / 20.0;
        self.sim_accumulator += frame_dt.clamp(0.0, 0.1);
        if self.sim_accumulator >= SOLVE_INTERVAL && self.sim_step_rx.is_none() {
            let dt = self.sim_accumulator.min(0.05);
            self.sim_accumulator = 0.0;
            self.start_simulation_step(dt);
        }
        true
    }

    /// Start one solver step away from egui's paint thread. A difficult
    /// non-linear circuit can take many matrix iterations, but it must never
    /// stop mouse input or animation while those iterations finish.
    fn start_simulation_step(&mut self, dt: f64) {
        let Some(mut next) = self.sim.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.sim_step_rx = Some(rx);
        thread::spawn(move || {
            let result = next.step(dt.min(0.05));
            let _ = tx.send((next, result));
        });
    }

    fn collect_simulation_step(&mut self) {
        let Some(rx) = self.sim_step_rx.as_ref() else {
            return;
        };
        let received = match rx.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.sim_step_rx = None;
                self.sim_error = "The simulation worker stopped unexpectedly.".into();
                if let Some(sim) = self.sim.as_mut() {
                    sim.running = false;
                }
                return;
            }
        };
        self.sim_step_rx = None;

        let Some((mut stepped, result)) = received else {
            return;
        };
        if !self.sim.as_ref().is_some_and(|sim| sim.running) {
            return;
        }
        if let Err(error) = result {
            self.sim_error = error;
            if let Some(sim) = self.sim.as_mut() {
                sim.running = false;
            }
            return;
        }

        // A switch can be clicked while the worker is solving its previous
        // state. Carry the latest UI state forward so the click is not lost.
        if let Some(current) = self.sim.as_ref() {
            for element in &current.els {
                if let sim::Kind::Sw { on } = &element.kind {
                    stepped.set_switch(&element.refdes, *on);
                }
            }
        }
        let faults = stepped.drain_fault_events();
        if !faults.is_empty() {
            self.status = faults.join(" | ");
        }
        self.sim = Some(stepped);
    }

    fn advance_simulation(&mut self, dt: f64) {
        let Some(sim) = self.sim.as_mut() else {
            return;
        };
        if !sim.running {
            return;
        }

        if let Err(e) = sim.step(dt.min(0.05)) {
            self.sim_error = e;
            sim.running = false;
            return;
        }
        let faults = sim.drain_fault_events();
        if !faults.is_empty() {
            self.status = faults.join(" | ");
        }
    }

    fn handle_panel_action(&mut self, action: PanelAction, switch: bool) {
        match action {
            PanelAction::None => {}
            PanelAction::SelectComp(id) => {
                self.schematic.clear_selection();
                self.schematic.sel.insert(id);
                if switch {
                    self.tab = Tab::Schematic;
                }
            }
            PanelAction::SelectFootprint(id) => {
                self.pcb.clear_selection();
                self.pcb.sel_fp.insert(id);
                if switch {
                    self.tab = Tab::Pcb;
                }
            }
            PanelAction::Place(part) => {
                self.schematic.pending = Some(part.clone());
                self.schematic.tool = schematic::Tool::Part;
                if switch {
                    self.tab = Tab::Schematic;
                }
                self.status = format!("Click the sheet to place {part}.");
            }
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            if ui.button(ui_text::FILE_NEW).clicked() {
                self.dialogs.new_name = "New Project".into();
                self.dialogs.open(Dialog::NewProject);
            }
            if ui.button(ui_text::FILE_OPEN).clicked() {
                self.dialogs.open_path = self
                    .proj
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| io::default_path(&self.proj).to_string_lossy().into_owned());
                self.dialogs.open(Dialog::Open);
            }
            ui.separator();
            if ui.button(ui_text::FILE_SAVE).clicked() {
                self.dialogs.save_path = self
                    .proj
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| io::default_path(&self.proj).to_string_lossy().into_owned());
                self.dialogs.open(Dialog::Save);
            }
            if ui.button(ui_text::FILE_SAVE_AS).clicked() {
                self.dialogs.save_path =
                    io::default_path(&self.proj).to_string_lossy().into_owned();
                self.dialogs.open(Dialog::Save);
            }
            if ui.button("Export...").clicked() {
                self.dialogs.open(Dialog::Export);
            }
            ui.separator();
            if ui.button(ui_text::FILE_EXIT).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("Examples", |ui| {
            for name in examples::names() {
                if ui.button(name).clicked() {
                    if let Some(p) = examples::build(name, &self.lib) {
                        self.proj = p;
                        self.reset_after_document_change();
                        self.set_tab(Tab::Schematic);
                        self.status = format!("Opened example '{name}'. Press Run to simulate.");
                    }
                }
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui.button("Clear Selection").clicked() {
                self.schematic.clear_selection();
                self.pcb.clear_selection();
                self.status = "Selection cleared.".into();
            }
            if ui.button("Delete Selection").clicked() {
                self.schematic.tool = schematic::Tool::Delete;
                self.pcb.tool = pcb::PcbTool::Delete;
                self.status = "Delete tool selected. Click an item to remove it.".into();
            }
            ui.separator();
            if ui.button("Grid Settings...").clicked() {
                self.dialogs.open(Dialog::Grid);
            }
            if ui.button("Design Rules...").clicked() {
                self.dialogs.open(Dialog::Rules);
            }
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut self.show_left, "Project Explorer");
            ui.checkbox(&mut self.show_status, "Status Bar");
            ui.separator();
            if ui.button("Home Page").clicked() {
                self.set_tab(Tab::Home);
            }
            if ui.button("Design Explorer").clicked() {
                self.set_tab(Tab::Explorer);
            }
        });
        ui.menu_button("Tools", |ui| {
            if ui.button("Simulation").clicked() {
                self.set_tab(Tab::Simulation);
            }
            if ui.button("Property Assignment...   A").clicked() {
                self.dialogs
                    .prepare_property_assign(self.schematic.sel.iter().copied().collect());
                self.dialogs.open(Dialog::PropertyAssign);
            }
            if ui.button("Search and Tag...   T").clicked() {
                self.dialogs.open(Dialog::SearchTag);
            }
            if ui.button("Design Rule Check").clicked() {
                let n = self.proj.drc().len();
                self.status = if n == 0 {
                    "No DRC errors".into()
                } else {
                    format!("{n} design rule violation(s)")
                };
            }
            if ui.button("Netlist Report").clicked() {
                let nl = self.proj.netlist(&self.lib);
                self.status = format!(
                    "{} nets, {} component pins",
                    nl.names.len(),
                    nl.pin_net.len()
                );
            }
            if ui.button("Bill of Materials").clicked() {
                self.set_tab(Tab::Bom);
            }
        });
        ui.menu_button("Library", |ui| {
            for (label, tab) in [
                ("Library Manager", Tab::Library),
                ("Device Library Manager", Tab::DeviceLibs),
                ("Package Library Manager", Tab::PackageLibs),
                ("Symbol Library Manager", Tab::SymbolLibs),
            ] {
                if ui.button(label).clicked() {
                    self.set_tab(tab);
                }
            }
        });
        ui.menu_button("Technology", |ui| {
            if ui.button("Grid and Snaps...").clicked() {
                self.dialogs.open(Dialog::Grid);
            }
            if ui.button("Design Rules...").clicked() {
                self.dialogs.open(Dialog::Rules);
            }
            if ui.button("Gerber Viewer").clicked() {
                self.set_tab(Tab::Gerber);
            }
        });
        ui.menu_button("System", |ui| {
            if ui.button("Preferences...").clicked() {
                self.dialogs.open(Dialog::Preferences);
            }
            if ui.button("About...").clicked() {
                self.dialogs.open(Dialog::About);
            }
        });
        ui.menu_button("Help", |ui| {
            if ui.button("Help Topics").clicked() {
                self.dialogs.open(Dialog::Help);
            }
            if ui.button("About Voltwright").clicked() {
                self.dialogs.open(Dialog::About);
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("New").clicked() {
                self.dialogs.open(Dialog::NewProject);
            }
            if ui.button("Open").clicked() {
                self.dialogs.open_path =
                    io::default_path(&self.proj).to_string_lossy().into_owned();
                self.dialogs.open(Dialog::Open);
            }
            if ui.button("Save").clicked() {
                self.dialogs.save_path = self
                    .proj
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| io::default_path(&self.proj).to_string_lossy().into_owned());
                self.dialogs.open(Dialog::Save);
            }
            ui.separator();
            let running = self.sim.as_ref().is_some_and(|sim| sim.running);
            let simulate = ui
                .add_enabled(
                    !running,
                    egui::Button::new(
                        RichText::new(if running {
                            "SIMULATION RUNNING"
                        } else {
                            "RUN SIMULATION"
                        })
                        .color(egui::Color32::WHITE)
                        .strong(),
                    )
                    .fill(theme::OK)
                    .min_size(Vec2::new(156.0, 26.0)),
                )
                .on_hover_text("Build the circuit, start simulation, and show the results");
            if simulate.clicked() {
                self.start_simulation();
            }
            if ui
                .add_enabled(running, egui::Button::new("■  Stop"))
                .on_hover_text("Stop the running simulation")
                .clicked()
            {
                self.stop_simulation();
            }
            if ui
                .button("Fit Sheet  F8")
                .on_hover_text("Bring the schematic sheet frame back into view")
                .clicked()
            {
                self.set_tab(Tab::Schematic);
                self.schematic.request_fit();
                self.status = "Schematic fitted to the window.".into();
            }
            ui.separator();
            match self.tab {
                Tab::Schematic => {
                    if ui.button("P  Pick Devices").clicked() {
                        self.handle_editor_action(schematic::EditorAction::PickParts);
                    }
                    if ui.button("W  Wire").clicked() {
                        self.schematic.tool = schematic::Tool::Wire;
                        self.status = "Wire tool: click a pin, then the target pin.".into();
                    }
                    if ui.button("Undo").clicked() {
                        self.undo();
                    }
                    if ui.button("Redo").clicked() {
                        self.redo();
                    }
                    for tool in [
                        schematic::Tool::Select,
                        schematic::Tool::Wire,
                        schematic::Tool::Part,
                        schematic::Tool::Label,
                        schematic::Tool::Text,
                        schematic::Tool::Probe,
                        schematic::Tool::Delete,
                    ] {
                        if ui
                            .selectable_value(&mut self.schematic.tool, tool, tool.label())
                            .clicked()
                        {
                            self.status = format!("Tool: {}", tool.label());
                        }
                    }
                    ui.separator();
                    ui.label("Label:");
                    ui.add(egui::TextEdit::singleline(&mut self.label_text).desired_width(70.0));
                    ui.label("Probe:");
                    ui.add(egui::TextEdit::singleline(&mut self.probe_text).desired_width(70.0));
                }
                Tab::Pcb => {
                    for tool in [
                        pcb::PcbTool::Select,
                        pcb::PcbTool::Track,
                        pcb::PcbTool::Via,
                        pcb::PcbTool::Board,
                        pcb::PcbTool::Delete,
                    ] {
                        ui.selectable_value(&mut self.pcb.tool, tool, tool.label());
                    }
                    ui.separator();
                    egui::ComboBox::from_label("Layer")
                        .selected_text(self.pcb.active_layer.name())
                        .show_ui(ui, |ui| {
                            for layer in model::Layer::all() {
                                ui.selectable_value(
                                    &mut self.pcb.active_layer,
                                    layer,
                                    layer.name(),
                                );
                            }
                        });
                    ui.checkbox(&mut self.pcb.show_ratsnest, "Ratsnest");
                    ui.checkbox(&mut self.pcb.show_drc, "DRC");
                }
                Tab::Simulation => {
                    if ui.button("Rebuild").clicked() {
                        self.rebuild_sim();
                        self.status = "Circuit rebuilt from the schematic netlist.".into();
                    }
                    if ui.button("DC").clicked() {
                        self.rebuild_sim();
                        if let Some(s) = self.sim.as_mut() {
                            match s.solve_dc() {
                                Ok(_) => self.status = "DC operating point solved.".into(),
                                Err(e) => self.sim_error = e,
                            }
                        }
                    }
                    if ui.button("Transient").clicked() {
                        let (stop, dt, max) = (self.sim_stop, self.sim_dt, self.sim_max);
                        self.rebuild_sim();
                        if let Some(s) = self.sim.as_mut() {
                            match sim::run_transient(s, stop, dt, max) {
                                Ok(w) => {
                                    self.status =
                                        format!("Transient run: {} samples", w.times.len());
                                    self.wave = Some(w);
                                    self.ac = None;
                                }
                                Err(e) => self.sim_error = e,
                            }
                        }
                    }
                    if ui.button("AC Sweep").clicked() {
                        let (f1, f2, dec) = (self.ac_f1, self.ac_f2, self.ac_dec);
                        self.rebuild_sim();
                        if let Some(s) = self.sim.as_mut() {
                            match sim::ac_sweep(s, f1, f2, dec) {
                                Ok(a) => {
                                    self.status = format!("AC sweep: {} points", a.points.len());
                                    self.ac = Some(a);
                                    self.wave = None;
                                }
                                Err(e) => self.sim_error = e,
                            }
                        }
                    }
                    ui.separator();
                    if ui.button("Run/Pause").clicked() {
                        if let Some(s) = self.sim.as_mut() {
                            s.running = !s.running;
                        }
                    }
                    if ui.button("Stop").clicked() {
                        self.stop_simulation();
                    }
                    ui.separator();
                    ui.label("Stop");
                    ui.add(
                        egui::DragValue::new(&mut self.sim_stop)
                            .range(0.0001..=10.0)
                            .speed(0.001),
                    );
                    ui.label("dt");
                    ui.add(
                        egui::DragValue::new(&mut self.sim_dt)
                            .range(0.0000001..=0.01)
                            .speed(0.000001),
                    );
                    ui.label("Hz");
                    ui.add(
                        egui::DragValue::new(&mut self.ac_f2)
                            .range(1.0..=1.0e9)
                            .speed(100.0),
                    );
                }
                _ => {
                    if ui.button("Schematic").clicked() {
                        self.set_tab(Tab::Schematic);
                    }
                    if ui.button("PCB Layout").clicked() {
                        self.set_tab(Tab::Pcb);
                    }
                    if ui.button("Simulation").clicked() {
                        self.set_tab(Tab::Simulation);
                    }
                    if ui.button("BOM").clicked() {
                        self.set_tab(Tab::Bom);
                    }
                }
            }
        });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        egui::Frame::NONE.fill(theme::CHROME_PALE).show(ui, |ui| {
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                ui.vertical(|ui| {
                    ui.heading(RichText::new("Voltwright").size(26.0).color(theme::WIRE));
                    ui.label(
                        RichText::new("Schematic capture, PCB layout, simulation and manufacturing in one workspace.")
                            .size(14.0),
                    );
                    ui.add_space(12.0);
                    ui.horizontal_wrapped(|ui| {
                        for (label, tab) in [
                            ("Schematic Capture", Tab::Schematic),
                            ("PCB Layout", Tab::Pcb),
                            ("Simulation", Tab::Simulation),
                            ("Bill of Materials", Tab::Bom),
                            ("3D Visualizer", Tab::Visualizer),
                        ] {
                            if ui.button(label).clicked() {
                                self.set_tab(tab);
                            }
                        }
                    });
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.strong("Current project");
                    ui.label(format!("Name        {}", self.proj.name));
                    ui.label(format!("Components  {}", self.proj.comps.len()));
                    ui.label(format!("Wires       {}", self.proj.wires.len()));
                    ui.label(format!("Footprints  {}", self.proj.fps.len()));
                    ui.label(format!("Tracks      {}", self.proj.tracks.len()));
                    ui.label(format!(
                        "Grid        {} / {} th",
                        match self.proj.grid.units {
                            model::UnitSystem::Imperial => "th",
                            model::UnitSystem::Metric => "mm",
                        },
                        self.proj.grid.step()
                    ));
                    ui.add_space(10.0);
                    ui.label("Tip: double-click finishes a wire. Use the library panel to place more parts.");
                });
            });
        });
    }

    fn simulation_view(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.strong("Simulation");
                if let Some(s) = &self.sim {
                    ui.label(
                        RichText::new(if s.running { "RUNNING" } else { "stopped" }).color(
                            if s.running {
                                theme::OK
                            } else {
                                theme::TEXT_DIM
                            },
                        ),
                    );
                    ui.label(format!("t = {:.6} s", s.t));
                } else {
                    ui.colored_label(
                        theme::WARN,
                        "No circuit yet - place parts or press Rebuild.",
                    );
                }
            });
            if !self.sim_error.is_empty() {
                ui.colored_label(theme::ERR, &self.sim_error);
            }

            ui.horizontal(|ui| {
                ui.label("Stop");
                ui.add(
                    egui::DragValue::new(&mut self.sim_stop)
                        .range(0.0001..=10.0)
                        .speed(0.001),
                );
                ui.label("s   dt");
                ui.add(
                    egui::DragValue::new(&mut self.sim_dt)
                        .range(0.0000001..=0.01)
                        .speed(0.000001),
                );
                ui.label("s   max");
                ui.add(
                    egui::DragValue::new(&mut self.sim_max)
                        .range(100..=50000)
                        .speed(100),
                );
                ui.label("AC");
                ui.add(
                    egui::DragValue::new(&mut self.ac_f1)
                        .range(0.01..=1.0e6)
                        .speed(1.0),
                );
                ui.label("to");
                ui.add(
                    egui::DragValue::new(&mut self.ac_f2)
                        .range(1.0..=1.0e9)
                        .speed(100.0),
                );
                ui.label("Hz /");
                ui.add(egui::DragValue::new(&mut self.ac_dec).range(1..=9));
                ui.label("decades");
            });

            if let Some(s) = &self.sim {
                ui.add_space(4.0);
                ui.strong("Operating point");
                ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, name) in s.names.iter().enumerate() {
                            let v = s.v.get(i).copied().unwrap_or(0.0);
                            ui.label(format!("{name} = {v:.3} V"));
                        }
                    });
                });
                ui.add_space(5.0);
                ui.strong("Real-world component limits");
                egui::Grid::new("component_limits")
                    .striped(true)
                    .spacing([18.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Part");
                        ui.strong("State");
                        ui.strong("Current");
                        ui.strong("Power / temperature");
                        ui.strong("Rating");
                        ui.end_row();
                        for el in &s.els {
                            if !el.is_led && !matches!(el.kind, sim::Kind::R) {
                                continue;
                            }
                            let current = s.current_of(el).abs();
                            let voltage = s.element_voltage(el).abs();
                            let power = current * voltage;
                            let state = if el.failed_open {
                                "BLOWN • OPEN"
                            } else if s.is_overloaded(&el.refdes) {
                                "OVERLOAD"
                            } else {
                                "OK"
                            };
                            let color = if el.failed_open {
                                theme::ERR
                            } else if s.is_overloaded(&el.refdes) {
                                theme::WARN
                            } else {
                                theme::OK
                            };
                            ui.label(&el.refdes);
                            ui.colored_label(color, state);
                            ui.label(format_current(current));
                            ui.label(format!("{power:.3} W / {:.0} °C", el.temperature_c));
                            if el.is_led {
                                ui.label("20 mA nominal, 30 mA max");
                            } else {
                                ui.label("0.25 W");
                            }
                            ui.end_row();
                        }
                    });
            }

            ui.add_space(6.0);
            if let Some(w) = &self.wave {
                let names: Vec<String> = self
                    .sim
                    .as_ref()
                    .map(|s| {
                        s.names
                            .iter()
                            .skip(1)
                            .take(w.volts.len())
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                plot(
                    ui,
                    "Transient Analysis",
                    &w.times,
                    &w.volts,
                    &names,
                    false,
                    "V",
                );
            }
            if let Some(a) = &self.ac {
                let xs: Vec<f64> = a.points.iter().map(|p| p.freq).collect();
                let series: Vec<Vec<f64>> = (0..a.nets.len())
                    .map(|k| {
                        a.points
                            .iter()
                            .map(|p| {
                                let m = p.mag.get(k).copied().unwrap_or(1.0).abs().max(1.0e-12);
                                20.0 * m.log10()
                            })
                            .collect()
                    })
                    .collect();
                plot(
                    ui,
                    "AC Sweep - magnitude",
                    &xs,
                    &series,
                    &a.nets,
                    true,
                    "dB",
                );
            }
            if self.wave.is_none() && self.ac.is_none() {
                ui.colored_label(
                    theme::TEXT_DIM,
                    "Press Transient or AC Sweep to draw a graph.",
                );
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(" {}", self.status))
                    .monospace()
                    .color(theme::TEXT_DIM),
            );
            ui.label(RichText::new("|").color(theme::CHROME_DARK));
            ui.label(format!("{}", self.tab.label()));
            ui.label(RichText::new("|").color(theme::CHROME_DARK));
            ui.label(format!(
                "x: +{:.1}  y: +{:.1}  th",
                self.schematic.mouse.x, self.schematic.mouse.y
            ));
            ui.label(RichText::new("|").color(theme::CHROME_DARK));
            let key = Self::edit_key(&self.proj);
            if key != self.drc_cache_key {
                self.drc_cache_count = self.proj.drc().len();
                self.drc_cache_key = key;
            }
            let drc = self.drc_cache_count;
            let drc_text = if drc == 0 {
                "No DRC errors".to_string()
            } else {
                format!("{drc} DRC errors")
            };
            ui.label(RichText::new(drc_text).color(if drc == 0 { theme::OK } else { theme::ERR }));
            ui.label(RichText::new("|").color(theme::CHROME_DARK));
            ui.label(format!("Snap {}", self.proj.grid.step()));
        });
    }
}

impl eframe::App for AppState {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frames += 1;
        let ctx = ui.ctx().clone();

        let frame_dt = ui.input(|i| i.unstable_dt as f64);
        if self.pump_simulation(frame_dt) {
            // Current arrows only need about 30 frames a second. An unbounded
            // repaint request made egui redraw every grid dot and wire on every
            // vsync while the solver ran, which starved input on slower PCs.
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        let typing = ctx.memory(|m| m.focused().is_some());
        let (save, open, new, undo, redo, pick, wire, edit, property_assign, search_tag) = ui
            .input(|i| {
                let ctrl = i.modifiers.ctrl || i.modifiers.command;
                (
                    ctrl && i.key_pressed(egui::Key::S),
                    ctrl && i.key_pressed(egui::Key::O),
                    ctrl && i.key_pressed(egui::Key::N),
                    ctrl && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
                    (ctrl && i.key_pressed(egui::Key::Y))
                        || (ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Z)),
                    !ctrl && i.key_pressed(egui::Key::P),
                    !ctrl && i.key_pressed(egui::Key::W),
                    !ctrl && i.key_pressed(egui::Key::E),
                    !ctrl && i.key_pressed(egui::Key::A),
                    !ctrl && i.key_pressed(egui::Key::T),
                )
            });
        if new {
            self.dialogs.open(Dialog::NewProject);
        }
        if open {
            self.dialogs.open_path = self
                .proj
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| io::default_path(&self.proj).to_string_lossy().into_owned());
            self.dialogs.open(Dialog::Open);
        }
        if save {
            self.dialogs.save_path = self
                .proj
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| io::default_path(&self.proj).to_string_lossy().into_owned());
            self.dialogs.open(Dialog::Save);
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if !typing {
            if pick {
                self.handle_editor_action(schematic::EditorAction::PickParts);
            }
            if wire && self.tab == Tab::Schematic {
                self.schematic.tool = schematic::Tool::Wire;
                self.status = "Wire tool: click a pin to start, click again to connect.".into();
            }
            if edit {
                if let Some(&id) = self.schematic.sel.iter().next() {
                    self.handle_editor_action(schematic::EditorAction::EditComponent(id));
                }
            }
            if property_assign {
                self.dialogs
                    .prepare_property_assign(self.schematic.sel.iter().copied().collect());
                self.dialogs.open(Dialog::PropertyAssign);
            }
            if search_tag {
                self.dialogs.open(Dialog::SearchTag);
            }
        }

        egui::Panel::top("menu")
            .frame(egui::Frame::NONE.fill(theme::CHROME))
            .show(ui, |ui| {
                // MenuBar lets an open menu follow the pointer to its
                // neighbours, like a desktop menu bar.
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Voltwright").size(15.0).color(theme::WIRE).strong());
                    ui.separator();
                    self.menu(ui);
                });
            });

        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::CHROME_LIGHT)
                    .inner_margin(egui::Margin::symmetric(4, 3)),
            )
            .show(ui, |ui| {
                self.toolbar(ui);
            });

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(4, 3)),
            )
            .show(ui, |ui| {
                if self.show_status {
                    self.status_bar(ui);
                }
            });

        if self.show_left {
            egui::Panel::left("explorer")
                .default_size(224.0)
                .min_size(180.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::CHROME_PALE)
                        .inner_margin(egui::Margin::same(4)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.left_library, false, "Project");
                        ui.selectable_value(&mut self.left_library, true, "Libraries");
                    });
                    ui.separator();
                    let action = if self.left_library {
                        self.panels.library_manager(ui, &self.lib)
                    } else {
                        self.panels
                            .design_explorer(ui, &self.proj, &self.lib, &self.schematic.sel)
                    };
                    self.handle_panel_action(action, true);
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::CHROME))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // The tab strip is wider than small windows. A floating
                    // scrollbar would cover the labels, so hide it and let the
                    // ordinary mouse wheel scroll the strip sideways.
                    let strip = ScrollArea::horizontal()
                        .id_salt("module_tabs")
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .horizontal_scroll_offset(self.tab_scroll)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for tab in Tab::ALL {
                                    if ui
                                        .selectable_value(&mut self.tab, tab, tab.label())
                                        .clicked()
                                    {
                                        self.status = format!("Opened {}", tab.label());
                                    }
                                }
                            });
                        });
                    self.tab_scroll = strip.state.offset().x;
                    if ui.rect_contains_pointer(strip.inner_rect) {
                        let wheel = ui.input(|i| i.smooth_scroll_delta);
                        let step = if wheel.x != 0.0 { wheel.x } else { wheel.y };
                        let max = (strip.content_size.x - strip.inner_rect.width()).max(0.0);
                        self.tab_scroll = (self.tab_scroll - step).clamp(0.0, max);
                        if step != 0.0 {
                            ui.ctx().request_repaint();
                        }
                    }
                    ui.separator();

                    match self.tab {
                        Tab::Home => self.home(ui),
                        Tab::Schematic => {
                            // Capture one undo snapshot at the start of an
                            // actual edit gesture. PointerMoved and wheel-only
                            // events must never clone the entire design.
                            let edit_started = ui.input(|i| {
                                i.pointer.any_pressed()
                                    || i.events.iter().any(|event| {
                                        matches!(event, egui::Event::Key { pressed: true, .. })
                                    })
                            });
                            if edit_started {
                                self.begin_edit();
                            }
                            let st = self.schematic.show(
                                ui,
                                &mut self.proj,
                                &self.lib,
                                self.sim.as_mut(),
                                &self.label_text,
                                &self.probe_text,
                            );
                            if !st.is_empty() {
                                self.status = st;
                            }
                            if let Some(action) = self.schematic.action.take() {
                                self.handle_editor_action(action);
                            }
                            let gesture_active = ui.input(|i| i.pointer.any_down());
                            self.finish_edit(gesture_active);
                        }
                        Tab::Pcb => {
                            let edit_started = ui.input(|i| {
                                i.pointer.any_pressed()
                                    || i.events.iter().any(|event| {
                                        matches!(event, egui::Event::Key { pressed: true, .. })
                                    })
                            });
                            if edit_started {
                                self.begin_edit();
                            }
                            let st = self.pcb.show(ui, &mut self.proj);
                            if !st.is_empty() {
                                self.status = st;
                            }
                            let gesture_active = ui.input(|i| i.pointer.any_down());
                            self.finish_edit(gesture_active);
                        }
                        Tab::Simulation => self.simulation_view(ui),
                        Tab::Source => self.panels.source_code(ui, &mut self.proj),
                        Tab::Explorer => {
                            let action = self.panels.design_explorer(
                                ui,
                                &self.proj,
                                &self.lib,
                                &self.schematic.sel,
                            );
                            self.handle_panel_action(action, false);
                        }
                        Tab::Library | Tab::DeviceLibs | Tab::PackageLibs | Tab::SymbolLibs => {
                            let category = match self.tab {
                                Tab::DeviceLibs => "All Categories".to_string(),
                                Tab::PackageLibs => "All Categories".to_string(),
                                Tab::SymbolLibs => "All Categories".to_string(),
                                _ => self.panels.category.clone(),
                            };
                            let old = self.panels.category.clone();
                            self.panels.category = category;
                            let action = self.panels.library_manager(ui, &self.lib);
                            self.panels.category = old;
                            self.handle_panel_action(action, true);
                        }
                        Tab::Notes => self.panels.notes(ui, &mut self.proj),
                        Tab::Visualizer => {
                            let st = self.panels.visualizer(ui, &self.proj);
                            if !st.is_empty() {
                                self.status = st;
                            }
                        }
                        Tab::Gerber => {
                            let layers = self.panels.gerber(ui, &self.proj);
                            if !layers.is_empty() {
                                self.status = format!("Gerber layers: {}", layers.join(", "));
                            }
                        }
                        Tab::Bom => self.panels.bom_view(ui, &self.proj, &self.lib),
                    }
                });
            });

        if let Some(pid) = self.dialogs.pick_selected.take() {
            self.schematic.pending = Some(pid.clone());
            self.schematic.tool = schematic::Tool::Part;
            self.status = format!("Place {pid}: click the sheet, right click or Esc to stop.");
        }
        if let Some(msg) = self.dialogs.show(&ctx, &mut self.proj, &self.lib) {
            self.status = msg.clone();
            if msg.starts_with("Created project")
                || msg.starts_with("Opened project")
                || msg.starts_with("Imported")
            {
                self.reset_after_document_change();
                self.dialogs.save_path =
                    io::default_path(&self.proj).to_string_lossy().into_owned();
            }
        }
        if self.dialogs.search_tag_apply {
            self.dialogs.search_tag_apply = false;
            let ids = self.dialogs.search_tag_tagged.clone();
            self.schematic.set_selection(ids);
        }
        if self.dialogs.property_assign_apply {
            self.dialogs.property_assign_apply = false;
        }

        if let Some((path, left, requested, done)) = &mut self.shot {
            ctx.request_repaint();
            if *left > 0 {
                *left -= 1;
            } else if !*requested {
                *requested = true;
                let p = path.clone();
                let d = done.clone();
                ctx.request_screenshot(move |img| {
                    save_png(&p, &img);
                    d.store(true, Ordering::SeqCst);
                    // Some Windows graphics backends close the hidden root
                    // viewport but leave the event loop alive. Screenshot mode
                    // is a one-shot command, so finish after the PNG is safely
                    // written instead of leaving a locked background process.
                    std::process::exit(0);
                });
            } else if done.load(Ordering::SeqCst) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
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

fn plot(
    ui: &mut egui::Ui,
    title: &str,
    xs: &[f64],
    series: &[Vec<f64>],
    names: &[String],
    log_x: bool,
    y_unit: &str,
) {
    ui.strong(title);
    let size = ui.available_size_before_wrap();
    let height = size.y.min(260.0).max(140.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(size.x.max(120.0), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::GRAPH_BG);

    let xf = |v: f64| if log_x { v.max(1.0e-12).log10() } else { v };
    let mut xmin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for (i, x) in xs.iter().enumerate() {
        let tx = xf(*x);
        if tx.is_finite() {
            xmin = xmin.min(tx);
            xmax = xmax.max(tx);
        }
        if let Some(row) = series.first() {
            if let Some(y) = row.get(i) {
                if y.is_finite() {
                    ymin = ymin.min(*y);
                    ymax = ymax.max(*y);
                }
            }
        }
    }
    for row in series.iter().skip(1) {
        for y in row {
            if y.is_finite() {
                ymin = ymin.min(*y);
                ymax = ymax.max(*y);
            }
        }
    }
    if !xmin.is_finite() || !xmax.is_finite() || !ymin.is_finite() || !ymax.is_finite() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No graph data",
            FontId::new(13.0, FontFamily::Proportional),
            theme::GRAPH_TEXT,
        );
        return;
    }
    if (xmax - xmin).abs() < 1.0e-12 {
        xmax = xmin + 1.0;
    }
    if (ymax - ymin).abs() < 1.0e-12 {
        ymin -= 1.0;
        ymax += 1.0;
    }

    let pad_l = 56.0;
    let pad_r = 12.0;
    let pad_t = 10.0;
    let pad_b = 26.0;
    let w = (rect.width() - pad_l - pad_r).max(10.0);
    let h = (rect.height() - pad_t - pad_b).max(10.0);
    let px = |v: f64| rect.left() + pad_l + ((xf(v) - xmin) / (xmax - xmin)) as f32 * w;
    let py = |v: f64| rect.bottom() - pad_b - ((v - ymin) / (ymax - ymin)) as f32 * h;

    for i in 0..=8 {
        let x = rect.left() + pad_l + i as f32 / 8.0 * w;
        painter.line_segment(
            [
                egui::pos2(x, rect.top() + pad_t),
                egui::pos2(x, rect.bottom() - pad_b),
            ],
            Stroke::new(1.0, theme::GRAPH_GRID),
        );
    }
    for i in 0..=5 {
        let y = rect.top() + pad_t + i as f32 / 5.0 * h;
        painter.line_segment(
            [
                egui::pos2(rect.left() + pad_l, y),
                egui::pos2(rect.right() - pad_r, y),
            ],
            Stroke::new(1.0, theme::GRAPH_GRID),
        );
        let value = ymax - i as f64 / 5.0 * (ymax - ymin);
        painter.text(
            egui::pos2(rect.left() + 4.0, y - 6.0),
            egui::Align2::LEFT_TOP,
            format!("{value:.2}"),
            FontId::new(10.0, FontFamily::Monospace),
            theme::GRAPH_TEXT,
        );
    }
    painter.text(
        egui::pos2(rect.left() + 6.0, rect.top() + 4.0),
        egui::Align2::LEFT_TOP,
        y_unit,
        FontId::new(11.0, FontFamily::Monospace),
        theme::GRAPH_TEXT,
    );

    for (k, row) in series.iter().enumerate() {
        let color = theme::TRACE[k % theme::TRACE.len()];
        let mut prev: Option<egui::Pos2> = None;
        let step = (xs.len() / 1400).max(1);
        let mut i = 0;
        while i < xs.len().min(row.len()) {
            let x = xs[i];
            let y = row[i];
            let tx = xf(x);
            if tx.is_finite() && y.is_finite() {
                let p = egui::pos2(px(x), py(y));
                if let Some(q) = prev {
                    painter.line_segment([q, p], Stroke::new(1.4, color));
                }
                prev = Some(p);
            }
            i += step;
        }
    }

    ui.horizontal_wrapped(|ui| {
        for (k, name) in names.iter().enumerate() {
            ui.label(
                RichText::new(format!("{} {}", name, y_unit))
                    .color(theme::TRACE[k % theme::TRACE.len()])
                    .small(),
            );
        }
    });
}

fn demo_project(lib: &model::Library) -> model::Project {
    let mut proj = model::Project::new("demo");
    let r = proj.add_component(lib, "RESISTOR", P::new(420.0, 260.0));
    let led = proj.add_component(lib, "LED-RED", P::new(700.0, 260.0));
    let source = proj.add_component(lib, "VCC", P::new(200.0, 260.0));
    let gnd = proj.add_component(lib, "GND", P::new(820.0, 580.0));
    if let Some(id) = r {
        if let Some(c) = proj.comp_mut(id) {
            c.value = "330R".into();
        }
    }
    let id = proj.alloc_id();
    proj.wires.push(model::Wire {
        id,
        pts: vec![P::new(480.0, 260.0), P::new(640.0, 260.0)],
        start_pin: r.map(|id| (id, 1)),
        end_pin: led.map(|id| (id, 0)),
    });
    let id = proj.alloc_id();
    proj.wires.push(model::Wire {
        id,
        pts: vec![
            P::new(760.0, 260.0),
            P::new(820.0, 260.0),
            P::new(820.0, 580.0),
        ],
        start_pin: led.map(|id| (id, 1)),
        end_pin: gnd.map(|id| (id, 0)),
    });
    let id = proj.alloc_id();
    proj.wires.push(model::Wire {
        id,
        pts: vec![P::new(200.0, 260.0), P::new(360.0, 260.0)],
        start_pin: source.map(|id| (id, 0)),
        end_pin: r.map(|id| (id, 0)),
    });
    let nl = proj.netlist(lib);
    proj.sync_pcb(lib, &nl);
    let led_label_id = proj.alloc_id();
    proj.labels.push(model::Label {
        id: led_label_id,
        kind: model::LabelKind::Net,
        text: "LED_DRIVE".into(),
        x: 480.0,
        y: 230.0,
        rot: 0,
        probe: false,
    });
    proj.dirty = false;
    let _ = led;
    proj
}

/// Black-and-white window icon: a diode symbol (triangle and bar on a lead).
fn app_icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    let inside_triangle = |x: f32, y: f32| {
        // Triangle pointing right: base at x = 14, tip at x = 40, y centred at 32.
        let (x0, x1, cy, half) = (14.0, 40.0, 32.0, 17.0);
        x >= x0 && x <= x1 && (y - cy).abs() <= half * (x1 - x) / (x1 - x0)
    };
    for py in 0..N {
        for px in 0..N {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            let tri = inside_triangle(x, y);
            let bar = (40.0..=46.0).contains(&x) && (14.0..=50.0).contains(&y);
            let lead = (y - 32.0).abs() <= 2.5 && ((3.0..=14.0).contains(&x) || (46.0..=61.0).contains(&x));
            let ink = tri || bar || lead;
            let tile = if ink { [0u8, 0, 0, 255] } else { [255u8, 255, 255, 255] };
            let at = (py * N + px) * 4;
            rgba[at..at + 4].copy_from_slice(&tile);
        }
    }
    egui::IconData { rgba, width: N as u32, height: N as u32 }
}

fn save_png(path: &std::path::Path, img: &egui::ColorImage) {
    let mut buf = Vec::with_capacity(img.size[0] * img.size[1] * 4);
    for p in &img.pixels {
        buf.extend_from_slice(&p.to_srgba_unmultiplied());
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match image::save_buffer(
        path,
        &buf,
        img.size[0] as u32,
        img.size[1] as u32,
        image::ColorType::Rgba8,
    ) {
        Ok(_) => println!("saved {}", path.display()),
        Err(e) => println!("screenshot failed: {e}"),
    }
}

fn main() {
    let mut shot: Option<PathBuf> = None;
    let args: Vec<String> = std::env::args().collect();
    let start_live = args.iter().any(|arg| arg == "--simulate");
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--screenshot" {
            if let Some(p) = args.get(i + 1) {
                shot = Some(PathBuf::from(p));
                i += 1;
            }
        }
        i += 1;
    }

    let mut vp = egui::ViewportBuilder::default()
        .with_title("Voltwright")
        .with_inner_size([1440.0, 900.0])
        .with_maximized(true)
        .with_icon(std::sync::Arc::new(app_icon()));
    if shot.is_some() {
        vp = vp.with_active(false);
        #[cfg(target_os = "windows")]
        {
            vp = vp.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }

    let options = eframe::NativeOptions {
        viewport: vp,
        centered: true,
        ..Default::default()
    };

    let shot_cell: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(shot);
    let res = eframe::run_native(
        "Voltwright",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let mut app = AppState::new();
            if start_live {
                app.start_simulation();
            }
            if let Ok(Some(p)) = shot_cell.lock().map(|mut g| g.take()) {
                app.shot = Some((p, 120, false, Arc::new(AtomicBool::new(false))));
            }
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = res {
        eprintln!("eframe error: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_simulation_runs_past_transient_stop_time() {
        let mut app = AppState::new();
        app.start_simulation();
        assert!(app.sim.as_ref().is_some_and(|sim| sim.running));

        let stop = app.sim_stop;
        for _ in 0..10 {
            app.advance_simulation(0.005);
        }

        let sim = app.sim.as_ref().expect("demo project should simulate");
        assert!(sim.t > stop);
        assert!(sim.running, "continuous Run must wait for the Stop command");
    }

    #[test]
    fn edit_gesture_creates_only_one_undo_snapshot() {
        let mut app = AppState::new();
        let original_x = app.proj.comps[0].x;

        app.begin_edit();
        app.proj.comps[0].x += 25.0;
        app.finish_edit(true);
        app.proj.comps[0].x += 25.0;
        app.finish_edit(false);

        assert_eq!(app.undo.len(), 1);
        app.undo();
        assert_eq!(app.proj.comps[0].x, original_x);
    }

    #[test]
    fn non_editing_gesture_drops_snapshot_without_undo() {
        let mut app = AppState::new();
        app.begin_edit();
        app.finish_edit(false);

        assert!(app.edit_snapshot.is_none());
        assert!(app.undo.is_empty());
    }
}
