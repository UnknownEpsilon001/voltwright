// Small MNA simulator: DC operating point, transient and AC sweep.
// Devices: R, C, L, independent sources, diode/LED, switch, BJT, ideal op-amp.

use std::collections::HashMap;

use crate::model::{parse_value, Library, Netlist, Project, SimKind, Symbol};

pub const VT: f64 = 0.025852; // thermal voltage at 300 K
const G_LEAK: f64 = 1e-12; // stops floating nodes making the matrix singular
const IS_DIODE: f64 = 1e-14;
pub const LED_NOMINAL_CURRENT: f64 = 0.020;
pub const LED_MAX_CURRENT: f64 = 0.030;
pub const LED_MAX_REVERSE_VOLTAGE: f64 = 5.0;
const LED_FORWARD_VOLTAGE: f64 = 1.85;
const LED_DYNAMIC_RESISTANCE: f64 = 10.0;
pub const RESISTOR_RATED_POWER: f64 = 0.25;
const AMBIENT_TEMPERATURE_C: f64 = 25.0;

#[derive(Clone, Debug)]
pub enum Wave {
    Dc(f64),
    Sine {
        off: f64,
        amp: f64,
        freq: f64,
    },
    Pulse {
        lo: f64,
        hi: f64,
        td: f64,
        tr: f64,
        tf: f64,
        pw: f64,
        per: f64,
    },
}

impl Wave {
    pub fn eval(&self, t: f64) -> f64 {
        match self {
            Wave::Dc(v) => *v,
            Wave::Sine { off, amp, freq } => {
                off + amp * (2.0 * std::f64::consts::PI * freq * t).sin()
            }
            Wave::Pulse {
                lo,
                hi,
                td,
                tr,
                tf,
                pw,
                per,
            } => {
                if t < *td {
                    return *lo;
                }
                let ph = (t - *td) % *per;
                if ph < *tr {
                    *lo + (*hi - *lo) * (ph / (*tr).max(1e-12))
                } else if ph < *tr + *pw {
                    *hi
                } else if ph < *tr + *pw + *tf {
                    let k = (ph - *tr - *pw) / (*tf).max(1e-12);
                    *hi + (*lo - *hi) * k.clamp(0.0, 1.0)
                } else {
                    *lo
                }
            }
        }
    }
    pub fn offset(&self) -> f64 {
        match self {
            Wave::Dc(v) => *v,
            Wave::Sine { off, .. } => *off,
            Wave::Pulse { lo, .. } => *lo,
        }
    }
    pub fn frequency(&self) -> f64 {
        match self {
            Wave::Sine { freq, .. } => *freq,
            Wave::Pulse { per, .. } => 1.0 / per.max(1e-12),
            Wave::Dc(_) => 0.0,
        }
    }
    pub fn describe(&self) -> String {
        match self {
            Wave::Dc(v) => format!("{v} V DC"),
            Wave::Sine { off, amp, freq } => format!("sine: {off} V + {amp} V @ {freq} Hz"),
            Wave::Pulse {
                lo,
                hi,
                td,
                tr,
                tf,
                per,
                ..
            } => {
                format!("pulse: {lo} V -> {hi} V, delay {td}s, {tr}s/{tf}s, period {per}s")
            }
        }
    }
}

pub fn parse_wave(text: &str) -> (Wave, f64) {
    let t = text.trim();
    let up = t.to_uppercase();
    if up.starts_with("SINE") {
        let inner = &t[t.find('(').map(|i| i + 1).unwrap_or(0)..];
        let inner = inner.trim_end_matches(')');
        let nums: Vec<f64> = inner
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| parse_value(s).unwrap_or(0.0))
            .collect();
        let off = nums.first().copied().unwrap_or(0.0);
        let amp = nums.get(1).copied().unwrap_or(5.0);
        let freq = nums.get(2).copied().unwrap_or(1000.0);
        return (Wave::Sine { off, amp, freq }, amp.abs());
    }
    if up.starts_with("PULSE") {
        let inner = &t[t.find('(').map(|i| i + 1).unwrap_or(0)..];
        let inner = inner.trim_end_matches(')');
        let nums: Vec<f64> = inner
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| parse_value(s).unwrap_or(0.0))
            .collect();
        let lo = nums.first().copied().unwrap_or(0.0);
        let hi = nums.get(1).copied().unwrap_or(5.0);
        let td = nums.get(2).copied().unwrap_or(0.0);
        let tr = nums.get(3).copied().unwrap_or(1e-6).max(1e-12);
        let tf = nums.get(4).copied().unwrap_or(1e-6).max(1e-12);
        // PULSE(low high delay rise fall width period). With only six
        // numbers the sixth is the period and the width is half of it.
        let (pw, per) = if nums.len() >= 7 {
            (nums[5].max(0.0), nums[6].max(1e-12))
        } else {
            let per = nums.get(5).copied().unwrap_or(1e-3).max(1e-12);
            ((per * 0.5 - tr).max(0.0), per)
        };
        return (
            Wave::Pulse {
                lo,
                hi,
                td,
                tr,
                tf,
                pw,
                per,
            },
            (hi - lo).abs(),
        );
    }
    let v = parse_value(t).unwrap_or(0.0);
    (Wave::Dc(v), v)
}

#[derive(Clone, Debug)]
pub enum Kind {
    R,
    C,
    L,
    V { wave: Wave },
    I { wave: Wave },
    D,
    Sw { on: bool },
    Npn,
    Pnp,
    OpAmp,
    Lamp,
    /// 555 timer: pins GND, TRIG, OUT, RESET, CTRL, THRESH, DISCH, VCC.
    Timer,
    /// Digital gate on 5 V logic: inputs first, output last.
    Logic(Gate),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
    Not,
    Buf,
}

impl Gate {
    fn from_id(id: &str) -> Gate {
        match id {
            "NAND2" => Gate::Nand,
            "OR2" => Gate::Or,
            "NOR2" => Gate::Nor,
            "XOR2" => Gate::Xor,
            "XNOR2" => Gate::Xnor,
            "NOT" => Gate::Not,
            "BUF" => Gate::Buf,
            _ => Gate::And,
        }
    }
    fn eval(self, a: bool, b: bool) -> bool {
        match self {
            Gate::And => a && b,
            Gate::Nand => !(a && b),
            Gate::Or => a || b,
            Gate::Nor => !(a || b),
            Gate::Xor => a != b,
            Gate::Xnor => a == b,
            Gate::Not => !a,
            Gate::Buf => a,
        }
    }
}

#[derive(Clone, Debug)]
pub struct El {
    pub refdes: String,
    pub kind: Kind,
    /// node index per pin of the part
    pub pins: Vec<usize>,
    pub value: f64,
    pub ac: f64,
    pub gain: f64,
    pub branch: Option<usize>,
    pub is_led: bool,
    pub failed_open: bool,
    pub damage: f64,
    pub temperature_c: f64,
    pub fault: Option<String>,
    /// Output latch of a timer; unused by other elements.
    pub latched: bool,
}

impl El {
    pub fn a(&self) -> usize {
        self.pins.first().copied().unwrap_or(0)
    }
    pub fn b(&self) -> usize {
        self.pins.get(1).copied().unwrap_or(0)
    }
}

/// A recorded voltage for the oscilloscope: (time, volts) samples of one net.
#[derive(Clone)]
pub struct Trace {
    pub label: String,
    pub node: usize,
    pub pts: Vec<(f64, f32)>,
}

const TRACE_LIMIT: usize = 8000;

