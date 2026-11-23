// Shared data model: parts library, schematic objects, PCB objects, project.
// LEO WAS HERE ;3

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub const TH_PER_IN: f32 = 1000.0;
pub const MM_PER_IN: f32 = 25.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P {
    pub x: f32,
    pub y: f32,
}

impl std::ops::Add for P {
    type Output = P;
    fn add(self, o: P) -> P {
        P::new(self.x + o.x, self.y + o.y)
    }
}

impl P {
    pub const ZERO: P = P { x: 0.0, y: 0.0 };
    pub fn new(x: f32, y: f32) -> P {
        P { x, y }
    }
    pub fn add(self, o: P) -> P {
        P {
            x: self.x + o.x,
            y: self.y + o.y,
        }
    }
    pub fn sub(self, o: P) -> P {
        P {
            x: self.x - o.x,
            y: self.y - o.y,
        }
    }
    pub fn mul(self, s: f32) -> P {
        P {
            x: self.x * s,
            y: self.y * s,
        }
    }
    pub fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    pub fn dist(self, o: P) -> f32 {
        self.sub(o).len()
    }
    /// Rotate by a multiple of 90 degrees, clockwise (screen Y grows down).
    pub fn rot(self, deg: i32) -> P {
        match deg.rem_euclid(360) {
            90 => P {
                x: -self.y,
                y: self.x,
            },
            180 => P {
                x: -self.x,
                y: -self.y,
            },
            270 => P {
                x: self.y,
                y: -self.x,
            },
            _ => self,
        }
    }
    pub fn snap(self, step: f32) -> P {
        if step <= 0.0 {
            return self;
        }
        P {
            x: (self.x / step).round() * step,
            y: (self.y / step).round() * step,
        }
    }
}

// ---------------------------------------------------------------- symbols

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    Resistor,
    Capacitor,
    PolarCap,
    Inductor,
    Diode,
    Led,
    Npn,
    Pnp,
    OpAmp,
    Ground,
    Rail,
    Switch,
    Button,
    Battery,
    Lamp,
    Probe,
    Terminal,
    Ic,
    Xtal,
    Speaker,
    Motor,
    Fuse,
    Pot,
    Frame,
    Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimKind {
    None,
    R,
    C,
    L,
    V,
    I,
    D,
    Sw,
    Npn,
    Pnp,
    OpAmp,
    Lamp,
}

#[derive(Clone, Debug)]
pub struct PinDef {
    pub name: String,
    pub pos: P,
    pub dir: i32,
}

#[derive(Clone, Debug)]
pub struct PartDef {
    pub id: String,
    pub name: String,
    pub value: String,
    pub category: String,
    pub library: String,
    pub package: String,
    pub symbol: Symbol,
    pub pins: Vec<PinDef>,
    pub sim: SimKind,
    pub prefix: String,
    pub datasheet: String,
}

impl PartDef {
    pub fn pin(&self, i: usize) -> Option<&PinDef> {
        self.pins.get(i)
    }
}

fn two_pin(
    id: &str,
    name: &str,
    cat: &str,
    lib: &str,
    pkg: &str,
    sym: Symbol,
    sim: SimKind,
    val: &str,
    prefix: &str,
) -> PartDef {
    PartDef {
        id: id.into(),
        name: name.into(),
        value: val.into(),
        category: cat.into(),
        library: lib.into(),
        package: pkg.into(),
        symbol: sym,
        pins: vec![
            PinDef {
                name: "1".into(),
                pos: P::new(-60.0, 0.0),
                dir: 180,
            },
            PinDef {
                name: "2".into(),
                pos: P::new(60.0, 0.0),
                dir: 0,
            },
        ],
        sim,
        prefix: prefix.into(),
        datasheet: String::new(),
    }
}

fn ic_pins(count: usize) -> Vec<PinDef> {
    let mut pins = Vec::new();
    let left = count / 2;
    for i in 0..left {
        pins.push(PinDef {
            name: format!("{}", i + 1),
            pos: P::new(
                -120.0,
                -((left as f32 - 1.0) * 50.0) * 0.5 + i as f32 * 50.0,
            ),
            dir: 180,
        });
    }
    for i in left..count {
        let r = i - left;
        let rows = count - left;
        pins.push(PinDef {
            name: format!("{}", i + 1),
            pos: P::new(120.0, -((rows as f32 - 1.0) * 50.0) * 0.5 + r as f32 * 50.0),
            dir: 0,
        });
    }
    pins
}

/// Build a numbered DIP/development-board symbol from its real pin functions.
/// Pin indices stay in physical-number order so saved wire endpoints are stable.
fn named_dual_row_pins(names: &[&str], width: f32) -> Vec<PinDef> {
    let left = names.len().div_ceil(2);
    let right = names.len() - left;
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let (x, row, rows, dir) = if i < left {
                (-width, i, left, 180)
            } else {
                // DIP numbering continues from the lower-right corner upward.
                (width, names.len() - 1 - i, right, 0)
            };
            PinDef {
                name: (*name).into(),
                pos: P::new(x, -((rows as f32 - 1.0) * 50.0) * 0.5 + row as f32 * 50.0),
                dir,
            }
        })
        .collect()
}

/// Build a Raspberry-Pi style 2xN header: odd pins on the left, even pins on
/// the right. The vector remains ordered by physical pin number.
fn named_gpio_header_pins(names: &[&str], width: f32) -> Vec<PinDef> {
    let rows = names.len().div_ceil(2);
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let left = i % 2 == 0;
            let row = i / 2;
            PinDef {
                name: (*name).into(),
                pos: P::new(
                    if left { -width } else { width },
                    -((rows as f32 - 1.0) * 50.0) * 0.5 + row as f32 * 50.0,
                ),
                dir: if left { 180 } else { 0 },
            }
        })
        .collect()
}

fn named_part(
    id: &str,
    name: &str,
    category: &str,
    library: &str,
    package: &str,
    value: &str,
    prefix: &str,
    pins: Vec<PinDef>,
) -> PartDef {
    PartDef {
        id: id.into(),
        name: name.into(),
        value: value.into(),
        category: category.into(),
        library: library.into(),
        package: package.into(),
        symbol: Symbol::Ic,
        pins,
        sim: SimKind::None,
        prefix: prefix.into(),
        datasheet: String::new(),
    }
}

fn multi_pin(
    id: &str,
    name: &str,
    category: &str,
    library: &str,
    package: &str,
    value: &str,
    prefix: &str,
    pins: usize,
) -> PartDef {
    PartDef {
        id: id.into(),
        name: name.into(),
        value: value.into(),
        category: category.into(),
        library: library.into(),
        package: package.into(),
        symbol: Symbol::Ic,
        pins: ic_pins(pins),
        sim: SimKind::None,
        prefix: prefix.into(),
        datasheet: String::new(),
    }
}

pub struct Library {
    pub parts: Vec<PartDef>,
}

