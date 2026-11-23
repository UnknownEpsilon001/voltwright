// Built-in example circuits. Each one is a list of parts and the nets that
// join their pins; wires are routed when the example opens.

use crate::model::{Library, Project, P};
use crate::schematic::Schematic;

struct Part {
    id: &'static str,
    refdes: &'static str,
    value: &'static str,
    at: (f32, f32),
    rot: i32,
}

struct Example {
    name: &'static str,
    notes: &'static str,
    parts: &'static [Part],
    /// Each net lists (reference, pin index).
    nets: &'static [&'static [(&'static str, usize)]],
}

const fn part(id: &'static str, refdes: &'static str, value: &'static str, x: f32, y: f32, rot: i32) -> Part {
    Part { id, refdes, value, at: (x, y), rot }
}

const EXAMPLES: &[Example] = &[
    Example {
        name: "LED with current-limiting resistor",
        notes: "A 9 V battery drives a red LED through 470 ohms, about 15 mA. \
                Change R1 to 47 ohms and run the simulation to see the LED burn out.",
        parts: &[
            part("BATTERY", "B1", "9V", -250.0, 0.0, 90),
            part("RESISTOR", "R1", "470R", -50.0, -150.0, 0),
            part("LED-RED", "D1", "LED-RED", 150.0, 0.0, 90),
            part("GND", "GND1", "0V", -250.0, 200.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("R1", 0)],
            &[("R1", 1), ("D1", 0)],
            &[("D1", 1), ("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Voltage divider",
        notes: "Two equal resistors split 12 V in half. Hover the middle wire, \
                or add a voltage probe there, to read 6 V.",
        parts: &[
            part("BATTERY", "B1", "12V", -250.0, 0.0, 90),
            part("RESISTOR", "R1", "10k", 50.0, -100.0, 90),
            part("RESISTOR", "R2", "10k", 50.0, 100.0, 90),
            part("GND", "GND1", "0V", -250.0, 250.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("R1", 0)],
            &[("R1", 1), ("R2", 0)],
            &[("R2", 1), ("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Transistor switch with lamp",
        notes: "Close SW1 to push current into the base of Q1. The transistor \
                turns on and lights the lamp from the same 9 V supply.",
        parts: &[
            part("BATTERY", "B1", "9V", -350.0, 50.0, 90),
            part("SWITCH", "SW1", "SWITCH", -200.0, -150.0, 0),
            part("RESISTOR", "R1", "1k", 0.0, 50.0, 0),
            part("2N3904", "Q1", "2N3904", 200.0, 50.0, 0),
            part("LAMP", "L1", "12V", 225.0, -150.0, 90),
            part("GND", "GND1", "0V", -350.0, 250.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("SW1", 0), ("L1", 0)],
            &[("SW1", 1), ("R1", 0)],
            &[("R1", 1), ("Q1", 0)],
            &[("L1", 1), ("Q1", 1)],
            &[("Q1", 2), ("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "RC charging",
        notes: "Close SW1 and watch C1 charge through R1. The time constant \
                is R x C = 10k x 100u = 1 second.",
        parts: &[
            part("BATTERY", "B1", "5V", -300.0, 0.0, 90),
            part("SWITCH", "SW1", "SWITCH", -150.0, -150.0, 0),
            part("RESISTOR", "R1", "10k", 50.0, -150.0, 0),
            part("CAP", "C1", "100u", 200.0, 0.0, 90),
            part("GND", "GND1", "0V", -300.0, 200.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("SW1", 0)],
            &[("SW1", 1), ("R1", 0)],
            &[("R1", 1), ("C1", 0)],
            &[("C1", 1), ("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Non-inverting op-amp amplifier",
        notes: "U1 amplifies the 1 V input by 1 + R2/R1 = 3, so the output sits \
                at 3 V. The op-amp runs from the +5 V and -5 V rails.",
        parts: &[
            part("DCSRC", "V1", "1V", -350.0, 50.0, 90),
            part("OPAMP", "U1", "OPAMP", 0.0, 0.0, 0),
            part("RESISTOR", "R1", "10k", -150.0, 200.0, 90),
            part("RESISTOR", "R2", "20k", 50.0, 150.0, 0),
            part("RESISTOR", "RL", "10k", 300.0, 100.0, 90),
            part("VCC", "VCC1", "5V", 0.0, -200.0, 0),
            part("VEE", "VEE1", "-5V", 100.0, 250.0, 0),
            part("GND", "GND1", "0V", -350.0, 350.0, 0),
        ],
        nets: &[
            &[("V1", 0), ("U1", 0)],
            &[("U1", 1), ("R1", 0), ("R2", 0)],
            &[("R2", 1), ("U1", 2), ("RL", 0)],
            &[("U1", 3), ("VCC1", 0)],
            &[("U1", 4), ("VEE1", 0)],
            &[("V1", 1), ("R1", 1), ("RL", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Diode rectifier",
        notes: "A sine source feeds D1, which passes only the positive half \
                of each cycle into the load. Open the Simulation tab to see it.",
        parts: &[
            part("SINE", "V1", "SINE", -300.0, 50.0, 90),
            part("DIODE", "D1", "1N4148", -100.0, -100.0, 0),
            part("RESISTOR", "RL", "1k", 100.0, 50.0, 90),
            part("GND", "GND1", "0V", -300.0, 250.0, 0),
        ],
        nets: &[
            &[("V1", 0), ("D1", 0)],
            &[("D1", 1), ("RL", 0)],
            &[("RL", 1), ("V1", 1), ("GND1", 0)],
        ],
    },
];

/// Names of the built-in examples, in menu order.
pub fn names() -> impl Iterator<Item = &'static str> {
    EXAMPLES.iter().map(|e| e.name)
}

/// Build the named example, or None if no example has that name.
pub fn build(name: &str, lib: &Library) -> Option<Project> {
    let ex = EXAMPLES.iter().find(|e| e.name == name)?;
    let mut proj = Project::new(ex.name);
    proj.notes = ex.notes.to_string();
    let mut ids = Vec::new();
    for p in ex.parts {
        let id = proj.add_component(lib, p.id, P::new(p.at.0, p.at.1))?;
        let c = proj.comps.iter_mut().find(|c| c.id == id)?;
        c.refdes = p.refdes.to_string();
        c.rot = p.rot;
        if !p.value.is_empty() && p.value != p.id {
            c.value = p.value.to_string();
            if let Some(v) = c.props.iter_mut().find(|(k, _)| k == "VALUE") {
                v.1 = p.value.to_string();
            }
        }
        ids.push((p.refdes, id));
    }
    let nets: Vec<Vec<(u64, usize)>> = ex
        .nets
        .iter()
        .map(|net| {
            net.iter()
                .filter_map(|(r, pin)| ids.iter().find(|(n, _)| n == r).map(|(_, id)| (*id, *pin)))
                .collect()
        })
        .collect();
    Schematic::wire_nets(&mut proj, lib, &nets);
    proj.dirty = false;
    Some(proj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Simulator;

    #[test]
    fn every_example_builds_and_solves() {
        let lib = Library::builtin();
        for name in names() {
            let proj = build(name, &lib).unwrap_or_else(|| panic!("{name} failed to build"));
            let ex = EXAMPLES.iter().find(|e| e.name == name).unwrap();
            assert_eq!(proj.comps.len(), ex.parts.len(), "{name}: missing parts");
            let mut sim = Simulator::from_project(&proj, &lib)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            sim.solve_dc().unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn divider_gives_half_supply() {
        let lib = Library::builtin();
        let proj = build("Voltage divider", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let r2 = proj.comps.iter().find(|c| c.refdes == "R2").unwrap();
        let el = sim.element(&r2.refdes).unwrap();
        let v = sim.element_voltage(el).abs();
        assert!((v - 6.0).abs() < 0.1, "R2 sees {v} V, expected 6 V");
    }

    #[test]
    fn led_example_lights_the_led() {
        let lib = Library::builtin();
        let proj = build("LED with current-limiting resistor", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        assert!(sim.led_level("D1") > 0.1, "LED should be lit");
    }

    #[test]
    fn amplifier_output_is_three_volts() {
        let lib = Library::builtin();
        let proj = build("Non-inverting op-amp amplifier", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let rl = sim.element("RL").unwrap();
        let v = sim.element_voltage(rl).abs();
        assert!((v - 3.0).abs() < 0.2, "output {v} V, expected 3 V");
    }
}
