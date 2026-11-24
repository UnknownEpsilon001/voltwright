// Floating oscilloscope. A CRT-style screen with knobs for time/div,
// volts/div, vertical position and trigger level, a RUN/STOP button, AC
// coupling, and per-channel switches. It draws the traces the simulator
// records for every connected channel of the SCOPE part.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};

use crate::schematic::SCOPE_COLORS;
use crate::sim::Simulator;

type Samples = Vec<(f64, f32)>;

pub struct ScopeState {
    pub open: bool,
    /// Knob positions, 0..1.
    time_knob: f32,
    volt_knob: f32,
    pos_knob: f32,
    trig_knob: f32,
    ac: bool,
    auto_trigger: bool,
    run: bool,
    channels: [bool; 4],
    frozen: Option<Vec<(String, Samples)>>,
    /// Previous frames of each trace, drawn fainter for a phosphor afterglow.
    afterglow: Vec<Vec<Vec<Pos2>>>,
}

impl Default for ScopeState {
    fn default() -> Self {
        ScopeState {
            open: false,
            time_knob: knob_of(TIME_DIVS.len(), 9),
            volt_knob: knob_of(VOLT_DIVS.len(), 6),
            pos_knob: 0.5,
            trig_knob: 0.5,
            ac: false,
            auto_trigger: true,
            run: true,
            channels: [true; 4],
            frozen: None,
            afterglow: Vec::new(),
        }
    }
}

const TIME_DIVS: [f64; 19] = [
    1e-6, 2e-6, 5e-6, 1e-5, 2e-5, 5e-5, 1e-4, 2e-4, 5e-4, 1e-3, 2e-3, 5e-3, 1e-2, 2e-2, 5e-2, 0.1, 0.2, 0.5, 1.0,
];
const VOLT_DIVS: [f64; 10] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0];

fn knob_of(n: usize, i: usize) -> f32 {
    i as f32 / (n - 1) as f32
}

fn index_of(n: usize, t: f32) -> usize {
    ((t * (n - 1) as f32).round() as usize).min(n - 1)
}

/// A rotary knob: drag or scroll to turn it. `t` runs 0..1; `step` is the
/// change for one notch of the mouse wheel.
fn knob(ui: &mut egui::Ui, label: &str, t: &mut f32, step: f32, readout: &str) {
    ui.vertical(|ui| {
        ui.set_width(84.0);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(84.0, 56.0), Sense::click_and_drag());
        if resp.dragged() {
            let d = resp.drag_delta();
            *t = (*t + (d.x - d.y) / 160.0).clamp(0.0, 1.0);
        }
        if resp.hovered() {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                *t = (*t + wheel.signum() * step).clamp(0.0, 1.0);
            }
        }
        let p = ui.painter_at(rect);
        let c = Pos2::new(rect.center().x, rect.top() + 28.0);
        // Scale ticks around the knob, then the knob body and its pointer.
        for i in 0..=10 {
            let (s, co) = angle(i as f32 / 10.0).sin_cos();
            p.line_segment(
                [c + Vec2::new(co, s) * 22.0, c + Vec2::new(co, s) * 26.0],
                Stroke::new(1.0, Color32::from_gray(110)),
            );
        }
        let body = if resp.hovered() || resp.dragged() { Color32::from_gray(70) } else { Color32::from_gray(52) };
        p.circle_filled(c, 19.0, body);
        p.circle_stroke(c, 19.0, Stroke::new(1.5, Color32::from_gray(150)));
        let (s, co) = angle(*t).sin_cos();
        p.line_segment(
            [c + Vec2::new(co, s) * 5.0, c + Vec2::new(co, s) * 17.0],
            Stroke::new(2.5, Color32::from_rgb(0xFF, 0xC8, 0x40)),
        );
        ui.label(egui::RichText::new(label).small().color(Color32::from_gray(170)));
        ui.label(egui::RichText::new(readout).monospace().color(Color32::from_rgb(0x60, 0xF0, 0x80)));
    });
}

/// Pointer angle in radians for a knob position: 270 degrees of travel,
/// starting at the lower left.
fn angle(t: f32) -> f32 {
    (135.0f32 + 270.0 * t).to_radians()
}