impl Library {
    pub fn builtin() -> Library {
        let mut p: Vec<PartDef> = Vec::new();

        p.push(two_pin(
            "RESISTOR",
            "RESISTOR",
            "Resistors",
            "DEVICE",
            "AXIAL-0.4",
            Symbol::Resistor,
            SimKind::R,
            "10k",
            "R",
        ));
        p.push(two_pin(
            "CHIPRES1K",
            "CHIP RESISTOR 1K",
            "Resistors",
            "DEVICE",
            "CHIP-0805",
            Symbol::Resistor,
            SimKind::R,
            "1k",
            "R",
        ));
        p.push(two_pin(
            "CHIPRES4K7",
            "CHIP RESISTOR 4K7",
            "Resistors",
            "DEVICE",
            "CHIP-0805",
            Symbol::Resistor,
            SimKind::R,
            "4k7",
            "R",
        ));
        p.push(two_pin(
            "CHIPRES10K",
            "CHIP RESISTOR 10K",
            "Resistors",
            "DEVICE",
            "CHIP-0805",
            Symbol::Resistor,
            SimKind::R,
            "10k",
            "R",
        ));
        p.push(two_pin(
            "CHIPRES100K",
            "CHIP RESISTOR 100K",
            "Resistors",
            "DEVICE",
            "CHIP-0805",
            Symbol::Resistor,
            SimKind::R,
            "100k",
            "R",
        ));
        p.push(two_pin(
            "POT-HG",
            "PRESET POTENTIOMETER",
            "Resistors",
            "DEVICE",
            "TRIM-3",
            Symbol::Pot,
            SimKind::R,
            "10k",
            "VR",
        ));

        p.push(two_pin(
            "CAP",
            "CAPACITOR",
            "Capacitors",
            "DEVICE",
            "CAP-100",
            Symbol::Capacitor,
            SimKind::C,
            "100n",
            "C",
        ));
        p.push(two_pin(
            "CAP-UF",
            "CAPACITOR (uF)",
            "Capacitors",
            "DEVICE",
            "RADIAL-2",
            Symbol::Capacitor,
            SimKind::C,
            "1u",
            "C",
        ));
        p.push(two_pin(
            "CPOL",
            "POLARISED CAPACITOR",
            "Capacitors",
            "DEVICE",
            "RADIAL-2",
            Symbol::PolarCap,
            SimKind::C,
            "10u",
            "C",
        ));

        p.push(two_pin(
            "INDUCTOR",
            "INDUCTOR",
            "Inductors",
            "DEVICE",
            "AXIAL-0.4",
            Symbol::Inductor,
            SimKind::L,
            "10m",
            "L",
        ));
        p.push(two_pin(
            "DIODE",
            "DIODE",
            "Diodes",
            "DEVICE",
            "DO-35",
            Symbol::Diode,
            SimKind::D,
            "1N4148",
            "D",
        ));
        p.push(two_pin(
            "ZENER",
            "ZENER DIODE",
            "Diodes",
            "DEVICE",
            "DO-35",
            Symbol::Diode,
            SimKind::D,
            "5V1",
            "D",
        ));
        p.push(two_pin(
            "LED-RED",
            "LED (RED)",
            "Optoelectronics",
            "DEVICE",
            "LED-5MM",
            Symbol::Led,
            SimKind::D,
            "RED",
            "LED",
        ));
        p.push(two_pin(
            "LED-GREEN",
            "LED (GREEN)",
            "Optoelectronics",
            "DEVICE",
            "LED-5MM",
            Symbol::Led,
            SimKind::D,
            "GREEN",
            "LED",
        ));
        p.push(two_pin(
            "LED-YELLOW",
            "LED (YELLOW)",
            "Optoelectronics",
            "DEVICE",
            "LED-5MM",
            Symbol::Led,
            SimKind::D,
            "YELLOW",
            "LED",
        ));

        {
            let mut npn = two_pin(
                "2N2222A",
                "NPN TRANSISTOR",
                "Transistors",
                "DEVICE",
                "TO-92",
                Symbol::Npn,
                SimKind::Npn,
                "2N2222A",
                "Q",
            );
            npn.pins = vec![
                PinDef {
                    name: "B".into(),
                    pos: P::new(-90.0, 0.0),
                    dir: 180,
                },
                PinDef {
                    name: "C".into(),
                    pos: P::new(50.0, -60.0),
                    dir: 90,
                },
                PinDef {
                    name: "E".into(),
                    pos: P::new(50.0, 60.0),
                    dir: 270,
                },
            ];
            p.push(npn);
        }
        {
            let mut q = two_pin(
                "BC547",
                "NPN TRANSISTOR",
                "Transistors",
                "DEVICE",
                "TO-92",
                Symbol::Npn,
                SimKind::Npn,
                "BC547",
                "Q",
            );
            q.pins = vec![
                PinDef {
                    name: "B".into(),
                    pos: P::new(-90.0, 0.0),
                    dir: 180,
                },
                PinDef {
                    name: "C".into(),
                    pos: P::new(50.0, -60.0),
                    dir: 90,
                },
                PinDef {
                    name: "E".into(),
                    pos: P::new(50.0, 60.0),
                    dir: 270,
                },
            ];
            p.push(q);
        }
        {
            let mut q = two_pin(
                "2N7002",
                "N-CHANNEL MOSFET",
                "Transistors",
                "DEVICE",
                "SOT23",
                Symbol::Npn,
                SimKind::Npn,
                "2N7002",
                "Q",
            );
            q.pins = vec![
                PinDef {
                    name: "G".into(),
                    pos: P::new(-90.0, 0.0),
                    dir: 180,
                },
                PinDef {
                    name: "D".into(),
                    pos: P::new(50.0, -60.0),
                    dir: 90,
                },
                PinDef {
                    name: "S".into(),
                    pos: P::new(50.0, 60.0),
                    dir: 270,
                },
            ];
            p.push(q);
        }
        {
            let mut q = two_pin(
                "2N3904",
                "NPN TRANSISTOR",
                "Transistors",
                "DEVICE",
                "TO-92",
                Symbol::Npn,
                SimKind::Npn,
                "2N3904",
                "Q",
            );
            q.pins = vec![
                PinDef {
                    name: "B".into(),
                    pos: P::new(-90.0, 0.0),
                    dir: 180,
                },
                PinDef {
                    name: "C".into(),
                    pos: P::new(50.0, -60.0),
                    dir: 90,
                },
                PinDef {
                    name: "E".into(),
                    pos: P::new(50.0, 60.0),
                    dir: 270,
                },
            ];
            p.push(q);
        }
        {
            let mut q = two_pin(
                "2N3906",
                "PNP TRANSISTOR",
                "Transistors",
                "DEVICE",
                "TO-92",
                Symbol::Pnp,
                SimKind::Pnp,
                "2N3906",
                "Q",
            );
            q.pins = vec![
                PinDef {
                    name: "B".into(),
                    pos: P::new(-90.0, 0.0),
                    dir: 180,
                },
                PinDef {
                    name: "C".into(),
                    pos: P::new(50.0, -60.0),
                    dir: 90,
                },
                PinDef {
                    name: "E".into(),
                    pos: P::new(50.0, 60.0),
                    dir: 270,
                },
            ];
            p.push(q);
        }

        {
            let mut op = two_pin(
                "OPAMP",
                "OPERATIONAL AMPLIFIER",
                "Amplifiers",
                "DEVICE",
                "SOIC-8",
                Symbol::OpAmp,
                SimKind::OpAmp,
                "理想",
                "U",
            );
            op.pins = vec![
                PinDef {
                    name: "+".into(),
                    pos: P::new(-90.0, -40.0),
                    dir: 180,
                },
                PinDef {
                    name: "-".into(),
                    pos: P::new(-90.0, 40.0),
                    dir: 180,
                },
                PinDef {
                    name: "OUT".into(),
                    pos: P::new(110.0, 0.0),
                    dir: 0,
                },
                PinDef {
                    name: "V+".into(),
                    pos: P::new(0.0, -110.0),
                    dir: 90,
                },
                PinDef {
                    name: "V-".into(),
                    pos: P::new(0.0, 110.0),
                    dir: 270,
                },
            ];
            p.push(op);
        }
        {
            let mut lm = two_pin(
                "LM358",
                "DUAL OPERATIONAL AMPLIFIER",
                "Amplifiers",
                "OPAMP",
                "DIL-8",
                Symbol::Ic,
                SimKind::OpAmp,
                "LM358",
                "U",
            );
            lm.pins = named_dual_row_pins(
                &[
                    "OUT1", "IN1-", "IN1+", "GND/V-", "IN2+", "IN2-", "OUT2", "V+",
                ],
                120.0,
            );
            p.push(lm);
        }
        {
            let mut ic = two_pin(
                "555",
                "TIMER",
                "TTL",
                "TTL",
                "DIL-8",
                Symbol::Ic,
                SimKind::None,
                "NE555",
                "U",
            );
            ic.pins = named_dual_row_pins(
                &[
                    "GND", "TRIG", "OUT", "RESET", "CTRL", "THRESH", "DISCH", "VCC",
                ],
                120.0,
            );
            p.push(ic);
        }
        {
            let mut ic = two_pin(
                "74HC00",
                "QUAD 2-INPUT NAND",
                "TTL",
                "TTL",
                "DIL-14",
                Symbol::Ic,
                SimKind::None,
                "74HC00",
                "U",
            );
            ic.pins = named_dual_row_pins(
                &[
                    "1A", "1B", "1Y", "2A", "2B", "2Y", "GND", "3Y", "3A", "3B", "4Y", "4A", "4B",
                    "VCC",
                ],
                120.0,
            );
            p.push(ic);
        }
        {
            let mut ic = two_pin(
                "74HC595",
                "8-BIT SHIFT REGISTER",
                "TTL",
                "TTL",
                "DIL-16",
                Symbol::Ic,
                SimKind::None,
                "74HC595",
                "U",
            );
            ic.pins = named_dual_row_pins(
                &[
                    "QB", "QC", "QD", "QE", "QF", "QG", "QH", "GND", "QH'", "MR", "SH_CP", "ST_CP",
                    "OE", "DS", "QA", "VCC",
                ],
                120.0,
            );
            p.push(ic);
        }
        {
            let mut mcu = two_pin(
                "PIC16F84A",
                "MICROCONTROLLER",
                "Microprocessors",
                "PIC",
                "DIL-18",
                Symbol::Ic,
                SimKind::None,
                "PIC16F84A",
                "U",
            );
            mcu.pins = named_dual_row_pins(
                &[
                    "RA2",
                    "RA3",
                    "RA4/T0CKI",
                    "MCLR/VPP",
                    "VSS",
                    "RB0/INT",
                    "RB1",
                    "RB2",
                    "RB3",
                    "RB4",
                    "RB5",
                    "RB6/PGC",
                    "RB7/PGD",
                    "VDD",
                    "OSC2/CLKOUT",
                    "OSC1/CLKIN",
                    "RA0",
                    "RA1",
                ],
                150.0,
            );
            p.push(mcu);
        }

        // Development boards use real, searchable pin functions instead of
        // anonymous numbered placeholders. The listed order is the physical
        // pin number shown by the library manager and schematic symbol.
        p.push(named_part(
            "ARDUINO-UNO-R3",
            "ARDUINO UNO R3",
            "Development Boards",
            "MICROCONTROLLERS",
            "ARDUINO-UNO",
            "ATmega328P board",
            "A",
            named_dual_row_pins(
                &[
                    "IOREF",
                    "RESET",
                    "3V3",
                    "5V",
                    "GND",
                    "GND",
                    "VIN",
                    "A0/ADC0",
                    "A1/ADC1",
                    "A2/ADC2",
                    "A3/ADC3",
                    "A4/SDA",
                    "A5/SCL",
                    "D0/RX",
                    "D1/TX",
                    "D2/INT0",
                    "D3/PWM",
                    "D4",
                    "D5/PWM",
                    "D6/PWM",
                    "D7",
                    "D8",
                    "D9/PWM",
                    "D10/SS/PWM",
                    "D11/MOSI/PWM",
                    "D12/MISO",
                    "D13/SCK/LED",
                    "AREF",
                    "SDA",
                    "SCL",
                ],
                185.0,
            ),
        ));
        p.push(named_part(
            "ESP32-DEVKIT-V1",
            "ESP32 DEVKIT V1",
            "Development Boards",
            "MICROCONTROLLERS",
            "ESP32-DEVKIT-38",
            "ESP32-WROOM-32",
            "U",
            named_dual_row_pins(
                &[
                    "3V3",
                    "EN",
                    "GPIO36/VP",
                    "GPIO39/VN",
                    "GPIO34",
                    "GPIO35",
                    "GPIO32",
                    "GPIO33",
                    "GPIO25/DAC1",
                    "GPIO26/DAC2",
                    "GPIO27",
                    "GPIO14",
                    "GPIO12",
                    "GND",
                    "GPIO13",
                    "GPIO9/SD2",
                    "GPIO10/SD3",
                    "GPIO11/CMD",
                    "5V/VIN",
                    "GND",
                    "GPIO23/MOSI",
                    "GPIO22/SCL",
                    "GPIO1/TX0",
                    "GPIO3/RX0",
                    "GPIO21/SDA",
                    "GND",
                    "GPIO19/MISO",
                    "GPIO18/SCK",
                    "GPIO5/SS",
                    "GPIO17/TX2",
                    "GPIO16/RX2",
                    "GPIO4",
                    "GPIO0/BOOT",
                    "GPIO2",
                    "GPIO15",
                    "GPIO8/SD1",
                    "GPIO7/SD0",
                    "GPIO6/CLK",
                ],
                200.0,
            ),
        ));
        p.push(named_part(
            "RPI-PICO",
            "RASPBERRY PI PICO",
            "Development Boards",
            "MICROCONTROLLERS",
            "PICO-40",
            "RP2040",
            "U",
            named_dual_row_pins(
                &[
                    "GP0/UART0-TX",
                    "GP1/UART0-RX",
                    "GND",
                    "GP2",
                    "GP3",
                    "GP4",
                    "GP5",
                    "GND",
                    "GP6",
                    "GP7",
                    "GP8",
                    "GP9",
                    "GND",
                    "GP10",
                    "GP11",
                    "GP12",
                    "GP13",
                    "GND",
                    "GP14",
                    "GP15",
                    "GP16",
                    "GP17",
                    "GND",
                    "GP18",
                    "GP19",
                    "GP20",
                    "GP21",
                    "GND",
                    "GP22",
                    "RUN",
                    "GP26/ADC0",
                    "GP27/ADC1",
                    "AGND",
                    "GP28/ADC2",
                    "ADC_VREF",
                    "3V3_OUT",
                    "3V3_EN",
                    "GND",
                    "VSYS",
                    "VBUS",
                ],
                190.0,
            ),
        ));
        p.push(named_part(
            "STM32-BLUEPILL",
            "STM32 BLUE PILL",
            "Development Boards",
            "MICROCONTROLLERS",
            "BLUEPILL-40",
            "STM32F103C8T6",
            "U",
            named_dual_row_pins(
                &[
                    "PB12",
                    "PB13/SPI2-SCK",
                    "PB14/SPI2-MISO",
                    "PB15/SPI2-MOSI",
                    "PA8",
                    "PA9/UART1-TX",
                    "PA10/UART1-RX",
                    "PA11/USB-DM",
                    "PA12/USB-DP",
                    "PA15/JTDI",
                    "PB3/JTDO",
                    "PB4/JTRST",
                    "PB5",
                    "PB6/I2C1-SCL",
                    "PB7/I2C1-SDA",
                    "PB8",
                    "PB9",
                    "5V",
                    "GND",
                    "3V3",
                    "VBAT",
                    "PC13/LED",
                    "PC14/OSC32-IN",
                    "PC15/OSC32-OUT",
                    "PA0/ADC0",
                    "PA1/ADC1",
                    "PA2/UART2-TX",
                    "PA3/UART2-RX",
                    "PA4/SPI1-SS",
                    "PA5/SPI1-SCK",
                    "PA6/SPI1-MISO",
                    "PA7/SPI1-MOSI",
                    "PB0/ADC8",
                    "PB1/ADC9",
                    "PB10/I2C2-SCL",
                    "PB11/I2C2-SDA",
                    "NRST",
                    "3V3",
                    "GND",
                    "GND",
                ],
                205.0,
            ),
        ));
        p.push(named_part(
            "RASPBERRY-PI-4B",
            "RASPBERRY PI 4 MODEL B (GPIO)",
            "Development Boards",
            "MICROCONTROLLERS",
            "RPI-40-GPIO",
            "40-pin GPIO header",
            "RPI",
            named_gpio_header_pins(
                &[
                    "3V3",
                    "5V",
                    "GPIO2/SDA1",
                    "5V",
                    "GPIO3/SCL1",
                    "GND",
                    "GPIO4",
                    "GPIO14/TXD0",
                    "GND",
                    "GPIO15/RXD0",
                    "GPIO17",
                    "GPIO18/PWM0",
                    "GPIO27",
                    "GND",
                    "GPIO22",
                    "GPIO23",
                    "3V3",
                    "GPIO24",
                    "GPIO10/MOSI",
                    "GND",
                    "GPIO9/MISO",
                    "GPIO25",
                    "GPIO11/SCLK",
                    "GPIO8/CE0",
                    "GND",
                    "GPIO7/CE1",
                    "GPIO0/ID_SD",
                    "GPIO1/ID_SC",
                    "GPIO5",
                    "GND",
                    "GPIO6",
                    "GPIO12/PWM0",
                    "GPIO13/PWM1",
                    "GND",
                    "GPIO19/PCM_FS",
                    "GPIO16",
                    "GPIO26",
                    "GPIO20/PCM_DIN",
                    "GND",
                    "GPIO21/PCM_DOUT",
                ],
                205.0,
            ),
        ));

        // Common sensors and support ICs make the built-in picker useful for
        // everyday hobby circuits.
        for part in [
            multi_pin(
                "DHT22",
                "DHT22 TEMPERATURE & HUMIDITY",
                "Sensors",
                "SENSORS",
                "DHT22-4",
                "DHT22",
                "U",
                4,
            ),
            multi_pin(
                "HC-SR04",
                "HC-SR04 ULTRASONIC SENSOR",
                "Sensors",
                "SENSORS",
                "HC-SR04",
                "ULTRASONIC",
                "U",
                4,
            ),
            multi_pin(
                "DS18B20",
                "DS18B20 TEMPERATURE SENSOR",
                "Sensors",
                "SENSORS",
                "TO-92",
                "DS18B20",
                "U",
                3,
            ),
            multi_pin(
                "MPU6050",
                "MPU6050 IMU MODULE",
                "Sensors",
                "SENSORS",
                "GY-521",
                "ACCEL + GYRO",
                "U",
                8,
            ),
            multi_pin(
                "LM7805",
                "LM7805 5V REGULATOR",
                "Power Management",
                "REGULATORS",
                "TO-220",
                "5V",
                "U",
                3,
            ),
            multi_pin(
                "AMS1117-3V3",
                "AMS1117 3.3V REGULATOR",
                "Power Management",
                "REGULATORS",
                "SOT-223",
                "3.3V",
                "U",
                3,
            ),
            multi_pin(
                "L293D",
                "L293D DUAL MOTOR DRIVER",
                "Drivers",
                "DRIVERS",
                "DIL-16",
                "L293D",
                "U",
                16,
            ),
            multi_pin(
                "ULN2003A",
                "ULN2003A DARLINGTON DRIVER",
                "Drivers",
                "DRIVERS",
                "DIL-16",
                "ULN2003A",
                "U",
                16,
            ),
            multi_pin(
                "LCD16X2",
                "LCD 16X2 DISPLAY",
                "Displays",
                "DISPLAYS",
                "LCD-16X2",
                "HD44780",
                "LCD",
                16,
            ),
            multi_pin(
                "SSD1306-I2C",
                "SSD1306 OLED DISPLAY",
                "Displays",
                "DISPLAYS",
                "OLED-I2C-4",
                "128X64 I2C",
                "OLED",
                4,
            ),
            multi_pin(
                "7SEG-CC",
                "7-SEGMENT DISPLAY",
                "Displays",
                "DISPLAYS",
                "7SEG-10",
                "COMMON CATHODE",
                "DS",
                10,
            ),
            multi_pin(
                "CONN-SIL3",
                "3-PIN HEADER",
                "Connectors",
                "CONNECTORS",
                "SIL-3",
                "1X3",
                "J",
                3,
            ),
            multi_pin(
                "CONN-SIL4",
                "4-PIN HEADER",
                "Connectors",
                "CONNECTORS",
                "SIL-4",
                "1X4",
                "J",
                4,
            ),
            multi_pin(
                "USB-C-POWER",
                "USB-C POWER CONNECTOR",
                "Connectors",
                "CONNECTORS",
                "USB-C-6",
                "POWER ONLY",
                "J",
                6,
            ),
        ] {
            p.push(part);
        }

        p.push(two_pin(
            "LDR",
            "LIGHT DEPENDENT RESISTOR",
            "Sensors",
            "SENSORS",
            "LDR-2",
            Symbol::Resistor,
            SimKind::R,
            "10k",
            "LDR",
        ));
        p.push(two_pin(
            "NTC-10K",
            "NTC THERMISTOR 10K",
            "Sensors",
            "SENSORS",
            "THERMISTOR-2",
            Symbol::Resistor,
            SimKind::R,
            "10k NTC",
            "TH",
        ));
        p.push(two_pin(
            "BUZZER",
            "PIEZO BUZZER",
            "Sounders",
            "DEVICE",
            "BUZZER-2",
            Symbol::Speaker,
            SimKind::None,
            "5V",
            "BZ",
        ));
        {
            let mut mcu = two_pin(
                "ATMEGA328P",
                "MICROCONTROLLER",
                "Microprocessors",
                "AVR",
                "DIL-28",
                Symbol::Ic,
                SimKind::None,
                "ATMEGA328P",
                "U",
            );
            mcu.pins = named_dual_row_pins(
                &[
                    "PC6/RESET",
                    "PD0/RXD",
                    "PD1/TXD",
                    "PD2/INT0",
                    "PD3/INT1/PWM",
                    "PD4/T0/XCK",
                    "VCC",
                    "GND",
                    "PB6/XTAL1",
                    "PB7/XTAL2",
                    "PD5/PWM/T1",
                    "PD6/PWM/AIN0",
                    "PD7/AIN1",
                    "PB0/ICP1",
                    "PB1/PWM",
                    "PB2/SS/PWM",
                    "PB3/MOSI/PWM",
                    "PB4/MISO",
                    "PB5/SCK",
                    "AVCC",
                    "AREF",
                    "GND",
                    "PC0/ADC0",
                    "PC1/ADC1",
                    "PC2/ADC2",
                    "PC3/ADC3",
                    "PC4/ADC4/SDA",
                    "PC5/ADC5/SCL",
                ],
                175.0,
            );
            p.push(mcu);
        }

        {
            let mut sw = two_pin(
                "SWITCH",
                "SPDT SWITCH",
                "Switches & Relays",
                "DEVICE",
                "SW-SPDT",
                Symbol::Switch,
                SimKind::Sw,
                "SPDT",
                "SW",
            );
            sw.pins = vec![
                PinDef {
                    name: "A".into(),
                    pos: P::new(-80.0, 0.0),
                    dir: 180,
                },
                PinDef {
                    name: "B".into(),
                    pos: P::new(80.0, 0.0),
                    dir: 0,
                },
            ];
            p.push(sw);
        }
        {
            let mut b = two_pin(
                "PUSH",
                "PUSH BUTTON",
                "Switches & Relays",
                "DEVICE",
                "BUTTON-2",
                Symbol::Button,
                SimKind::Sw,
                "N.O.",
                "SW",
            );
            b.pins = vec![
                PinDef {
                    name: "A".into(),
                    pos: P::new(-70.0, 0.0),
                    dir: 180,
                },
                PinDef {
                    name: "B".into(),
                    pos: P::new(70.0, 0.0),
                    dir: 0,
                },
            ];
            p.push(b);
        }

        p.push(two_pin(
            "LAMP",
            "LAMP",
            "Optoelectronics",
            "DEVICE",
            "LAMP-12V",
            Symbol::Lamp,
            SimKind::Lamp,
            "12V",
            "LAMP",
        ));
        p.push(two_pin(
            "BATTERY",
            "BATTERY CELL",
            "Simulation Primitives",
            "SOURCE",
            "BAT-2",
            Symbol::Battery,
            SimKind::V,
            "9V",
            "BT",
        ));
        p.push(two_pin(
            "SINE",
            "SINE VOLTAGE SOURCE",
            "Simulation Primitives",
            "SOURCE",
            "NONE",
            Symbol::Source,
            SimKind::V,
            "SINE(0 5 1k)",
            "VS",
        ));
        p.push(two_pin(
            "PULSE",
            "PULSE VOLTAGE SOURCE",
            "Simulation Primitives",
            "SOURCE",
            "NONE",
            Symbol::Source,
            SimKind::V,
            "PULSE(0 5 0 1m 1m 5m 10m)",
            "VP",
        ));
        p.push(two_pin(
            "DCSRC",
            "DC VOLTAGE SOURCE",
            "Simulation Primitives",
            "SOURCE",
            "NONE",
            Symbol::Source,
            SimKind::V,
            "5V",
            "VDC",
        ));
        p.push(two_pin(
            "ISRC",
            "CURRENT SOURCE",
            "Simulation Primitives",
            "SOURCE",
            "NONE",
            Symbol::Source,
            SimKind::I,
            "1m",
            "IS",
        ));
        p.push(two_pin(
            "XTAL",
            "CRYSTAL",
            "Miscellaneous",
            "DEVICE",
            "CRYSTAL-2",
            Symbol::Xtal,
            SimKind::None,
            "16MHz",
            "Y",
        ));
        p.push(two_pin(
            "FUSE",
            "FUSE",
            "Miscellaneous",
            "DEVICE",
            "FUSE-2",
            Symbol::Fuse,
            SimKind::R,
            "0R",
            "F",
        ));
        p.push(two_pin(
            "SPEAKER",
            "SPEAKER",
            "Optoelectronics",
            "DEVICE",
            "SPEAKER-2",
            Symbol::Speaker,
            SimKind::None,
            "8R",
            "SP",
        ));
        p.push(two_pin(
            "MOTOR",
            "DC MOTOR",
            "Motors",
            "DEVICE",
            "MOTOR-2",
            Symbol::Motor,
            SimKind::L,
            "100m",
            "M",
        ));
        p.push(two_pin(
            "FRAME",
            "SHEET FRAME",
            "Miscellaneous",
            "FRAME",
            "A4-L",
            Symbol::Frame,
            SimKind::None,
            "A4",
            "FRAME",
        ));

        // Power and ground rails (single pin).
        for (id, name, sym, val) in [
            ("GND", "GROUND", Symbol::Ground, "0V"),
            ("VCC", "POWER RAIL", Symbol::Rail, "5V"),
            ("VDD", "POWER RAIL", Symbol::Rail, "5V"),
            ("VEE", "POWER RAIL", Symbol::Rail, "-5V"),
            ("VSS", "POWER RAIL", Symbol::Rail, "0V"),
        ] {
            p.push(PartDef {
                id: id.into(),
                name: name.into(),
                value: val.into(),
                category: "Power".into(),
                library: "POWER".into(),
                package: "NONE".into(),
                symbol: sym,
                pins: vec![PinDef {
                    name: "1".into(),
                    pos: P::ZERO,
                    dir: 270,
                }],
                sim: SimKind::None,
                prefix: "#PWR".into(),
                datasheet: String::new(),
            });
        }
        p.push(PartDef {
            id: "TERMINAL".into(),
            name: "TERMINAL".into(),
            value: "".into(),
            category: "Connectors".into(),
            library: "CONNECTORS".into(),
            package: "NONE".into(),
            symbol: Symbol::Terminal,
            pins: vec![PinDef {
                name: "1".into(),
                pos: P::ZERO,
                dir: 180,
            }],
            sim: SimKind::None,
            prefix: "TP".into(),
            datasheet: String::new(),
        });
        p.push(PartDef {
            id: "PROBE".into(),
            name: "VOLTAGE PROBE".into(),
            value: "".into(),
            category: "Simulation Primitives".into(),
            library: "SOURCE".into(),
            package: "NONE".into(),
            symbol: Symbol::Probe,
            pins: vec![PinDef {
                name: "1".into(),
                pos: P::ZERO,
                dir: 270,
            }],
            sim: SimKind::None,
            prefix: "PRB".into(),
            datasheet: String::new(),
        });

        Library { parts: p }
    }

