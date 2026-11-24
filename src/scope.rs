// Floating oscilloscope window. It draws the traces the simulator records for
// every connected channel of the SCOPE part.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};

use crate::schematic::SCOPE_COLORS;
use crate::sim::Simulator;

pub struct ScopeState {
    pub open: bool,
    pub time_div: f64,
    pub volt_div: f64,
    /// Remove each channel's average so a signal riding on a DC level fills the screen.
    pub ac: bool,
}

impl Default for ScopeState {
    fn default() -> Self {
        ScopeState { open: false, time_div: 5e-4, volt_div: 1.0, ac: false }
    }
}

const TIME_DIVS: [f64; 19] = [
    1e-6, 2e-6, 5e-6, 1e-5, 2e-5, 5e-5, 1e-4, 2e-4, 5e-4, 1e-3, 2e-3, 5e-3, 1e-2, 2e-2, 5e-2, 0.1, 0.2, 0.5, 1.0,
];
const VOLT_DIVS: [f64; 10] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0];

pub fn show(ctx: &egui::Context, st: &mut ScopeState, sim: Option<&Simulator>) {
    if !st.open {
        return;
    }
    let mut open = st.open;
    egui::Window::new("Oscilloscope").open(&mut open).default_size([560.0, 420.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Time/div");
            step_combo(ui, "tdiv", &mut st.time_div, &TIME_DIVS, "s");
            ui.label("Volts/div");
            step_combo(ui, "vdiv", &mut st.volt_div, &VOLT_DIVS, "V");
            ui.checkbox(&mut st.ac, "AC coupling");
        });
        let size = Vec2::new(ui.available_width().max(200.0), (ui.available_width() * 0.7).clamp(160.0, 420.0));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 3.0, Color32::from_rgb(0x04, 0x10, 0x0A));
        let grid = Stroke::new(1.0, Color32::from_rgb(0x1C, 0x3A, 0x28));
        for i in 0..=10 {
            let x = rect.left() + rect.width() * i as f32 / 10.0;
            p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], grid);
        }
        for j in 0..=8 {
            let y = rect.top() + rect.height() * j as f32 / 8.0;
            p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], grid);
        }
        let mid = rect.center().y;
        p.line_segment(
            [Pos2::new(rect.left(), mid), Pos2::new(rect.right(), mid)],
            Stroke::new(1.0, Color32::from_rgb(0x2C, 0x5A, 0x40)),
        );

        let Some(sim) = sim.filter(|s| !s.traces.is_empty()) else {
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Place an oscilloscope part, wire its channels and run the simulation.",
                egui::FontId::proportional(14.0),
                Color32::from_rgb(0x90, 0xA8, 0x98),
            );
            return;
        };
        let window = st.time_div * 10.0;
        let t_end = sim.traces[0].pts.last().map(|s| s.0).unwrap_or(0.0);
        // Trigger on a rising edge of channel 1 through its mid level, using
        // the latest edge that leaves a full screen of data after it.
        let first = &sim.traces[0].pts;
        let (lo, hi) = first
            .iter()
            .filter(|s| s.0 >= t_end - window * 2.0)
            .fold((f32::MAX, f32::MIN), |(l, h), s| (l.min(s.1), h.max(s.1)));
        let level = (lo + hi) * 0.5;
        let mut start = (t_end - window).max(0.0);
        if hi - lo > 0.05 {
            for w in first.windows(2).rev() {
                if w[1].0 > t_end - window {
                    continue;
                }
                if w[0].1 < level && w[1].1 >= level {
                    start = w[1].0;
                    break;
                }
            }
        }
        let vdiv = st.volt_div as f32;
        for (k, tr) in sim.traces.iter().enumerate() {
            let shown: Vec<&(f64, f32)> = tr.pts.iter().filter(|s| s.0 >= start && s.0 <= start + window).collect();
            let offset = if st.ac && !shown.is_empty() {
                shown.iter().map(|s| s.1).sum::<f32>() / shown.len() as f32
            } else {
                0.0
            };
            let pts: Vec<Pos2> = shown
                .into_iter()
                .map(|s| {
                    let s = &(s.0, s.1 - offset);
                    let x = rect.left() + rect.width() * ((s.0 - start) / window) as f32;
                    let y = mid - s.1 / vdiv * rect.height() / 8.0;
                    Pos2::new(x, y.clamp(rect.top(), rect.bottom()))
                })
                .collect();
            if pts.len() > 1 {
                p.add(Shape::line(pts, Stroke::new(1.6, SCOPE_COLORS[k % 4])));
            }
        }
        let _ = Rect::NOTHING;
        ui.horizontal_wrapped(|ui| {
            for (k, tr) in sim.traces.iter().enumerate() {
                let v = tr.pts.last().map(|s| s.1).unwrap_or(0.0);
                ui.colored_label(SCOPE_COLORS[k % 4], format!("{}: {}", tr.label, crate::util::si(v as f64, "V")));
            }
        });
    });
    st.open = open;
}

fn step_combo(ui: &mut egui::Ui, id: &str, value: &mut f64, steps: &[f64], unit: &str) {
    egui::ComboBox::from_id_salt(id).selected_text(crate::util::si(*value, unit)).show_ui(ui, |ui| {
        for s in steps {
            ui.selectable_value(value, *s, crate::util::si(*s, unit));
        }
    });
}