/// A one-pin instrument (frequency counter or logic probe) on a net.
#[derive(Clone)]
pub struct Tap {
    pub refdes: String,
    pub node: usize,
    pub counter: bool,
    last: bool,
    lo: f32,
    hi: f32,
    edges: Vec<f64>,
}

impl Tap {
    /// Frequency from the last few rising edges, or 0 with fewer than two.
    pub fn frequency(&self) -> f64 {
        let n = self.edges.len();
        if n < 2 {
            return 0.0;
        }
        (n - 1) as f64 / (self.edges[n - 1] - self.edges[0]).max(1e-12)
    }
}

#[derive(Clone)]
pub struct Simulator {
    pub traces: Vec<Trace>,
    pub taps: Vec<Tap>,
    pub els: Vec<El>,
    pub nodes: usize,
    pub names: Vec<String>,
    pub v: Vec<f64>,
    pub branch: Vec<f64>,
    pub t: f64,
    pub running: bool,
    pub fault_events: Vec<String>,
    cap_v: HashMap<usize, f64>,
    ind_i: HashMap<usize, f64>,
    lin: Vec<f64>,
}

impl Simulator {
    pub fn from_project(proj: &Project, lib: &Library) -> Result<Simulator, String> {
        let nl = proj.netlist(lib);
        let mut els: Vec<El> = Vec::new();

        let mut traces: Vec<Trace> = Vec::new();
        let mut taps: Vec<Tap> = Vec::new();
        for c in &proj.comps {
            let Some(def) = lib.get(&c.part) else {
                continue;
            };
            if def.symbol == Symbol::Tap {
                if let Some(net) = nl.net_of(c.id, 0) {
                    taps.push(Tap {
                        refdes: c.refdes.clone(),
                        node: node_index(&nl, net),
                        counter: def.id == "COUNTER",
                        last: false,
                        lo: f32::MAX,
                        hi: f32::MIN,
                        edges: Vec::new(),
                    });
                }
                continue;
            }
            if def.symbol == Symbol::Scope {
                for (i, pd) in def.pins.iter().enumerate() {
                    if let Some(net) = nl.net_of(c.id, i) {
                        let node = node_index(&nl, net);
                        if node != 0 {
                            traces.push(Trace { label: format!("{} {}", c.refdes, pd.name), node, pts: Vec::new() });
                        }
                    }
                }
                continue;
            }
            let is_lamp = def.symbol == Symbol::Lamp;
            if def.sim == SimKind::None && !is_lamp {
                continue;
            }
            let node =
                |i: usize| -> usize { nl.net_of(c.id, i).map(|s| node_index(&nl, s)).unwrap_or(0) };
            let mut pins: Vec<usize> = def.pins.iter().enumerate().map(|(i, _)| node(i)).collect();
            if pins.is_empty() {
                continue;
            }

            let (wave, ac) = parse_wave(&c.value);
            let raw = parse_value(&c.value).unwrap_or_else(|| default_for(def.sim));
            let kind = match def.sim {
                SimKind::R => Kind::R,
                SimKind::C => Kind::C,
                SimKind::L => Kind::L,
                SimKind::V => Kind::V { wave },
                SimKind::I => Kind::I { wave },
                SimKind::D => Kind::D,
                SimKind::Sw => Kind::Sw { on: false },
                SimKind::Npn => Kind::Npn,
                SimKind::Pnp => Kind::Pnp,
                SimKind::OpAmp => Kind::OpAmp,
                SimKind::Lamp => Kind::Lamp,
                SimKind::Timer => Kind::Timer,
                SimKind::Logic => Kind::Logic(Gate::from_id(&def.id)),
                SimKind::None => continue,
            };
            let value = match &kind {
                Kind::Lamp => raw.max(1.0),
                Kind::Sw { .. } => 1000.0,
                Kind::R | Kind::C | Kind::L => raw,
                Kind::V { wave } => wave.offset(),
                Kind::I { wave } => wave.offset(),
                _ => raw,
            };
            let value = match def.id.as_str() {
                "VOLTMETER" => 1.0e7,
                "AMMETER" => 1.0e-3,
                _ => value,
            };
            let gain = match def.sim {
                SimKind::Npn | SimKind::Pnp => 100.0,
                SimKind::OpAmp => 1e5,
                _ => 0.0,
            };
            if def.sim == SimKind::OpAmp {
                // Solver order is [+, -, OUT]; parts list their pins in
                // different orders, so look the roles up by name.
                let by_name = |names: &[&str]| {
                    def.pins
                        .iter()
                        .position(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n)))
                        .map(node)
                        .unwrap_or(0)
                };
                pins = vec![
                    by_name(&["+", "IN+", "IN1+"]),
                    by_name(&["-", "IN-", "IN1-"]),
                    by_name(&["OUT", "OUT1"]),
                ];
            }
            els.push(El {
                refdes: c.refdes.clone(),
                kind,
                pins,
                value,
                ac,
                gain,
                branch: None,
                is_led: def.symbol == Symbol::Led,
                failed_open: false,
                damage: 0.0,
                temperature_c: AMBIENT_TEMPERATURE_C,
                fault: None,
                latched: true,
            });
        }

        // Rails nothing drives become ideal supplies at their default voltage.
        for i in 0..nl.names.len() {
            let name = nl.names[i].clone();
            let rail = match name.to_uppercase().as_str() {
                "VCC" | "VDD" => Some(5.0),
                "VEE" => Some(-5.0),
                _ => None,
            };
            if let Some(rv) = rail {
                let driven = els
                    .iter()
                    .any(|e| matches!(e.kind, Kind::V { .. }) && (e.a() == i || e.b() == i));
                if !driven && i != 0 {
                    els.push(El {
                        refdes: name.clone(),
                        kind: Kind::V { wave: Wave::Dc(rv) },
                        pins: vec![i, 0],
                        value: rv,
                        ac: rv.abs(),
                        gain: 0.0,
                        branch: None,
                        is_led: false,
                        failed_open: false,
                        damage: 0.0,
                        temperature_c: AMBIENT_TEMPERATURE_C,
                        fault: None,
                        latched: true,
                    });
                }
            }
        }

        if els.is_empty() {
            return Err("Nothing to simulate - place parts with simulation models first.".into());
        }

        // Record each element's matrix branch so current_of can read the right
        // slot. These slots were previously left as None forever, which made
        // every voltage-source and inductor current read as zero.
        let slots = branch_slots(&els, false);
        for (e, slot) in els.iter_mut().zip(slots) {
            e.branch = slot;
        }

        let nodes = nl.names.len().max(2);
        let mut names = nl.names.clone();
        names.resize(nodes, "N$?".into());
        let initial_linearisation: Vec<f64> = els
            .iter()
            .map(|element| {
                if element.is_led {
                    LED_FORWARD_VOLTAGE + 0.1
                } else {
                    0.0
                }
            })
            .collect();

        // Channels that are not wired to anything have nothing to show.
        traces.retain(|t| els.iter().any(|e| e.pins.contains(&t.node)));

        Ok(Simulator {
            traces,
            taps,
            els,
            nodes,
            names,
            v: vec![0.0; nodes],
            branch: Vec::new(),
            t: 0.0,
            running: false,
            fault_events: Vec::new(),
            cap_v: HashMap::new(),
            ind_i: HashMap::new(),
            lin: initial_linearisation,
        })
    }

    pub fn node_of_net(&self, net: &str) -> Option<usize> {
        self.names.iter().position(|n| n.eq_ignore_ascii_case(net))
    }

    pub fn element(&self, refdes: &str) -> Option<&El> {
        self.els.iter().find(|e| e.refdes == refdes)
    }

    pub fn set_switch(&mut self, refdes: &str, on: bool) {
        for e in &mut self.els {
            if e.refdes == refdes {
                if let Kind::Sw { on: o } = &mut e.kind {
                    *o = on;
                }
            }
        }
    }

    pub fn switch_state(&self, refdes: &str) -> bool {
        self.element(refdes)
            .map(|e| matches!(&e.kind, Kind::Sw { on: true }))
            .unwrap_or(false)
    }

    pub fn voltage(&self, net: &str) -> Option<f64> {
        let i = self.node_of_net(net)?;
        self.v.get(i).copied()
    }

    pub fn current_of(&self, el: &El) -> f64 {
        if el.failed_open {
            return 0.0;
        }
        let va = self.v.get(el.a()).copied().unwrap_or(0.0);
        let vb = self.v.get(el.b()).copied().unwrap_or(0.0);
        match &el.kind {
            Kind::R | Kind::Lamp => (va - vb) / el.value.max(1e-9),
            Kind::C | Kind::Timer | Kind::Logic(_) => 0.0,
            Kind::L | Kind::V { .. } | Kind::OpAmp => el
                .branch
                .and_then(|b| self.branch.get(b).copied())
                .unwrap_or(0.0),
            Kind::I { wave } => wave.eval(self.t),
            Kind::Sw { on } => (va - vb) * if *on { 2.0 } else { 1e-6 },
            // Non-linear devices have no matrix branch. Their current comes
            // from the same linearisation the solver stamped into the matrix.
            Kind::D => {
                let idx = self
                    .els
                    .iter()
                    .position(|e| std::ptr::eq(e, el))
                    .unwrap_or(0);
                let (g, ieq) = if el.is_led {
                    led_params(self.lin.get(idx).copied().unwrap_or(0.0))
                } else {
                    diode_params(self.lin.get(idx).copied().unwrap_or(0.0))
                };
                g * (va - vb) + ieq
            }
            Kind::Npn | Kind::Pnp => {
                let idx = self
                    .els
                    .iter()
                    .position(|e| std::ptr::eq(e, el))
                    .unwrap_or(0);
                let (g, _) = diode_params(self.lin.get(idx).copied().unwrap_or(0.0));
                let gm = el.gain.max(1.0) * g;
                let base = self
                    .v
                    .get(el.pins.first().copied().unwrap_or(0))
                    .copied()
                    .unwrap_or(0.0);
                let emit = self
                    .v
                    .get(el.pins.get(2).copied().unwrap_or(0))
                    .copied()
                    .unwrap_or(0.0);
                let control = if matches!(el.kind, Kind::Npn) {
                    base - emit
                } else {
                    emit - base
                };
                gm * control
            }
        }
    }

    /// Brightness of an LED part, 0..1, from forward current through it.
    pub fn led_level(&self, refdes: &str) -> f64 {
        let Some(el) = self.element(refdes) else {
            return 0.0;
        };
        if !el.is_led || el.failed_open {
            return 0.0;
        }
        let current = self.current_of(el).max(0.0);
        if current < 50e-6 {
            return 0.0;
        }
        (current / LED_NOMINAL_CURRENT).clamp(0.0, 1.0).powf(0.55)
    }

    pub fn is_overloaded(&self, refdes: &str) -> bool {
        let Some(el) = self.element(refdes) else {
            return false;
        };
        if el.failed_open {
            return false;
        }
        let current = self.current_of(el);
        let voltage = self.element_voltage(el);
        if el.is_led {
            current > LED_MAX_CURRENT || voltage < -LED_MAX_REVERSE_VOLTAGE
        } else if matches!(el.kind, Kind::R) {
            (current * voltage).abs() > RESISTOR_RATED_POWER
        } else {
            false
        }
    }

    pub fn element_voltage(&self, el: &El) -> f64 {
        let va = self.v.get(el.a()).copied().unwrap_or(0.0);
        let vb = self.v.get(el.b()).copied().unwrap_or(0.0);
        va - vb
    }

    pub fn drain_fault_events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.fault_events)
    }

    fn update_device_damage(&mut self, dt: f64) -> bool {
        let readings: Vec<(f64, f64)> = self
            .els
            .iter()
            .map(|el| (self.current_of(el), self.element_voltage(el)))
            .collect();
        let mut any_new_failure = false;

        for (el, (current, voltage)) in self.els.iter_mut().zip(readings) {
            if el.failed_open {
                continue;
            }

            let power = (current * voltage).abs();
            let (target_temperature, damage_rate, fault) = if el.is_led {
                let forward_ratio = current.max(0.0) / LED_MAX_CURRENT;
                let reverse_ratio = (-voltage).max(0.0) / LED_MAX_REVERSE_VOLTAGE;
                let rate = if forward_ratio > 1.0 {
                    6.0 * (forward_ratio - 1.0).powi(2)
                } else if reverse_ratio > 1.0 {
                    12.0 * (reverse_ratio - 1.0).powi(2)
                } else {
                    0.0
                };
                let reason = if reverse_ratio > 1.0 {
                    format!(
                        "{} failed open: reverse voltage {:.1} V exceeded {:.1} V",
                        el.refdes, -voltage, LED_MAX_REVERSE_VOLTAGE
                    )
                } else {
                    format!(
                        "{} failed open: LED current {:.0} mA exceeded {:.0} mA",
                        el.refdes,
                        current.max(0.0) * 1_000.0,
                        LED_MAX_CURRENT * 1_000.0
                    )
                };
                (AMBIENT_TEMPERATURE_C + power * 900.0, rate, reason)
            } else if matches!(el.kind, Kind::R) {
                let ratio = power / RESISTOR_RATED_POWER;
                let rate = if ratio > 1.0 {
                    2.0 * (ratio - 1.0).powi(2)
                } else {
                    0.0
                };
                (
                    AMBIENT_TEMPERATURE_C + power * 220.0,
                    rate,
                    format!(
                        "{} failed open: resistor power {:.2} W exceeded {:.2} W",
                        el.refdes, power, RESISTOR_RATED_POWER
                    ),
                )
            } else {
                continue;
            };

            let thermal_alpha = 1.0 - (-dt / 0.8).exp();
            el.temperature_c += (target_temperature - el.temperature_c) * thermal_alpha;
            if damage_rate <= 0.0 {
                el.damage = (el.damage - dt * 0.08).max(0.0);
                continue;
            }

            el.damage += damage_rate * dt;
            if damage_rate >= 50.0 {
                el.damage = 1.0;
            }
            if el.damage >= 1.0 {
                el.failed_open = true;
                el.fault = Some(fault.clone());
                self.fault_events.push(fault);
                any_new_failure = true;
            }
        }

        any_new_failure
    }

    pub fn solve_dc(&mut self) -> Result<(), String> {
        let (v, b) = solve(self, None, 0.0)?;
        self.v = v;
        self.branch = b;
        // Gate outputs depend on their inputs; settle a few passes so a
        // chain of gates starts in a consistent state.
        if self.els.iter().any(|e| matches!(e.kind, Kind::Logic(_))) {
            for e in &mut self.els {
                if matches!(e.kind, Kind::Logic(_)) {
                    e.latched = false;
                }
            }
            for _ in 0..12 {
                self.update_timers();
                let (v, b) = solve(self, None, 0.0)?;
                self.v = v;
                self.branch = b;
            }
        }
        Ok(())
    }

    pub fn step(&mut self, dt: f64) -> Result<(), String> {
        // A timer switches on voltage thresholds, so advance in small sub-steps
        // to keep the blink period close to the real one.
        let digital = self.els.iter().any(|e| matches!(e.kind, Kind::Timer | Kind::Logic(_)));
        let mut n = if digital { (dt / 0.002).ceil() as usize } else { 1 };
        if !self.traces.is_empty() || self.taps.iter().any(|t| t.counter) {
            // Take about 40 samples per cycle of the fastest source.
            let fmax = self
                .els
                .iter()
                .filter_map(|e| match &e.kind {
                    Kind::V { wave } | Kind::I { wave } => Some(wave.frequency()),
                    _ => None,
                })
                .fold(1.0f64, f64::max);
            n = n.max((dt * 40.0 * fmax).ceil() as usize);
        }
        let n = n.clamp(1, 400);
        for _ in 0..n {
            self.step_once(dt / n as f64)?;
            if digital {
                self.update_timers();
            }
            self.record_traces();
        }
        Ok(())
    }

    pub fn tap(&self, refdes: &str) -> Option<&Tap> {
        self.taps.iter().find(|t| t.refdes == refdes)
    }

    fn record_traces(&mut self) {
        let t = self.t;
        for tap in self.taps.iter_mut().filter(|t| t.counter) {
            let v = self.v.get(tap.node).copied().unwrap_or(0.0) as f32;
            tap.lo = tap.lo.min(v);
            tap.hi = tap.hi.max(v);
            if tap.hi - tap.lo < 0.2 {
                continue;
            }
            // Count rising edges through the middle of the observed swing.
            let high = v > (tap.lo + tap.hi) * 0.5;
            if high && !tap.last {
                tap.edges.push(t);
                if tap.edges.len() > 8 {
                    tap.edges.remove(0);
                }
            }
            tap.last = high;
        }
        for tr in &mut self.traces {
            let v = self.v.get(tr.node).copied().unwrap_or(0.0) as f32;
            tr.pts.push((self.t, v));
            if tr.pts.len() > TRACE_LIMIT {
                tr.pts.drain(..TRACE_LIMIT / 4);
            }
        }
    }

    /// Move each 555's output latch according to its comparators: a trigger
    /// below 1/3 of the supply sets it, a threshold above 2/3 resets it, and
    /// a low reset pin or a dead supply forces it low.
    fn update_timers(&mut self) {
        let v = self.v.clone();
        // Gates are updated one after another and see each other's new output
        // at once. That breaks the tie in a cross-coupled latch, which would
        // otherwise flip both gates together forever.
        let mut driven: HashMap<usize, bool> = self
            .els
            .iter()
            .filter(|e| matches!(e.kind, Kind::Logic(_)))
            .filter_map(|e| e.pins.last().map(|&n| (n, e.latched)))
            .collect();
        for e in &mut self.els {
            if let Kind::Logic(g) = e.kind {
                let high = |i: usize| {
                    let n = e.pins[i];
                    driven.get(&n).copied().unwrap_or_else(|| v.get(n).copied().unwrap_or(0.0) > 2.5)
                };
                let n = e.pins.len();
                if n >= 2 {
                    let b = if n >= 3 { high(1) } else { false };
                    e.latched = g.eval(high(0), b);
                    if let Some(&out) = e.pins.last() {
                        driven.insert(out, e.latched);
                    }
                }
                continue;
            }
            if !matches!(e.kind, Kind::Timer) || e.pins.len() < 8 {
                continue;
            }
            let at = |i: usize| v.get(e.pins[i]).copied().unwrap_or(0.0);
            let gnd = at(0);
            let supply = at(7) - gnd;
            if supply < 2.0 || at(3) - gnd < 0.4 {
                e.latched = false;
            } else if at(1) - gnd < supply / 3.0 {
                e.latched = true;
            } else if at(5) - gnd > supply * 2.0 / 3.0 {
                e.latched = false;
            }
        }
    }

    fn step_once(&mut self, dt: f64) -> Result<(), String> {
        let (v, b) = solve(self, Some(dt), self.t)?;
        self.v = v;
        self.branch = b;
        self.t += dt;
        if self.update_device_damage(dt) {
            let (v, b) = solve(self, Some(dt), self.t)?;
            self.v = v;
            self.branch = b;
        }
        Ok(())
    }
}