    pub fn get(&self, id: &str) -> Option<&PartDef> {
        self.parts.iter().find(|p| p.id == id)
    }

    pub fn categories(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for p in &self.parts {
            if seen.insert(p.category.clone()) {
                out.push(p.category.clone());
            }
        }
        out.sort();
        out
    }

    pub fn search(&self, keywords: &str, category: &str) -> Vec<&PartDef> {
        let kw = keywords.trim().to_lowercase();
        self.parts
            .iter()
            .filter(|p| {
                let cat_ok =
                    category.is_empty() || category == "All Categories" || p.category == category;
                if !cat_ok {
                    return false;
                }
                if kw.is_empty() {
                    return true;
                }
                p.id.to_lowercase().contains(&kw)
                    || p.name.to_lowercase().contains(&kw)
                    || p.value.to_lowercase().contains(&kw)
                    || p.category.to_lowercase().contains(&kw)
                    || p.package.to_lowercase().contains(&kw)
                    || p.library.to_lowercase().contains(&kw)
                    || p.pins
                        .iter()
                        .any(|pin| pin.name.to_lowercase().contains(&kw))
            })
            .collect()
    }
}

// ---------------------------------------------------------------- schematic

#[derive(Clone, Debug)]
pub struct Component {
    pub id: u64,
    pub part: String,
    pub refdes: String,
    pub value: String,
    pub x: f32,
    pub y: f32,
    pub rot: i32,
    pub props: Vec<(String, String)>,
    /// Property names hidden by the Property Assignment Tool. Names are
    /// compared case-insensitively.
    pub hidden_props: Vec<String>,
    /// TEXT height and width, in schematic units.
    pub text_size: Option<(i32, i32)>,
}

