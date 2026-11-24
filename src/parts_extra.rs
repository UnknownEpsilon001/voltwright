// Additional library parts. Most are described by a short table row: an id,
// a name, a category, a package, and the pin names in package order.
//
// Parts with a simulation model (diodes, LEDs, transistors, dual op-amps)
// take part in the live simulation. Logic chips, regulators, drivers,
// sensors, displays and microcontrollers are pin-accurate blocks for
// schematic capture and PCB layout; they are not simulated yet.

use crate::model::{named_dual_row_pins, named_part, two_pin, PartDef, PinDef, SimKind, Symbol, P};

/// Add a part. An older part with the same id is kept unless its pins are only
/// numbered placeholders and the new part has the same pin count, in which
/// case the pin-accurate version replaces it.
fn add(parts: &mut Vec<PartDef>, part: PartDef) {
    match parts.iter().position(|p| p.id == part.id) {
        None => parts.push(part),
        Some(i) => {
            let old = &parts[i];
            let placeholder = old.pins.iter().all(|pin| pin.name.parse::<u32>().is_ok());
            if placeholder && old.pins.len() == part.pins.len() && part.pins.iter().any(|pin| pin.name.parse::<u32>().is_err()) {
                parts[i] = part;
            }
        }
    }
}

/// Smallest symbol half-width (in schematic units) that keeps the pin labels
/// on the left and right sides from running into each other.
fn fit_width(pins: &[&str]) -> f32 {
    let n = pins.len();
    let left = n.div_ceil(2);
    let label = |i: usize| {
        let num = (i + 1).to_string();
        if pins[i] == num { num.len() } else { num.len() + 2 + pins[i].chars().count() }
    };
    let l = (0..left).map(label).max().unwrap_or(0) as f32;
    let r = (left..n).map(label).max().unwrap_or(0) as f32;
    let need = 18.0 + (50.0 + 10.7 * (l + r)) / 2.0;
    ((need.max(120.0) / 5.0).ceil()) * 5.0
}

/// A chip drawn as a box with its pins on two sides.
fn chip(
    parts: &mut Vec<PartDef>,
    id: &str,
    name: &str,
    category: &str,
    package: &str,
    prefix: &str,
    pins: &[&str],
) {
    let width = fit_width(pins).max(if pins.len() > 24 { 175.0 } else { 120.0 });
    add(
        parts,
        named_part(id, name, category, "DEVICE", package, id, prefix, named_dual_row_pins(pins, width)),
    );
}

/// A single-row package (header, sensor, regulator). The symbol splits the
/// pins across both sides of the box so inputs and outputs read naturally.
fn sil(
    parts: &mut Vec<PartDef>,
    id: &str,
    name: &str,
    category: &str,
    package: &str,
    prefix: &str,
    pins: &[&str],
) {
    add(
        parts,
        named_part(id, name, category, "DEVICE", package, id, prefix, named_dual_row_pins(pins, fit_width(pins))),
    );
}

fn two(
    parts: &mut Vec<PartDef>,
    id: &str,
    name: &str,
    category: &str,
    package: &str,
    symbol: Symbol,
    sim: SimKind,
    value: &str,
    prefix: &str,
) {
    add(parts, two_pin(id, name, category, "DEVICE", package, symbol, sim, value, prefix));
}

/// Three-terminal transistor with the same symbol geometry as the built-ins.
fn transistor(
    parts: &mut Vec<PartDef>,
    id: &str,
    name: &str,
    package: &str,
    symbol: Symbol,
    sim: SimKind,
    names: [&str; 3],
) {
    let mut q = two_pin(id, name, "Transistors", "DEVICE", package, symbol, sim, id, "Q");
    // PNP and P-channel parts draw the emitter/source on top, so swap the
    // positions of the second and third pins; pin order stays the same.
    let flip = matches!(symbol, Symbol::Pnp | Symbol::Pmos);
    let (top, bottom) = (((50.0, -60.0), 90), ((50.0, 60.0), 270));
    let (second, third) = if flip { (bottom, top) } else { (top, bottom) };
    q.pins = vec![
        PinDef { name: names[0].into(), pos: P::new(-90.0, 0.0), dir: 180 },
        PinDef { name: names[1].into(), pos: P::new(second.0 .0, second.0 .1), dir: second.1 },
        PinDef { name: names[2].into(), pos: P::new(third.0 .0, third.0 .1), dir: third.1 },
    ];
    add(parts, q);
}