fn default_for(k: SimKind) -> f64 {
    match k {
        SimKind::Timer => 0.0,
        SimKind::R => 10_000.0,
        SimKind::C => 1e-7,
        SimKind::L => 1e-2,
        _ => 0.0,
    }
}

fn node_index(nl: &Netlist, name: &str) -> usize {
    nl.node(name).unwrap_or(0)
}

// ---------------------------------------------------------------- matrix

struct Matrix {
    n: usize,
    a: Vec<f64>,
    z: Vec<f64>,
}

impl Matrix {
    fn new(n: usize) -> Matrix {
        Matrix {
            n,
            a: vec![0.0; n * n],
            z: vec![0.0; n],
        }
    }
    fn add(&mut self, r: usize, c: usize, v: f64) {
        if r < self.n && c < self.n {
            self.a[r * self.n + c] += v;
        }
    }
    fn rhs(&mut self, r: usize, v: f64) {
        if r < self.n {
            self.z[r] += v;
        }
    }
    fn solve(&self) -> Option<Vec<f64>> {
        let n = self.n;
        let mut m = self.a.clone();
        let mut z = self.z.clone();
        for col in 0..n {
            let mut piv = col;
            let mut best = 0.0f64;
            for r in col..n {
                let val = m[r * n + col].abs();
                if val > best {
                    best = val;
                    piv = r;
                }
            }
            if best < 1e-300 {
                return None;
            }
            if piv != col {
                for c in 0..n {
                    m.swap(col * n + c, piv * n + c);
                }
                z.swap(col, piv);
            }
            let d = m[col * n + col];
            for r in (col + 1)..n {
                let f = m[r * n + col] / d;
                if f == 0.0 {
                    continue;
                }
                for c in col..n {
                    m[r * n + c] -= f * m[col * n + c];
                }
                z[r] -= f * z[col];
            }
        }
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut s = z[i];
            for c in (i + 1)..n {
                s -= m[i * n + c] * x[c];
            }
            let v = s / m[i * n + i];
            if !v.is_finite() {
                return None;
            }
            x[i] = v;
        }
        Some(x)
    }
}

