// Electrical rules check for the schematic: problems a person would want to
// know about before simulating or laying out a board.

use std::collections::HashMap;

use crate::model::{Library, Netlist, Project, SimKind, Symbol, P};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct ErcIssue {
    pub severity: Severity,
    pub message: String,
    pub at: P,
}

/// Symbols whose pins connect by name or are only markers, so a lone pin is fine.
fn is_marker(sym: Symbol) -> bool {
    matches!(sym, Symbol::Ground | Symbol::Rail | Symbol::Terminal | Symbol::Probe | Symbol::Frame)
}

/// Pins that are normally left open: unused scope channels, op-amp supply
/// pins (the model is ideal), a timer's control pin, and no-connect pins.
fn may_float(def: &crate::model::PartDef, name: &str) -> bool {
    let n = name.to_uppercase();
    def.symbol == Symbol::Scope
        || n == "CTRL"
        || n.starts_with("NC")
        || n.starts_with("N/C")
        || (def.symbol == Symbol::OpAmp && (n == "V+" || n == "V-"))
}

/// True for a pin that drives its net: gate and flip-flop outputs, op-amp and
/// timer outputs.
fn drives(def: &crate::model::PartDef, pin: usize) -> bool {
    let name = def.pins.get(pin).map(|p| p.name.as_str()).unwrap_or("");
    match def.symbol {
        Symbol::Gate => name == "Y",
        Symbol::OpAmp => pin == 2 || name.eq_ignore_ascii_case("OUT"),
        _ => match def.id.as_str() {
            "DFF" => name == "Q" || name == "QN",
            "555" => name == "OUT",
            _ => false,
        },
    }
}

