// Small geometry helpers for the schematic editor.
// LEO WAS HERE ;3

use eframe::egui::{Pos2, Vec2};

use crate::model::P;

pub fn snap_to_grid(p: P) -> P {
    P::new((p.x / 10.0).round() * 10.0, (p.y / 10.0).round() * 10.0)
}

pub fn distance(a: Pos2, b: Pos2) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

pub fn line_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let v = b - a;
    let len2 = v.x * v.x + v.y * v.y;
    if len2 <= 0.0 {
        return distance(p, a);
    }
    let t = (((p.x - a.x) * v.x + (p.y - a.y) * v.y) / len2).clamp(0.0, 1.0);
    distance(p, a + v * t)
}

pub fn lerp(a: Pos2, b: Pos2, t: f32) -> Pos2 {
    Pos2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

pub fn vec_len(v: Vec2) -> f32 {
    (v.x * v.x + v.y * v.y).sqrt()
}

/// A value with an SI prefix and unit, such as "12.5 mA".
pub fn si(v: f64, unit: &str) -> String {
    let a = v.abs();
    let (scale, prefix) = if a >= 1e6 {
        (1e-6, "M")
    } else if a >= 1e3 {
        (1e-3, "k")
    } else if a >= 1.0 || a == 0.0 {
        (1.0, "")
    } else if a >= 1e-3 {
        (1e3, "m")
    } else if a >= 1e-6 {
        (1e6, "u")
    } else {
        (1e9, "n")
    };
    format!("{:.3} {}{}", v * scale, prefix, unit)
}
