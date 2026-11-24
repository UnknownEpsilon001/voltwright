// Plain-text project files.

use std::path::Path;

use crate::ui_text;
use crate::model::{
    Board, Component, DesignRules, Footprint, GridSettings, Label, LabelKind, Layer, Pad, PadShape,
    Project, Track, UnitSystem, Via, Wire, P,
};

const HEADER: &str = "# VOLTWRIGHT PROJECT 1";

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '|' => out.push_str("\\|"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '=' => out.push_str("\\="),
            _ => out.push(c),
        }
    }
    out
}

fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn split_unescaped(s: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            cur.push(c);
            if let Some(next) = chars.next() {
                cur.push(next);
            }
        } else if c == delim {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    parts.push(cur);
    parts
}

fn f32v(s: &str) -> Result<f32, String> {
    s.trim()
        .parse::<f32>()
        .map_err(|_| format!("bad number '{s}'"))
}

fn u64v(s: &str) -> Result<u64, String> {
    s.trim().parse::<u64>().map_err(|_| format!("bad id '{s}'"))
}

fn usizev(s: &str) -> Result<usize, String> {
    s.trim()
        .parse::<usize>()
        .map_err(|_| format!("bad index '{s}'"))
}

fn boolv(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("true")
}

fn point(s: &str) -> Result<P, String> {
    let p = split_unescaped(s, ',');
    if p.len() != 2 {
        return Err(format!("bad point '{s}'"));
    }
    Ok(P::new(f32v(&p[0])?, f32v(&p[1])?))
}

fn opt_string(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(unesc(s))
    }
}

fn layer_index(l: Layer) -> usize {
    Layer::all().iter().position(|x| *x == l).unwrap_or(0)
}

fn layer_from(s: &str) -> Layer {
    match usizev(s).unwrap_or(0) {
        1 => Layer::BottomCopper,
        2 => Layer::TopSilk,
        3 => Layer::BottomSilk,
        4 => Layer::TopMask,
        5 => Layer::BottomMask,
        6 => Layer::Board,
        7 => Layer::Drill,
        _ => Layer::TopCopper,
    }
}

fn shape_index(s: PadShape) -> usize {
    match s {
        PadShape::Circle => 0,
        PadShape::Square => 1,
        PadShape::Octagon => 2,
    }
}

fn shape_from(s: &str) -> PadShape {
    match s {
        "1" => PadShape::Square,
        "2" => PadShape::Octagon,
        _ => PadShape::Circle,
    }
}

fn label_kind(s: LabelKind) -> &'static str {
    match s {
        LabelKind::Net => "Net",
        LabelKind::Power => "Power",
        LabelKind::Text => "Text",
    }
}

fn label_kind_from(s: &str) -> LabelKind {
    match s {
        "Power" => LabelKind::Power,
        "Text" => LabelKind::Text,
        _ => LabelKind::Net,
    }
}

fn props_text(c: &Component) -> String {
    let mut raw = String::new();
    for (i, (k, v)) in c.props.iter().enumerate() {
        if i > 0 {
            raw.push(';');
        }
        raw.push_str(&esc(k));
        raw.push('|');
        raw.push_str(&esc(v));
    }
    raw
}

fn parse_props(s: &str) -> Vec<(String, String)> {
    if s.is_empty() {
        return Vec::new();
    }
    split_unescaped(s, ';')
        .iter()
        .filter_map(|part| {
            let p = split_unescaped(part, '|');
            if p.len() == 2 {
                Some((unesc(&p[0]), unesc(&p[1])))
            } else {
                None
            }
        })
        .collect()
}

fn hidden_props_text(c: &Component) -> String {
    c.hidden_props
        .iter()
        .map(|name| esc(name))
        .collect::<Vec<_>>()
        .join(";")
}

fn parse_hidden_props(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        split_unescaped(s, ';')
            .into_iter()
            .map(|name| unesc(&name))
            .collect()
    }
}

