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

// Layouts are drawn on the pin positions of each part so that every
// connection is a straight run or a single bend: vertical parts have pin 1 on
// top and pin 2 below, horizontal parts have pin 1 on the left.
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
            &[("D1", 1), ("B1", 1)],
            &[("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Voltage divider",
        notes: "Two equal resistors split 12 V in half. Hover the middle wire, \
                or add a voltage probe there, to read 6 V.",
        parts: &[
            part("BATTERY", "B1", "12V", -250.0, 100.0, 90),
            part("RESISTOR", "R1", "10k", 50.0, -100.0, 90),
            part("RESISTOR", "R2", "10k", 50.0, 100.0, 90),
            part("GND", "GND1", "0V", -250.0, 260.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("R1", 0)],
            &[("R1", 1), ("R2", 0)],
            &[("R2", 1), ("B1", 1)],
            &[("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Transistor switch with lamp",
        notes: "Close SW1 to push current into the base of Q1. The transistor \
                turns on and lights the lamp from the same 9 V supply.",
        parts: &[
            part("BATTERY", "B1", "9V", -350.0, 100.0, 90),
            part("SWITCH", "SW1", "SWITCH", -200.0, -100.0, 0),
            part("RESISTOR", "R1", "1k", 0.0, -100.0, 0),
            part("2N3904", "Q1", "2N3904", 150.0, 0.0, 0),
            part("LAMP", "L1", "12V", 200.0, -200.0, 90),
            part("GND", "GND1", "0V", -350.0, 300.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("SW1", 0)],
            &[("B1", 0), ("L1", 0)],
            &[("SW1", 1), ("R1", 0)],
            &[("R1", 1), ("Q1", 0)],
            &[("L1", 1), ("Q1", 1)],
            &[("Q1", 2), ("B1", 1)],
            &[("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "RC charging",
        notes: "Close SW1 and watch C1 charge through R1. The time constant \
                is R x C = 10k x 100u = 1 second.",
        parts: &[
            part("BATTERY", "B1", "5V", -300.0, 100.0, 90),
            part("SWITCH", "SW1", "SWITCH", -150.0, -100.0, 0),
            part("RESISTOR", "R1", "10k", 50.0, -100.0, 0),
            part("CAP", "C1", "100u", 250.0, -40.0, 90),
            part("GND", "GND1", "0V", -300.0, 300.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("SW1", 0)],
            &[("SW1", 1), ("R1", 0)],
            &[("R1", 1), ("C1", 0)],
            &[("C1", 1), ("B1", 1)],
            &[("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Non-inverting op-amp amplifier",
        notes: "U1 amplifies the 1 V input by 1 + R2/R1 = 3, so the output sits \
                at 3 V. This is an ideal op-amp; its supply pins are not used \
                by the simulation.",
        parts: &[
            part("DCSRC", "V1", "1V", -300.0, 20.0, 90),
            part("OPAMP", "U1", "OPAMP", 0.0, 0.0, 0),
            part("RESISTOR", "R1", "10k", -190.0, 100.0, 90),
            part("RESISTOR", "R2", "20k", 200.0, 100.0, 90),
            part("RESISTOR", "RL", "10k", 420.0, 60.0, 90),
            part("GND", "GND1", "0V", -190.0, 260.0, 0),
        ],
        nets: &[
            &[("V1", 0), ("U1", 0)],
            &[("U1", 1), ("R1", 0)],
            &[("U1", 2), ("R2", 0)],
            &[("R2", 0), ("RL", 0)],
            &[("R2", 1), ("U1", 1)],
            &[("R1", 1), ("GND1", 0)],
            &[("V1", 1), ("GND1", 0)],
            &[("RL", 1), ("GND1", 0)],
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
            &[("RL", 1), ("V1", 1)],
            &[("V1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "555 timer LED blinker",
        notes: "A 555 in astable mode: C1 charges through R1 and R2 and \
                discharges through R2, so the output flips about 1.5 times a \
                second and D1 blinks. f = 1.44 / ((R1 + 2 x R2) x C1). Press \
                Run Simulation and watch the LED.",
        parts: &[
            part("555", "U1", "NE555", 0.0, 0.0, 0),
            part("VCC", "VCC1", "5V", 200.0, -75.0, 0),
            part("VCC", "VCC2", "5V", -200.0, 110.0, 0),
            part("VCC", "VCC3", "5V", 400.0, -205.0, 0),
            part("GND", "GND1", "0V", -200.0, -75.0, 0),
            part("RESISTOR", "R1", "1k", 400.0, -85.0, 90),
            part("RESISTOR", "R2", "47k", 400.0, 35.0, 90),
            part("CAP", "C1", "10u", 400.0, 230.0, 90),
            part("GND", "GND2", "0V", 400.0, 350.0, 0),
            part("RESISTOR", "R3", "220R", -400.0, 25.0, 0),
            part("LED-RED", "D1", "LED-RED", -460.0, 85.0, 90),
            part("GND", "GND3", "0V", -460.0, 205.0, 0),
        ],
        nets: &[
            &[("U1", 7), ("VCC1", 0)],
            &[("U1", 3), ("VCC2", 0)],
            &[("U1", 0), ("GND1", 0)],
            &[("VCC3", 0), ("R1", 0)],
            &[("R1", 1), ("U1", 6)],
            &[("R2", 0), ("U1", 6)],
            &[("R2", 1), ("U1", 5)],
            &[("R2", 1), ("C1", 0)],
            &[("C1", 0), ("U1", 1)],
            &[("C1", 1), ("GND2", 0)],
            &[("U1", 2), ("R3", 1)],
            &[("R3", 0), ("D1", 0)],
            &[("D1", 1), ("GND3", 0)],
        ],
    },
    Example {
        name: "NAND gate truth table",
        notes: "Input A toggles every second and B every two seconds, so the \
                gate sees all four input pairs in turn. The LED lights unless \
                both inputs are high. Press Run Simulation and watch D1.",
        parts: &[
            part("PULSE", "VA", "PULSE(0 5 0 1m 1m 1 2)", -500.0, 135.0, 90),
            part("PULSE", "VB", "PULSE(0 5 0 1m 1m 2 4)", -300.0, 185.0, 90),
            part("GND", "GND1", "0V", -500.0, 260.0, 0),
            part("GND", "GND2", "0V", -300.0, 310.0, 0),
            part("NAND2", "U1", "NAND2", 0.0, 100.0, 0),
            part("RESISTOR", "R1", "330R", 250.0, 100.0, 0),
            part("LED-GREEN", "D1", "LED-GREEN", 370.0, 160.0, 90),
            part("GND", "GND3", "0V", 370.0, 280.0, 0),
        ],
        nets: &[
            &[("VA", 0), ("U1", 0)],
            &[("VB", 0), ("U1", 1)],
            &[("VA", 1), ("GND1", 0)],
            &[("VB", 1), ("GND2", 0)],
            &[("U1", 2), ("R1", 0)],
            &[("R1", 1), ("D1", 0)],
            &[("D1", 1), ("GND3", 0)],
        ],
    },
    Example {
        name: "AND, OR and XOR gates",
        notes: "The same two inputs feed an AND, an OR and an XOR gate. AND \
                lights only when both inputs are high, OR when either is, and \
                XOR when exactly one is.",
        parts: &[
            part("PULSE", "VA", "PULSE(0 5 0 1m 1m 1 2)", -700.0, 15.0, 90),
            part("PULSE", "VB", "PULSE(0 5 0 1m 1m 2 4)", -400.0, 65.0, 90),
            part("GND", "GND1", "0V", -700.0, 140.0, 0),
            part("GND", "GND2", "0V", -400.0, 190.0, 0),
            part("AND2", "U1", "AND2", -50.0, -150.0, 0),
            part("OR2", "U2", "OR2", -50.0, 100.0, 0),
            part("XOR2", "U3", "XOR2", -50.0, 350.0, 0),
            part("RESISTOR", "R1", "330R", 200.0, -150.0, 0),
            part("RESISTOR", "R2", "330R", 200.0, 100.0, 0),
            part("RESISTOR", "R3", "330R", 200.0, 350.0, 0),
            part("LED-RED", "D1", "LED-RED", 320.0, -90.0, 90),
            part("LED-YELLOW", "D2", "LED-YELLOW", 320.0, 160.0, 90),
            part("LED-GREEN", "D3", "LED-GREEN", 320.0, 410.0, 90),
            part("GND", "GND3", "0V", 320.0, 10.0, 0),
            part("GND", "GND4", "0V", 320.0, 260.0, 0),
            part("GND", "GND5", "0V", 320.0, 510.0, 0),
        ],
        nets: &[
            &[("VA", 0), ("U1", 0), ("U2", 0), ("U3", 0)],
            &[("VB", 0), ("U1", 1), ("U2", 1), ("U3", 1)],
            &[("VA", 1), ("GND1", 0)],
            &[("VB", 1), ("GND2", 0)],
            &[("U1", 2), ("R1", 0)],
            &[("U2", 2), ("R2", 0)],
            &[("U3", 2), ("R3", 0)],
            &[("R1", 1), ("D1", 0)],
            &[("R2", 1), ("D2", 0)],
            &[("R3", 1), ("D3", 0)],
            &[("D1", 1), ("GND3", 0)],
            &[("D2", 1), ("GND4", 0)],
            &[("D3", 1), ("GND5", 0)],
        ],
    },
    Example {
        name: "Half adder",
        notes: "An XOR gate gives the sum bit and an AND gate gives the carry. \
                Adding 1 + 1 lights only the carry LED: sum 0, carry 1 is \
                binary 10, which is two.",
        parts: &[
            part("PULSE", "VA", "PULSE(0 5 0 1m 1m 1 2)", -700.0, 15.0, 90),
            part("PULSE", "VB", "PULSE(0 5 0 1m 1m 2 4)", -400.0, 65.0, 90),
            part("GND", "GND1", "0V", -700.0, 140.0, 0),
            part("GND", "GND2", "0V", -400.0, 190.0, 0),
            part("XOR2", "U1", "XOR2", -50.0, -125.0, 0),
            part("AND2", "U2", "AND2", -50.0, 125.0, 0),
            part("RESISTOR", "R1", "330R", 200.0, -125.0, 0),
            part("RESISTOR", "R2", "330R", 200.0, 125.0, 0),
            part("LED-GREEN", "D1", "SUM", 320.0, -65.0, 90),
            part("LED-RED", "D2", "CARRY", 320.0, 185.0, 90),
            part("GND", "GND3", "0V", 320.0, 35.0, 0),
            part("GND", "GND4", "0V", 320.0, 285.0, 0),
        ],
        nets: &[
            &[("VA", 0), ("U1", 0), ("U2", 0)],
            &[("VB", 0), ("U1", 1), ("U2", 1)],
            &[("VA", 1), ("GND1", 0)],
            &[("VB", 1), ("GND2", 0)],
            &[("U1", 2), ("R1", 0)],
            &[("U2", 2), ("R2", 0)],
            &[("R1", 1), ("D1", 0)],
            &[("R2", 1), ("D2", 0)],
            &[("D1", 1), ("GND3", 0)],
            &[("D2", 1), ("GND4", 0)],
        ],
    },
    Example {
        name: "SR latch from NOR gates",
        notes: "Two cross-coupled NOR gates remember one bit. A pulse on S \
                sets Q high and a later pulse on R clears it; between pulses \
                the latch holds its state.",
        parts: &[
            part("PULSE", "VR", "PULSE(0 5 4 1m 1m 0.5 6)", -450.0, -65.0, 90),
            part("PULSE", "VS", "PULSE(0 5 1 1m 1m 0.5 6)", -450.0, 235.0, 90),
            part("GND", "GND1", "0V", -450.0, 60.0, 0),
            part("GND", "GND2", "0V", -450.0, 360.0, 0),
            // U2 comes first so it settles first and the latch starts with Q low.
            part("NOR2", "U2", "NOR2", 0.0, 150.0, 0),
            part("NOR2", "U1", "NOR2", 0.0, -100.0, 0),
            part("RESISTOR", "R1", "330R", 250.0, -100.0, 0),
            part("RESISTOR", "R2", "330R", 250.0, 150.0, 0),
            part("LED-GREEN", "D1", "Q", 370.0, -40.0, 90),
            part("LED-RED", "D2", "QBAR", 370.0, 210.0, 90),
            part("GND", "GND3", "0V", 370.0, 80.0, 0),
            part("GND", "GND4", "0V", 370.0, 330.0, 0),
        ],
        nets: &[
            &[("VR", 0), ("U1", 0)],
            &[("VS", 0), ("U2", 0)],
            &[("VR", 1), ("GND1", 0)],
            &[("VS", 1), ("GND2", 0)],
            &[("U1", 2), ("U2", 1)],
            &[("U2", 2), ("U1", 1)],
            &[("U1", 2), ("R1", 0)],
            &[("U2", 2), ("R2", 0)],
            &[("R1", 1), ("D1", 0)],
            &[("R2", 1), ("D2", 0)],
            &[("D1", 1), ("GND3", 0)],
            &[("D2", 1), ("GND4", 0)],
        ],
    },
    Example {
        name: "Voltmeter and ammeter",
        notes: "A 9 V battery drives R1 (1k) and R2 (2k) in series. The ammeter \
                sits in the loop and reads 3 mA; the voltmeter across R2 reads \
                6 V. Meters show readings while the simulation runs.",
        parts: &[
            part("BATTERY", "B1", "9V", -250.0, 0.0, 90),
            part("AMMETER", "AM1", "", -50.0, -150.0, 0),
            part("RESISTOR", "R1", "1k", 150.0, -60.0, 90),
            part("RESISTOR", "R2", "2k", 150.0, 120.0, 90),
            part("VOLTMETER", "VM1", "", 300.0, 120.0, 90),
            part("GND", "GND1", "0V", -250.0, 200.0, 0),
            part("GND", "GND2", "0V", 150.0, 260.0, 0),
        ],
        nets: &[
            &[("B1", 0), ("AM1", 0)],
            &[("AM1", 1), ("R1", 0)],
            &[("R1", 1), ("R2", 0)],
            &[("R1", 1), ("VM1", 0)],
            &[("R2", 1), ("VM1", 1)],
            &[("R2", 1), ("GND2", 0)],
            &[("B1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Oscilloscope on an RC filter",
        notes: "A 1 kHz sine goes through R1 and C1. Scope channel A watches \
                the input and channel B the output, which is smaller and \
                delayed. The oscilloscope window opens when you run the \
                simulation; change time/div and volts/div there.",
        parts: &[
            part("SIGGEN", "G1", "SINE(0 2 1k)", -300.0, 60.0, 90),
            part("RESISTOR", "R1", "1k", -100.0, 0.0, 0),
            part("CAP", "C1", "220n", 100.0, 60.0, 90),
            part("GND", "GND1", "0V", -300.0, 200.0, 0),
            part("GND", "GND2", "0V", 100.0, 200.0, 0),
            part("SCOPE", "OSC1", "", 350.0, 60.0, 0),
        ],
        nets: &[
            &[("G1", 0), ("R1", 0)],
            &[("G1", 0), ("OSC1", 0)],
            &[("R1", 1), ("C1", 0)],
            &[("C1", 0), ("OSC1", 1)],
            &[("C1", 1), ("GND2", 0)],
            &[("G1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Frequency counter and logic probe",
        notes: "A 100 Hz square wave drives a frequency counter and a logic \
                probe. The counter shows the measured frequency; the probe \
                shows H or L and flips as the signal switches.",
        parts: &[
            part("SIGGEN", "G1", "PULSE(0 5 0 1u 1u 5m 10m)", -250.0, 60.0, 90),
            part("COUNTER", "FC1", "", 150.0, -50.0, 0),
            part("LOGICPROBE", "LP1", "", 150.0, 70.0, 0),
            part("GND", "GND1", "0V", -250.0, 180.0, 0),
        ],
        nets: &[
            &[("G1", 0), ("FC1", 0)],
            &[("G1", 0), ("LP1", 0)],
            &[("G1", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Rectifier with smoothing capacitor",
        notes: "D1 passes the positive half of a 10 V, 50 Hz sine. C1 charges on \
                each peak and holds the load voltage up between peaks, leaving \
                a small ripple. A smaller C1 gives more ripple.",
        parts: &[
            part("SINE", "V1", "SINE(0 10 50)", -300.0, 50.0, 90),
            part("DIODE", "D1", "1N4007", -100.0, -10.0, 0),
            part("CPOL", "C1", "470u", 60.0, 50.0, 90),
            part("RESISTOR", "RL", "1k", 160.0, 50.0, 90),
            part("GND", "GND1", "0V", -300.0, 150.0, 0),
            part("GND", "GND2", "0V", 60.0, 150.0, 0),
            part("GND", "GND3", "0V", 160.0, 150.0, 0),
        ],
        nets: &[
            &[("V1", 0), ("D1", 0)],
            &[("D1", 1), ("C1", 0)],
            &[("C1", 0), ("RL", 0)],
            &[("V1", 1), ("GND1", 0)],
            &[("C1", 1), ("GND2", 0)],
            &[("RL", 1), ("GND3", 0)],
        ],
    },
    Example {
        name: "Inverting op-amp amplifier",
        notes: "The gain is -R2/R1 = -2, so a 1 V input gives -2 V at the \
                output. The inverting input is a virtual ground. This is an \
                ideal op-amp; its supply pins are not used by the simulation.",
        parts: &[
            part("DCSRC", "V1", "1V", -400.0, 100.0, 90),
            part("RESISTOR", "R1", "10k", -200.0, 40.0, 0),
            part("OPAMP", "U1", "OPAMP", 0.0, 0.0, 0),
            part("RESISTOR", "R2", "20k", -40.0, -140.0, 0),
            part("RESISTOR", "RL", "10k", 250.0, 60.0, 90),
            part("GND", "GND1", "0V", -400.0, 220.0, 0),
            part("GND", "GND2", "0V", -150.0, -40.0, 0),
            part("GND", "GND3", "0V", 250.0, 180.0, 0),
        ],
        nets: &[
            &[("V1", 0), ("R1", 0)],
            &[("R1", 1), ("U1", 1)],
            &[("R1", 1), ("R2", 0)],
            &[("R2", 1), ("U1", 2)],
            &[("U1", 2), ("RL", 0)],
            &[("U1", 0), ("GND2", 0)],
            &[("V1", 1), ("GND1", 0)],
            &[("RL", 1), ("GND3", 0)],
        ],
    },
    Example {
        name: "Series RLC resonance",
        notes: "R1, L1 and C1 resonate at 1 / (2 x pi x sqrt(L x C)), about \
                500 Hz. The source is at that frequency, so the voltage across \
                C1 (scope channel B) swings several times larger than the 1 V \
                input (channel A). Run it and open the oscilloscope.",
        parts: &[
            part("SIGGEN", "G1", "SINE(0 1 500)", -350.0, 60.0, 90),
            part("RESISTOR", "R1", "4R", -150.0, 0.0, 0),
            part("INDUCTOR", "L1", "10m", 50.0, 0.0, 0),
            part("CAP", "C1", "10u", 250.0, 60.0, 90),
            part("GND", "GND1", "0V", -350.0, 160.0, 0),
            part("GND", "GND2", "0V", 250.0, 160.0, 0),
            part("SCOPE", "OSC1", "", 500.0, 60.0, 0),
        ],
        nets: &[
            &[("G1", 0), ("R1", 0)],
            &[("G1", 0), ("OSC1", 0)],
            &[("R1", 1), ("L1", 0)],
            &[("L1", 1), ("C1", 0)],
            &[("C1", 0), ("OSC1", 1)],
            &[("G1", 1), ("GND1", 0)],
            &[("C1", 1), ("GND2", 0)],
        ],
    },
    Example {
        name: "Common-emitter amplifier",
        notes: "R1 and R2 bias Q1 so its collector rests near 9 V. A small 1 kHz \
                signal reaches the base through C1 and comes out inverted and \
                about twice as large (gain is about -RC/RE). The oscilloscope \
                shows the collector; tick AC coupling to centre it.",
        parts: &[
            part("SIGGEN", "G1", "SINE(0 0.3 1k)", -500.0, 160.0, 90),
            part("CAP", "C1", "10u", -350.0, 100.0, 0),
            part("RESISTOR", "R1", "47k", -150.0, 20.0, 90),
            part("RESISTOR", "R2", "10k", -150.0, 180.0, 90),
            part("RESISTOR", "RC", "2.2k", 50.0, -80.0, 90),
            part("2N3904", "Q1", "2N3904", 0.0, 100.0, 0),
            part("RESISTOR", "RE", "1k", 50.0, 220.0, 90),
            part("DCSRC", "V2", "12V", 400.0, 100.0, 90),
            part("VCC", "VCC1", "12V", -150.0, -40.0, 0),
            part("VCC", "VCC2", "12V", 50.0, -140.0, 0),
            part("VCC", "VCC3", "12V", 400.0, 40.0, 0),
            part("GND", "GND1", "0V", -500.0, 260.0, 0),
            part("GND", "GND2", "0V", -150.0, 240.0, 0),
            part("GND", "GND3", "0V", 50.0, 300.0, 0),
            part("GND", "GND4", "0V", 400.0, 200.0, 0),
            part("SCOPE", "OSC1", "", 250.0, 100.0, 0),
        ],
        nets: &[
            &[("G1", 0), ("C1", 0)],
            &[("C1", 1), ("R1", 1)],
            &[("R1", 1), ("R2", 0)],
            &[("R1", 1), ("Q1", 0)],
            &[("R1", 0), ("VCC1", 0)],
            &[("RC", 0), ("VCC2", 0)],
            &[("RC", 1), ("Q1", 1)],
            &[("Q1", 1), ("OSC1", 0)],
            &[("Q1", 2), ("RE", 0)],
            &[("V2", 0), ("VCC3", 0)],
            &[("G1", 1), ("GND1", 0)],
            &[("R2", 1), ("GND2", 0)],
            &[("RE", 1), ("GND3", 0)],
            &[("V2", 1), ("GND4", 0)],
        ],
    },
    Example {
        name: "D flip-flop divide-by-two",
        notes: "The inverted output QN is wired back to D, so Q flips on every                 rising clock edge and changes once per second while the clock                 runs at 1 Hz. This is the building block of binary counters.",
        parts: &[
            part("PULSE", "VC", "PULSE(0 5 0 1m 1m 0.5 1)", -300.0, 100.0, 90),
            part("GND", "GND1", "0V", -300.0, 200.0, 0),
            part("DFF", "U1", "DFF", 0.0, 0.0, 0),
            part("RESISTOR", "R1", "330R", 240.0, -25.0, 0),
            part("LED-GREEN", "D1", "Q", 300.0, 35.0, 90),
            part("GND", "GND2", "0V", 300.0, 140.0, 0),
        ],
        nets: &[
            &[("VC", 0), ("U1", 1)],
            &[("U1", 2), ("U1", 0)],
            &[("U1", 3), ("R1", 0)],
            &[("R1", 1), ("D1", 0)],
            &[("D1", 1), ("GND2", 0)],
            &[("VC", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "Op-amp voltage follower",
        notes: "The output is wired straight back to the inverting input, so \
                U1 copies the 2 V input with unity gain and can drive the \
                load. This is an ideal op-amp; its supply pins are not used by \
                the simulation.",
        parts: &[
            part("DCSRC", "V1", "2V", -250.0, 20.0, 90),
            part("OPAMP", "U1", "OPAMP", 0.0, 0.0, 0),
            part("RESISTOR", "RL", "1k", 300.0, 60.0, 90),
            part("GND", "GND1", "0V", -250.0, 200.0, 0),
        ],
        nets: &[
            &[("V1", 0), ("U1", 0)],
            &[("U1", 2), ("RL", 0)],
            &[("U1", 2), ("U1", 1)],
            &[("V1", 1), ("GND1", 0)],
            &[("RL", 1), ("GND1", 0)],
        ],
    },
    Example {
        name: "RC low-pass filter",
        notes: "A 1 kHz sine passes through R1 and C1. The corner frequency is \
                1 / (2 x pi x R x C), about 1.6 kHz. Run an AC sweep on the \
                Simulation tab to see the output fall away above it.",
        parts: &[
            part("SINE", "V1", "SINE(0 1 1k)", -250.0, 60.0, 90),
            part("RESISTOR", "R1", "1k", -50.0, 0.0, 0),
            part("CAP", "C1", "100n", 150.0, 60.0, 90),
            part("GND", "GND1", "0V", -250.0, 260.0, 0),
        ],
        nets: &[
            &[("V1", 0), ("R1", 0)],
            &[("R1", 1), ("C1", 0)],
            &[("C1", 1), ("V1", 1)],
            &[("V1", 1), ("GND1", 0)],
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
    // Centre the circuit on the sheet (the sheet's origin is its top-left
    // corner), on the 25-thou grid so pins stay aligned.
    let (mut lo, mut hi) = (P::new(f32::MAX, f32::MAX), P::new(f32::MIN, f32::MIN));
    for c in &proj.comps {
        lo = P::new(lo.x.min(c.x), lo.y.min(c.y));
        hi = P::new(hi.x.max(c.x), hi.y.max(c.y));
    }
    let snap = |v: f32| (v / 25.0).round() * 25.0;
    let (dx, dy) = (
        snap(crate::schematic::SHEET_W * 0.5 - (lo.x + hi.x) * 0.5),
        snap(crate::schematic::SHEET_H * 0.5 - (lo.y + hi.y) * 0.5),
    );
    for c in &mut proj.comps {
        c.x += dx;
        c.y += dy;
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
    fn examples_sit_inside_the_sheet() {
        let lib = Library::builtin();
        for name in names() {
            let proj = build(name, &lib).unwrap();
            for c in &proj.comps {
                assert!(
                    c.x > 100.0 && c.x < crate::schematic::SHEET_W - 100.0
                        && c.y > 100.0 && c.y < crate::schematic::SHEET_H - 100.0,
                    "{name}: {} is at ({}, {}), outside the sheet",
                    c.refdes, c.x, c.y
                );
            }
        }
    }

    #[test]
    fn the_555_blinker_blinks() {
        let lib = Library::builtin();
        let proj = build("555 timer LED blinker", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let (mut on, mut off, mut flips) = (0, 0, 0);
        let mut last = false;
        // Eight seconds in 50 ms steps, like the live simulation.
        for i in 0..160 {
            sim.step(0.05).unwrap();
            let lit = sim.led_level("D1") > 0.05;
            if lit { on += 1 } else { off += 1 }
            if i > 0 && lit != last {
                flips += 1;
            }
            last = lit;
        }
        assert!(on > 20 && off > 20, "LED was on {on} and off {off} steps");
        assert!(flips >= 6, "expected several blinks in 8 s, got {flips} changes");
    }

    #[test]
    fn gate_examples_follow_their_truth_tables() {
        let lib = Library::builtin();
        // Inputs by one-second window: (A, B) = (1,1), (0,1), (1,0), (0,0).
        let cases: [(&str, &[(&str, [bool; 4])]); 3] = [
            ("NAND gate truth table", &[("D1", [false, true, true, true])]),
            (
                "AND, OR and XOR gates",
                &[
                    ("D1", [true, false, false, false]),
                    ("D2", [true, true, true, false]),
                    ("D3", [false, true, true, false]),
                ],
            ),
            ("Half adder", &[("D1", [false, true, true, false]), ("D2", [true, false, false, false])]),
        ];
        for (name, leds) in cases {
            let proj = build(name, &lib).unwrap();
            let mut sim = Simulator::from_project(&proj, &lib).unwrap();
            sim.solve_dc().unwrap();
            let mut seen = vec![[false; 4]; leds.len()];
            for step in 0..80 {
                sim.step(0.05).unwrap();
                // sample the middle of each one-second window (B changes every 2 s)
                if step % 20 == 9 {
                    let k = step / 20;
                    for (i, (led, _)) in leds.iter().enumerate() {
                        seen[i][k] = sim.led_level(led) > 0.05;
                    }
                }
            }
            for (i, (led, want)) in leds.iter().enumerate() {
                assert_eq!(&seen[i][..4], want, "{name}: {led}");
            }
        }
    }

    #[test]
    fn sr_latch_sets_holds_and_resets() {
        let lib = Library::builtin();
        let proj = build("SR latch from NOR gates", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let mut at = |t: f64| {
            while sim.t < t {
                sim.step(0.05).unwrap();
            }
            sim.led_level("D1") > 0.05
        };
        assert!(!at(0.9), "Q starts low");
        assert!(at(1.3), "S pulse sets Q");
        assert!(at(3.0), "Q is held between pulses");
        assert!(!at(4.3), "R pulse clears Q");
        assert!(!at(5.5), "Q stays low");
    }

    #[test]
    fn meters_read_the_loop() {
        let lib = Library::builtin();
        let proj = build("Voltmeter and ammeter", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let a = sim.current_of(sim.element("AM1").unwrap());
        let v = sim.element_voltage(sim.element("VM1").unwrap());
        assert!((a.abs() - 0.003).abs() < 1e-4, "ammeter {a}");
        assert!((v.abs() - 6.0).abs() < 0.05, "voltmeter {v}");
    }

    #[test]
    fn scope_records_input_and_smaller_output() {
        let lib = Library::builtin();
        let proj = build("Oscilloscope on an RC filter", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        for _ in 0..10 {
            sim.step(0.05).unwrap();
        }
        assert_eq!(sim.traces.len(), 2);
        let peak = |i: usize| sim.traces[i].pts.iter().rev().take(400).map(|s| s.1.abs()).fold(0.0f32, f32::max);
        assert!((peak(0) - 2.0).abs() < 0.1, "input peak {}", peak(0));
        assert!(peak(1) > 0.8 && peak(1) < 1.8, "output peak {}", peak(1));
    }

    #[test]
    fn counter_measures_the_signal_frequency() {
        let lib = Library::builtin();
        let proj = build("Frequency counter and logic probe", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        for _ in 0..20 {
            sim.step(0.05).unwrap();
        }
        let f = sim.tap("FC1").unwrap().frequency();
        assert!((f - 100.0).abs() < 3.0, "counter read {f} Hz");
        assert!(sim.tap("LP1").is_some());
    }

    fn run(name: &str, secs: f64) -> Simulator {
        let lib = Library::builtin();
        let proj = build(name, &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        for _ in 0..(secs / 0.05) as usize {
            sim.step(0.05).unwrap();
        }
        sim
    }

    #[test]
    fn smoothing_capacitor_holds_the_load_up() {
        let sim = run("Rectifier with smoothing capacitor", 1.0);
        let rl = sim.element("RL").unwrap();
        let v = sim.element_voltage(rl);
        assert!(v > 8.0 && v < 10.0, "load {v} V");
    }

    #[test]
    fn inverting_amplifier_gives_minus_two_volts() {
        let sim = run("Inverting op-amp amplifier", 0.0);
        let rl = sim.element("RL").unwrap();
        let v = sim.element_voltage(rl);
        assert!((v + 2.0).abs() < 0.1, "output {v} V");
    }

    #[test]
    fn rlc_resonance_magnifies_the_capacitor_voltage() {
        let sim = run("Series RLC resonance", 0.5);
        let peak = sim.traces[1].pts.iter().rev().take(400).map(|s| s.1.abs()).fold(0.0f32, f32::max);
        assert!(peak > 3.0, "capacitor peak {peak} V");
    }

    #[test]
    fn common_emitter_collector_is_biased_and_swings() {
        let sim = run("Common-emitter amplifier", 0.3);
        let pts: Vec<f32> = sim.traces[0].pts.iter().rev().take(400).map(|s| s.1).collect();
        let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
        let mid = (lo + hi) * 0.5;
        assert!(mid > 6.5 && mid < 11.0, "collector rests at {mid} V");
        assert!(hi - lo > 0.6, "collector swing {} V", hi - lo);
    }

    #[test]
    fn flip_flop_divides_the_clock_by_two() {
        let lib = Library::builtin();
        let proj = build("D flip-flop divide-by-two", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let mut at = |t: f64| {
            while sim.t < t {
                sim.step(0.05).unwrap();
            }
            sim.led_level("D1") > 0.05
        };
        assert!(at(0.3), "first rising edge sets Q");
        assert!(!at(1.3), "second edge clears it");
        assert!(at(2.3), "third edge sets it again");
    }

    #[test]
    fn follower_output_equals_input() {
        let lib = Library::builtin();
        let proj = build("Op-amp voltage follower", &lib).unwrap();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().unwrap();
        let rl = sim.element("RL").unwrap();
        let v = sim.element_voltage(rl).abs();
        assert!((v - 2.0).abs() < 0.05, "output {v} V, expected 2 V");
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
#[cfg(test)]
mod bench {
    use super::*;
    use crate::sim::Simulator;
    #[test]
    #[ignore]
    fn step_times() {
        let lib = Library::builtin();
        for name in names() {
            let proj = build(name, &lib).unwrap();
            let mut sim = Simulator::from_project(&proj, &lib).unwrap();
            sim.solve_dc().unwrap();
            let t = std::time::Instant::now();
            for _ in 0..20 { sim.step(0.05).unwrap(); }
            let step = t.elapsed().as_secs_f64() / 20.0 * 1000.0;
            let t = std::time::Instant::now();
            for _ in 0..20 { let _c = sim.clone(); }
            let clone = t.elapsed().as_secs_f64() / 20.0 * 1000.0;
            eprintln!("BENCH {name:45} step {step:7.2} ms  clone {clone:6.3} ms");
        }
    }
}