fn unknown(v: usize) -> Option<usize> {
    if v == 0 {
        None
    } else {
        Some(v - 1)
    }
}

fn branch_slots(els: &[El], _dc: bool) -> Vec<Option<usize>> {
    let mut slots = vec![None; els.len()];
    let mut next = 0usize;
    for (i, e) in els.iter().enumerate() {
        // An inductor needs a branch current in both the DC short-circuit
        // model and the transient companion model. Withholding it in AC and
        // transient made `solve` unwrap a missing slot and crash the app.
        let needs = match &e.kind {
            Kind::V { .. } | Kind::OpAmp | Kind::L => true,
            _ => false,
        };
        if needs {
            slots[i] = Some(next);
            next += 1;
        }
    }
    slots
}

fn stamp_g(m: &mut Matrix, ua: Option<usize>, ub: Option<usize>, g: f64) {
    if let Some(a) = ua {
        m.add(a, a, g);
        if let Some(b) = ub {
            m.add(a, b, -g);
            m.add(b, a, -g);
            m.add(b, b, g);
        }
    } else if let Some(b) = ub {
        m.add(b, b, g);
    }
}

/// Current source pushing `cur` from node a to node b.
fn stamp_i(m: &mut Matrix, ua: Option<usize>, ub: Option<usize>, cur: f64) {
    if let Some(a) = ua {
        m.rhs(a, -cur);
    }
    if let Some(b) = ub {
        m.rhs(b, cur);
    }
}

