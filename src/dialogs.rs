// Modal dialogs: About, Grid Settings, Design Rules, New Project, Save,
// Export and Preferences.

use std::collections::HashSet;

use eframe::egui::{self, Color32, FontFamily, FontId};

use crate::ui_text;
use crate::model::{Component, Library, Project, UnitSystem};
use crate::theme::*;

/// How a new search combines with the current selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    New,
    Or,
    And,
}

/// How the search text is compared with a property value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    NotBegins,
    NotEquals,
    Contains,
    NotContains,
    Begins,
    Equals,
}

impl MatchMode {
    fn matches(self, hay: &str, needle: &str) -> bool {
        match self {
            MatchMode::NotBegins => !hay.starts_with(needle),
            MatchMode::NotEquals => hay != needle,
            MatchMode::Contains => hay.contains(needle),
            MatchMode::NotContains => !hay.contains(needle),
            MatchMode::Begins => hay.starts_with(needle),
            MatchMode::Equals => hay == needle,
        }
    }
}

/// Qt mnemonics use `&` (and `&&` for a literal ampersand); egui does not.
fn qt(s: &'static str) -> String {
    s.replace("&&", "\u{0}")
        .replace('&', "")
        .replace('\u{0}', "&")
}

// A single '#'
// prints the current count and advances it by the increment; '##' prints a
// literal '#' and leaves the count alone.
fn expand_prop_value(pattern: &str, count: &mut i32, increment: i32) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '#' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'#') {
            chars.next();
            out.push('#');
        } else {
            out.push_str(&count.to_string());
            *count = count.wrapping_add(increment);
        }
    }
    out
}

// Standard object properties in the Help group are REF, VALUE and DEVICE.
// Property names compare case-insensitively.
fn prop_matches(name: &str, candidate: &str) -> bool {
    name.eq_ignore_ascii_case(candidate)
}

fn standard_prop<'a>(comp: &'a mut Component, name: &str) -> Option<&'a mut String> {
    if prop_matches(name, "ref") || prop_matches(name, "reference") {
        Some(&mut comp.refdes)
    } else if prop_matches(name, "value") {
        Some(&mut comp.value)
    } else if prop_matches(name, "device") {
        Some(&mut comp.part)
    } else {
        None
    }
}

fn take_prop(comp: &mut Component, name: &str) -> Option<String> {
    if let Some(slot) = standard_prop(comp, name) {
        return Some(std::mem::take(slot));
    }
    comp.props
        .iter()
        .position(|(key, _)| prop_matches(key, name))
        .map(|i| comp.props.remove(i).1)
}

fn set_prop(comp: &mut Component, name: &str, value: String) {
    if let Some(slot) = standard_prop(comp, name) {
        *slot = value;
        return;
    }
    if let Some((_, slot)) = comp
        .props
        .iter_mut()
        .find(|(key, _)| prop_matches(key, name))
    {
        *slot = value;
    } else {
        comp.props.push((name.to_string(), value));
    }
}

fn set_prop_hidden(comp: &mut Component, name: &str, hidden: bool) {
    if let Some(index) = comp
        .hidden_props
        .iter()
        .position(|candidate| prop_matches(candidate, name))
    {
        if !hidden {
            comp.hidden_props.remove(index);
        }
    } else if hidden {
        comp.hidden_props.push(name.to_string());
    }
}

/// Strips brace markers and turns them into visibility flags. The literal escapes are "{}" -> "{" and "}}" -> "}".
fn parse_prop_expression(raw: &str) -> (String, String, u16) {
    let bytes = raw.as_bytes();
    let mut clean = Vec::with_capacity(bytes.len());
    let mut flags = ui_text::PROPERTY_ASSIGN_FLAG_DEFAULT;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if i == 0 {
                    flags = (flags & !ui_text::PROPERTY_ASSIGN_FLAG_DEFAULT)
                        | ui_text::PROPERTY_ASSIGN_FLAG_FIRST_BRACE;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                    clean.push(b'{');
                    i += 1;
                } else if bytes[i - 1] == b'=' {
                    flags = (flags & !ui_text::PROPERTY_ASSIGN_FLAG_DEFAULT)
                        | ui_text::PROPERTY_ASSIGN_FLAG_EQUALS_BRACE;
                }
            }
            b'}' => {
                if i + 1 == bytes.len() {
                    flags = (flags & !ui_text::PROPERTY_ASSIGN_FLAG_DEFAULT)
                        | ui_text::PROPERTY_ASSIGN_FLAG_EQUALS_BRACE;
                } else if bytes[i + 1] == b'}' {
                    clean.push(b'}');
                    i += 1;
                }
            }
            byte => clean.push(byte),
        }
        i += 1;
    }
    let clean = String::from_utf8_lossy(&clean);
    let (name, pattern) = clean
        .split_once('=')
        .map(|(name, value)| (name.trim(), value.trim_start()))
        .unwrap_or((clean.trim(), ""));
    (name.to_string(), pattern.to_string(), flags)
}

fn parse_prop_size(pattern: &str, count: &mut i32, increment: i32) -> Option<(i32, i32)> {
    let value = expand_prop_value(pattern, count, increment);
    let mut dimensions = value.split(',').map(str::trim);
    let height = dimensions.next()?.parse::<i32>().ok()?;
    if !(1..=ui_text::PROPERTY_ASSIGN_MAX_DIM).contains(&height) {
        return None;
    }
    let width = dimensions
        .next()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| (1..=ui_text::PROPERTY_ASSIGN_MAX_DIM).contains(value))
        .unwrap_or(height);
    Some((height, width))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    About,
    Grid,
    Rules,
    NewProject,
    Open,
    Save,
    Export,
    Preferences,
    Help,
    PickDevices,
    Properties,
    PropertyAssign,
    SearchTag,
}

