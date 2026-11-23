// Import of .pdsprj projects from another schematic tool.
//
// A .pdsprj file is a ZIP archive. The schematic lives in ROOT.DSN, a binary
// stream of records. We read only what we need: parts (device name, position,
// rotation, reference, value), wires, terminals and the pin table each device
// symbol embeds. Nets are rebuilt from geometry, then each net is rewired in
// Voltwright with our own router, so the layout is ours, not a copy.

use std::collections::HashMap;
use std::path::Path;

use crate::model::{Library, Project, P};
use crate::schematic::Schematic;

/// Source files store positions in 1/2540 of a thou.
const UNITS_PER_THOU: f64 = 2540.0;
/// Our symbols are about a quarter of the source symbols' size.
const SCALE: f32 = 0.25;
const SNAP: f32 = 25.0;

// ------------------------------------------------------------------ ZIP

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn i32le(b: &[u8], o: usize) -> Option<i32> {
    u32le(b, o).map(|v| v as i32)
}

/// Read one member of a ZIP archive by name (case-insensitive).
fn zip_member(zip: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let eocd = (0..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| zip[i..].starts_with(b"PK\x05\x06"))
        .ok_or("not a ZIP archive")?;
    let count = u16le(zip, eocd + 10).ok_or("bad ZIP directory")? as usize;
    let mut o = u32le(zip, eocd + 16).ok_or("bad ZIP directory")? as usize;
    for _ in 0..count {
        if !zip.get(o..).is_some_and(|s| s.starts_with(b"PK\x01\x02")) {
            break;
        }
        let method = u16le(zip, o + 10).unwrap_or(0);
        let csize = u32le(zip, o + 20).unwrap_or(0) as usize;
        let nlen = u16le(zip, o + 28).unwrap_or(0) as usize;
        let xlen = u16le(zip, o + 30).unwrap_or(0) as usize;
        let clen = u16le(zip, o + 32).unwrap_or(0) as usize;
        let local = u32le(zip, o + 42).unwrap_or(0) as usize;
        let entry = zip.get(o + 46..o + 46 + nlen).unwrap_or(&[]);
        o += 46 + nlen + xlen + clen;
        if !String::from_utf8_lossy(entry).eq_ignore_ascii_case(name) {
            continue;
        }
        let lnlen = u16le(zip, local + 26).ok_or("bad ZIP entry")? as usize;
        let lxlen = u16le(zip, local + 28).ok_or("bad ZIP entry")? as usize;
        let start = local + 30 + lnlen + lxlen;
        let data = zip.get(start..start + csize).ok_or("truncated ZIP entry")?;
        return match method {
            0 => Ok(data.to_vec()),
            8 => miniz_oxide::inflate::decompress_to_vec(data)
                .map_err(|e| format!("cannot inflate {name}: {e:?}")),
            m => Err(format!("unsupported ZIP compression method {m}")),
        };
    }
    Err(format!("{name} not found in archive"))
}

// ------------------------------------------------------------------ DSN records

#[derive(Debug, Clone)]
struct SrcPart {
    device: String,
    refdes: Option<String>,
    value: Option<String>,
    x: f64,
    y: f64,
    /// Low 16 bits: signed angle in tenths of a degree. 0x10000 mirrors X,
    /// 0x20000 mirrors Y.
    orient: u32,
}

#[derive(Debug, Clone)]
struct SrcPin {
    name: String,
    /// Connection point in symbol space, thou, y up.
    x: f64,
    y: f64,
}

