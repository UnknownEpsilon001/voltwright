// Drawn icons for the editor mode buttons. Each icon is a few strokes painted
// in a 28x28 button, so there are no image files and they follow the theme.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2};

#[derive(Clone, Copy)]
pub enum Icon {
    Select,
    Wire,
    Part,
    Label,
    Text,
    Probe,
    Bus,
    Delete,
    Track,
    Via,
    Board,
}

/// A square mode button that sets `*current = value` when clicked and shows
/// a tooltip with the mode name.
pub fn mode_button<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    icon: Icon,
    tip: &str,
) -> Response {
    let selected = *current == value;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let visuals = ui.visuals();
    let fill = if selected {
        visuals.selection.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let ink = if selected {
        Color32::WHITE
    } else {
        visuals.widgets.inactive.fg_stroke.color
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, fill);
    draw(painter, rect.shrink(5.0), icon, ink);
    if response.clicked() {
        *current = value;
    }
    response.on_hover_text(tip)
}

fn draw(p: &egui::Painter, r: Rect, icon: Icon, ink: Color32) {
    let s = Stroke::new(1.6, ink);
    // Map 0..1 coordinates into the icon rectangle.
    let at = |x: f32, y: f32| Pos2::new(r.left() + x * r.width(), r.top() + y * r.height());
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s));
    };
    match icon {
        Icon::Select => {
            // Arrow pointer in the classic shape: a head with a notch and a
            // tail, filled in two convex pieces and outlined as one.
            let q = |x: f32, y: f32| at(0.2 + x * 0.62, y);
            p.add(Shape::convex_polygon(vec![q(0.0, 0.0), q(0.0, 0.86), q(1.0, 0.62)], ink, Stroke::NONE));
            p.add(Shape::convex_polygon(
                vec![q(0.32, 0.70), q(0.52, 1.0), q(0.74, 0.92), q(0.5, 0.62)],
                ink,
                Stroke::NONE,
            ));
            let outline = [(0.0, 0.0), (0.0, 0.86), (0.32, 0.70), (0.52, 1.0), (0.74, 0.92), (0.54, 0.64), (1.0, 0.62)];
            p.add(Shape::closed_line(
                outline.iter().map(|&(x, y)| q(x, y)).collect(),
                Stroke::new(1.0, Color32::from_rgb(0x10, 0x14, 0x18)),
            ));
        }
        Icon::Bus => {
            // A thick line with two short branches.
            p.add(Shape::line(vec![at(0.0, 0.5), at(1.0, 0.5)], Stroke::new(4.0, ink)));
            line(&[(0.3, 0.5), (0.3, 0.05)]);
            line(&[(0.7, 0.5), (0.7, 0.95)]);
        }
        Icon::Wire => {
            line(&[(0.0, 0.85), (0.4, 0.85), (0.4, 0.15), (1.0, 0.15)]);
            p.circle_filled(at(0.0, 0.85), 2.4, ink);
            p.circle_filled(at(1.0, 0.15), 2.4, ink);
        }
        Icon::Part => {
            // A resistor between two leads.
            line(&[(0.0, 0.5), (0.2, 0.5)]);
            p.rect_stroke(Rect::from_min_max(at(0.2, 0.25), at(0.8, 0.75)), 1.0, s, egui::StrokeKind::Middle);
            line(&[(0.8, 0.5), (1.0, 0.5)]);
        }
        Icon::Label => {
            // Flag with a pole: a net label.
            line(&[(0.15, 0.0), (0.15, 1.0)]);
            p.add(Shape::convex_polygon(
                vec![at(0.15, 0.05), at(0.95, 0.05), at(0.78, 0.33), at(0.95, 0.6), at(0.15, 0.6)],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::Text => {
            // A capital T.
            line(&[(0.1, 0.1), (0.9, 0.1)]);
            line(&[(0.5, 0.1), (0.5, 0.95)]);
            line(&[(0.35, 0.95), (0.65, 0.95)]);
        }
        Icon::Probe => {
            // A needle tip with a small meter ring.
            line(&[(0.1, 0.95), (0.55, 0.45)]);
            p.circle_stroke(at(0.68, 0.32), r.width() * 0.2, s);
            p.circle_filled(at(0.1, 0.95), 2.2, ink);
        }
        Icon::Delete => {
            line(&[(0.15, 0.15), (0.85, 0.85)]);
            line(&[(0.85, 0.15), (0.15, 0.85)]);
        }
        Icon::Track => {
            line(&[(0.1, 0.85), (0.5, 0.85), (0.5, 0.2), (0.95, 0.2)]);
            p.circle_stroke(at(0.1, 0.85), 3.4, s);
            p.circle_stroke(at(0.95, 0.2), 3.4, s);
        }
        Icon::Via => {
            p.circle_stroke(at(0.5, 0.5), r.width() * 0.46, s);
            p.circle_filled(at(0.5, 0.5), r.width() * 0.16, ink);
        }
        Icon::Board => {
            p.rect_stroke(Rect::from_min_max(at(0.0, 0.1), at(1.0, 0.9)), 1.0, s, egui::StrokeKind::Middle);
            p.circle_filled(at(0.12, 0.22), 1.6, ink);
            p.circle_filled(at(0.88, 0.22), 1.6, ink);
            p.circle_filled(at(0.12, 0.78), 1.6, ink);
            p.circle_filled(at(0.88, 0.78), 1.6, ink);
        }
    }
}