impl Dialog {
    fn title(&self) -> &'static str {
        match self {
            Dialog::About => "About Voltwright",
            Dialog::Grid => "Grid Settings",
            Dialog::Rules => "Design Rules",
            Dialog::NewProject => ui_text::NEW_PROJECT_WIZARD_TITLE,
            Dialog::Open => ui_text::OPEN_PROJECT_CAPTION,
            Dialog::Save => ui_text::SAVE_PROJECT_CAPTION,
            Dialog::Export => "Export",
            Dialog::Preferences => "Preferences",
            Dialog::Help => "Help",
            Dialog::PickDevices => "Pick Devices",
            Dialog::Properties => "Edit Properties",
            Dialog::PropertyAssign => ui_text::PROPERTY_ASSIGN_TITLE,
            Dialog::SearchTag => ui_text::SEARCH_TAG_TITLE,
        }
    }
}

pub struct Dialogs {
    pub active: Option<Dialog>,
    pub new_name: String,
    pub open_path: String,
    pub save_path: String,
    pub export_dir: String,
    pub message: String,
    /// Search text and result for the P-key Pick Devices dialog.
    pub pick_search: String,
    pub pick_selected: Option<String>,
    /// Component being edited by the E-key properties dialog.
    pub edit_comp: Option<u64>,
    /// Property Assignment tool state.
    pub property_assign_string: String,
    pub property_assign_count: i32,
    pub property_assign_increment: i32,
    pub property_assign_action: usize,
    pub property_assign_apply_to: usize,
    pub property_assign_flags: u16,
    pub property_assign_result: String,
    pub property_assign_apply: bool,
    /// Selection captured by the shell when the dialog opens.
    pub property_assign_targets: Vec<u64>,
    /// Search and Tag tool state (Tools menu, shortcut T).
    pub search_tag_property: String,
    pub search_tag_string: String,
    pub search_tag_mode: SearchMode,
    pub search_tag_match: MatchMode,
    pub search_tag_whole: bool,
    pub search_tag_case: bool,
    /// Ids kept between searches so OR/AND refinement can use the prior list.
    pub search_tag_tagged: Vec<u64>,
    pub search_tag_result: String,
    /// One-shot signal for the shell to mirror tags onto the schematic.
    pub search_tag_apply: bool,
}

impl Default for Dialogs {
    fn default() -> Self {
        Dialogs {
            active: None,
            new_name: "New Project".into(),
            open_path: ui_text::NEW_PROJECT_FILENAME.into(),
            save_path: ui_text::NEW_PROJECT_FILENAME.into(),
            export_dir: "exports".into(),
            message: String::new(),
            pick_search: String::new(),
            pick_selected: None,
            edit_comp: None,
            property_assign_string: ui_text::PROPERTY_ASSIGN_DEFAULT_STRING.into(),
            property_assign_count: ui_text::PROPERTY_ASSIGN_DEFAULT_COUNT,
            property_assign_increment: ui_text::PROPERTY_ASSIGN_DEFAULT_INCREMENT,
            property_assign_action: ui_text::PROP_ACTION_ASSIGN,
            property_assign_apply_to: 0,
            property_assign_flags: ui_text::PROPERTY_ASSIGN_FLAG_DEFAULT,
            property_assign_result: String::new(),
            property_assign_apply: false,
            property_assign_targets: Vec::new(),
            search_tag_property: "Reference".into(),
            search_tag_string: String::new(),
            search_tag_mode: SearchMode::New,
            search_tag_match: MatchMode::Contains,
            search_tag_whole: false,
            search_tag_case: false,
            search_tag_tagged: Vec::new(),
            search_tag_result: String::new(),
            search_tag_apply: false,
        }
    }
}

impl Dialogs {
    /// Reset count, step, action and flags each time the tool opens; keep the
    /// last text the user typed.
    pub fn prepare_property_assign(&mut self, targets: Vec<u64>) {
        self.property_assign_targets = targets;
        self.property_assign_result.clear();
        self.property_assign_count = ui_text::PROPERTY_ASSIGN_DEFAULT_COUNT;
        self.property_assign_increment = ui_text::PROPERTY_ASSIGN_DEFAULT_INCREMENT;
        self.property_assign_action = ui_text::PROP_ACTION_ASSIGN;
        self.property_assign_flags = ui_text::PROPERTY_ASSIGN_FLAG_DEFAULT;
    }

    pub fn open(&mut self, d: Dialog) {
        self.active = Some(d);
        self.message.clear();
    }