/// Link the two terminal nodes of a branch element to its current unknown.
/// `br` is the branch *slot*; branch columns start after the node columns.
fn stamp_branch_nodes(m: &mut Matrix, ua: Option<usize>, ub: Option<usize>, br: usize, nn: usize) {
    if let Some(a) = ua {
        m.add(a, nn + br, 1.0);
    }
    if let Some(b) = ub {
        m.add(b, nn + br, -1.0);
    }
}

/// i = gm * (vp - vn) flowing from node `from` to node `to`.
fn stamp_vccs(m: &mut Matrix, from: usize, to: usize, vp: usize, vn: usize, gm: f64) {
    let (fr, tr) = (unknown(from), unknown(to));
    let (pr, nr) = (unknown(vp), unknown(vn));
    if let Some(f) = fr {
        if let Some(p) = pr {
            m.add(f, p, gm);
        }
        if let Some(nn) = nr {
            m.add(f, nn, -gm);
        }
    }
    if let Some(t) = tr {
        if let Some(p) = pr {
            m.add(t, p, -gm);
        }
        if let Some(nn) = nr {
            m.add(t, nn, gm);
        }
    }
}

fn diode_params(v0: f64) -> (f64, f64) {
    let v = v0.clamp(-5.0, 1.2);
    if v < 0.0 {
        (1e-9, v * 1e-9)
    } else {
        let g = IS_DIODE / VT * (v / VT).exp();
        let i = IS_DIODE * ((v / VT).exp() - 1.0);
        (g, i - g * v)
    }
}

/// A small-signal LED model with a forward knee and package resistance.
/// The old unbounded exponential tangent could report billions of amps when
/// an LED was connected straight across a rail. This model still identifies
/// that circuit as an overload, but keeps the reading finite and useful.
fn led_params(v0: f64) -> (f64, f64) {
    if v0 < LED_FORWARD_VOLTAGE {
        (1e-9, 0.0)
    } else {
        let g = 1.0 / LED_DYNAMIC_RESISTANCE;
        (g, -g * LED_FORWARD_VOLTAGE)
    }
}

fn solve(sim: &mut Simulator, dt: Option<f64>, t: f64) -> Result<(Vec<f64>, Vec<f64>), String> {
    let dc = dt.is_none();
    let slots = branch_slots(&sim.els, dc);
    let nbranch = slots.iter().filter(|s| s.is_some()).count();
    let nn = sim.nodes.saturating_sub(1);
    let size = nn + nbranch;
    if size == 0 {
        return Ok((vec![0.0; sim.nodes], Vec::new()));
    }

    let mut lin = sim.lin.clone();
    let mut out_v = sim.v.clone();
    let mut out_b = vec![0.0; nbranch];
    let mut prev = sim.v.clone();
    let mut fresh = true;

    for _ in 0..80 {
        let mut m = Matrix::new(size);
        for i in 0..nn {
            m.add(i, i, G_LEAK);
        }

        for (ei, e) in sim.els.iter().enumerate() {
            let ua = unknown(e.a());
            let ub = unknown(e.b());
            let br = slots[ei];
            if e.failed_open {
                stamp_g(&mut m, ua, ub, G_LEAK);
                continue;
            }
            match &e.kind {
                Kind::R | Kind::Lamp => {
                    stamp_g(&mut m, ua, ub, 1.0 / e.value.max(1e-9));
                }
                Kind::C => {
                    if let Some(dt) = dt {
                        let g = e.value / dt;
                        stamp_g(&mut m, ua, ub, g);
                        let vp = sim.cap_v.get(&ei).copied().unwrap_or(0.0);
                        stamp_i(&mut m, ua, ub, -g * vp);
                    }
                }
                Kind::L => {
                    let b = br.unwrap();
                    stamp_branch_nodes(&mut m, ua, ub, b, nn);
                    if let Some(dt) = dt {
                        let ldt = e.value / dt;
                        if let Some(a) = ua {
                            m.add(nn + b, a, -1.0);
                        }
                        if let Some(bn) = ub {
                            m.add(nn + b, bn, 1.0);
                        }
                        m.add(nn + b, nn + b, ldt);
                        let ip = sim.ind_i.get(&ei).copied().unwrap_or(0.0);
                        m.rhs(nn + b, ldt * ip);
                    } else {
                        if let Some(a) = ua {
                            m.add(nn + b, a, 1.0);
                        }
                        if let Some(bn) = ub {
                            m.add(nn + b, bn, -1.0);
                        }
                    }
                }
                Kind::V { wave } => {
                    let b = br.unwrap();
                    stamp_branch_nodes(&mut m, ua, ub, b, nn);
                    if let Some(a) = ua {
                        m.add(nn + b, a, 1.0);
                    }
                    if let Some(bn) = ub {
                        m.add(nn + b, bn, -1.0);
                    }
                    let val = if dc { wave.offset() } else { wave.eval(t) };
                    m.rhs(nn + b, val);
                }
                Kind::I { wave } => {
                    let val = if dc { wave.offset() } else { wave.eval(t) };
                    stamp_i(&mut m, ua, ub, val);
                }
                Kind::D => {
                    let (g, ieq) = if e.is_led {
                        led_params(lin[ei])
                    } else {
                        diode_params(lin[ei])
                    };
                    stamp_g(&mut m, ua, ub, g);
                    stamp_i(&mut m, ua, ub, ieq);
                }
                Kind::Sw { on } => {
                    let g = if *on { 2.0 } else { 1e-6 };
                    stamp_g(&mut m, ua, ub, g);
                }
                Kind::Npn => {
                    // pins: 0 = B, 1 = C, 2 = E
                    let b_pin = e.pins.first().copied().unwrap_or(0);
                    let c_pin = e.pins.get(1).copied().unwrap_or(0);
                    let e_pin = e.pins.get(2).copied().unwrap_or(0);
                    let (g, _) = diode_params(lin[ei]);
                    stamp_g(&mut m, unknown(b_pin), unknown(e_pin), g);
                    let gm = e.gain.max(1.0) * g;
                    stamp_vccs(&mut m, c_pin, e_pin, b_pin, e_pin, gm);
                }
                Kind::Pnp => {
                    // pins: 0 = B, 1 = C, 2 = E
                    let b_pin = e.pins.first().copied().unwrap_or(0);
                    let c_pin = e.pins.get(1).copied().unwrap_or(0);
                    let e_pin = e.pins.get(2).copied().unwrap_or(0);
                    let (g, _) = diode_params(lin[ei]);
                    stamp_g(&mut m, unknown(e_pin), unknown(b_pin), g);
                    let gm = e.gain.max(1.0) * g;
                    // current gm*(vE - vB) flowing E -> C
                    stamp_vccs(&mut m, e_pin, c_pin, e_pin, b_pin, gm);
                }
                Kind::Timer => {
                    if e.pins.len() >= 8 {
                        let (gnd, out, dis) = (unknown(e.pins[0]), unknown(e.pins[2]), unknown(e.pins[6]));
                        let supply = sim.v.get(e.pins[7]).copied().unwrap_or(0.0)
                            - sim.v.get(e.pins[0]).copied().unwrap_or(0.0);
                        // Output stage: about 10 ohms to the high level (one
                        // diode drop and a bit below the supply) or to near 0 V.
                        let target = if supply < 2.0 {
                            0.0
                        } else if e.latched {
                            (supply - 1.7).max(0.0)
                        } else {
                            0.1
                        };
                        stamp_g(&mut m, out, gnd, 0.1);
                        stamp_i(&mut m, gnd, out, 0.1 * target);
                        // Discharge transistor: on while the output is low.
                        let g = if e.latched && supply >= 2.0 { 1e-9 } else { 0.1 };
                        stamp_g(&mut m, dis, gnd, g);
                    }
                }
                Kind::Logic(_) => {
                    if let Some(&out) = e.pins.last() {
                        // Output stage: 50 ohms to 5 V or to 0 V.
                        let g = 0.02;
                        stamp_g(&mut m, unknown(out), None, g);
                        if e.latched {
                            stamp_i(&mut m, None, unknown(out), g * 5.0);
                        }
                    }
                }
                Kind::OpAmp => {
                    let b = br.unwrap();
                    let out = e.pins.get(2).copied().unwrap_or(0);
                    let uo = unknown(out);
                    stamp_branch_nodes(&mut m, uo, None, b, nn);
                    if let Some(o) = uo {
                        m.add(nn + b, o, 1.0);
                    }
                    let vp = e.pins.first().copied().unwrap_or(0);
                    let vn = e.pins.get(1).copied().unwrap_or(1);
                    if let Some(p) = unknown(vp) {
                        m.add(nn + b, p, -e.gain);
                    }
                    if let Some(n2) = unknown(vn) {
                        m.add(nn + b, n2, e.gain);
                    }
                }
            }
        }

        let Some(x) = m.solve() else {
            return Err("The circuit matrix is singular - check for floating nets.".into());
        };

        let mut v = vec![0.0; sim.nodes];
        for i in 0..nn.min(x.len()) {
            v[i + 1] = x[i];
        }
        let mut brv = vec![0.0; nbranch];
        for i in 0..nbranch {
            brv[i] = x[nn + i];
        }

        // advance the linearisation point for the non-linear parts
        let mut next_lin = lin.clone();
        let mut d_lin = 0.0f64;
        for (ei, e) in sim.els.iter().enumerate() {
            let nv = match e.kind {
                Kind::D => {
                    let va = v.get(e.a()).copied().unwrap_or(0.0);
                    let vb = v.get(e.b()).copied().unwrap_or(0.0);
                    va - vb
                }
                Kind::Npn => {
                    let vb = v
                        .get(e.pins.first().copied().unwrap_or(0))
                        .copied()
                        .unwrap_or(0.0);
                    let ve = v
                        .get(e.pins.get(2).copied().unwrap_or(0))
                        .copied()
                        .unwrap_or(0.0);
                    vb - ve
                }
                Kind::Pnp => {
                    let vb = v
                        .get(e.pins.first().copied().unwrap_or(0))
                        .copied()
                        .unwrap_or(0.0);
                    let ve = v
                        .get(e.pins.get(2).copied().unwrap_or(0))
                        .copied()
                        .unwrap_or(0.0);
                    ve - vb
                }
                _ => continue,
            };
            let nv = nv.clamp(-5.0, if e.is_led { 5.0 } else { 1.5 });
            let blended = lin[ei] * 0.5 + nv * 0.5;
            d_lin = d_lin.max((blended - lin[ei]).abs());
            next_lin[ei] = blended;
        }

        let mut d_v = 0.0f64;
        if !fresh {
            for i in 0..sim.nodes {
                d_v = d_v.max((v[i] - prev[i]).abs());
            }
        }
        out_v = v;
        out_b = brv;
        prev = out_v.clone();
        lin = next_lin;
        fresh = false;

        if d_lin < 1e-7 && d_v < 1e-9 {
            break;
        }
    }

    if dt.is_some() {
        for (ei, e) in sim.els.iter().enumerate() {
            match e.kind {
                Kind::C => {
                    let va = out_v.get(e.a()).copied().unwrap_or(0.0);
                    let vb = out_v.get(e.b()).copied().unwrap_or(0.0);
                    sim.cap_v.insert(ei, va - vb);
                }
                Kind::L => {
                    if let Some(b) = slots[ei] {
                        sim.ind_i.insert(ei, out_b.get(b).copied().unwrap_or(0.0));
                    }
                }
                _ => {}
            }
        }
    }
    sim.lin = lin;

    Ok((out_v, out_b))
}

