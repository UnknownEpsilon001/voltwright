// G-code for milling a board on a CNC router (GRBL-style, millimetres).
// Three jobs: isolation routing around the top copper, drilling, and cutting
// out the board. Design units are thousandths of an inch.

use crate::model::{Layer, Project, P};

#[derive(Clone, Debug)]
pub struct GcodeParams {
    pub iso_bit: f32,
    pub iso_depth: f32,
    pub drill_depth: f32,
    pub outline_bit: f32,
    pub outline_depth: f32,
    pub outline_pass: f32,
    pub feed: f32,
    pub plunge: f32,
    pub safe_z: f32,
    pub rpm: f32,
}

impl Default for GcodeParams {
    fn default() -> Self {
        GcodeParams {
            iso_bit: 0.2,
            iso_depth: 0.1,
            drill_depth: 1.8,
            outline_bit: 1.0,
            outline_depth: 1.7,
            outline_pass: 0.6,
            feed: 150.0,
            plunge: 60.0,
            safe_z: 2.0,
            rpm: 10000.0,
        }
    }
}

const MM_PER_TH: f32 = 0.0254;

/// Milling coordinates: millimetres from the board's lower-left corner, y up.
struct Frame {
    ox: f32,
    oy_top: f32,
}

impl Frame {
    fn of(proj: &Project) -> Frame {
        Frame { ox: proj.board.origin.x, oy_top: proj.board.origin.y + proj.board.height }
    }
    fn pt(&self, p: P) -> (f32, f32) {
        ((p.x - self.ox) * MM_PER_TH, (self.oy_top - p.y) * MM_PER_TH)
    }
}

/// A piece of copper: a round pad or via (a == b) or a track (a stadium).
struct Copper {
    a: (f32, f32),
    b: (f32, f32),
    r: f32,
}

impl Copper {
    fn dist(&self, p: (f32, f32)) -> f32 {
        let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 <= 1e-12 {
            0.0
        } else {
            (((p.0 - self.a.0) * dx + (p.1 - self.a.1) * dy) / len2).clamp(0.0, 1.0)
        };
        let (cx, cy) = (self.a.0 + dx * t, self.a.1 + dy * t);
        ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
    }

    /// Points around the outline grown by `grow`, about `step` mm apart.
    fn outline(&self, grow: f32, step: f32) -> Vec<(f32, f32)> {
        let r = self.r + grow;
        let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
        let len = (dx * dx + dy * dy).sqrt();
        let ang = if len > 1e-6 { dy.atan2(dx) } else { 0.0 };
        let arc = ((std::f32::consts::PI * r / step).ceil() as usize).max(8);
        let mut out = Vec::new();
        // Half circle around b, then along one side, half circle around a, back.
        for i in 0..=arc {
            let t = ang - std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / arc as f32;
            out.push((self.b.0 + r * t.cos(), self.b.1 + r * t.sin()));
        }
        for i in 0..=arc {
            let t = ang + std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / arc as f32;
            out.push((self.a.0 + r * t.cos(), self.a.1 + r * t.sin()));
        }
        out
    }
}

fn top_copper(proj: &Project, fr: &Frame) -> Vec<Copper> {
    let mut v = Vec::new();
    for t in proj.tracks.iter().filter(|t| t.layer == Layer::TopCopper) {
        v.push(Copper { a: fr.pt(t.a), b: fr.pt(t.b), r: t.width * 0.5 * MM_PER_TH });
    }
    for f in &proj.fps {
        for pad in &f.pads {
            let at = fr.pt(crate::pcb::absolute_pad(f, pad));
            v.push(Copper { a: at, b: at, r: pad.size * 0.5 * MM_PER_TH });
        }
    }
    for via in &proj.vias {
        let at = fr.pt(via.pos);
        v.push(Copper { a: at, b: at, r: via.size * 0.5 * MM_PER_TH });
    }
    v
}