#[derive(Debug, Clone)]
struct SrcTerminal {
    kind: String,
    label: String,
    /// Connection point, thou, y up.
    x: f64,
    y: f64,
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn printable(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(|&c| (32..127).contains(&c))
}

fn signed_angle(v: u32) -> i32 {
    (v & 0xffff) as u16 as i16 as i32
}

/// Rotate a unit-grid vector by a multiple of 90 degrees (tenths), y up.
fn rotate(x: f64, y: f64, tenths: i32) -> (f64, f64) {
    match (tenths.rem_euclid(3600) + 450) / 900 % 4 {
        1 => (-y, x),
        2 => (-x, -y),
        3 => (y, -x),
        _ => (x, y),
    }
}

/// Apply a part's mirror flags, then its rotation.
fn orient(x: f64, y: f64, o: u32) -> (f64, f64) {
    let x = if o & 0x10000 != 0 { -x } else { x };
    let y = if o & 0x20000 != 0 { -y } else { y };
    rotate(x, y, signed_angle(o))
}

/// Font names are written as 0x20, 0xff, flag, name; labels are not.
fn is_font(seg: &[u8], i: usize) -> bool {
    i > 0 && seg[i - 1] == 0x20
}

/// Text of the label record (0xff, length, text) closest before `before`
/// inside `seg`, skipping font names.
fn label_before(seg: &[u8], before: usize) -> Option<String> {
    let mut best = None;
    let mut i = 0;
    while i + 2 < before {
        if seg[i] == 0xff {
            let len = seg[i + 1] as usize;
            if (1..128).contains(&len) && i + 2 + len <= before {
                let text = &seg[i + 2..i + 2 + len];
                if printable(text) && !is_font(seg, i) {
                    best = Some(String::from_utf8_lossy(text).into_owned());
                }
            }
        }
        i += 1;
    }
    best
}

/// First label record at or after `from` in `seg`.
fn label_after(seg: &[u8], from: usize) -> Option<String> {
    let mut i = from;
    while i + 2 < seg.len() {
        if seg[i] == 0xff {
            let len = seg[i + 1] as usize;
            if let Some(text) = seg.get(i + 2..i + 2 + len) {
                if len > 0 && printable(text) && !is_font(seg, i) {
                    return Some(String::from_utf8_lossy(text).into_owned());
                }
            }
        }
        i += 1;
    }
    None
}

fn parse_parts(d: &[u8]) -> Vec<SrcPart> {
    let mut out = Vec::new();
    let mut prev = 0usize;
    let mut at = 0usize;
    while let Some(m) = find(d, b"PROPERTIES\0", at) {
        at = m + 1;
        let Some(part) = (|| {
            let mut o = m + 11 + 4;
            let plen = u32le(d, o)? as usize;
            o += 4;
            if d.get(o) == Some(&b'{') {
                o += plen;
            }
            let pins = u16le(d, o)? as usize;
            o += 2;
            if !(1..=128).contains(&pins) || plen >= 4096 {
                return None;
            }
            let n = *d.get(o)? as usize;
            let raw = d.get(o + 1..o + 1 + n)?;
            if !(1..64).contains(&n) || !printable(raw) {
                return None;
            }
            let device = String::from_utf8_lossy(raw).into_owned();
            o += 1 + n;
            let x = i32le(d, o)? as f64 / UNITS_PER_THOU;
            let y = i32le(d, o + 4)? as f64 / UNITS_PER_THOU;
            let orient = u32le(d, o + 8)?;
            let end = o + 21 + 4 * pins;
            let seg = d.get(prev..m)?;
            let id = find(seg, b"COMPONENT ID\0", 0);
            let refdes = id.and_then(|k| label_before(seg, k));
            let value = id.and_then(|k| label_after(seg, k));
            Some((
                SrcPart {
                    device,
                    refdes,
                    value,
                    x,
                    y,
                    orient,
                },
                end,
            ))
        })() else {
            continue;
        };
        prev = part.1;
        out.push(part.0);
    }
    out
}

fn parse_wires(d: &[u8]) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(m) = find(d, b"WIRE\0\0\0", at) {
        at = m + 1;
        if m < 2 || d[m - 2] != 0x02 {
            continue;
        }
        let o = m + 7;
        let Some(n) = u16le(d, o) else { continue };
        let n = n as usize;
        if !(2..200).contains(&n) {
            continue;
        }
        let pts: Option<Vec<_>> = (0..n)
            .map(|i| {
                let x = i32le(d, o + 2 + 8 * i)? as f64 / UNITS_PER_THOU;
                let y = i32le(d, o + 6 + 8 * i)? as f64 / UNITS_PER_THOU;
                Some((x, y))
            })
            .collect();
        if let Some(p) = pts {
            out.push(p);
        }
    }
    out
}