    /// Show the active dialog. Returns a status message when something happened.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        proj: &mut Project,
        lib: &Library,
    ) -> Option<String> {
        let d = self.active.clone()?;
        let mut open = true;
        let mut result: Option<String> = None;

        let mut window = egui::Window::new(d.title())
            .id(egui::Id::unique("dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]);
        if d == Dialog::NewProject {
            window = window.default_size([
                ui_text::NEW_PROJECT_WIZARD_SIZE.0 as f32,
                ui_text::NEW_PROJECT_WIZARD_SIZE.1 as f32,
            ]);
        }
        window.show(ctx, |ui| match d {
                Dialog::About => {
                    ui.heading("Voltwright");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(6.0);
                    ui.label("Open-source schematic capture, live circuit simulation and PCB layout,");
                    ui.label("written in Rust.");
                    ui.add_space(6.0);
                    ui.weak("Released under the MIT licence.");
                    ui.add_space(6.0);
                    ui.weak(format!("Grid snap values: {:?}", proj.grid.snaps));
                    ui.weak(format!("Design rules: {} / {} / {} th", proj.rules.pad_pad, proj.rules.pad_trace, proj.rules.trace_trace));
                }
                Dialog::Grid => {
                    ui.horizontal(|ui| {
                        ui.label("Units:");
                        ui.radio_value(&mut proj.grid.units, UnitSystem::Imperial, "Imperial (th)");
                        ui.radio_value(&mut proj.grid.units, UnitSystem::Metric, "Metric (mm)");
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut proj.grid.show_dots, "Show grid");
                        ui.checkbox(&mut proj.grid.snap_on, "Snap to grid");
                    });
                    ui.add_space(6.0);
                    ui.strong("Snap sizes (th)");
                    ui.horizontal(|ui| {
                        for i in 0..4 {
                            ui.label(format!("#{}", i + 1));
                            let r = ui.add(
                                egui::DragValue::new(&mut proj.grid.snaps[i])
                                    .speed(0.5)
                                    .range(0.1..=500.0),
                            );
                            if r.changed() {
                                proj.dirty = true;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Active snap:");
                        ui.selectable_value(&mut proj.grid.active, 0, "F1");
                        ui.selectable_value(&mut proj.grid.active, 1, "F2");
                        ui.selectable_value(&mut proj.grid.active, 2, "F3");
                        ui.selectable_value(&mut proj.grid.active, 3, "F4");
                    });
                    ui.add_space(6.0);
                    ui.weak(format!(
                        "Active step = {:.2} th ({:.3} mm)",
                        proj.grid.step(),
                        proj.grid.step() / 1000.0 * 25.4
                    ));
                    if !self.message.is_empty() {
                        ui.weak(&self.message);
                    }
                }
                Dialog::Rules => {
                    ui.strong("Clearances (th)");
                    let mut changed = false;
                    for (label, val) in [
                        ("Pad to Pad", &mut proj.rules.pad_pad),
                        ("Pad to Trace", &mut proj.rules.pad_trace),
                        ("Trace to Trace", &mut proj.rules.trace_trace),
                        ("Copper to Graphic", &mut proj.rules.graphic),
                        ("Copper to Edge", &mut proj.rules.edge),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            changed |= ui
                                .add(egui::DragValue::new(val).speed(0.5).range(0.0..=500.0))
                                .changed();
                        });
                    }
                    if changed {
                        proj.dirty = true;
                    }
                    ui.add_space(6.0);
                    ui.weak("Violations are reported in the PCB status bar and marked on the board.");
                    let n = proj.drc().len();
                    if n == 0 {
                        ui.colored_label(OK, "No design rule violations.");
                    } else {
                        ui.colored_label(ERR, format!("{n} design rule violation(s)."));
                    }
                }
                Dialog::NewProject => {
                    ui.horizontal(|ui| {
                        ui.label("Project name:");
                        ui.add(egui::TextEdit::singleline(&mut self.new_name).desired_width(220.0));
                    });
                    if !self.message.is_empty() {
                        ui.add_space(4.0);
                        ui.colored_label(WARN, &self.message);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled(
                            false,
                            egui::Button::new(ui_text::NEW_PROJECT_WIZARD_BUTTONS[0]),
                        );
                        if ui.button(ui_text::NEW_PROJECT_WIZARD_BUTTONS[1]).clicked() {
                            let name = self.new_name.trim().to_string();
                            if name.is_empty() {
                                self.message = "Enter a project name.".into();
                            } else {
                                let units = proj.grid.units;
                                *proj = Project::new(&name);
                                proj.grid = match units {
                                    UnitSystem::Imperial => crate::model::GridSettings::imperial(),
                                    UnitSystem::Metric => crate::model::GridSettings::metric(),
                                };
                                result = Some(format!("Created project '{name}'"));
                                self.active = None;
                            }
                        }
                        if ui.button(ui_text::NEW_PROJECT_WIZARD_BUTTONS[2]).clicked() {
                            self.active = None;
                        }
                        ui.add_enabled(
                            false,
                            egui::Button::new(ui_text::NEW_PROJECT_WIZARD_BUTTONS[3]),
                        );
                    });
                }
                Dialog::Open => {
                    ui.horizontal(|ui| {
                        ui.label("Project file:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.open_path)
                                .desired_width(280.0)
                                .hint_text(ui_text::NEW_PROJECT_FILENAME),
                        );
                    });
                    ui.add_space(4.0);
                    ui.label("Opens a project saved by Voltwright. Export files are separate.");
                    ui.weak(ui_text::OPEN_FILTERS);
                    if !self.message.is_empty() {
                        let ok = self.message.starts_with("Opened");
                        ui.add_space(4.0);
                        ui.colored_label(if ok { OK } else { ERR }, &self.message);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            let path = std::path::PathBuf::from(self.open_path.trim());
                            let foreign = path
                                .extension()
                                .is_some_and(|e| e.eq_ignore_ascii_case("pdsprj"));
                            let opened = if foreign {
                                crate::pdsprj::import(&path, lib)
                            } else {
                                crate::io::open(&path).map(|p| {
                                    let msg = format!("Opened project '{}'", p.name);
                                    (p, msg)
                                })
                            };
                            match opened {
                                Ok((p, msg)) => {
                                    result = Some(msg);
                                    *proj = p;
                                    // Imported files are saved as our own format.
                                    self.save_path = if foreign {
                                        path.with_extension("volt").to_string_lossy().into_owned()
                                    } else {
                                        self.open_path.clone()
                                    };
                                    self.active = None;
                                }
                                Err(e) => self.message = format!("Open failed: {e}"),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.active = None;
                        }
                    });
                }
                Dialog::Save => {
                    ui.horizontal(|ui| {
                        ui.label("Project file:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.save_path)
                                .desired_width(280.0)
                                .hint_text(ui_text::NEW_PROJECT_FILENAME),
                        );
                    });
                    ui.add_space(4.0);
                    ui.label("Writes the schematic, PCB, notes, source and grid settings.");
                    ui.weak(format!(
                        "Project: {}   Parts: {}   Wires: {}   Tracks: {}",
                        proj.name,
                        proj.comps.len(),
                        proj.wires.len(),
                        proj.tracks.len()
                    ));
                    if !self.message.is_empty() {
                        let ok = self.message.starts_with("Saved");
                        ui.add_space(4.0);
                        ui.colored_label(if ok { OK } else { ERR }, &self.message);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let path = std::path::PathBuf::from(&self.save_path);
                            match crate::io::save(proj, &path) {
                                Ok(msg) => {
                                    proj.path = Some(path);
                                    proj.dirty = false;
                                    self.open_path = self.save_path.clone();
                                    result = Some(msg);
                                    self.active = None;
                                }
                                Err(e) => self.message = format!("Save failed: {e}"),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.active = None;
                        }
                    });
                }
                Dialog::Export => {
                    ui.horizontal(|ui| {
                        ui.label("Folder:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.export_dir)
                                .desired_width(240.0)
                                .hint_text("relative to the working directory"),
                        );
                    });
                    ui.add_space(4.0);
                    if !self.message.is_empty() {
                        let ok = self.message.starts_with("Wrote");
                        ui.colored_label(if ok { OK } else { ERR }, &self.message);
                    }
                    ui.add_space(6.0);
                    ui.strong("Bill of Materials");
                    ui.horizontal(|ui| {
                        for (label, kind) in [
                            ("CSV", "csv"),
                            ("HTML", "html"),
                            ("Text", "txt"),
                        ] {
                            if ui.button(label).clicked() {
                                result = Some(export_bom(proj, lib, &self.export_dir, kind));
                                self.message = result.clone().unwrap_or_default();
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.strong("Manufacturing");
                    ui.horizontal(|ui| {
                        if ui.button("Gerber + Drill").clicked() {
                            result = Some(export_gerber(proj, &self.export_dir));
                            self.message = result.clone().unwrap_or_default();
                        }
                        if ui.button("Pick and Place").clicked() {
                            result = Some(export_pnp(proj, &self.export_dir));
                            self.message = result.clone().unwrap_or_default();
                        }
                    });
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.active = None;
                    }
                }
                Dialog::Preferences => {
                    ui.strong("Display");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut proj.grid.show_dots, "Show grid dots");
                        ui.checkbox(&mut proj.grid.snap_on, "Enable snapping");
                    });
                    ui.add_space(6.0);
                    ui.strong("Default units");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut proj.grid.units, UnitSystem::Imperial, "Imperial (th)");
                        ui.radio_value(&mut proj.grid.units, UnitSystem::Metric, "Metric (mm)");
                    });
                    ui.add_space(6.0);
                    ui.strong("Canvas");
                    ui.label(format!(
                        "Background {:02X}{:02X}{:02X}, wire {:02X}{:02X}{:02X}, components {:02X}{:02X}{:02X}",
                        CANVAS.r(), CANVAS.g(), CANVAS.b(),
                        WIRE.r(), WIRE.g(), WIRE.b(),
                        BODY_GREEN.r(), BODY_GREEN.g(), BODY_GREEN.b(),
                    ));
                    ui.add_space(8.0);
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.active = None;
                    }
                }
                Dialog::Help => {
                    ui.strong("Mouse");
                    for t in [
                        "Left click on a green pin - start or finish a wire",
                        "Left click in empty space - add a wire anchor",
                        "Right click on a wire    - anchor undo / context menu",
                        "Right click on a part    - Edit Properties, Rotate, Delete",
                        "Left drag a wire segment - move just that segment",
                        "Double click a green pin - repeat the last aligned wire",
                        "Middle wheel             - zoom around the cursor",
                        "Middle click + move      - track pan, click to drop",
                        "Shift + drag             - zoom into a box",
                    ] {
                        ui.label(format!("   {t}"));
                    }
                    ui.add_space(6.0);
                    ui.strong("Keyboard");
                    for t in [
                        "P                - Pick Devices",
                        "W                - Wire tool",
                        "E                - Edit Properties of selected part",
                        "R                - rotate the selection 90 degrees (added)",
                        "+ / - (numpad)   - rotate part while placing or moving (help)",
                        "Delete / Esc     - delete selection / cancel tool",
                        "Enter            - finish the current wire",
                        "F5               - centre view on cursor",
                        "F6 / F7          - zoom in / out",
                        "F8               - zoom to whole drawing",
                        "G                - grid dots / lines / off",
                        "O                - toggle manual origin",
                        "X                - toggle X cursor",
                        "Ctrl+F1 / F2/F3/F4 - select 10th/50th/100th/500th grid",
                        "R                - redraw display (also rotates while placing)",
                        "Ctrl+A / B / F   - align / send back / bring front",
                        "Ctrl+Z / Ctrl+Y  - undo / redo",
                        "Ctrl+N/O/S       - new / open / save project",
                    ] {
                        ui.label(format!("   {t}"));
                    }
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.active = None;
                    }
                }
                Dialog::PickDevices => {
                    ui.label("Search by name, reference or category:");
                    let search = ui.text_edit_singleline(&mut self.pick_search);
                    search.request_focus();
                    let q = self.pick_search.trim().to_uppercase();
                    egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                        for def in &lib.parts {
                            let hay = format!(
                                "{} {} {} {} {}",
                                def.id, def.name, def.category, def.library, def.prefix
                            )
                            .to_uppercase();
                            if !q.is_empty() && !hay.contains(&q) {
                                continue;
                            }
                            let clicked = ui
                                .horizontal(|ui| {
                                    crate::schematic::part_icon(ui, def, lib);
                                    ui.button(format!("{}  -  {}", def.id, def.name)).clicked()
                                })
                                .inner;
                            if clicked {
                                self.pick_selected = Some(def.id.clone());
                                self.active = None;
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.active = None;
                    }
                }
                Dialog::Properties => {
                    let Some(id) = self.edit_comp else {
                        ui.label("Nothing selected.");
                        if ui.button("Close").clicked() {
                            self.active = None;
                        }
                        return;
                    };
                    let Some(c) = proj.comp_mut(id) else {
                        ui.label("The part no longer exists.");
                        if ui.button("Close").clicked() {
                            self.active = None;
                        }
                        return;
                    };
                    ui.horizontal(|ui| {
                        ui.label("Reference");
                        ui.text_edit_singleline(&mut c.refdes);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Value");
                        ui.text_edit_singleline(&mut c.value);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Rotation");
                        ui.add(egui::DragValue::new(&mut c.rot).speed(90).range((-360)..=360));
                        ui.label("degrees");
                    });
                    ui.separator();
                    ui.weak(format!("Device: {}", c.part));
                    for (k, v) in &c.props {
                        ui.horizontal(|ui| {
                            ui.weak(format!("{k}:"));
                            ui.label(v);
                        });
                    }
                    ui.add_space(8.0);
                    if ui.button("Done").clicked() {
                        proj.dirty = true;
                        self.active = None;
                    }
                }
                Dialog::PropertyAssign => {
                    ui.weak(qt(ui_text::PROPERTY_ASSIGN_HINT));
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(qt(ui_text::PROPERTY_ASSIGN_COUNT_LABEL));
                        ui.add(egui::DragValue::new(&mut self.property_assign_count));
                    });
                    ui.horizontal(|ui| {
                        ui.label(qt(ui_text::PROPERTY_ASSIGN_INCREMENT_LABEL));
                        ui.add(egui::DragValue::new(&mut self.property_assign_increment));
                    });
                    ui.horizontal(|ui| {
                        ui.label(qt(ui_text::PROPERTY_ASSIGN_STRING_LABEL));
                        ui.text_edit_singleline(&mut self.property_assign_string);
                    });
                    ui.separator();
                    ui.label(qt(ui_text::PROPERTY_ASSIGN_APPLY_TO_TITLE));
                    for (i, label) in ui_text::PROPERTY_ASSIGN_APPLY_TO.iter().enumerate() {
                        ui.radio_value(&mut self.property_assign_apply_to, i, qt(*label));
                    }
                    ui.label(qt(ui_text::PROPERTY_ASSIGN_ACTION_TITLE));
                    for (i, label) in ui_text::PROPERTY_ASSIGN_ACTIONS.iter().enumerate() {
                        ui.radio_value(&mut self.property_assign_action, i, qt(*label));
                    }
                    ui.separator();
                    ui.collapsing(qt(ui_text::PROPERTY_ASSIGN_HELP_TITLE), |ui| {
                        for label in ui_text::PROPERTY_ASSIGN_HELP_LABELS {
                            ui.label(qt(label));
                        }
                    });
                    ui.separator();

                    if ui.button(qt(ui_text::PROPERTY_ASSIGN_OK)).clicked() {
                        let (name, pattern, flags) =
                            parse_prop_expression(self.property_assign_string.trim());
                        self.property_assign_flags = match self.property_assign_action {
                            ui_text::PROP_ACTION_SHOW => 0,
                            ui_text::PROP_ACTION_HIDE => 3,
                            _ => flags,
                        };
                        if name.is_empty() {
                            self.property_assign_result = "Enter a property name.".into();
                        } else {
                            let targets: Vec<u64> = match self.property_assign_apply_to {
                                0 | 1 => self.property_assign_targets.clone(),
                                2 => self.search_tag_tagged.clone(),
                                _ => proj.comps.iter().map(|c| c.id).collect(),
                            };
                            if targets.is_empty() {
                                self.property_assign_result =
                                    "No objects in the selected scope.".into();
                            } else {
                                let wanted = self.property_assign_action;
                                let mut count = self.property_assign_count;
                                let increment = self.property_assign_increment;
                                let mut changed = 0usize;
                                for comp in &mut proj.comps {
                                    if !targets.contains(&comp.id) {
                                        continue;
                                    }
                                    match wanted {
                                        ui_text::PROP_ACTION_ASSIGN => {
                                            let value =
                                                expand_prop_value(&pattern, &mut count, increment);
                                            set_prop(comp, &name, value);
                                        }
                                        ui_text::PROP_ACTION_REMOVE => {
                                            take_prop(comp, &name);
                                            set_prop_hidden(comp, &name, false);
                                        }
                                        ui_text::PROP_ACTION_RENAME => {
                                            let value =
                                                expand_prop_value(&pattern, &mut count, increment);
                                            let was_hidden = comp
                                                .hidden_props
                                                .iter()
                                                .any(|candidate| prop_matches(candidate, &name));
                                            if let Some(old) = take_prop(comp, &name) {
                                                set_prop(comp, &value, old);
                                                set_prop_hidden(comp, &name, false);
                                                set_prop_hidden(comp, &value, was_hidden);
                                            }
                                        }
                                        ui_text::PROP_ACTION_SHOW => {
                                            set_prop_hidden(comp, &name, false);
                                        }
                                        ui_text::PROP_ACTION_HIDE => {
                                            set_prop_hidden(comp, &name, true);
                                        }
                                        ui_text::PROP_ACTION_RESIZE => {
                                            if prop_matches(&name, "TEXT") {
                                                if let Some(size) = parse_prop_size(
                                                    &pattern,
                                                    &mut count,
                                                    increment,
                                                ) {
                                                    comp.text_size = Some(size);
                                                } else {
                                                    continue;
                                                }
                                            } else {
                                                continue;
                                            }
                                        }
                                        _ => continue,
                                    }
                                    changed += 1;
                                }
                                self.property_assign_count = count;
                                self.property_assign_result =
                                    format!("{changed} object(s) processed.");
                                if changed > 0 {
                                    self.property_assign_apply = true;
                                    proj.dirty = true;
                                    result = Some(self.property_assign_result.clone());
                                }
                            }
                        }
                        self.active = None;
                    }
                    if ui.button(qt(ui_text::PROPERTY_ASSIGN_CANCEL)).clicked() {
                        self.property_assign_result.clear();
                        self.active = None;
                    }
                    if !self.property_assign_result.is_empty() {
                        ui.add_space(4.0);
                        ui.label(&self.property_assign_result);
                    }
                }
                Dialog::SearchTag => {
                    ui.horizontal(|ui| {
                        ui.label(qt(ui_text::SEARCH_TAG_PROPERTY_LABEL));
                        ui.text_edit_singleline(&mut self.search_tag_property);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Reference").clicked() {
                            self.search_tag_property = "Reference".into();
                        }
                        if ui.button("Value").clicked() {
                            self.search_tag_property = "Value".into();
                        }
                        if ui.button("Device").clicked() {
                            self.search_tag_property = "Device".into();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(qt(ui_text::SEARCH_TAG_STRING_LABEL));
                        ui.text_edit_singleline(&mut self.search_tag_string).request_focus();
                    });
                    ui.group(|ui| {
                        ui.label(qt(ui_text::SEARCH_TAG_MODE_TITLE));
                        ui.radio_value(&mut self.search_tag_mode, SearchMode::New, qt(ui_text::SEARCH_TAG_MODE_NEW));
                        ui.radio_value(&mut self.search_tag_mode, SearchMode::Or, qt(ui_text::SEARCH_TAG_MODE_OR));
                        ui.radio_value(&mut self.search_tag_mode, SearchMode::And, qt(ui_text::SEARCH_TAG_MODE_AND));
                    });
                    ui.group(|ui| {
                        ui.label(qt(ui_text::SEARCH_TAG_MATCH_TITLE));
                        let modes = [
                            (MatchMode::NotBegins, ui_text::SEARCH_TAG_MATCH_MODES[0]),
                            (MatchMode::NotEquals, ui_text::SEARCH_TAG_MATCH_MODES[1]),
                            (MatchMode::Contains, ui_text::SEARCH_TAG_MATCH_MODES[2]),
                            (MatchMode::NotContains, ui_text::SEARCH_TAG_MATCH_MODES[3]),
                            (MatchMode::Begins, ui_text::SEARCH_TAG_MATCH_MODES[4]),
                            (MatchMode::Equals, ui_text::SEARCH_TAG_MATCH_MODES[5]),
                        ];
                        for (mode, label) in modes {
                            ui.radio_value(&mut self.search_tag_match, mode, qt(label));
                        }
                    });
                    ui.checkbox(
                        &mut self.search_tag_whole,
                        qt(ui_text::SEARCH_TAG_WHOLE_DESIGN),
                    );
                    ui.checkbox(
                        &mut self.search_tag_case,
                        qt(ui_text::SEARCH_TAG_CASE_SENSITIVE),
                    );
                    ui.weak("Projects have one sheet for now; this searches every part.");
                    ui.separator();

                    if ui.button("Tag All").clicked() {
                        self.search_tag_tagged = proj.comps.iter().map(|c| c.id).collect();
                        self.search_tag_result =
                            format!("All {} component(s) tagged.", self.search_tag_tagged.len());
                        self.search_tag_apply = true;
                        result = Some(self.search_tag_result.clone());
                    }
                    if ui.button("Un-tag All").clicked() {
                        self.search_tag_tagged.clear();
                        self.search_tag_result = "All tags cleared.".into();
                        self.search_tag_apply = true;
                        result = Some(self.search_tag_result.clone());
                    }
                    if ui.button(qt(ui_text::SEARCH_TAG_BUTTON)).clicked() {
                        let needle = if self.search_tag_case {
                            self.search_tag_string.clone()
                        } else {
                            self.search_tag_string.to_lowercase()
                        };
                        let prop = self.search_tag_property.trim().to_lowercase();
                        let mut matches = Vec::new();
                        for c in &proj.comps {
                            let field = match prop.as_str() {
                                "reference" | "ref" | "refdes" => c.refdes.clone(),
                                "value" => c.value.clone(),
                                "device" | "part" | "name" => c.part.clone(),
                                other => c
                                    .props
                                    .iter()
                                    .find(|(k, _)| k.eq_ignore_ascii_case(other))
                                    .map(|(_, v)| v.clone())
                                    .unwrap_or_default(),
                            };
                            let hay = if self.search_tag_case {
                                field
                            } else {
                                field.to_lowercase()
                            };
                            if self.search_tag_match.matches(&hay, &needle) {
                                matches.push(c.id);
                            }
                        }
                        let matched = matches.len();
                        self.search_tag_tagged = match self.search_tag_mode {
                            SearchMode::New => matches,
                            SearchMode::Or => {
                                let mut ids: HashSet<u64> =
                                    self.search_tag_tagged.iter().copied().collect();
                                ids.extend(matches);
                                ids.into_iter().collect()
                            }
                            SearchMode::And => {
                                let ids: HashSet<u64> =
                                    self.search_tag_tagged.iter().copied().collect();
                                matches.into_iter().filter(|id| ids.contains(id)).collect()
                            }
                        };
                        self.search_tag_result = ui_text::SEARCH_TAG_RESULT_FORMAT
                            .replacen("%d", &matched.to_string(), 1)
                            .replacen("%d", &self.search_tag_tagged.len().to_string(), 1);
                        self.search_tag_apply = true;
                        result = Some(self.search_tag_result.clone());
                    }
                    if !self.search_tag_result.is_empty() {
                        ui.add_space(4.0);
                        ui.label(&self.search_tag_result);
                        let refs: Vec<String> = self
                            .search_tag_tagged
                            .iter()
                            .filter_map(|id| proj.comp(*id))
                            .take(12)
                            .map(|c| c.refdes.clone())
                            .collect();
                        ui.weak(format!("Tagged: {}", refs.join(", ")));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(qt(ui_text::SEARCH_TAG_DONE)).clicked() {
                            self.active = None;
                        }
                        if ui.button(qt(ui_text::SEARCH_TAG_CANCEL)).clicked() {
                            self.search_tag_tagged.clear();
                            self.search_tag_result.clear();
                            self.search_tag_apply = true;
                            self.active = None;
                        }
                    });
                }
            });

        if !open {
            self.active = None;
        }
        result
    }
}