impl Component {
    pub fn pos(&self) -> P {
        P::new(self.x, self.y)
    }
}

#[derive(Clone, Debug)]
pub struct Wire {
    pub id: u64,
    pub pts: Vec<P>,
    /// Component pin at the start of `pts`, when the wire is attached.
    pub start_pin: Option<(u64, usize)>,
    /// Component pin at the end of `pts`, when the wire is attached.
    pub end_pin: Option<(u64, usize)>,
}

impl Wire {
    pub fn uses_component(&self, comp: u64) -> bool {
        self.start_pin.map(|(id, _)| id == comp).unwrap_or(false)
            || self.end_pin.map(|(id, _)| id == comp).unwrap_or(false)
    }

    pub fn uses_any(&self, ids: &HashSet<u64>) -> bool {
        ids.iter().any(|id| self.uses_component(*id))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Net,
    Power,
    Text,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub id: u64,
    pub kind: LabelKind,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub rot: i32,
    pub probe: bool,
}

#[derive(Clone, Debug)]
pub struct Pin {
    pub comp: u64,
    pub index: usize,
    pub pos: P,
    pub name: String,
}

// ---------------------------------------------------------------- pcb

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadShape {
    Circle,
    Square,
    Octagon,
}

#[derive(Clone, Debug)]
pub struct Pad {
    pub number: String,
    pub pos: P,
    pub size: f32,
    pub drill: f32,
    pub shape: PadShape,
    pub net: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Footprint {
    pub id: u64,
    pub comp: u64,
    pub refdes: String,
    pub package: String,
    pub x: f32,
    pub y: f32,
    pub rot: i32,
    pub pads: Vec<Pad>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    TopCopper,
    BottomCopper,
    TopSilk,
    BottomSilk,
    TopMask,
    BottomMask,
    Board,
    Drill,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Layer::TopCopper => "Top Copper",
            Layer::BottomCopper => "Bottom Copper",
            Layer::TopSilk => "Top Silk",
            Layer::BottomSilk => "Bottom Silk",
            Layer::TopMask => "Top Solder Mask",
            Layer::BottomMask => "Bottom Solder Mask",
            Layer::Board => "Board Edge",
            Layer::Drill => "Drill Data",
        }
    }
    pub fn all() -> [Layer; 8] {
        [
            Layer::TopCopper,
            Layer::BottomCopper,
            Layer::TopSilk,
            Layer::BottomSilk,
            Layer::TopMask,
            Layer::BottomMask,
            Layer::Board,
            Layer::Drill,
        ]
    }
    pub fn copper(self) -> bool {
        matches!(self, Layer::TopCopper | Layer::BottomCopper)
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: u64,
    pub a: P,
    pub b: P,
    pub width: f32,
    pub layer: Layer,
    pub net: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Via {
    pub id: u64,
    pub pos: P,
    pub size: f32,
    pub drill: f32,
    pub net: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Board {
    pub origin: P,
    pub width: f32,
    pub height: f32,
}

impl Board {
    pub fn rect(&self) -> (P, P) {
        (
            self.origin,
            self.origin.add(P::new(self.width, self.height)),
        )
    }
}

// ---------------------------------------------------------------- settings

#[derive(Clone, Debug)]
pub struct DesignRules {
    pub pad_pad: f32,
    pub pad_trace: f32,
    pub trace_trace: f32,
    pub graphic: f32,
    pub edge: f32,
}

impl Default for DesignRules {
    fn default() -> Self {
        DesignRules {
            pad_pad: 5.0,
            pad_trace: 6.0,
            trace_trace: 6.0,
            graphic: 1.0,
            edge: 15.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitSystem {
    Imperial,
    Metric,
}

#[derive(Clone, Debug)]
pub struct GridSettings {
    pub units: UnitSystem,
    pub snaps: [f32; 4],
    pub active: usize,
    pub show_dots: bool,
    pub snap_on: bool,
}

impl GridSettings {
    pub fn imperial() -> GridSettings {
        GridSettings {
            units: UnitSystem::Imperial,
            snaps: [1.0, 5.0, 25.0, 50.0],
            active: 2,
            show_dots: true,
            snap_on: true,
        }
    }
    pub fn metric() -> GridSettings {
        GridSettings {
            units: UnitSystem::Metric,
            snaps: [0.1, 0.5, 1.0, 2.5],
            active: 2,
            show_dots: true,
            snap_on: true,
        }
    }
    pub fn step(&self) -> f32 {
        self.snaps[self.active.min(3)]
    }
    /// Screen spacing of the fine grid in points (never below 8 px).
    pub fn dot_spacing(&self, px_per_th: f32) -> f32 {
        let mut s = self.step() * px_per_th;
        while s < 8.0 && s > 0.0 {
            s *= 2.0;
        }
        s.max(8.0)
    }
}

// ---------------------------------------------------------------- netlist

#[derive(Clone, Debug, Default)]
pub struct Netlist {
    /// Net names, index 0 is always the ground net.
    pub names: Vec<String>,
    /// (component id, pin index) -> net name
    pub pin_net: HashMap<(u64, usize), String>,
    /// net name -> node index
    pub index: HashMap<String, usize>,
}

impl Netlist {
    pub fn node(&self, net: &str) -> Option<usize> {
        self.index.get(&net.to_uppercase()).copied()
    }
    pub fn net_of(&self, comp: u64, pin: usize) -> Option<&str> {
        self.pin_net.get(&(comp, pin)).map(|s| s.as_str())
    }
    pub fn is_ground(net: &str) -> bool {
        matches!(
            net.to_uppercase().as_str(),
            "GND" | "VSS" | "0" | "0V" | "GROUND"
        )
    }
}

// ---------------------------------------------------------------- project

#[derive(Clone, Debug)]
pub struct DrcViolation {
    pub kind: String,
    pub pos: P,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub path: Option<PathBuf>,
    pub comps: Vec<Component>,
    pub wires: Vec<Wire>,
    pub labels: Vec<Label>,
    pub fps: Vec<Footprint>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub board: Board,
    pub notes: String,
    pub source: String,
    pub rules: DesignRules,
    pub grid: GridSettings,
    pub next_id: u64,
    pub dirty: bool,
}

impl Project {
    pub fn new(name: &str) -> Project {
        Project {
            name: name.into(),
            path: None,
            comps: Vec::new(),
            wires: Vec::new(),
            labels: Vec::new(),
            fps: Vec::new(),
            tracks: Vec::new(),
            vias: Vec::new(),
            board: Board {
                origin: P::new(0.0, 0.0),
                width: 2000.0,
                height: 1500.0,
            },
            notes: String::new(),
            source: default_source(),
            rules: DesignRules::default(),
            grid: GridSettings::imperial(),
            next_id: 1,
            dirty: false,
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn part<'a>(&self, lib: &'a Library, comp: &Component) -> Option<&'a PartDef> {
        lib.get(&comp.part)
    }

    pub fn next_refdes(&self, prefix: &str) -> String {
        let mut max = 0i32;
        for c in &self.comps {
            if let Some(rest) = c.refdes.strip_prefix(prefix) {
                if let Ok(n) = rest.parse::<i32>() {
                    max = max.max(n);
                }
            }
        }
        format!("{}{}", prefix, max + 1)
    }

    pub fn add_component(&mut self, lib: &Library, part_id: &str, at: P) -> Option<u64> {
        let def = lib.get(part_id)?;
        let id = self.alloc_id();
        let refdes = self.next_refdes(&def.prefix);
        let mut props = vec![
            ("VALUE".into(), def.value.clone()),
            ("PACKAGE".into(), def.package.clone()),
            ("LIBRARY".into(), def.library.clone()),
            ("DEVICE".into(), def.id.clone()),
        ];
        if !def.datasheet.is_empty() {
            props.push(("DATASHEET".into(), def.datasheet.clone()));
        }
        self.comps.push(Component {
            id,
            part: def.id.clone(),
            refdes,
            value: def.value.clone(),
            x: at.x,
            y: at.y,
            rot: 0,
            props,
            hidden_props: Vec::new(),
            text_size: None,
        });
        self.dirty = true;
        Some(id)
    }

    pub fn comp(&self, id: u64) -> Option<&Component> {
        self.comps.iter().find(|c| c.id == id)
    }
    pub fn comp_mut(&mut self, id: u64) -> Option<&mut Component> {
        self.comps.iter_mut().find(|c| c.id == id)
    }

    pub fn pins_of(&self, lib: &Library, c: &Component) -> Vec<P> {
        match lib.get(&c.part) {
            Some(def) => def
                .pins
                .iter()
                .map(|p| c.pos().add(p.pos.rot(c.rot)))
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn pin_pos(&self, lib: &Library, comp: u64, index: usize) -> Option<P> {
        let c = self.comp(comp)?;
        let def = lib.get(&c.part)?;
        let pd = def.pins.get(index)?;
        Some(c.pos().add(pd.pos.rot(c.rot)))
    }

    pub fn pin_is_horizontal(&self, lib: &Library, comp: u64, index: usize) -> bool {
        let Some(c) = self.comp(comp) else {
            return true;
        };
        let Some(def) = lib.get(&c.part) else {
            return true;
        };
        let Some(pd) = def.pins.get(index) else {
            return true;
        };
        let local = pd.pos.rot(c.rot);
        local.x.abs() >= local.y.abs()
    }

    /// Union-find net extraction over wire vertices, pins and labels.
    pub fn netlist(&self, lib: &Library) -> Netlist {
        let q = |p: P| ((p.x * 4.0).round() as i64, (p.y * 4.0).round() as i64);

        let mut parent: HashMap<(i64, i64), (i64, i64)> = HashMap::new();
        let find = |parent: &mut HashMap<(i64, i64), (i64, i64)>, k: (i64, i64)| -> (i64, i64) {
            let mut root = k;
            while let Some(&p) = parent.get(&root) {
                if p == root {
                    break;
                }
                root = p;
            }
            // path compression
            let mut cur = k;
            while let Some(&p) = parent.get(&cur) {
                if p == cur {
                    break;
                }
                parent.insert(cur, root);
                cur = p;
            }
            root
        };
        let union = |parent: &mut HashMap<(i64, i64), (i64, i64)>, a: (i64, i64), b: (i64, i64)| {
            parent.entry(a).or_insert(a);
            parent.entry(b).or_insert(b);
            let ra = find(parent, a);
            let rb = find(parent, b);
            if ra != rb {
                parent.insert(ra, rb);
            }
        };

        let ensure = |parent: &mut HashMap<(i64, i64), (i64, i64)>, k: (i64, i64)| {
            parent.entry(k).or_insert(k);
        };

        // Wires join their own vertices.
        for w in &self.wires {
            for w2 in w.pts.windows(2) {
                union(&mut parent, q(w2[0]), q(w2[1]));
            }
            for p in &w.pts {
                ensure(&mut parent, q(*p));
            }
        }

        // Pins: join coincident points, and pins sitting on a wire segment.
        let mut pin_pts: Vec<(u64, usize, P, String)> = Vec::new();
        for c in &self.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            for (i, pd) in def.pins.iter().enumerate() {
                let wp = c.pos().add(pd.pos.rot(c.rot));
                pin_pts.push((c.id, i, wp, pd.name.clone()));
                ensure(&mut parent, q(wp));
            }
        }
        for (_, _, p, _) in &pin_pts {
            for w in &self.wires {
                for seg in w.pts.windows(2) {
                    if point_on_segment(*p, seg[0], seg[1], 2.0) {
                        union(&mut parent, q(*p), q(seg[0]));
                    }
                }
            }
        }
        // Coincident pins and labels.
        let mut labels: Vec<(P, String, LabelKind)> = Vec::new();
        for l in &self.labels {
            labels.push((P::new(l.x, l.y), l.text.clone(), l.kind));
            ensure(&mut parent, q(P::new(l.x, l.y)));
        }
        for i in 0..pin_pts.len() {
            for j in (i + 1)..pin_pts.len() {
                if q(pin_pts[i].2) == q(pin_pts[j].2) {
                    union(&mut parent, q(pin_pts[i].2), q(pin_pts[j].2));
                }
            }
        }
        for i in 0..labels.len() {
            for j in (i + 1)..labels.len() {
                if q(labels[i].0) == q(labels[j].0) {
                    union(&mut parent, q(labels[i].0), q(labels[j].0));
                }
            }
        }
        for (_, _, p, _) in &pin_pts {
            for (lp, _, _) in &labels {
                if q(*p) == q(*lp) {
                    union(&mut parent, q(*p), q(*lp));
                }
            }
        }

        // Group roots.
        let mut groups: HashMap<(i64, i64), Vec<(u64, usize)>> = HashMap::new();
        for (cid, idx, p, _) in &pin_pts {
            let r = find(&mut parent, q(*p));
            groups.entry(r).or_default().push((*cid, *idx));
        }

        // Name the groups: explicit labels first, then power symbols.
        let mut root_name: HashMap<(i64, i64), String> = HashMap::new();
        for (lp, text, kind) in &labels {
            if *kind == LabelKind::Text {
                continue;
            }
            let t = text.trim().to_string();
            if t.is_empty() {
                continue;
            }
            let r = find(&mut parent, q(*lp));
            root_name.entry(r).or_insert(t);
        }
        for c in &self.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            if matches!(def.symbol, Symbol::Ground | Symbol::Rail) {
                let Some(pd) = def.pins.first() else { continue };
                let wp = c.pos().add(pd.pos.rot(c.rot));
                let r = find(&mut parent, q(wp));
                let name = if def.symbol == Symbol::Ground {
                    "GND".to_string()
                } else {
                    def.id.clone()
                };
                root_name.entry(r).or_insert(name);
            }
        }

        let mut nl = Netlist::default();
        nl.names.push("GND".into());
        nl.index.insert("GND".into(), 0);
        let mut anon = 1usize;

        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by_key(|(r, _)| *r);
        for (root, members) in groups {
            if members.is_empty() {
                continue;
            }
            let name = match root_name.get(&root) {
                Some(n) => n.clone(),
                None => {
                    let n = format!("N${}", anon);
                    anon += 1;
                    n
                }
            };
            let upper = name.to_uppercase();
            let node_name = if Netlist::is_ground(&upper) {
                "GND".to_string()
            } else {
                name.clone()
            };
            let key = node_name.to_uppercase();
            if !nl.index.contains_key(&key) {
                let idx = if key == "GND" {
                    0
                } else {
                    let i = nl.names.len();
                    nl.names.push(node_name.clone());
                    i
                };
                nl.index.insert(key, idx);
            }
            for (cid, idx_pin) in members {
                nl.pin_net.insert((cid, idx_pin), node_name.clone());
            }
        }
        // Defensive fallback: any pin that still has no net gets its own
        // registered node. A net without an index would collapse to ground in
        // the simulator.
        for (cid, idx, _, _) in &pin_pts {
            let k = (*cid, *idx);
            if nl.pin_net.contains_key(&k) {
                continue;
            }
            let name = loop {
                let candidate = format!("N${}", anon);
                anon += 1;
                if !nl.index.contains_key(&candidate.to_uppercase()) {
                    break candidate;
                }
            };
            let node = nl.names.len();
            nl.names.push(name.clone());
            nl.index.insert(name.to_uppercase(), node);
            nl.pin_net.insert(k, name);
        }
        nl
    }

    /// Place a footprint for any component that does not have one yet.
    pub fn sync_pcb(&mut self, lib: &Library, netlist: &Netlist) {
        // Continue the placement grid after footprints already on the board,
        // so new parts never land on top of earlier ones.
        let mut col = self.fps.len() % 4;
        let mut row = self.fps.len() / 4;
        for c in &self.comps {
            if self.fps.iter().any(|f| f.comp == c.id) {
                continue;
            }
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            if def.package == "NONE" || def.symbol == Symbol::Frame {
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            let pads = footprint_pads(&def.package, def.pins.len());
            let mut pads = pads;
            for (i, pad) in pads.iter_mut().enumerate() {
                pad.net = netlist.net_of(c.id, i).map(|s| s.to_string());
            }
            let x = 300.0 + col as f32 * 450.0;
            let y = 300.0 + row as f32 * 400.0;
            col += 1;
            if col >= 4 {
                col = 0;
                row += 1;
            }
            self.fps.push(Footprint {
                id,
                comp: c.id,
                refdes: c.refdes.clone(),
                package: def.package.clone(),
                x,
                y,
                rot: 0,
                pads,
            });
        }
        // refresh nets on existing footprints
        for f in &mut self.fps {
            let cid = f.comp;
            for (i, pad) in f.pads.iter_mut().enumerate() {
                let n = netlist.net_of(cid, i).map(|s| s.to_string());
                pad.net = n;
            }
        }
        self.dirty = true;
    }

    pub fn drc(&self) -> Vec<DrcViolation> {
        let mut out = Vec::new();
        let r = &self.rules;
        // track to track on the same layer
        for i in 0..self.tracks.len() {
            for j in (i + 1)..self.tracks.len() {
                let a = &self.tracks[i];
                let b = &self.tracks[j];
                if a.layer != b.layer {
                    continue;
                }
                if a.net.is_some() && a.net == b.net {
                    continue;
                }
                let d = seg_dist(a.a, a.b, b.a, b.b) - a.width * 0.5 - b.width * 0.5;
                if d < r.trace_trace {
                    out.push(DrcViolation {
                        kind: "Trace-Trace".into(),
                        pos: a.a,
                        detail: format!("{:.1} th gap, rule {:.0} th", d, r.trace_trace),
                    });
                }
            }
        }
        // track to pad
        for t in &self.tracks {
            for f in &self.fps {
                for pad in &f.pads {
                    if pad.net.is_some() && pad.net == t.net {
                        continue;
                    }
                    let d = point_seg_dist(pad.pos, t.a, t.b) - pad.size * 0.5 - t.width * 0.5;
                    if d < r.pad_trace {
                        out.push(DrcViolation {
                            kind: "Pad-Trace".into(),
                            pos: pad.pos,
                            detail: format!(
                                "{} pad, {:.1} th gap, rule {:.0} th",
                                f.refdes, d, r.pad_trace
                            ),
                        });
                    }
                }
            }
        }
        // board edge
        let (o, e) = self.board.rect();
        for t in &self.tracks {
            for p in [t.a, t.b] {
                let inside = p.x >= o.x && p.x <= e.x && p.y >= o.y && p.y <= e.y;
                if !inside {
                    out.push(DrcViolation {
                        kind: "Board Edge".into(),
                        pos: p,
                        detail: "Track leaves the board outline".into(),
                    });
                }
            }
        }
        out
    }
}

pub fn default_source() -> String {
    "// Voltwright demo project\n// Simulation firmware placeholder\n\n#include <stdint.h>\n\n#define LED_PIN 5\n\nvolatile uint8_t tick = 0;\n\nvoid setup(void) {\n    /* set LED pin as output */\n    DDRB |= (1 << LED_PIN);\n}\n\nvoid loop(void) {\n    tick++;\n    if (tick & 0x10) {\n        PORTB |= (1 << LED_PIN);   // LED on\n    } else {\n        PORTB &= ~(1 << LED_PIN);  // LED off\n    }\n}\n\nint main(void) {\n    setup();\n    for (;;) {\n        loop();\n    }\n    return 0;\n}\n".to_string()
}

// ---------------------------------------------------------------- helpers

pub fn footprint_pads(package: &str, pins: usize) -> Vec<Pad> {
    let pkg = package.to_uppercase();
    let mut pads = Vec::new();
    if pkg.starts_with("DIL") || pkg.starts_with("SOIC") || pkg.starts_with("SIL") {
        let pitch = if pkg.starts_with("DIL") { 100.0 } else { 50.0 };
        let rows = if pkg.starts_with("SIL") { 1 } else { 2 };
        let per_row = if rows == 1 { pins } else { (pins + 1) / 2 };
        let span = (per_row as f32 - 1.0) * pitch;
        for i in 0..pins {
            let (col, row) = if rows == 1 {
                (i, 0usize)
            } else if i < per_row {
                (i, 0usize)
            } else {
                (pins - 1 - i, 1usize)
            };
            let x = -span * 0.5 + col as f32 * pitch;
            let y = if rows == 1 {
                0.0
            } else if row == 0 {
                -150.0
            } else {
                150.0
            };
            pads.push(Pad {
                number: format!("{}", i + 1),
                pos: P::new(x, y),
                size: 50.0,
                drill: 30.0,
                shape: if i == 0 {
                    PadShape::Square
                } else {
                    PadShape::Circle
                },
                net: None,
            });
        }
    } else if pkg.starts_with("CHIP") || pkg.starts_with("SOT") {
        for i in 0..pins {
            let x = if pins == 2 {
                -60.0 + i as f32 * 120.0
            } else if i < 2 {
                -80.0
            } else {
                80.0
            };
            let y = if pins == 2 {
                0.0
            } else if i % 2 == 0 {
                -60.0
            } else {
                60.0
            };
            pads.push(Pad {
                number: format!("{}", i + 1),
                pos: P::new(x, y),
                size: 55.0,
                drill: 0.0,
                shape: PadShape::Square,
                net: None,
            });
        }
    } else {
        // Through-hole default: axial / radial / inline
        let spacing = if pins == 2 { 400.0 } else { 100.0 };
        for i in 0..pins {
            let x = if pins == 2 {
                -spacing * 0.5 + i as f32 * spacing
            } else {
                -(pins as f32 - 1.0) * spacing * 0.5 + i as f32 * spacing
            };
            pads.push(Pad {
                number: format!("{}", i + 1),
                pos: P::new(x, 0.0),
                size: 60.0,
                drill: 30.0,
                shape: if i == 0 {
                    PadShape::Square
                } else {
                    PadShape::Circle
                },
                net: None,
            });
        }
    }
    pads
}

pub fn parse_value(v: &str) -> Option<f64> {
    let raw = v.trim().to_lowercase().replace(' ', "");
    let s = raw
        .strip_suffix('v')
        .or_else(|| raw.strip_suffix('a'))
        .unwrap_or(raw.as_str());

    let (num, mult, tail) = if let Some(pos) = s.find("meg") {
        (&s[..pos], 1e6, &s[pos + 3..])
    } else if let Some(pos) =
        s.find(|c: char| matches!(c, 'm' | 'k' | 'u' | 'n' | 'p' | 'g' | 'f' | 'r'))
    {
        let mult = match s.as_bytes()[pos] as char {
            'm' => 1e-3,
            'k' => 1e3,
            'u' => 1e-6,
            'n' => 1e-9,
            'p' => 1e-12,
            'g' => 1e9,
            _ => 1.0,
        };
        (&s[..pos], mult, &s[pos + 1..])
    } else {
        (s, 1.0, "")
    };

    let base: f64 = num.parse().ok()?;
    let n = if tail.is_empty() {
        base
    } else {
        let frac: f64 = format!("0.{tail}").parse().ok()?;
        if base < 0.0 {
            base - frac
        } else {
            base + frac
        }
    };
    Some(n * mult)
}

/// Bounding box of a set of points, expanded by `pad`.
pub fn bbox(pts: &[P], pad: f32) -> Option<(P, P)> {
    let mut it = pts.iter();
    let first = *it.next()?;
    let mut min = first;
    let mut max = first;
    for p in it {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    Some((
        P::new(min.x - pad, min.y - pad),
        P::new(max.x + pad, max.y + pad),
    ))
}

pub fn point_on_segment(p: P, a: P, b: P, tol: f32) -> bool {
    let d = b.sub(a);
    let l2 = d.x * d.x + d.y * d.y;
    if l2 < 1e-9 {
        return p.dist(a) <= tol;
    }
    let t = (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0);
    let proj = P::new(a.x + t * d.x, a.y + t * d.y);
    p.dist(proj) <= tol
}

pub fn point_seg_dist(p: P, a: P, b: P) -> f32 {
    let d = b.sub(a);
    let l2 = d.x * d.x + d.y * d.y;
    if l2 < 1e-9 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0);
    p.dist(P::new(a.x + t * d.x, a.y + t * d.y))
}

pub fn seg_dist(a1: P, a2: P, b1: P, b2: P) -> f32 {
    if segments_cross(a1, a2, b1, b2) {
        return 0.0;
    }
    point_seg_dist(a1, b1, b2)
        .min(point_seg_dist(a2, b1, b2))
        .min(point_seg_dist(b1, a1, a2))
        .min(point_seg_dist(b2, a1, a2))
}

fn cross(o: P, a: P, b: P) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

pub fn segments_cross(a1: P, a2: P, b1: P, b2: P) -> bool {
    let d1 = cross(b1, b2, a1);
    let d2 = cross(b1, b2, a2);
    let d3 = cross(a1, a2, b1);
    let d4 = cross(a1, a2, b2);
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_value_handles_units_and_compact_multipliers() {
        assert_eq!(parse_value("9V"), Some(9.0));
        assert_eq!(parse_value("1kV"), Some(1000.0));
        assert_eq!(parse_value("100mA"), Some(0.1));
        assert_eq!(parse_value("4k7"), Some(4700.0));
        assert_eq!(parse_value("10R"), Some(10.0));
    }

    #[test]
    fn every_pin_has_a_registered_node() {
        let lib = Library::builtin();
        let mut proj = Project::new("test");
        let id = proj
            .add_component(&lib, "RESISTOR", P::new(0.0, 0.0))
            .unwrap();
        let nl = proj.netlist(&lib);

        for pin in 0..2 {
            let net = nl.net_of(id, pin).expect("pin should have a net");
            assert!(nl.node(net).is_some(), "net {net} should have a node");
        }
    }

    #[test]
    fn builtin_library_has_practical_categories_and_boards() {
        let lib = Library::builtin();
        assert!(lib.parts.len() >= 68);
        for category in [
            "Development Boards",
            "Sensors",
            "Displays",
            "Power Management",
            "Drivers",
        ] {
            assert!(lib.categories().iter().any(|item| item == category));
        }
        assert!(lib.get("ARDUINO-UNO-R3").is_some());
        assert!(lib.get("ESP32-DEVKIT-V1").is_some());
        assert!(lib.get("RASPBERRY-PI-4B").is_some());
    }

    #[test]
    fn microcontroller_pinouts_are_named_and_searchable() {
        let lib = Library::builtin();
        let pico = lib.get("RPI-PICO").unwrap();
        assert_eq!(pico.pins.len(), 40);
        assert_eq!(pico.pins[0].name, "GP0/UART0-TX");
        assert_eq!(pico.pins[39].name, "VBUS");

        let raspberry = lib.get("RASPBERRY-PI-4B").unwrap();
        assert_eq!(raspberry.pins[2].name, "GPIO2/SDA1");
        assert_eq!(raspberry.pins[39].name, "GPIO21/PCM_DOUT");
        assert!(raspberry.pins[0].pos.x < 0.0);
        assert!(raspberry.pins[1].pos.x > 0.0);

        let hits = lib.search("GPIO21", "All Categories");
        assert!(hits.iter().any(|part| part.id == "RASPBERRY-PI-4B"));
        assert!(hits.iter().any(|part| part.id == "ESP32-DEVKIT-V1"));
    }
}