// ---------------------------------------------------------------- AC sweep

#[derive(Clone, Copy, Debug, Default)]
pub struct C64 {
    pub re: f64,
    pub im: f64,
}

impl C64 {
    fn new(re: f64, im: f64) -> C64 {
        C64 { re, im }
    }
    fn of(v: f64) -> C64 {
        C64 { re: v, im: 0.0 }
    }
    fn mag(self) -> f64 {
        self.re.hypot(self.im)
    }
}

impl std::ops::Add for C64 {
    type Output = C64;
    fn add(self, o: C64) -> C64 {
        C64::new(self.re + o.re, self.im + o.im)
    }
}
impl std::ops::Sub for C64 {
    type Output = C64;
    fn sub(self, o: C64) -> C64 {
        C64::new(self.re - o.re, self.im - o.im)
    }
}
impl std::ops::Mul for C64 {
    type Output = C64;
    fn mul(self, o: C64) -> C64 {
        C64::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}
impl std::ops::Div for C64 {
    type Output = C64;
    fn div(self, o: C64) -> C64 {
        let d = o.re * o.re + o.im * o.im;
        C64::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
}

struct CMatrix {
    n: usize,
    a: Vec<C64>,
    z: Vec<C64>,
}

impl CMatrix {
    fn new(n: usize) -> CMatrix {
        CMatrix {
            n,
            a: vec![C64::default(); n * n],
            z: vec![C64::default(); n],
        }
    }
    fn add(&mut self, r: usize, c: usize, v: C64) {
        if r < self.n && c < self.n {
            let cur = self.a[r * self.n + c];
            self.a[r * self.n + c] = cur + v;
        }
    }
    fn rhs(&mut self, r: usize, v: C64) {
        if r < self.n {
            let cur = self.z[r];
            self.z[r] = cur + v;
        }
    }
    fn solve(&self) -> Option<Vec<C64>> {
        let n = self.n;
        let mut m = self.a.clone();
        let mut z = self.z.clone();
        for col in 0..n {
            let mut piv = col;
            let mut best = 0.0f64;
            for r in col..n {
                let val = m[r * n + col].mag();
                if val > best {
                    best = val;
                    piv = r;
                }
            }
            if best < 1e-300 {
                return None;
            }
            if piv != col {
                for c in 0..n {
                    m.swap(col * n + c, piv * n + c);
                }
                z.swap(col, piv);
            }
            let d = m[col * n + col];
            for r in (col + 1)..n {
                let f = m[r * n + col] / d;
                if f.mag() == 0.0 {
                    continue;
                }
                for c in col..n {
                    let cur = m[r * n + c];
                    m[r * n + c] = cur - f * m[col * n + c];
                }
                let cur = z[r];
                z[r] = cur - f * z[col];
            }
        }
        let mut x = vec![C64::default(); n];
        for i in (0..n).rev() {
            let mut s = z[i];
            for c in (i + 1)..n {
                s = s - m[i * n + c] * x[c];
            }
            let v = s / m[i * n + i];
            if !v.re.is_finite() || !v.im.is_finite() {
                return None;
            }
            x[i] = v;
        }
        Some(x)
    }
}

pub struct AcPoint {
    pub freq: f64,
    pub mag: Vec<f64>,
    pub phase: Vec<f64>,
}

pub struct AcResult {
    pub points: Vec<AcPoint>,
    pub nets: Vec<String>,
}

fn ccg(m: &mut CMatrix, ua: Option<usize>, ub: Option<usize>, g: C64) {
    if let Some(a) = ua {
        m.add(a, a, g);
        if let Some(b) = ub {
            let ng = C64::of(-1.0) * g;
            m.add(a, b, ng);
            m.add(b, a, ng);
            m.add(b, b, g);
        }
    } else if let Some(b) = ub {
        m.add(b, b, g);
    }
}

/// AC sweep of the small-signal model around the operating point.
pub fn ac_sweep(sim: &mut Simulator, f1: f64, f2: f64, decades: usize) -> Result<AcResult, String> {
    sim.solve_dc()?;
    let nn = sim.nodes.saturating_sub(1);
    let slots = branch_slots(&sim.els, false);
    let nbranch = slots.iter().filter(|s| s.is_some()).count();
    let size = nn + nbranch;
    let steps = (decades * 12).max(12);
    let mut points = Vec::new();

    for s in 0..=steps {
        let frac = s as f64 / steps as f64;
        let f = f1 * (f2 / f1).powf(frac);
        let w = 2.0 * std::f64::consts::PI * f;
        let mut m = CMatrix::new(size);
        for i in 0..nn {
            m.add(i, i, C64::of(G_LEAK));
        }
        for (ei, e) in sim.els.iter().enumerate() {
            let ua = unknown(e.a());
            let ub = unknown(e.b());
            let br = slots[ei];
            match &e.kind {
                Kind::Timer | Kind::Logic(_) => {}
                Kind::R | Kind::Lamp => {
                    ccg(&mut m, ua, ub, C64::of(1.0 / e.value.max(1e-9)));
                }
                Kind::C => {
                    ccg(&mut m, ua, ub, C64::new(0.0, w * e.value));
                }
                Kind::L => {
                    let b = br.unwrap();
                    stamp_cbranch(&mut m, ua, ub, b, nn);
                    if let Some(a) = ua {
                        m.add(nn + b, a, C64::of(-1.0));
                    }
                    if let Some(bn) = ub {
                        m.add(nn + b, bn, C64::of(1.0));
                    }
                    m.add(nn + b, nn + b, C64::new(0.0, -w * e.value));
                }
                Kind::V { .. } => {
                    let b = br.unwrap();
                    stamp_cbranch(&mut m, ua, ub, b, nn);
                    if let Some(a) = ua {
                        m.add(nn + b, a, C64::of(1.0));
                    }
                    if let Some(bn) = ub {
                        m.add(nn + b, bn, C64::of(-1.0));
                    }
                    m.rhs(nn + b, C64::of(e.ac));
                }
                Kind::I { .. } => {
                    if let Some(a) = ua {
                        m.rhs(a, C64::of(-e.ac));
                    }
                    if let Some(b) = ub {
                        m.rhs(b, C64::of(e.ac));
                    }
                }
                Kind::D | Kind::Sw { .. } => {
                    let g = match &e.kind {
                        Kind::D => {
                            let v0 = sim.lin.get(ei).copied().unwrap_or(0.0);
                            if e.is_led {
                                led_params(v0).0
                            } else {
                                diode_params(v0).0
                            }
                        }
                        Kind::Sw { on } => {
                            if *on {
                                2.0
                            } else {
                                1e-6
                            }
                        }
                        _ => 0.0,
                    };
                    ccg(&mut m, ua, ub, C64::of(g));
                }
                Kind::Npn | Kind::Pnp => {
                    let b_pin = e.pins.first().copied().unwrap_or(0);
                    let c_pin = e.pins.get(1).copied().unwrap_or(0);
                    let e_pin = e.pins.get(2).copied().unwrap_or(0);
                    let v0 = sim.lin.get(ei).copied().unwrap_or(0.0);
                    let g = diode_params(v0).0;
                    let gm = e.gain.max(1.0) * g;
                    if matches!(e.kind, Kind::Npn) {
                        ccg(&mut m, unknown(b_pin), unknown(e_pin), C64::of(g));
                        cvccs(&mut m, c_pin, e_pin, b_pin, e_pin, gm, nn);
                    } else {
                        ccg(&mut m, unknown(e_pin), unknown(b_pin), C64::of(g));
                        cvccs(&mut m, e_pin, c_pin, e_pin, b_pin, gm, nn);
                    }
                }
                Kind::OpAmp => {
                    let b = br.unwrap();
                    let uo = unknown(e.pins.get(2).copied().unwrap_or(0));
                    stamp_cbranch(&mut m, uo, None, b, nn);
                    if let Some(o) = uo {
                        m.add(nn + b, o, C64::of(1.0));
                    }
                    let vp = e.pins.first().copied().unwrap_or(0);
                    let vn = e.pins.get(1).copied().unwrap_or(1);
                    if let Some(p) = unknown(vp) {
                        m.add(nn + b, p, C64::of(-e.gain));
                    }
                    if let Some(n2) = unknown(vn) {
                        m.add(nn + b, n2, C64::of(e.gain));
                    }
                }
            }
        }
        let Some(x) = m.solve() else {
            return Err("AC analysis failed: singular matrix.".into());
        };
        let mut mag = Vec::new();
        let mut phase = Vec::new();
        for i in 0..sim.nodes {
            let c = if i == 0 || i > nn {
                C64::default()
            } else {
                x[i - 1]
            };
            mag.push(c.mag());
            phase.push(c.im.atan2(c.re).to_degrees());
        }
        points.push(AcPoint {
            freq: f,
            mag,
            phase,
        });
    }

    Ok(AcResult {
        points,
        nets: sim.names.clone(),
    })
}

fn stamp_cbranch(m: &mut CMatrix, ua: Option<usize>, ub: Option<usize>, br: usize, nn: usize) {
    if let Some(a) = ua {
        m.add(a, nn + br, C64::of(1.0));
    }
    if let Some(b) = ub {
        m.add(b, nn + br, C64::of(-1.0));
    }
}

fn cvccs(m: &mut CMatrix, from: usize, to: usize, vp: usize, vn: usize, gm: f64, nn: usize) {
    let _ = nn;
    let (fr, tr) = (unknown(from), unknown(to));
    let (pr, nr) = (unknown(vp), unknown(vn));
    let g = C64::of(gm);
    let ng = C64::of(-gm);
    if let Some(f) = fr {
        if let Some(p) = pr {
            m.add(f, p, g);
        }
        if let Some(n) = nr {
            m.add(f, n, ng);
        }
    }
    if let Some(t) = tr {
        if let Some(p) = pr {
            m.add(t, p, ng);
        }
        if let Some(n) = nr {
            m.add(t, n, g);
        }
    }
}

// ---------------------------------------------------------------- batch runs

#[derive(Clone, Debug, Default)]
pub struct Waveform {
    pub times: Vec<f64>,
    pub volts: Vec<Vec<f64>>,
    pub current: Vec<Vec<f64>>,
}

pub fn run_transient(
    sim: &mut Simulator,
    stop: f64,
    dt: f64,
    max_points: usize,
) -> Result<Waveform, String> {
    sim.solve_dc()?;
    let mut w = Waveform {
        times: vec![sim.t],
        volts: sim.v.clone().into_iter().map(|v| vec![v]).collect(),
        current: sim.els.iter().map(|e| vec![sim.current_of(e)]).collect(),
    };
    let steps = ((stop / dt).ceil() as usize).clamp(1, 500_000);
    let stride = (steps / max_points.max(1)).max(1);
    for i in 0..steps {
        sim.step(dt)?;
        if i % stride == 0 {
            w.times.push(sim.t);
            for (ni, v) in sim.v.iter().enumerate() {
                if w.volts.len() <= ni {
                    w.volts.push(Vec::new());
                }
                w.volts[ni].push(*v);
            }
            for (ei, e) in sim.els.iter().enumerate() {
                if w.current.len() <= ei {
                    w.current.push(Vec::new());
                }
                w.current[ei].push(sim.current_of(e));
            }
        }
    }
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Library, Project, Wire, P};