fn text_size_text(c: &Component) -> String {
    c.text_size
        .map(|(height, width)| format!("{height},{width}"))
        .unwrap_or_default()
}

fn parse_text_size(s: &str) -> Result<Option<(i32, i32)>, String> {
    if s.is_empty() {
        return Ok(None);
    }
    let Some((height, width)) = s.split_once(',') else {
        return Err("bad component text size".into());
    };
    let height = height
        .trim()
        .parse::<i32>()
        .map_err(|_| format!("bad component text height '{height}'"))?;
    let width = width
        .trim()
        .parse::<i32>()
        .map_err(|_| format!("bad component text width '{width}'"))?;
    Ok(Some((height, width)))
}

fn pads_text(f: &Footprint) -> String {
    let mut raw = String::new();
    for (i, pad) in f.pads.iter().enumerate() {
        if i > 0 {
            raw.push(';');
        }
        raw.push_str(&esc(&pad.number));
        raw.push('|');
        raw.push_str(&format!("{},{}", pad.pos.x, pad.pos.y));
        raw.push('|');
        raw.push_str(&format!(
            "{}|{}|{}|{}",
            pad.size,
            pad.drill,
            shape_index(pad.shape),
            pad.net.clone().unwrap_or_default()
        ));
    }
    raw
}

fn parse_pads(s: &str) -> Vec<Pad> {
    if s.is_empty() {
        return Vec::new();
    }
    split_unescaped(s, ';')
        .iter()
        .filter_map(|part| {
            let p = split_unescaped(part, '|');
            if p.len() < 6 {
                return None;
            }
            Some(Pad {
                number: unesc(&p[0]),
                pos: point(&p[1]).ok()?,
                size: f32v(&p[2]).ok()?,
                drill: f32v(&p[3]).ok()?,
                shape: shape_from(&p[4]),
                net: opt_string(&p[5..].join("|")),
            })
        })
        .collect()
}

fn points_text(pts: &[P]) -> String {
    pts.iter()
        .map(|p| format!("{},{}", p.x, p.y))
        .collect::<Vec<_>>()
        .join(";")
}

fn parse_points(s: &str) -> Vec<P> {
    split_unescaped(s, ';')
        .iter()
        .filter_map(|p| point(p).ok())
        .collect()
}

fn pin_text(pin: Option<(u64, usize)>) -> String {
    pin.map(|(comp, index)| format!("{comp},{index}"))
        .unwrap_or_default()
}

fn parse_pin(s: &str) -> Option<(u64, usize)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut it = s.split(',');
    let comp = u64v(it.next()?).ok()?;
    let index = it.next()?.trim().parse::<usize>().ok()?;
    Some((comp, index))
}