pub fn show(ctx: &egui::Context, st: &mut ScopeState, sim: Option<&Simulator>) {
    if !st.open {
        return;
    }
    let mut open = st.open;
    egui::Window::new("Oscilloscope").open(&mut open).default_size([620.0, 560.0]).show(ctx, |ui| {
        let time_div = TIME_DIVS[index_of(TIME_DIVS.len(), st.time_knob)];
        let volt_div = VOLT_DIVS[index_of(VOLT_DIVS.len(), st.volt_knob)];
        let live: Vec<(String, Samples)> = match (st.run, &st.frozen) {
            (false, Some(f)) => f.clone(),
            _ => sim
                .map(|s| s.traces.iter().map(|t| (t.label.clone(), t.pts.clone())).collect())
                .unwrap_or_default(),
        };

        // ---- screen
        let width = ui.available_width().max(260.0);
        let size = Vec2::new(width - 12.0, ((width - 12.0) * 0.68).clamp(180.0, 400.0));
        let (bezel, _) = ui.allocate_exact_size(size + Vec2::splat(12.0), Sense::hover());
        let rect = bezel.shrink(6.0);
        let p = ui.painter_at(bezel);
        p.rect_filled(bezel, 8.0, Color32::from_rgb(0x1C, 0x1E, 0x22));
        p.rect_filled(rect, 4.0, Color32::from_rgb(0x03, 0x0E, 0x08));
        draw_grid(&p, rect);

        if live.is_empty() {
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Place an oscilloscope part, wire its channels and run the simulation.",
                egui::FontId::proportional(14.0),
                Color32::from_rgb(0x90, 0xA8, 0x98),
            );
        } else {
            draw_traces(&p, rect, st, &live, time_div, volt_div);
        }

        // ---- controls
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (label, color) = if st.run {
                ("RUN", Color32::from_rgb(0x20, 0xA0, 0x40))
            } else {
                ("STOP", Color32::from_rgb(0xC0, 0x30, 0x30))
            };
            if ui
                .add(egui::Button::new(egui::RichText::new(label).strong()).fill(color).min_size(Vec2::new(60.0, 28.0)))
                .clicked()
            {
                st.run = !st.run;
                st.frozen = if st.run { None } else { Some(live.clone()) };
            }
            ui.toggle_value(&mut st.ac, "AC");
            ui.toggle_value(&mut st.auto_trigger, "AUTO");
            ui.separator();
            for (k, on) in st.channels.iter_mut().enumerate() {
                let name = live.get(k).map(|t| t.0.clone()).unwrap_or_else(|| format!("CH{}", k + 1));
                let text = egui::RichText::new(name).color(if *on { SCOPE_COLORS[k % 4] } else { Color32::GRAY });
                ui.toggle_value(on, text);
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            knob(ui, "TIME/DIV", &mut st.time_knob, 1.0 / (TIME_DIVS.len() - 1) as f32, &crate::util::si(time_div, "s"));
            knob(ui, "VOLTS/DIV", &mut st.volt_knob, 1.0 / (VOLT_DIVS.len() - 1) as f32, &crate::util::si(volt_div, "V"));
            let pos = (st.pos_knob - 0.5) * 8.0 * volt_div as f32;
            knob(ui, "POSITION", &mut st.pos_knob, 0.02, &crate::util::si(pos as f64, "V"));
            let trig = (st.trig_knob - 0.5) * 8.0 * volt_div as f32;
            let trig_text = if st.auto_trigger { "auto".to_string() } else { crate::util::si(trig as f64, "V") };
            knob(ui, "TRIG LEVEL", &mut st.trig_knob, 0.02, &trig_text);
        });
    });
    st.open = open;
}