fn ensure(dir: &str) -> std::io::Result<std::path::PathBuf> {
    let p = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

fn export_bom(proj: &Project, lib: &Library, dir: &str, kind: &str) -> String {
    let rows = crate::bom::build(proj, lib);
    let body = match kind {
        "csv" => crate::bom::to_csv(&rows),
        "html" => crate::bom::to_html(proj, &rows),
        _ => crate::bom::to_text(proj, &rows),
    };
    let name = match kind {
        "csv" => "bom.csv",
        "html" => "bom.html",
        _ => "bom.txt",
    };
    match ensure(dir) {
        Ok(p) => {
            let p = p.join(name);
            match std::fs::write(&p, body) {
                Ok(_) => format!("Wrote {} ({} line items)", p.display(), rows.len()),
                Err(e) => format!("Export failed: {e}"),
            }
        }
        Err(e) => format!("Export failed: {e}"),
    }
}

fn export_gerber(proj: &Project, dir: &str) -> String {
    let mut top = gerber_header("Top Copper");
    let mut bottom = gerber_header("Bottom Copper");
    let mut outline = gerber_header("Board Outline");
    let mut drill = String::from("M48\n; drill file\nMETRIC,TZ\n");

    for t in &proj.tracks {
        let (dst, code) = if t.layer == crate::model::Layer::TopCopper {
            (&mut top, "D10")
        } else {
            (&mut bottom, "D11")
        };
        dst.push_str(&format!("{}\n", format!("X{}Y{}", mm(t.a.x), mm(t.a.y))));
        dst.push_str(&format!("{}\n", format!("D02*")));
        dst.push_str(&format!("G01*\n{}*\n", code));
        dst.push_str(&format!("X{}Y{}D01*\n", mm(t.b.x), mm(t.b.y)));
    }
    let (o, e) = proj.board.rect();
    outline.push_str(&format!("G01*\nD12*\n"));
    outline.push_str(&format!("X{}Y{}D02*\n", mm(o.x), mm(o.y)));
    outline.push_str(&format!("X{}Y{}D01*\n", mm(e.x), mm(o.y)));
    outline.push_str(&format!("X{}Y{}D01*\n", mm(e.x), mm(e.y)));
    outline.push_str(&format!("X{}Y{}D01*\n", mm(o.x), mm(e.y)));
    outline.push_str(&format!("X{}Y{}D01*\n", mm(o.x), mm(o.y)));

    for f in &proj.fps {
        for pad in &f.pads {
            let wp = crate::pcb::absolute_pad(f, pad);
            drill.push_str(&format!("X{}Y{}\n", mm(wp.x), mm(wp.y)));
            top.push_str(&format!("X{}Y{}D03*\n", mm(wp.x), mm(wp.y)));
        }
    }
    for v in &proj.vias {
        drill.push_str(&format!("X{}Y{}\n", mm(v.pos.x), mm(v.pos.y)));
        top.push_str(&format!("X{}Y{}D03*\n", mm(v.pos.x), mm(v.pos.y)));
    }
    drill.push_str("M30\n");
    for f in [&mut top, &mut bottom, &mut outline] {
        f.push_str("M02*\n");
    }

    match ensure(dir) {
        Ok(p) => {
            let mut n = 0;
            for (name, body) in [
                ("board.GTL", top),
                ("board.GBL", bottom),
                ("board.GKO", outline),
                ("board.TXT", drill),
            ] {
                match std::fs::write(p.join(name), body) {
                    Ok(_) => n += 1,
                    Err(e) => return format!("Export failed: {e}"),
                }
            }
            format!("Wrote {n} manufacturing files to {}", p.display())
        }
        Err(e) => format!("Export failed: {e}"),
    }
}

fn export_pnp(proj: &Project, dir: &str) -> String {
    let mut out = String::from("Designator,Package,Mid X,Mid Y,Rotation,Layer\r\n");
    for f in &proj.fps {
        out.push_str(&format!(
            "{},{},{:.4}mm,{:.4}mm,{},Top\r\n",
            f.refdes,
            f.package,
            f.x / 1000.0 * 25.4,
            f.y / 1000.0 * 25.4,
            f.rot
        ));
    }
    match ensure(dir) {
        Ok(p) => {
            let p = p.join("pick-and-place.csv");
            match std::fs::write(&p, out) {
                Ok(_) => format!("Wrote {} ({} parts)", p.display(), proj.fps.len()),
                Err(e) => format!("Export failed: {e}"),
            }
        }
        Err(e) => format!("Export failed: {e}"),
    }
}

fn mm(th: f32) -> i64 {
    (th as f64 / 1000.0 * 25.4 * 1000.0).round() as i64
}

fn gerber_header(name: &str) -> String {
    format!(
        "%TF.FileFunction,{}*%\n%FSLAX46Y46*%\n%MOMM*%\n%LPD*%\nG04 generated by Voltwright*\n",
        name
    )
}

/// Paint a small monospace caption used by the status bar.
pub fn caption(ui: &egui::Ui, text: &str, color: Color32) {
    ui.painter().text(
        ui.next_widget_position(),
        egui::Align2::LEFT_TOP,
        text,
        FontId::new(12.0, FontFamily::Monospace),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_counter_expands_and_advances() {
        let mut count = 0;
        assert_eq!(expand_prop_value("R#", &mut count, 1), "R0");
        assert_eq!(count, 1);
        assert_eq!(expand_prop_value("#", &mut count, 1), "1");
        assert_eq!(count, 2);
    }

    #[test]
    fn prop_counter_honours_increment_and_negative_count() {
        let mut count = 5;
        assert_eq!(expand_prop_value("A#B#C", &mut count, 2), "A5B7C");
        assert_eq!(count, 9);
        let mut count = -1;
        assert_eq!(expand_prop_value("#", &mut count, 1), "-1");
        assert_eq!(count, 0);
    }

    #[test]
    fn prop_counter_double_hash_is_literal() {
        let mut count = 3;
        assert_eq!(expand_prop_value("R###", &mut count, 1), "R#3");
        assert_eq!(count, 4);
        let mut count = 3;
        assert_eq!(expand_prop_value("##", &mut count, 1), "#");
        assert_eq!(count, 3);
    }

    #[test]
    fn prop_names_are_case_insensitive() {
        assert!(prop_matches("REF", "ref"));
        assert!(prop_matches("text", "TEXT"));
        assert!(!prop_matches("value", "part"));
    }

    #[test]
    fn prop_assign_sets_and_removes_case_insensitively() {
        let mut c = Component {
            id: 1,
            part: "RES".into(),
            refdes: "R1".into(),
            value: "10k".into(),
            x: 0.0,
            y: 0.0,
            rot: 0,
            props: vec![("Note".into(), "old".into())],
            hidden_props: Vec::new(),
            text_size: None,
        };
        set_prop(&mut c, "REF", "R2".into());
        assert_eq!(c.refdes, "R2");
        set_prop(&mut c, "note", "new".into());
        assert_eq!(c.props, vec![("Note".into(), "new".into())]);
        assert_eq!(take_prop(&mut c, "NOTE").as_deref(), Some("new"));
        assert!(c.props.is_empty());
    }

    // {} and }} are literal brace escapes, while
    // leading/equals/trailing markers are removed and set bits 1/2.
    #[test]
    fn prop_expression_brace_rules() {
        let (name, value, flags) = parse_prop_expression("REF={}A}}");
        assert_eq!((name.as_str(), value.as_str(), flags), ("REF", "{A}", 4));

        let (name, value, flags) = parse_prop_expression("{REF={R#}");
        assert_eq!((name.as_str(), value.as_str(), flags), ("REF", "R#", 3));
    }

    // Sizes must be 1..=2540; a missing or bad width falls back to
    // a square when the second dimension is absent or invalid.
    #[test]
    fn prop_size_bounds_and_fallback() {
        let mut count = 10;
        assert_eq!(parse_prop_size("25,50", &mut count, 1), Some((25, 50)));
        assert_eq!(parse_prop_size("25", &mut count, 1), Some((25, 25)));
        assert_eq!(parse_prop_size("2541,1", &mut count, 1), None);
    }

    #[test]
    fn property_assign_prepare_restores_edit_defaults() {
        let mut dialogs = Dialogs::default();
        dialogs.property_assign_count = 99;
        dialogs.property_assign_increment = -5;
        dialogs.property_assign_action = ui_text::PROP_ACTION_HIDE;
        dialogs.prepare_property_assign(vec![7, 8]);
        assert_eq!(dialogs.property_assign_targets, vec![7, 8]);
        assert_eq!(dialogs.property_assign_count, 0);
        assert_eq!(dialogs.property_assign_increment, 1);
        assert_eq!(
            dialogs.property_assign_action,
            ui_text::PROP_ACTION_ASSIGN
        );
        assert_eq!(dialogs.property_assign_flags, 4);
    }
}