pub fn save(proj: &Project, path: &Path) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    let field = |out: &mut String, key: &str, value: String| {
        out.push_str(key);
        out.push('=');
        out.push_str(&value);
        out.push('\n');
    };

    field(&mut out, "name", esc(&proj.name));
    field(&mut out, "next_id", proj.next_id.to_string());
    field(&mut out, "notes", esc(&proj.notes));
    field(&mut out, "source", esc(&proj.source));
    field(
        &mut out,
        "grid",
        format!(
            "{}|{}|{}|{}|{}|{}",
            match proj.grid.units {
                UnitSystem::Imperial => "Imperial",
                UnitSystem::Metric => "Metric",
            },
            proj.grid
                .snaps
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
            proj.grid.active,
            proj.grid.show_dots,
            proj.grid.snap_on,
            match proj.grid.units {
                UnitSystem::Imperial => 1000.0,
                UnitSystem::Metric => 25400.0,
            }
        ),
    );
    field(
        &mut out,
        "rules",
        format!(
            "{}|{}|{}|{}|{}",
            proj.rules.pad_pad,
            proj.rules.pad_trace,
            proj.rules.trace_trace,
            proj.rules.graphic,
            proj.rules.edge
        ),
    );
    field(
        &mut out,
        "board",
        format!(
            "{},{}|{},{}",
            proj.board.origin.x, proj.board.origin.y, proj.board.width, proj.board.height
        ),
    );

    for c in &proj.comps {
        field(
            &mut out,
            "component",
            format!(
                "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                c.id,
                esc(&c.part),
                esc(&c.refdes),
                esc(&c.value),
                c.x,
                c.y,
                c.rot,
                // The property list uses | and ; itself, so it is escaped as one field.
                esc(&props_text(c)),
                hidden_props_text(c),
                text_size_text(c)
            ),
        );
    }
    field(&mut out, "sheets", proj.sheets.to_string());
    for b in &proj.buses {
        field(&mut out, "bus", format!("{}|{}", b.id, points_text(&b.pts)));
    }
    for w in &proj.wires {
        field(
            &mut out,
            "wire",
            format!(
                "{}|{}|{}|{}",
                w.id,
                points_text(&w.pts),
                pin_text(w.start_pin),
                pin_text(w.end_pin)
            ),
        );
    }
    for l in &proj.labels {
        field(
            &mut out,
            "label",
            format!(
                "{}|{}|{}|{}|{}|{}|{}",
                l.id,
                label_kind(l.kind),
                esc(&l.text),
                l.x,
                l.y,
                l.rot,
                l.probe
            ),
        );
    }
    for f in &proj.fps {
        field(
            &mut out,
            "footprint",
            format!(
                "{}|{}|{}|{}|{}|{}|{}|{}",
                f.id,
                f.comp,
                esc(&f.refdes),
                esc(&f.package),
                f.x,
                f.y,
                f.rot,
                // Pad lists use | and ; themselves, so escape them as one field.
                esc(&pads_text(f))
            ),
        );
    }
    for t in &proj.tracks {
        field(
            &mut out,
            "track",
            format!(
                "{}|{},{}|{},{}|{}|{}|{}",
                t.id,
                t.a.x,
                t.a.y,
                t.b.x,
                t.b.y,
                t.width,
                layer_index(t.layer),
                esc(&t.net.clone().unwrap_or_default())
            ),
        );
    }
    for v in &proj.vias {
        field(
            &mut out,
            "via",
            format!(
                "{}|{},{}|{}|{}|{}",
                v.id,
                v.pos.x,
                v.pos.y,
                v.size,
                v.drill,
                esc(&v.net.clone().unwrap_or_default())
            ),
        );
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(path, out).map_err(|e| e.to_string())?;
    Ok(format!("Saved {}", path.display()))
}