/// Connection offset of each terminal kind at rotation 0, thou, y up.
fn terminal_offset(kind: &str) -> (f64, f64) {
    match kind {
        "POWER" => (0.0, -100.0),
        "GROUND" => (0.0, 100.0),
        "OUTPUT" | "BIDIR" | "ENTRY" => (-100.0, 0.0),
        _ => (100.0, 0.0),
    }
}

fn parse_terminals(d: &[u8]) -> Vec<SrcTerminal> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(m) = find(d, b"$TER", at) {
        at = m + 1;
        // Layout: 0x10, x, y, orient, name length, "$TER<KIND>", 6 bytes, label.
        if m < 14 || d[m - 14] != 0x10 {
            continue;
        }
        let nlen = d[m - 1] as usize;
        let Some(kind_raw) = d.get(m + 4..m + nlen) else { continue };
        if !printable(kind_raw) && !kind_raw.is_empty() {
            continue;
        }
        let kind = String::from_utf8_lossy(kind_raw).into_owned();
        let (Some(x), Some(y), Some(o)) = (i32le(d, m - 13), i32le(d, m - 9), u32le(d, m - 5))
        else {
            continue;
        };
        let lo = m + nlen + 6;
        let label = if d.get(lo) == Some(&0xff) {
            let len = d.get(lo + 1).copied().unwrap_or(0) as usize;
            d.get(lo + 2..lo + 2 + len)
                .filter(|t| t.is_empty() || printable(t))
                .map(|t| String::from_utf8_lossy(t).into_owned())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let (ox, oy) = terminal_offset(&kind);
        let (rx, ry) = orient(ox, oy, o);
        out.push(SrcTerminal {
            kind,
            label,
            x: x as f64 / UNITS_PER_THOU + rx,
            y: y as f64 / UNITS_PER_THOU + ry,
        });
    }
    out
}

/// Pin connection points of a device symbol, in the order pins are defined.
fn device_pins(d: &[u8], device: &str) -> Vec<SrcPin> {
    let name = device.as_bytes();
    let mut key = vec![name.len() as u8];
    key.extend_from_slice(name);
    let mut at = 0;
    while let Some(k) = find(d, &key, at) {
        at = k + 1;
        let Some(end) = find(d, b"{*DEVICE}", k) else { break };
        let mut pins = Vec::new();
        let mut i = k;
        while let Some(p) = find(d, b"$PIN", i).filter(|&p| p < end) {
            i = p + 1;
            // Layout: flags 00 00, x, y, angle, "$PIN<STYLE>\0", "<name>\0".
            if p < 15 || d[p - 14] != 0 || d[p - 13] != 0 {
                continue;
            }
            let (Some(x), Some(y), Some(a)) = (i32le(d, p - 12), i32le(d, p - 8), u32le(d, p - 4))
            else {
                continue;
            };
            let Some(style_end) = find(d, b"\0", p) else { continue };
            let style = String::from_utf8_lossy(&d[p + 4..style_end]).into_owned();
            let Some(name_end) = find(d, b"\0", style_end + 1) else { continue };
            let pname = String::from_utf8_lossy(&d[style_end + 1..name_end]).into_owned();
            let len = if style == "SHORT" { 100.0 } else { 200.0 };
            let (mut dx, mut dy) = rotate(-1.0, 0.0, signed_angle(a));
            if a & 0x10000 != 0 {
                dx = -dx;
                dy = -dy;
            }
            pins.push(SrcPin {
                name: pname,
                x: x as f64 / UNITS_PER_THOU + dx * len,
                y: y as f64 / UNITS_PER_THOU + dy * len,
            });
        }
        if !pins.is_empty() {
            return pins;
        }
    }
    Vec::new()
}

// ------------------------------------------------------------------ mapping