    fn connect_points(proj: &mut Project, pts: Vec<P>) {
        let id = proj.alloc_id();
        proj.wires.push(Wire {
            id,
            pts,
            start_pin: None,
            end_pin: None,
        });
    }

    /// Battery (9 V) and resistor (100 ohm) in a closed loop. Pin 1 of each
    /// two-terminal part sits at pos + (60, 0); pin 0 at pos + (-60, 0).
    fn battery_resistor_loop() -> (Project, Library) {
        let lib = Library::builtin();
        let mut proj = Project::new("loop");
        let bt = proj
            .add_component(&lib, "BATTERY", P::new(-200.0, 0.0))
            .unwrap();
        let r = proj
            .add_component(&lib, "RESISTOR", P::new(0.0, 0.0))
            .unwrap();
        proj.comp_mut(r).unwrap().value = "100R".into();
        // battery pin 1 (-140,0) -> resistor pin 0 (-60,0)
        connect_points(&mut proj, vec![P::new(-140.0, 0.0), P::new(-60.0, 0.0)]);
        // resistor pin 1 (60,0) -> battery pin 0 (-260,0)
        connect_points(
            &mut proj,
            vec![
                P::new(60.0, 0.0),
                P::new(60.0, -100.0),
                P::new(-260.0, -100.0),
                P::new(-260.0, 0.0),
            ],
        );
        let _ = bt;
        (proj, lib)
    }