pub fn open(path: &Path) -> Result<Project, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Err("Not an Voltwright project file".into());
    }

    let name = String::new();
    let mut proj = Project::new(&name);
    for line in lines {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, val) = line
            .split_once('=')
            .ok_or_else(|| format!("bad line: {line}"))?;
        let p = split_unescaped(val, '|');
        match key {
            "name" => proj.name = unesc(val),
            "next_id" => proj.next_id = u64v(val)?,
            "notes" => proj.notes = unesc(val),
            "source" => proj.source = unesc(val),
            "grid" => {
                if p.len() < 5 {
                    return Err("bad grid".into());
                }
                let snaps = split_unescaped(&p[1], ',')
                    .iter()
                    .map(|s| f32v(s))
                    .collect::<Result<Vec<_>, _>>()?;
                if snaps.len() != 4 {
                    return Err("bad grid snaps".into());
                }
                proj.grid = GridSettings {
                    units: if p[0] == "Metric" {
                        UnitSystem::Metric
                    } else {
                        UnitSystem::Imperial
                    },
                    snaps: [snaps[0], snaps[1], snaps[2], snaps[3]],
                    active: usizev(&p[2])?.min(3),
                    show_dots: boolv(&p[3]),
                    snap_on: boolv(&p[4]),
                };
            }
            "rules" => {
                if p.len() != 5 {
                    return Err("bad rules".into());
                }
                proj.rules = DesignRules {
                    pad_pad: f32v(&p[0])?,
                    pad_trace: f32v(&p[1])?,
                    trace_trace: f32v(&p[2])?,
                    graphic: f32v(&p[3])?,
                    edge: f32v(&p[4])?,
                };
            }
            "board" => {
                if p.len() != 2 {
                    return Err("bad board".into());
                }
                let o = point(&p[0])?;
                let d = point(&p[1])?;
                proj.board = Board {
                    origin: o,
                    width: d.x,
                    height: d.y,
                };
            }
            "component" => {
                if p.len() != 8 && p.len() != 10 {
                    return Err("bad component".into());
                }
                proj.comps.push(Component {
                    id: u64v(&p[0])?,
                    part: unesc(&p[1]),
                    refdes: unesc(&p[2]),
                    value: unesc(&p[3]),
                    x: f32v(&p[4])?,
                    y: f32v(&p[5])?,
                    rot: f32v(&p[6])? as i32,
                    props: parse_props(&unesc(&p[7])),
                    hidden_props: p.get(8).map(|s| parse_hidden_props(s)).unwrap_or_default(),
                    text_size: p.get(9).map(|s| parse_text_size(s)).transpose()?.flatten(),
                });
            }
            "sheets" => {
                proj.sheets = p.first().and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, 64);
            }
            "bus" => {
                if p.len() != 2 {
                    return Err("bad bus".into());
                }
                proj.buses.push(crate::model::Bus { id: u64v(&p[0])?, pts: parse_points(&p[1]) });
            }
            "wire" => {
                if p.len() != 2 && p.len() != 4 {
                    return Err("bad wire".into());
                }
                proj.wires.push(Wire {
                    id: u64v(&p[0])?,
                    pts: parse_points(&p[1]),
                    start_pin: p.get(2).and_then(|s| parse_pin(s)),
                    end_pin: p.get(3).and_then(|s| parse_pin(s)),
                });
            }
            "label" => {
                if p.len() != 7 {
                    return Err("bad label".into());
                }
                proj.labels.push(Label {
                    id: u64v(&p[0])?,
                    kind: label_kind_from(&p[1]),
                    text: unesc(&p[2]),
                    x: f32v(&p[3])?,
                    y: f32v(&p[4])?,
                    rot: f32v(&p[5])? as i32,
                    probe: boolv(&p[6]),
                });
            }
            "footprint" => {
                if p.len() != 8 {
                    return Err("bad footprint".into());
                }
                proj.fps.push(Footprint {
                    id: u64v(&p[0])?,
                    comp: u64v(&p[1])?,
                    refdes: unesc(&p[2]),
                    package: unesc(&p[3]),
                    x: f32v(&p[4])?,
                    y: f32v(&p[5])?,
                    rot: f32v(&p[6])? as i32,
                    pads: parse_pads(&unesc(&p[7])),
                });
            }
            "track" => {
                if p.len() < 6 {
                    return Err("bad track".into());
                }
                proj.tracks.push(Track {
                    id: u64v(&p[0])?,
                    a: point(&p[1])?,
                    b: point(&p[2])?,
                    width: f32v(&p[3])?,
                    layer: layer_from(&p[4]),
                    net: opt_string(&p[5..].join("|")),
                });
            }
            "via" => {
                if p.len() < 5 {
                    return Err("bad via".into());
                }
                proj.vias.push(Via {
                    id: u64v(&p[0])?,
                    pos: point(&p[1])?,
                    size: f32v(&p[2])?,
                    drill: f32v(&p[3])?,
                    net: opt_string(&p[4]),
                });
            }
            _ => {}
        }
    }
    proj.path = Some(path.to_path_buf());
    proj.dirty = false;
    Ok(proj)
}