/// Voltwright part id for a source device name, plus how its pins line up.
fn map_device(device: &str) -> Option<&'static str> {
    let d = device.to_uppercase();
    let starts = |p: &str| d.starts_with(p);
    Some(match () {
        _ if starts("LED") => {
            if d.contains("GREEN") {
                "LED-GREEN"
            } else if d.contains("YELLOW") {
                "LED-YELLOW"
            } else {
                "LED-RED"
            }
        }
        _ if starts("RES") || starts("MINRES") || starts("CHIPRES") => "RESISTOR",
        _ if starts("POT") => "POT-HG",
        _ if d.contains("ELEC") || d.contains("POL") => "CPOL",
        _ if starts("CAP") => "CAP",
        _ if starts("IND") => "INDUCTOR",
        _ if starts("ZENER") || starts("1N47") => "ZENER",
        _ if starts("DIODE") || starts("1N4") || starts("1N9") => "DIODE",
        _ if starts("2N7002") => "2N7002",
        _ if d == "PNP" || starts("2N3906") || starts("BC55") => "2N3906",
        _ if d == "NPN" || starts("2N3904") || starts("BC54") => "2N3904",
        _ if starts("2N2222") => "2N2222A",
        _ if starts("LAMP") => "LAMP",
        _ if starts("BATTERY") || starts("CELL") => "BATTERY",
        _ if starts("SW-") || starts("SWITCH") => "SWITCH",
        _ if starts("BUTTON") || starts("PUSH") => "PUSH",
        _ if d == "OPAMP" || d.contains("741") => "OPAMP",
        _ if starts("FUSE") => "FUSE",
        _ if starts("MOTOR") => "MOTOR",
        _ => return None,
    })
}

/// Our pin index for a source pin, by name first, then by order.
fn map_pin(part: &str, names: &[&str], src: &str, order: usize) -> Option<usize> {
    let s = src.to_uppercase();
    let alias = match (part, s.as_str()) {
        (_, "+") | (_, "A") | (_, "COM") => Some(0),
        (_, "-") | (_, "K") | (_, "NO") => Some(1),
        ("OPAMP", "POS IP") => Some(0),
        ("OPAMP", "NEG IP") => Some(1),
        ("OPAMP", "OP") => Some(2),
        _ => None,
    };
    if part == "OPAMP" {
        if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(&s)) {
            return Some(i);
        }
        return alias.filter(|_| !matches!(s.as_str(), "A" | "K" | "COM" | "NO"));
    }
    if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(&s)) {
        return Some(i);
    }
    alias.or((order < names.len()).then_some(order))
}

// ------------------------------------------------------------------ nets

struct Dsu(Vec<usize>);
impl Dsu {
    fn new(n: usize) -> Dsu {
        Dsu((0..n).collect())
    }
    fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        self.0[a] = b;
    }
}

fn on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    let within = |v: f64, lo: f64, hi: f64| v >= lo.min(hi) - 0.5 && v <= lo.max(hi) + 0.5;
    within(p.0, a.0, b.0)
        && within(p.1, a.1, b.1)
        && ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)).abs() < 1.0
}

fn on_wire(p: (f64, f64), w: &[(f64, f64)]) -> bool {
    w.windows(2).any(|s| on_segment(p, s[0], s[1]))
}

// ------------------------------------------------------------------ import