fn header(title: &str, p: &GcodeParams) -> String {
    format!(
        "; {title}\n; Voltwright G-code, millimetres, origin at the board's lower-left corner\nG21\nG90\nG17\nM3 S{:.0}\nG0 Z{:.2}\n",
        p.rpm, p.safe_z
    )
}

fn footer(p: &GcodeParams) -> String {
    format!("G0 Z{:.2}\nM5\nM2\n", p.safe_z)
}

/// Isolation routing: a path around the outside of every copper feature, with
/// the parts that would cut into neighbouring copper left out.
pub fn isolation(proj: &Project, p: &GcodeParams) -> String {
    let fr = Frame::of(proj);
    let copper = top_copper(proj, &fr);
    let grow = p.iso_bit * 0.5;
    let tol = 0.01;
    let mut out = header("Isolation routing, top copper", p);
    let mut paths = 0;
    for (i, f) in copper.iter().enumerate() {
        let pts = f.outline(grow, 0.15);
        let keep: Vec<bool> = pts
            .iter()
            .map(|q| copper.iter().enumerate().all(|(j, g)| j == i || g.dist(*q) >= g.r + grow - tol))
            .collect();
        // Chain consecutive kept points (the outline is a closed loop).
        let n = pts.len();
        let Some(start) = (0..n).find(|&k| !keep[k]) else {
            emit_path(&mut out, &pts, true, p.iso_depth, p);
            paths += 1;
            continue;
        };
        let mut run: Vec<(f32, f32)> = Vec::new();
        for k in 1..=n {
            let idx = (start + k) % n;
            if keep[idx] {
                run.push(pts[idx]);
            } else if !run.is_empty() {
                emit_path(&mut out, &run, false, p.iso_depth, p);
                paths += 1;
                run.clear();
            }
        }
        if !run.is_empty() {
            emit_path(&mut out, &run, false, p.iso_depth, p);
            paths += 1;
        }
    }
    out.push_str(&format!("; {paths} paths\n"));
    out.push_str(&footer(p));
    out
}

/// One cut at depth `depth` (positive mm below the surface).
fn emit_path(out: &mut String, pts: &[(f32, f32)], closed: bool, depth: f32, p: &GcodeParams) {
    let Some(first) = pts.first() else { return };
    out.push_str(&format!("G0 X{:.3} Y{:.3}\nG1 Z{:.3} F{:.0}\n", first.0, first.1, -depth, p.plunge));
    out.push_str(&format!("F{:.0}\n", p.feed));
    for q in &pts[1..] {
        out.push_str(&format!("G1 X{:.3} Y{:.3}\n", q.0, q.1));
    }
    if closed {
        out.push_str(&format!("G1 X{:.3} Y{:.3}\n", first.0, first.1));
    }
    out.push_str(&format!("G0 Z{:.2}\n", p.safe_z));
}

/// Drill holes, grouped by diameter with a pause for each tool change.
pub fn drill(proj: &Project, p: &GcodeParams) -> String {
    let fr = Frame::of(proj);
    let mut holes: Vec<(f32, (f32, f32))> = Vec::new();
    for f in &proj.fps {
        for pad in f.pads.iter().filter(|pad| pad.drill > 0.0) {
            holes.push((pad.drill * MM_PER_TH, fr.pt(crate::pcb::absolute_pad(f, pad))));
        }
    }
    for v in proj.vias.iter().filter(|v| v.drill > 0.0) {
        holes.push((v.drill * MM_PER_TH, fr.pt(v.pos)));
    }
    let mut sizes: Vec<f32> = holes.iter().map(|h| (h.0 * 100.0).round() / 100.0).collect();
    sizes.sort_by(|a, b| a.total_cmp(b));
    sizes.dedup();
    let mut out = header("Drilling", p);
    for d in sizes {
        out.push_str(&format!("G0 Z{:.2}\nM5\n; Change to a {d:.2} mm drill bit, then resume\nM0\nM3 S{:.0}\n", p.safe_z, p.rpm));
        for (_, at) in holes.iter().filter(|h| ((h.0 * 100.0).round() / 100.0 - d).abs() < 1e-4) {
            out.push_str(&format!(
                "G0 X{:.3} Y{:.3}\nG1 Z{:.3} F{:.0}\nG0 Z{:.2}\n",
                at.0, at.1, -p.drill_depth, p.plunge, p.safe_z
            ));
        }
    }
    out.push_str(&footer(p));
    out
}