pub fn default_path(proj: &Project) -> std::path::PathBuf {
    let name: String = proj
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = if name.is_empty() {
        ui_text::NEW_PROJECT_FILENAME
            .strip_suffix(ui_text::PROJECT_EXTENSION)
            .unwrap_or("untitled")
            .to_string()
    } else {
        name
    };
    std::path::PathBuf::from(format!("{name}{}", ui_text::PROJECT_EXTENSION))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bus, Library, P};

    #[test]
    fn sheets_buses_and_mirrored_parts_survive_save_and_open() {
        let lib = Library::builtin();
        let mut proj = crate::examples::build("NAND gate truth table", &lib).unwrap();
        proj.sheets = 3;
        proj.buses.push(Bus { id: 900, pts: vec![P::new(100.0, 200.0), P::new(700.0, 200.0), P::new(700.0, 500.0)] });
        let id = proj.comps.iter().find(|c| c.refdes == "U1").unwrap().id;
        proj.comp_mut(id).unwrap().rot = 450; // mirrored, turned 90 degrees
        let dir = std::env::temp_dir().join("voltwright_io_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("roundtrip.volt");
        save(&proj, &path).unwrap();
        let back = open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.sheets, 3);
        assert_eq!(back.buses.len(), 1);
        assert_eq!(back.buses[0].pts.len(), 3);
        assert_eq!(back.comp(id).unwrap().rot, 450);
        assert!(back.comp(id).unwrap().mirrored());
    }

    #[test]
    fn every_example_survives_save_and_open_including_the_pcb() {
        let lib = Library::builtin();
        let dir = std::env::temp_dir().join("voltwright_io_test");
        std::fs::create_dir_all(&dir).unwrap();
        for name in crate::examples::names() {
            let mut proj = crate::examples::build(name, &lib).unwrap();
            let nl = proj.netlist(&lib);
            proj.sync_pcb(&lib, &nl);
            let path = dir.join("example.volt");
            save(&proj, &path).unwrap();
            let back = open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let _ = std::fs::remove_file(&path);
            assert_eq!(back.comps.len(), proj.comps.len(), "{name}");
            assert_eq!(back.wires.len(), proj.wires.len(), "{name}");
            assert_eq!(back.fps.len(), proj.fps.len(), "{name}");
            for (a, b) in proj.comps.iter().zip(&back.comps) {
                assert_eq!((&a.refdes, &a.value, &a.props), (&b.refdes, &b.value, &b.props), "{name}");
            }
            assert_eq!(back.netlist(&lib).names.len(), proj.netlist(&lib).names.len(), "{name}: nets");
        }
    }

    #[test]
    fn routed_tracks_and_vias_survive_save_and_open() {
        let lib = Library::builtin();
        let mut proj = crate::examples::build("Transistor switch with lamp", &lib).unwrap();
        let nl = proj.netlist(&lib);
        proj.sync_pcb(&lib, &nl);
        let (o, e) = (P::new(0.0, 0.0), P::new(3000.0, 2500.0));
        proj.board.origin = o;
        proj.board.width = e.x;
        proj.board.height = e.y;
        for (i, f) in proj.fps.iter_mut().enumerate() {
            f.x = 400.0 + 450.0 * (i % 4) as f32;
            f.y = 500.0 + 400.0 * (i / 4) as f32;
        }
        let res = crate::autoroute::route(&proj, &crate::autoroute::RouteParams::default());
        assert!(res.routed > 0);
        for mut t in res.tracks {
            t.id = proj.alloc_id();
            proj.tracks.push(t);
        }
        for mut v in res.vias {
            v.id = proj.alloc_id();
            proj.vias.push(v);
        }
        let dir = std::env::temp_dir().join("voltwright_io_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("routed.volt");
        save(&proj, &path).unwrap();
        let back = open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.tracks.len(), proj.tracks.len());
        assert_eq!(back.vias.len(), proj.vias.len());
        assert_eq!(back.tracks[0].net, proj.tracks[0].net);
        assert_eq!(back.fps[0].pads[0].net, proj.fps[0].pads[0].net);
        assert!(back.drc().len() == proj.drc().len());
    }
}