/// Read a .pdsprj file into a new project. Returns the project and a short
/// report of what could not be brought across.
pub fn import(path: &Path, lib: &Library) -> Result<(Project, String), String> {
    let zip = std::fs::read(path).map_err(|e| e.to_string())?;
    let dsn = zip_member(&zip, "ROOT.DSN")?;
    if !dsn.starts_with(b"ISIS SCHEMATIC FILE") {
        return Err("ROOT.DSN is not a schematic".into());
    }
    let title = zip_member(&zip, "PROJECT.XML")
        .ok()
        .and_then(|x| {
            let s = String::from_utf8_lossy(&x).into_owned();
            let a = s.find("TITLE=\"")? + 7;
            let b = s[a..].find('"')? + a;
            Some(s[a..b].to_string())
        })
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Imported".into())
        });

    let parts = parse_parts(&dsn);
    let wires = parse_wires(&dsn);
    let terms = parse_terminals(&dsn);
    let mut pin_tables: HashMap<String, Vec<SrcPin>> = HashMap::new();
    for p in &parts {
        if !pin_tables.contains_key(&p.device) {
            pin_tables.insert(p.device.clone(), device_pins(&dsn, &p.device));
        }
    }

    // Nodes: wires, then terminals, then part pins (part index, pin index, point).
    let mut pin_nodes: Vec<(usize, usize, (f64, f64))> = Vec::new();
    for (pi, p) in parts.iter().enumerate() {
        for (k, sp) in pin_tables[&p.device].iter().enumerate() {
            let (rx, ry) = orient(sp.x, sp.y, p.orient);
            pin_nodes.push((pi, k, (p.x + rx, p.y + ry)));
        }
    }
    let nw = wires.len();
    let nt = terms.len();
    let mut dsu = Dsu::new(nw + nt + pin_nodes.len());
    for i in 0..nw {
        for j in 0..nw {
            if i != j
                && (on_wire(wires[i][0], &wires[j])
                    || on_wire(*wires[i].last().unwrap(), &wires[j]))
            {
                dsu.union(i, j);
            }
        }
    }
    let point_node = |p: (f64, f64), dsu: &mut Dsu, me: usize| {
        for (j, w) in wires.iter().enumerate() {
            if on_wire(p, w) {
                dsu.union(me, j);
            }
        }
    };
    for (ti, t) in terms.iter().enumerate() {
        point_node((t.x, t.y), &mut dsu, nw + ti);
    }
    for (k, (_, _, p)) in pin_nodes.iter().enumerate() {
        point_node(*p, &mut dsu, nw + nt + k);
        for (l, (_, _, q)) in pin_nodes.iter().enumerate().skip(k + 1) {
            if (p.0 - q.0).abs() < 0.5 && (p.1 - q.1).abs() < 0.5 {
                dsu.union(nw + nt + k, nw + nt + l);
            }
        }
        for (ti, t) in terms.iter().enumerate() {
            if (p.0 - t.x).abs() < 0.5 && (p.1 - t.y).abs() < 0.5 {
                dsu.union(nw + nt + k, nw + ti);
            }
        }
    }
    // Terminals with the same name join the same net; all grounds are one net.
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (ti, t) in terms.iter().enumerate() {
        let key = match t.kind.as_str() {
            "GROUND" => "GND".to_string(),
            "POWER" if t.label.is_empty() => "VCC".to_string(),
            "NC" => continue,
            _ if t.label.is_empty() => continue,
            _ => t.label.to_uppercase(),
        };
        let node = nw + ti;
        match by_name.get(&key) {
            Some(&other) => dsu.union(node, other),
            None => {
                by_name.insert(key, node);
            }
        }
    }

    // Place parts.
    let mut proj = Project::new(&title);
    let mut placed: HashMap<usize, u64> = HashMap::new();
    let mut skipped: Vec<String> = Vec::new();
    let to_ours = |x: f64, y: f64| P::new(x as f32 * SCALE, -(y as f32) * SCALE).snap(SNAP);
    for (pi, p) in parts.iter().enumerate() {
        let Some(part_id) = map_device(&p.device) else {
            skipped.push(p.refdes.clone().unwrap_or_else(|| p.device.clone()));
            continue;
        };
        let pts: Vec<_> = pin_nodes.iter().filter(|n| n.0 == pi).map(|n| n.2).collect();
        let (cx, cy) = if pts.is_empty() {
            (p.x, p.y)
        } else {
            let n = pts.len() as f64;
            (
                pts.iter().map(|q| q.0).sum::<f64>() / n,
                pts.iter().map(|q| q.1).sum::<f64>() / n,
            )
        };
        let Some(id) = proj.add_component(lib, part_id, to_ours(cx, cy)) else {
            skipped.push(p.device.clone());
            continue;
        };
        let deg = (signed_angle(p.orient) / 10).rem_euclid(360);
        let c = proj.comps.iter_mut().find(|c| c.id == id).unwrap();
        // Source angles turn counter-clockwise with y up; ours turn clockwise
        // with y down, so the number of degrees carries over unchanged.
        c.rot = (deg + 45) / 90 * 90 % 360;
        if let Some(r) = p.refdes.clone() {
            c.refdes = r;
        }
        if let Some(v) = p.value.clone() {
            if let Some(prop) = c.props.iter_mut().find(|(k, _)| k == "VALUE") {
                prop.1 = v.clone();
            }
            c.value = v;
        }
        placed.insert(pi, id);
    }

    // Collect our pins per net.
    let mut nets: HashMap<usize, Vec<(u64, usize)>> = HashMap::new();
    for (k, (pi, src_index, _)) in pin_nodes.iter().enumerate() {
        let Some(&cid) = placed.get(pi) else { continue };
        let part_id = map_device(&parts[*pi].device).unwrap();
        let Some(def) = lib.get(part_id) else { continue };
        let names: Vec<&str> = def.pins.iter().map(|p| p.name.as_str()).collect();
        let src = &pin_tables[&parts[*pi].device][*src_index];
        let Some(our) = map_pin(part_id, &names, &src.name, *src_index) else {
            continue;
        };
        let root = dsu.find(nw + nt + k);
        nets.entry(root).or_default().push((cid, our));
    }
    // One ground or rail symbol per net that has such a terminal.
    for (ti, t) in terms.iter().enumerate() {
        let part_id = match t.kind.as_str() {
            "GROUND" => "GND",
            "POWER" if t.label.contains('-') || t.label.eq_ignore_ascii_case("VEE") => "VEE",
            "POWER" => "VCC",
            _ => continue,
        };
        let root = dsu.find(nw + ti);
        let net = nets.entry(root).or_default();
        let has_rail = net.iter().any(|(cid, _)| {
            proj.comp(*cid)
                .is_some_and(|c| c.part == part_id)
        });
        if has_rail || net.is_empty() {
            continue;
        }
        if let Some(id) = proj.add_component(lib, part_id, to_ours(t.x, t.y)) {
            nets.get_mut(&root).unwrap().push((id, 0));
        }
    }

    let mut roots: Vec<_> = nets.keys().copied().collect();
    roots.sort_unstable();
    let groups: Vec<_> = roots.into_iter().map(|r| nets[&r].clone()).collect();
    let wired = Schematic::wire_nets(&mut proj, lib, &groups);

    proj.dirty = true;
    let mut report = format!(
        "Imported '{}': {} part(s), {} wire(s)",
        title,
        placed.len(),
        wired
    );
    if !skipped.is_empty() {
        skipped.sort();
        skipped.dedup();
        report.push_str(&format!(
            "; skipped {} unsupported part(s): {}",
            skipped.len(),
            skipped.join(", ")
        ));
    }
    Ok((proj, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_and_mirror() {
        assert_eq!(rotate(1.0, 0.0, 900), (0.0, 1.0));
        assert_eq!(orient(1.0, 0.0, 0x10000), (-1.0, 0.0));
        assert_eq!(orient(1.0, 0.0, 0x10000 | 900), (0.0, -1.0));
        assert_eq!(signed_angle(0xfc7c), -900);
    }

    #[test]
    fn device_mapping() {
        assert_eq!(map_device("RESISTOR"), Some("RESISTOR"));
        assert_eq!(map_device("LED-GREEN"), Some("LED-GREEN"));
        assert_eq!(map_device("CAP-ELEC"), Some("CPOL"));
        assert_eq!(map_device("NPN"), Some("2N3904"));
        assert_eq!(map_device("1N914"), Some("DIODE"));
        assert_eq!(map_device("ATMEGA328P"), None);
        let bat = ["1", "2"];
        assert_eq!(map_pin("BATTERY", &bat, "-", 1), Some(1));
        assert_eq!(map_pin("BATTERY", &bat, "+", 0), Some(0));
        let op = ["+", "-", "OUT", "V+", "V-"];
        assert_eq!(map_pin("OPAMP", &op, "OP", 2), Some(2));
        assert_eq!(map_pin("OPAMP", &op, "V-", 4), Some(4));
    }

    /// Set PDSPRJ_SAMPLE to a .pdsprj file to run an end-to-end import.
    #[test]
    fn imports_sample_from_env() {
        let Ok(path) = std::env::var("PDSPRJ_SAMPLE") else { return };
        let lib = Library::builtin();
        let (proj, report) = import(Path::new(&path), &lib).expect("import");
        println!("{report}");
        for c in &proj.comps {
            println!("  {} {} {} at {:?} rot {}", c.refdes, c.part, c.value, (c.x, c.y), c.rot);
        }
        let nl = proj.netlist(&lib);
        println!("  nets: {:?}", nl.names);
        assert!(!proj.comps.is_empty());
    }
}