    fn battery_resistor_led_loop(resistance: &str) -> (Project, Library, String) {
        let lib = Library::builtin();
        let mut proj = Project::new("led-loop");
        let source = proj
            .add_component(&lib, "VCC", P::new(-300.0, 0.0))
            .unwrap();
        let resistor = proj
            .add_component(&lib, "RESISTOR", P::new(-100.0, 0.0))
            .unwrap();
        let led = proj
            .add_component(&lib, "LED-RED", P::new(100.0, 0.0))
            .unwrap();
        let ground = proj.add_component(&lib, "GND", P::new(300.0, 0.0)).unwrap();
        proj.comp_mut(resistor).unwrap().value = resistance.into();
        let led_refdes = proj.comp(led).unwrap().refdes.clone();

        connect_points(&mut proj, vec![P::new(-300.0, 0.0), P::new(-160.0, 0.0)]);
        connect_points(&mut proj, vec![P::new(-40.0, 0.0), P::new(40.0, 0.0)]);
        connect_points(&mut proj, vec![P::new(160.0, 0.0), P::new(300.0, 0.0)]);
        let _ = (source, ground);
        (proj, lib, led_refdes)
    }

    #[test]
    fn voltage_suffix_is_not_lost() {
        let (wave, ac) = parse_wave("9V");
        assert_eq!(wave.offset(), 9.0);
        assert_eq!(ac, 9.0);

        let lib = Library::builtin();
        let mut proj = Project::new("test");
        proj.add_component(&lib, "BATTERY", P::new(0.0, 0.0))
            .unwrap();
        let sim = Simulator::from_project(&proj, &lib).unwrap();
        let battery = sim.els.iter().find(|e| e.refdes == "BT1").unwrap();
        assert_eq!(battery.value, 9.0);
    }

    #[test]
    fn voltage_source_current_is_measured() {
        let (proj, lib) = battery_resistor_loop();
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().expect("dc operating point");

        // Battery is the first element; with a 100 ohm load across 9 V the
        // source current must be about 90 mA, never the old constant zero.
        let cur = sim.current_of(&sim.els[0]).abs();
        assert!(
            (cur - 0.09).abs() < 1e-3,
            "expected ~0.09 A through the source, got {cur}"
        );

        let resistor = &sim.els[1];
        let vdrop = sim.voltage(&sim.names[resistor.a()]).unwrap_or(0.0)
            - sim.voltage(&sim.names[resistor.b()]).unwrap_or(0.0);
        assert!(
            (vdrop.abs() - 9.0).abs() < 1e-3,
            "expected 9 V across R, got {vdrop}"
        );
    }

    #[test]
    fn safe_led_uses_current_for_brightness_and_survives() {
        let (proj, lib, led_refdes) = battery_resistor_led_loop("330R");
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().expect("dc operating point");
        let level = sim.led_level(&led_refdes);
        let led = sim.element(&led_refdes).unwrap();
        assert!(
            level > 0.1 && level <= 1.0,
            "unexpected LED level {level}; current={} voltage={} lin={:?}",
            sim.current_of(led),
            sim.element_voltage(led),
            sim.lin
        );

        for _ in 0..100 {
            sim.step(0.01).expect("safe LED transient");
        }
        assert!(!sim.element(&led_refdes).unwrap().failed_open);
    }

    #[test]
    fn overloaded_led_burns_open_and_stops_lighting() {
        let (proj, lib, led_refdes) = battery_resistor_led_loop("10R");
        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        sim.solve_dc().expect("dc operating point");
        assert!(sim.is_overloaded(&led_refdes));

        for _ in 0..100 {
            sim.step(0.01).expect("overloaded LED transient");
            if sim.element(&led_refdes).unwrap().failed_open {
                break;
            }
        }

        let led = sim.element(&led_refdes).unwrap();
        assert!(led.failed_open, "overloaded LED should fail open");
        assert_eq!(sim.led_level(&led_refdes), 0.0);
        assert!(sim.current_of(led).abs() < 1e-9);
        assert!(led
            .fault
            .as_deref()
            .is_some_and(|msg| msg.contains("LED current")));
    }

    #[test]
    fn inductor_transient_and_ac_do_not_panic() {
        let lib = Library::builtin();
        let mut proj = Project::new("rl");
        let bt = proj
            .add_component(&lib, "BATTERY", P::new(-200.0, 0.0))
            .unwrap();
        let r = proj
            .add_component(&lib, "RESISTOR", P::new(0.0, 0.0))
            .unwrap();
        let l = proj
            .add_component(&lib, "INDUCTOR", P::new(200.0, 0.0))
            .unwrap();
        proj.comp_mut(r).unwrap().value = "100R".into();
        proj.comp_mut(l).unwrap().value = "10m".into();
        connect_points(&mut proj, vec![P::new(-140.0, 0.0), P::new(-60.0, 0.0)]);
        connect_points(&mut proj, vec![P::new(60.0, 0.0), P::new(140.0, 0.0)]);
        connect_points(
            &mut proj,
            vec![
                P::new(260.0, 0.0),
                P::new(260.0, -100.0),
                P::new(-260.0, -100.0),
                P::new(-260.0, 0.0),
            ],
        );
        let _ = bt;

        let mut sim = Simulator::from_project(&proj, &lib).unwrap();
        let wf = run_transient(&mut sim, 1e-3, 1e-5, 100).expect("RL transient");
        assert!(!wf.times.is_empty());
        assert!(wf.times.len() == wf.current[0].len());

        let mut sim2 = Simulator::from_project(&proj, &lib).unwrap();
        let ac = ac_sweep(&mut sim2, 10.0, 1.0e5, 2).expect("RL AC sweep");
        assert!(!ac.points.is_empty());
    }
}