pub fn add_all(p: &mut Vec<PartDef>) {
    // ---- Logic gates (simulated, 5 V logic, no supply pins) ------------
    for (id, name) in [
        ("AND2", "2-INPUT AND GATE"),
        ("NAND2", "2-INPUT NAND GATE"),
        ("OR2", "2-INPUT OR GATE"),
        ("NOR2", "2-INPUT NOR GATE"),
        ("XOR2", "2-INPUT XOR GATE"),
        ("XNOR2", "2-INPUT XNOR GATE"),
        ("NOT", "INVERTER"),
        ("BUF", "BUFFER"),
    ] {
        let mut g = two_pin(id, name, "Logic Gates", "DEVICE", "DIL-14", Symbol::Gate, SimKind::Logic, id, "U");
        let pin = |n: &str, x: f32, y: f32, dir: i32| PinDef { name: n.into(), pos: P::new(x, y), dir };
        g.pins = if id == "NOT" || id == "BUF" {
            vec![pin("A", -90.0, 0.0, 180), pin("Y", 90.0, 0.0, 0)]
        } else {
            vec![pin("A", -90.0, -25.0, 180), pin("B", -90.0, 25.0, 180), pin("Y", 90.0, 0.0, 0)]
        };
        add(p, g);
    }

    use SimKind::D;
    use Symbol::{Diode, Led, Speaker};

    // ---- Diodes -------------------------------------------------------
    for (id, name, pkg) in [
        ("1N4001", "RECTIFIER DIODE 50V 1A", "DO-41"),
        ("1N4002", "RECTIFIER DIODE 100V 1A", "DO-41"),
        ("1N4004", "RECTIFIER DIODE 400V 1A", "DO-41"),
        ("1N4007", "RECTIFIER DIODE 1000V 1A", "DO-41"),
        ("1N4148", "SWITCHING DIODE", "DO-35"),
        ("1N5817", "SCHOTTKY DIODE 20V 1A", "DO-41"),
        ("1N5819", "SCHOTTKY DIODE 40V 1A", "DO-41"),
        ("1N5822", "SCHOTTKY DIODE 40V 3A", "DO-201"),
        ("BAT54", "SCHOTTKY DIODE 30V", "SOT-23"),
    ] {
        two(p, id, name, "Diodes", pkg, Diode, D, id, "D");
    }
    for (id, v) in [
        ("1N4728", "3V3"),
        ("1N4733", "5V1"),
        ("1N4735", "6V2"),
        ("1N4739", "9V1"),
        ("1N4742", "12V"),
        ("1N4744", "15V"),
        ("1N4749", "24V"),
    ] {
        two(p, id, &format!("ZENER DIODE {v}"), "Diodes", "DO-41", Diode, D, v, "D");
    }

    // ---- LEDs ---------------------------------------------------------
    for (id, colour) in [
        ("LED-BLUE", "BLUE"),
        ("LED-WHITE", "WHITE"),
        ("LED-ORANGE", "ORANGE"),
    ] {
        two(p, id, &format!("LED ({colour})"), "Optoelectronics", "LED-5MM", Led, D, colour, "LED");
    }

    // ---- Transistors --------------------------------------------------
    for (id, name, pkg) in [
        ("BC547", "NPN TRANSISTOR 45V", "TO-92"),
        ("BC548", "NPN TRANSISTOR 30V", "TO-92"),
        ("BC549", "NPN LOW-NOISE TRANSISTOR", "TO-92"),
        ("BC337", "NPN TRANSISTOR 800mA", "TO-92"),
        ("PN2222", "NPN TRANSISTOR 600mA", "TO-92"),
        ("S8050", "NPN TRANSISTOR 25V 700mA", "TO-92"),
        ("2N5551", "NPN HIGH-VOLTAGE TRANSISTOR", "TO-92"),
        ("TIP31C", "NPN POWER TRANSISTOR 3A", "TO-220"),
        ("TIP120", "NPN DARLINGTON 5A", "TO-220"),
    ] {
        transistor(p, id, name, pkg, Symbol::Npn, SimKind::Npn, ["B", "C", "E"]);
    }
    for (id, name, pkg) in [
        ("BC557", "PNP TRANSISTOR 45V", "TO-92"),
        ("BC558", "PNP TRANSISTOR 30V", "TO-92"),
        ("BC327", "PNP TRANSISTOR 800mA", "TO-92"),
        ("2N2907", "PNP TRANSISTOR 600mA", "TO-92"),
        ("S8550", "PNP TRANSISTOR 25V 700mA", "TO-92"),
        ("2N5401", "PNP HIGH-VOLTAGE TRANSISTOR", "TO-92"),
        ("TIP32C", "PNP POWER TRANSISTOR 3A", "TO-220"),
        ("TIP127", "PNP DARLINGTON 5A", "TO-220"),
    ] {
        transistor(p, id, name, pkg, Symbol::Pnp, SimKind::Pnp, ["B", "C", "E"]);
    }
    for (id, name, pkg) in [
        ("2N7000", "N-CHANNEL MOSFET 60V", "TO-92"),
        ("IRF540N", "N-CHANNEL MOSFET 100V 33A", "TO-220"),
        ("IRFZ44N", "N-CHANNEL MOSFET 55V 49A", "TO-220"),
        ("IRLZ44N", "LOGIC-LEVEL N-MOSFET 55V", "TO-220"),
    ] {
        transistor(p, id, name, pkg, Symbol::Nmos, SimKind::Npn, ["G", "D", "S"]);
    }
    for (id, name, pkg) in [
        ("IRF9540N", "P-CHANNEL MOSFET 100V 23A", "TO-220"),
        ("IRF4905", "P-CHANNEL MOSFET 55V 74A", "TO-220"),
    ] {
        transistor(p, id, name, pkg, Symbol::Pmos, SimKind::Pnp, ["G", "D", "S"]);
    }

    // ---- Dual op-amps (simulated) and other amplifiers ---------------
    for (id, name) in [
        ("TL072", "DUAL JFET OP-AMP"),
        ("TL082", "DUAL JFET OP-AMP"),
        ("NE5532", "DUAL LOW-NOISE OP-AMP"),
        ("LM833", "DUAL AUDIO OP-AMP"),
        ("MCP6002", "DUAL RAIL-TO-RAIL OP-AMP"),
    ] {
        let mut c = named_part(
            id,
            name,
            "Amplifiers",
            "DEVICE",
            "DIL-8",
            id,
            "U",
            named_dual_row_pins(&["OUT1", "IN1-", "IN1+", "V-", "IN2+", "IN2-", "OUT2", "V+"], 120.0),
        );
        c.sim = SimKind::OpAmp;
        add(p, c);
    }
    chip(p, "LM324", "QUAD OP-AMP", "Amplifiers", "DIL-14", "U",
        &["1OUT", "1IN-", "1IN+", "V+", "2IN+", "2IN-", "2OUT", "3OUT", "3IN-", "3IN+", "GND", "4IN+", "4IN-", "4OUT"]);
    chip(p, "LM393", "DUAL COMPARATOR", "Amplifiers", "DIL-8", "U",
        &["OUT1", "IN1-", "IN1+", "GND", "IN2+", "IN2-", "OUT2", "V+"]);
    chip(p, "LM386", "AUDIO POWER AMPLIFIER", "Amplifiers", "DIL-8", "U",
        &["GAIN", "IN-", "IN+", "GND", "OUT", "V+", "BYPASS", "GAIN2"]);

    // ---- Logic: 7400 series ------------------------------------------
    const QUAD_2IN: [&str; 14] = [
        "1A", "1B", "1Y", "2A", "2B", "2Y", "GND", "3Y", "3A", "3B", "4Y", "4A", "4B", "VCC",
    ];
    const QUAD_NOR: [&str; 14] = [
        "1Y", "1A", "1B", "2Y", "2A", "2B", "GND", "3A", "3B", "3Y", "4A", "4B", "4Y", "VCC",
    ];
    const HEX_INV: [&str; 14] = [
        "1A", "1Y", "2A", "2Y", "3A", "3Y", "GND", "4Y", "4A", "5Y", "5A", "6Y", "6A", "VCC",
    ];
    for fam in ["74LS", "74HC"] {
        let id = |n: &str| format!("{fam}{n}");
        chip(p, &id("00"), "QUAD 2-INPUT NAND", "TTL", "DIL-14", "U", &QUAD_2IN);
        chip(p, &id("02"), "QUAD 2-INPUT NOR", "TTL", "DIL-14", "U", &QUAD_NOR);
        chip(p, &id("04"), "HEX INVERTER", "TTL", "DIL-14", "U", &HEX_INV);
        chip(p, &id("08"), "QUAD 2-INPUT AND", "TTL", "DIL-14", "U", &QUAD_2IN);
        chip(p, &id("14"), "HEX SCHMITT INVERTER", "TTL", "DIL-14", "U", &HEX_INV);
        chip(p, &id("32"), "QUAD 2-INPUT OR", "TTL", "DIL-14", "U", &QUAD_2IN);
        chip(p, &id("86"), "QUAD 2-INPUT XOR", "TTL", "DIL-14", "U", &QUAD_2IN);
        chip(p, &id("10"), "TRIPLE 3-INPUT NAND", "TTL", "DIL-14", "U",
            &["1A", "1B", "2A", "2B", "2C", "2Y", "GND", "3Y", "3A", "3B", "3C", "1Y", "1C", "VCC"]);
        chip(p, &id("74"), "DUAL D FLIP-FLOP", "TTL", "DIL-14", "U",
            &["1CLR", "1D", "1CLK", "1PRE", "1Q", "1QN", "GND", "2QN", "2Q", "2PRE", "2CLK", "2D", "2CLR", "VCC"]);
        chip(p, &id("138"), "3-TO-8 DECODER", "TTL", "DIL-16", "U",
            &["A", "B", "C", "G2A", "G2B", "G1", "Y7", "GND", "Y6", "Y5", "Y4", "Y3", "Y2", "Y1", "Y0", "VCC"]);
        chip(p, &id("161"), "4-BIT BINARY COUNTER", "TTL", "DIL-16", "U",
            &["CLR", "CLK", "A", "B", "C", "D", "ENP", "GND", "LOAD", "ENT", "QD", "QC", "QB", "QA", "RCO", "VCC"]);
        chip(p, &id("164"), "8-BIT SHIFT REGISTER", "TTL", "DIL-14", "U",
            &["A", "B", "QA", "QB", "QC", "QD", "GND", "CLK", "CLR", "QE", "QF", "QG", "QH", "VCC"]);
        chip(p, &id("245"), "OCTAL BUS TRANSCEIVER", "TTL", "DIL-20", "U",
            &["DIR", "A1", "A2", "A3", "A4", "A5", "A6", "A7", "A8", "GND", "B8", "B7", "B6", "B5", "B4", "B3", "B2", "B1", "OE", "VCC"]);
        chip(p, &id("373"), "OCTAL D LATCH", "TTL", "DIL-20", "U",
            &["OE", "1Q", "1D", "2D", "2Q", "3Q", "3D", "4D", "4Q", "GND", "LE", "5Q", "5D", "6D", "6Q", "7Q", "7D", "8D", "8Q", "VCC"]);
        chip(p, &id("595"), "8-BIT SHIFT REGISTER WITH LATCH", "TTL", "DIL-16", "U",
            &["QB", "QC", "QD", "QE", "QF", "QG", "QH", "GND", "QH'", "SRCLR", "SRCLK", "RCLK", "OE", "SER", "QA", "VCC"]);
    }

    // ---- Logic: 4000 series and timers -------------------------------
    chip(p, "CD4011", "QUAD 2-INPUT NAND", "TTL", "DIL-14", "U",
        &["A1", "B1", "J1", "J2", "A2", "B2", "VSS", "A3", "B3", "J3", "J4", "A4", "B4", "VDD"]);
    chip(p, "CD4093", "QUAD 2-INPUT NAND SCHMITT", "TTL", "DIL-14", "U",
        &["A1", "B1", "J1", "J2", "A2", "B2", "VSS", "A3", "B3", "J3", "J4", "A4", "B4", "VDD"]);
    chip(p, "CD4017", "DECADE COUNTER", "TTL", "DIL-16", "U",
        &["Q5", "Q1", "Q0", "Q2", "Q6", "Q7", "Q3", "VSS", "Q8", "Q4", "Q9", "CO", "INH", "CLK", "RST", "VDD"]);
    chip(p, "CD4511", "BCD TO 7-SEGMENT DECODER", "TTL", "DIL-16", "U",
        &["B", "C", "LT", "BI", "LE", "D", "A", "VSS", "e", "d", "c", "b", "a", "g", "f", "VDD"]);

    // ---- Power management --------------------------------------------
    for (id, name) in [
        ("7805", "+5V REGULATOR 1A"),
        ("7806", "+6V REGULATOR 1A"),
        ("7809", "+9V REGULATOR 1A"),
        ("7812", "+12V REGULATOR 1A"),
        ("7815", "+15V REGULATOR 1A"),
    ] {
        sil(p, id, name, "Power Management", "TO-220", "U", &["IN", "GND", "OUT"]);
    }
    sil(p, "7905", "-5V REGULATOR 1A", "Power Management", "TO-220", "U", &["GND", "IN", "OUT"]);
    sil(p, "LM317", "ADJUSTABLE REGULATOR 1.5A", "Power Management", "TO-220", "U", &["ADJ", "OUT", "IN"]);
    sil(p, "LM337", "ADJUSTABLE NEGATIVE REGULATOR", "Power Management", "TO-220", "U", &["ADJ", "IN", "OUT"]);
    sil(p, "LM1117-3V3", "3.3V LDO REGULATOR", "Power Management", "SOT-223", "U", &["GND", "OUT", "IN"]);
    sil(p, "AMS1117-5.0", "5.0V LDO REGULATOR", "Power Management", "SOT-223", "U", &["GND", "OUT", "IN"]);

    // ---- Drivers ------------------------------------------------------
    chip(p, "L293D", "DUAL H-BRIDGE MOTOR DRIVER", "Drivers", "DIL-16", "U",
        &["EN12", "1A", "1Y", "GND1", "GND2", "2Y", "2A", "VS", "EN34", "3A", "3Y", "GND3", "GND4", "4Y", "4A", "VCC"]);
    chip(p, "ULN2003A", "7-CHANNEL DARLINGTON ARRAY", "Drivers", "DIL-16", "U",
        &["1B", "2B", "3B", "4B", "5B", "6B", "7B", "E", "COM", "7C", "6C", "5C", "4C", "3C", "2C", "1C"]);

    // ---- Microprocessors ---------------------------------------------
    chip(p, "ATTINY85", "8-BIT AVR MCU 8K", "Microprocessors", "DIL-8", "U",
        &["PB5/RESET", "PB3/ADC3", "PB4/ADC2", "GND", "PB0/MOSI", "PB1/MISO", "PB2/SCK", "VCC"]);
    chip(p, "ATTINY13", "8-BIT AVR MCU 1K", "Microprocessors", "DIL-8", "U",
        &["PB5/RESET", "PB3/ADC3", "PB4/ADC2", "GND", "PB0/MOSI", "PB1/MISO", "PB2/SCK", "VCC"]);
    chip(p, "PIC16F84A", "8-BIT PIC MCU 1.75K", "Microprocessors", "DIL-18", "U",
        &["RA2", "RA3", "RA4/T0CKI", "MCLR", "VSS", "RB0/INT", "RB1", "RB2", "RB3", "RB4", "RB5", "RB6", "RB7", "VDD", "OSC2", "OSC1", "RA0", "RA1"]);
    chip(p, "AT89C51", "8051 MCU 4K FLASH", "Microprocessors", "DIL-40", "U",
        &["P1.0", "P1.1", "P1.2", "P1.3", "P1.4", "P1.5", "P1.6", "P1.7", "RST", "P3.0/RXD", "P3.1/TXD", "P3.2/INT0", "P3.3/INT1", "P3.4/T0", "P3.5/T1", "P3.6/WR", "P3.7/RD", "XTAL2", "XTAL1", "GND",
          "P2.0", "P2.1", "P2.2", "P2.3", "P2.4", "P2.5", "P2.6", "P2.7", "PSEN", "ALE", "EA", "P0.7", "P0.6", "P0.5", "P0.4", "P0.3", "P0.2", "P0.1", "P0.0", "VCC"]);

    // ---- Sensors ------------------------------------------------------
    sil(p, "LM35", "PRECISION TEMPERATURE SENSOR", "Sensors", "TO-92", "U", &["+VS", "VOUT", "GND"]);
    sil(p, "DS18B20", "1-WIRE TEMPERATURE SENSOR", "Sensors", "TO-92", "U", &["GND", "DQ", "VDD"]);
    sil(p, "DHT11", "TEMPERATURE AND HUMIDITY SENSOR", "Sensors", "SIL-4", "U", &["VCC", "DATA", "NC", "GND"]);
    sil(p, "HC-SR04", "ULTRASONIC DISTANCE SENSOR", "Sensors", "SIL-4", "U", &["VCC", "TRIG", "ECHO", "GND"]);
    sil(p, "HC-SR501", "PIR MOTION SENSOR", "Sensors", "SIL-3", "U", &["VCC", "OUT", "GND"]);
    sil(p, "MPU6050", "6-AXIS ACCELEROMETER AND GYRO", "Sensors", "SIL-8", "U",
        &["VCC", "GND", "SCL", "SDA", "XDA", "XCL", "AD0", "INT"]);
    sil(p, "BMP280", "PRESSURE SENSOR MODULE", "Sensors", "SIL-4", "U", &["VCC", "GND", "SCL", "SDA"]);

    // ---- Displays -----------------------------------------------------
    chip(p, "LCD16X2", "16x2 CHARACTER LCD", "Displays", "SIL-16", "LCD",
        &["VSS", "VDD", "VEE", "RS", "RW", "E", "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "A", "K"]);
    chip(p, "7SEG-CC", "7-SEGMENT DISPLAY (COMMON CATHODE)", "Displays", "DIL-10", "DS",
        &["E", "D", "COM", "C", "DP", "B", "A", "COM2", "F", "G"]);
    chip(p, "7SEG-CA", "7-SEGMENT DISPLAY (COMMON ANODE)", "Displays", "DIL-10", "DS",
        &["E", "D", "COM", "C", "DP", "B", "A", "COM2", "F", "G"]);
    sil(p, "SSD1306-I2C", "0.96\" OLED DISPLAY (I2C)", "Displays", "SIL-4", "U", &["GND", "VCC", "SCL", "SDA"]);

    // ---- Connectors ---------------------------------------------------
    for n in 2..=8usize {
        let names: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        sil(p, &format!("CONN-SIL{n}"), &format!("{n}-PIN HEADER"), "Connectors", &format!("SIL-{n}"), "J", &refs);
    }
    sil(p, "USB-A", "USB TYPE-A CONNECTOR", "Connectors", "SIL-4", "J", &["VBUS", "D-", "D+", "GND"]);
    sil(p, "USB-MICRO", "USB MICRO-B CONNECTOR", "Connectors", "SIL-5", "J", &["VBUS", "D-", "D+", "ID", "GND"]);
    sil(p, "DC-JACK", "DC POWER JACK", "Connectors", "SIL-3", "J", &["TIP", "SLEEVE", "SWITCH"]);

    // ---- Switches, relays, sounders, motors --------------------------
    sil(p, "RELAY-SPDT", "SPDT RELAY", "Switches & Relays", "SIL-5", "RL", &["COIL+", "COIL-", "COM", "NO", "NC"]);
    two(p, "BUZZER", "PIEZO BUZZER", "Sounders", "BUZZER", Speaker, SimKind::None, "BUZZER", "BZ");
    sil(p, "SERVO", "HOBBY SERVO MOTOR", "Motors", "SIL-3", "M", &["GND", "VCC", "SIG"]);
    sil(p, "STEPPER-4", "4-WIRE STEPPER MOTOR", "Motors", "SIL-4", "M", &["A+", "A-", "B+", "B-"]);
}

#[cfg(test)]
mod tests {
    use crate::model::{footprint_pads, Library, SimKind};
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_pins_named() {
        let lib = Library::builtin();
        let mut seen = HashSet::new();
        for p in &lib.parts {
            assert!(seen.insert(p.id.clone()), "duplicate part id {}", p.id);
            assert!(!p.pins.is_empty(), "{} has no pins", p.id);
            for pin in &p.pins {
                assert!(!pin.name.is_empty(), "{} has an unnamed pin", p.id);
            }
        }
        assert!(lib.parts.len() >= 170, "library has {} parts", lib.parts.len());
    }

    #[test]
    fn every_package_makes_pads() {
        let lib = Library::builtin();
        for p in &lib.parts {
            if p.package == "NONE" {
                continue;
            }
            let pads = footprint_pads(&p.package, p.pins.len());
            assert_eq!(pads.len(), p.pins.len(), "{} ({})", p.id, p.package);
        }
    }

    #[test]
    fn simulated_opamps_expose_plus_minus_out() {
        let lib = Library::builtin();
        for p in lib.parts.iter().filter(|p| p.sim == SimKind::OpAmp) {
            let has = |names: &[&str]| {
                p.pins
                    .iter()
                    .any(|pin| names.iter().any(|n| pin.name.eq_ignore_ascii_case(n)))
            };
            assert!(has(&["+", "IN+", "IN1+"]), "{} lacks a + input", p.id);
            assert!(has(&["-", "IN-", "IN1-"]), "{} lacks a - input", p.id);
            assert!(has(&["OUT", "OUT1"]), "{} lacks an output", p.id);
        }
    }

    #[test]
    fn emitter_or_source_is_on_top_for_p_type_parts() {
        use crate::model::Symbol;
        let lib = Library::builtin();
        for p in &lib.parts {
            if !matches!(p.symbol, Symbol::Npn | Symbol::Pnp | Symbol::Nmos | Symbol::Pmos) || p.pins.len() != 3 {
                continue;
            }
            let (c, e) = (p.pins[1].pos.y, p.pins[2].pos.y);
            if matches!(p.symbol, Symbol::Pnp | Symbol::Pmos) {
                assert!(e < c, "{}: P-type emitter/source belongs on top", p.id);
            } else {
                assert!(c < e, "{}: N-type collector/drain belongs on top", p.id);
            }
        }
    }

    #[test]
    fn search_finds_by_pin_function() {
        let lib = Library::builtin();
        assert!(lib.search("srclk", "All Categories").iter().any(|p| p.id == "74LS595"));
        assert!(lib.search("echo", "All Categories").iter().any(|p| p.id == "HC-SR04"));
    }
}