fn draw_grid(p: &egui::Painter, rect: Rect) {
    let grid = Stroke::new(1.0, Color32::from_rgb(0x16, 0x38, 0x24));
    for i in 0..=10 {
        let x = rect.left() + rect.width() * i as f32 / 10.0;
        p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], grid);
    }
    for j in 0..=8 {
        let y = rect.top() + rect.height() * j as f32 / 8.0;
        p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], grid);
    }
    // Fine ticks along the centre lines, five to a division.
    let tick = Stroke::new(1.0, Color32::from_rgb(0x2E, 0x6A, 0x46));
    let (cx, cy) = (rect.center().x, rect.center().y);
    for i in 0..=50 {
        let x = rect.left() + rect.width() * i as f32 / 50.0;
        p.line_segment([Pos2::new(x, cy - 3.0), Pos2::new(x, cy + 3.0)], tick);
    }
    for j in 0..=40 {
        let y = rect.top() + rect.height() * j as f32 / 40.0;
        p.line_segment([Pos2::new(cx - 3.0, y), Pos2::new(cx + 3.0, y)], tick);
    }
}

fn draw_traces(p: &egui::Painter, rect: Rect, st: &mut ScopeState, live: &[(String, Samples)], time_div: f64, volt_div: f64) {
    let window = time_div * 10.0;
    let t_end = live.first().and_then(|t| t.1.last()).map(|s| s.0).unwrap_or(0.0);
    let vdiv = volt_div as f32;
    let mid = rect.center().y;
    let shift = (st.pos_knob - 0.5) * 8.0 * vdiv;

    // Trigger on a rising edge of the first channel. In AUTO the level is the
    // middle of the signal; otherwise it is the knob setting.
    let first = &live[0].1;
    let (lo, hi) = first
        .iter()
        .filter(|s| s.0 >= t_end - window * 2.0)
        .fold((f32::MAX, f32::MIN), |(l, h), s| (l.min(s.1), h.max(s.1)));
    let level = if st.auto_trigger { (lo + hi) * 0.5 } else { (st.trig_knob - 0.5) * 8.0 * vdiv };
    let mut start = (t_end - window).max(0.0);
    let triggered = hi - lo > 0.02 || !st.auto_trigger;
    if triggered {
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
    // Trigger level marker on the right edge.
    let ty = mid - (level + shift) / vdiv * rect.height() / 8.0;
    if rect.y_range().contains(ty) {
        p.text(
            Pos2::new(rect.right() - 2.0, ty),
            egui::Align2::RIGHT_CENTER,
            "T",
            egui::FontId::monospace(11.0),
            Color32::from_rgb(0xFF, 0xC8, 0x40),
        );
    }

    let mut frame: Vec<Vec<Pos2>> = Vec::new();
    for (k, (_, pts)) in live.iter().enumerate() {
        if !st.channels[k % 4] {
            frame.push(Vec::new());
            continue;
        }
        let shown: Vec<&(f64, f32)> = pts.iter().filter(|s| s.0 >= start && s.0 <= start + window).collect();
        let offset = if st.ac && !shown.is_empty() {
            shown.iter().map(|s| s.1).sum::<f32>() / shown.len() as f32
        } else {
            0.0
        };
        let line: Vec<Pos2> = shown
            .into_iter()
            .map(|s| {
                let x = rect.left() + rect.width() * ((s.0 - start) / window) as f32;
                let y = mid - (s.1 - offset + shift) / vdiv * rect.height() / 8.0;
                Pos2::new(x, y.clamp(rect.top(), rect.bottom()))
            })
            .collect();
        frame.push(line);
    }

    // Afterglow of the last few screens, then the live trace with a soft halo.
    let older = st.afterglow.len();
    for (age, old) in st.afterglow.iter().enumerate() {
        let fade = (age + 1) as f32 / (older + 1) as f32 * 0.35;
        for (k, line) in old.iter().enumerate() {
            if line.len() > 1 {
                p.add(Shape::line(line.clone(), Stroke::new(1.4, SCOPE_COLORS[k % 4].gamma_multiply(fade))));
            }
        }
    }
    for (k, line) in frame.iter().enumerate() {
        if line.len() > 1 {
            let c = SCOPE_COLORS[k % 4];
            p.add(Shape::line(line.clone(), Stroke::new(5.0, c.gamma_multiply(0.14))));
            p.add(Shape::line(line.clone(), Stroke::new(2.4, c.gamma_multiply(0.45))));
            p.add(Shape::line(line.clone(), Stroke::new(1.2, c)));
        }
    }
    if st.run {
        st.afterglow.push(frame);
        if st.afterglow.len() > 3 {
            st.afterglow.remove(0);
        }
    }
}