pub fn check(proj: &Project, lib: &Library) -> Vec<ErcIssue> {
    let mut out = Vec::new();
    let nl = proj.netlist(lib);

    // Pins per net.
    let mut members: HashMap<&str, Vec<(u64, usize)>> = HashMap::new();
    for ((comp, pin), net) in &nl.pin_net {
        members.entry(net.as_str()).or_default().push((*comp, *pin));
    }
    let wired: std::collections::HashSet<(u64, usize)> = proj
        .wires
        .iter()
        .flat_map(|w| [w.start_pin, w.end_pin].into_iter().flatten())
        .collect();

    // Duplicate reference designators.
    let mut seen: HashMap<&str, u64> = HashMap::new();
    for c in &proj.comps {
        if c.refdes.is_empty() {
            continue;
        }
        if let Some(&first) = seen.get(c.refdes.as_str()) {
            let _ = first;
            out.push(ErcIssue {
                severity: Severity::Error,
                message: format!("Reference {} is used by more than one part", c.refdes),
                at: c.pos(),
            });
        } else {
            seen.insert(c.refdes.as_str(), c.id);
        }
    }

    for c in &proj.comps {
        let Some(def) = lib.get(&c.part) else {
            out.push(ErcIssue { severity: Severity::Error, message: format!("{} uses a part that is not in the library", c.refdes), at: c.pos() });
            continue;
        };
        if is_marker(def.symbol) {
            continue;
        }
        for (i, pd) in def.pins.iter().enumerate() {
            let net = nl.net_of(c.id, i);
            let alone = net.map_or(true, |n| members.get(n).map_or(0, |m| m.len()) < 2);
            let has_wire = wired.contains(&(c.id, i));
            // A pin counts as connected when a wire reaches it, or it shares a net
            // (through a label or a power symbol) with another pin.
            if alone && !has_wire && !may_float(def, &pd.name) {
                let at = proj.pin_pos(lib, c.id, i).unwrap_or(c.pos());
                let what = if def.symbol == Symbol::Gate && !drives(def, i) { "input" } else { "pin" };
                out.push(ErcIssue {
                    severity: Severity::Warning,
                    message: format!("{} {what} {} ({}) is not connected", c.refdes, i + 1, pd.name),
                    at,
                });
            }
        }
    }

    // Nets that end at a single pin through a wire (a wire to nowhere).
    for w in &proj.wires {
        for pin in [w.start_pin, w.end_pin].into_iter().flatten() {
            let Some(net) = nl.net_of(pin.0, pin.1) else { continue };
            let count = members.get(net).map_or(0, |m| m.len());
            let free_end = if w.start_pin == Some(pin) { w.pts.last() } else { w.pts.first() };
            let other_is_pin = if w.start_pin == Some(pin) { w.end_pin.is_some() } else { w.start_pin.is_some() };
            if count < 2 && !other_is_pin {
                if let Some(at) = free_end {
                    let name = proj.comp(pin.0).map(|c| c.refdes.clone()).unwrap_or_default();
                    out.push(ErcIssue { severity: Severity::Warning, message: format!("A wire from {name} ends in empty space"), at: *at });
                }
            }
        }
    }

    // Two outputs driving one net.
    for (net, pins) in &members {
        let drivers: Vec<&(u64, usize)> = pins
            .iter()
            .filter(|(c, p)| proj.comp(*c).and_then(|c| lib.get(&c.part)).is_some_and(|d| drives(d, *p)))
            .collect();
        if drivers.len() > 1 {
            let names: Vec<String> = drivers.iter().filter_map(|(c, _)| proj.comp(*c).map(|c| c.refdes.clone())).collect();
            let at = drivers.first().and_then(|(c, p)| proj.pin_pos(lib, *c, *p)).unwrap_or(P::ZERO);
            out.push(ErcIssue {
                severity: Severity::Error,
                message: format!("Outputs of {} drive the same net ({net})", names.join(" and ")),
                at,
            });
        }
    }

    // Two ideal voltage sources across the same pair of nets.
    let sources: Vec<(&crate::model::Component, String, String)> = proj
        .comps
        .iter()
        .filter(|c| lib.get(&c.part).is_some_and(|d| d.sim == SimKind::V && d.pins.len() == 2))
        .filter_map(|c| Some((c, nl.net_of(c.id, 0)?.to_string(), nl.net_of(c.id, 1)?.to_string())))
        .collect();
    for i in 0..sources.len() {
        for j in (i + 1)..sources.len() {
            let (a, b) = (&sources[i], &sources[j]);
            if (a.1 == b.1 && a.2 == b.2) || (a.1 == b.2 && a.2 == b.1) {
                out.push(ErcIssue {
                    severity: Severity::Error,
                    message: format!("{} and {} are ideal sources wired in parallel", a.0.refdes, b.0.refdes),
                    at: a.0.pos(),
                });
            }
        }
    }

    // Simulated circuit with no ground.
    let simulated = proj.comps.iter().any(|c| lib.get(&c.part).is_some_and(|d| d.sim != SimKind::None));
    let grounded = nl.pin_net.values().any(|n| Netlist::is_ground(n));
    if simulated && !grounded {
        out.push(ErcIssue { severity: Severity::Warning, message: "The circuit has no ground; add a GND symbol".into(), at: P::ZERO });
    }
    out.sort_by(|a, b| (a.severity != Severity::Error).cmp(&(b.severity != Severity::Error)).then(a.message.cmp(&b.message)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_examples_pass_the_rules_check() {
        let lib = Library::builtin();
        for name in crate::examples::names() {
            let proj = crate::examples::build(name, &lib).unwrap();
            let errors: Vec<_> = check(&proj, &lib).into_iter().filter(|i| i.severity == Severity::Error).collect();
            assert!(errors.is_empty(), "{name}: {:?}", errors.iter().map(|e| &e.message).collect::<Vec<_>>());
        }
    }

    #[test]
    fn finds_a_loose_pin_a_duplicate_reference_and_a_missing_ground() {
        let lib = Library::builtin();
        let mut proj = Project::new("t");
        let a = proj.add_component(&lib, "RESISTOR", P::new(500.0, 500.0)).unwrap();
        let b = proj.add_component(&lib, "RESISTOR", P::new(900.0, 500.0)).unwrap();
        proj.comp_mut(b).unwrap().refdes = proj.comp(a).unwrap().refdes.clone();
        let issues = check(&proj, &lib);
        assert!(issues.iter().any(|i| i.message.contains("not connected")), "{issues:?}");
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("more than one part")));
        // No simulated ground check fires without simulated parts having no ground: a resistor is simulated.
        assert!(issues.iter().any(|i| i.message.contains("no ground")));
    }

    #[test]
    fn two_gate_outputs_on_one_net_are_an_error() {
        let lib = Library::builtin();
        let mut proj = Project::new("t");
        let a = proj.add_component(&lib, "NOT", P::new(500.0, 500.0)).unwrap();
        let b = proj.add_component(&lib, "NOT", P::new(500.0, 800.0)).unwrap();
        // Join the two output pins with a wire.
        let pa = proj.pin_pos(&lib, a, 1).unwrap();
        let pb = proj.pin_pos(&lib, b, 1).unwrap();
        let id = proj.alloc_id();
        proj.wires.push(crate::model::Wire { id, pts: vec![pa, P::new(pa.x + 100.0, pa.y), P::new(pa.x + 100.0, pb.y), pb], start_pin: Some((a, 1)), end_pin: Some((b, 1)) });
        let issues = check(&proj, &lib);
        assert!(issues.iter().any(|i| i.severity == Severity::Error && i.message.contains("drive the same net")), "{issues:?}");
    }

    #[test]
    fn annotate_fixes_duplicates_and_renumbers_in_reading_order() {
        let lib = Library::builtin();
        let mut proj = Project::new("t");
        let a = proj.add_component(&lib, "RESISTOR", P::new(900.0, 300.0)).unwrap();
        let b = proj.add_component(&lib, "RESISTOR", P::new(300.0, 300.0)).unwrap();
        let c = proj.add_component(&lib, "RESISTOR", P::new(600.0, 900.0)).unwrap();
        for id in [a, b, c] {
            proj.comp_mut(id).unwrap().refdes = "R1".into();
        }
        // Fixing duplicates keeps one R1 and gives the others new numbers.
        assert_eq!(proj.annotate(&lib, false), 2);
        let mut names: Vec<String> = proj.comps.iter().map(|c| c.refdes.clone()).collect();
        names.sort();
        assert_eq!(names, ["R1", "R2", "R3"]);
        // Renumbering everything follows the reading order: row one left to right, then row two.
        proj.annotate(&lib, true);
        let r = |id: u64| proj.comp(id).unwrap().refdes.clone();
        assert_eq!((r(b), r(a), r(c)), ("R1".into(), "R2".into(), "R3".into()));
        assert_eq!(proj.annotate(&lib, false), 0, "already unique");
    }
}