/// Cut the board out, with the bit centre running just outside the edge.
pub fn outline(proj: &Project, p: &GcodeParams) -> String {
    let w = proj.board.width * MM_PER_TH;
    let h = proj.board.height * MM_PER_TH;
    let g = p.outline_bit * 0.5;
    let rect = [(-g, -g), (w + g, -g), (w + g, h + g), (-g, h + g), (-g, -g)];
    let mut out = header("Board outline", p);
    let step = p.outline_pass.max(0.05);
    let mut depth = 0.0;
    while depth < p.outline_depth - 1e-4 {
        depth = (depth + step).min(p.outline_depth);
        emit_path(&mut out, &rect, false, depth, p);
    }
    out.push_str(&footer(p));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Library;

    fn board() -> Project {
        let lib = Library::builtin();
        let mut proj = crate::examples::build("LED with current-limiting resistor", &lib).unwrap();
        proj.board.origin = P::new(0.0, 0.0);
        proj.board.width = 2000.0;
        proj.board.height = 1500.0;
        proj.tracks.push(crate::model::Track {
            id: 1,
            a: P::new(500.0, 500.0),
            b: P::new(1000.0, 500.0),
            width: 20.0,
            layer: Layer::TopCopper,
            net: None,
        });
        proj.tracks.push(crate::model::Track {
            id: 2,
            a: P::new(500.0, 560.0),
            b: P::new(1000.0, 560.0),
            width: 20.0,
            layer: Layer::TopCopper,
            net: None,
        });
        proj.vias.push(crate::model::Via { id: 3, pos: P::new(1500.0, 900.0), size: 60.0, drill: 30.0, net: None });
        proj
    }

    #[test]
    fn isolation_cuts_and_drill_and_outline_are_well_formed() {
        let proj = board();
        let p = GcodeParams::default();
        let iso = isolation(&proj, &p);
        assert!(iso.starts_with("; Isolation") && iso.contains("G1 Z-0.100") && iso.ends_with("M2\n"));
        let drl = drill(&proj, &p);
        assert!(drl.contains("M0") && drl.contains("G1 Z-1.800"));
        // via at (1500, 900) th = (38.1, 15.24) mm from the lower-left corner
        assert!(drl.contains("X38.100 Y15.240"), "{drl}");
        let out = outline(&proj, &p);
        // 1.7 mm deep in 0.6 mm passes is three passes
        assert_eq!(out.matches("G1 Z-").count(), 3);
        assert!(out.contains("X-0.500 Y-0.500"));
    }

    #[test]
    fn isolation_never_cuts_into_neighbouring_copper() {
        let proj = board();
        let p = GcodeParams::default();
        let fr = Frame::of(&proj);
        let copper = top_copper(&proj, &fr);
        let iso = isolation(&proj, &p);
        for line in iso.lines().filter(|l| l.starts_with("G1 X")) {
            let nums: Vec<f32> = line
                .split_whitespace()
                .skip(1)
                .map(|t| t[1..].parse().unwrap())
                .collect();
            let q = (nums[0], nums[1]);
            // the cutting bit's edge must stay off every copper feature
            assert!(
                copper.iter().all(|c| c.dist(q) >= c.r + p.iso_bit * 0.5 - 0.02),
                "cut point {q:?} runs into copper"
            );
        }
    }
}
