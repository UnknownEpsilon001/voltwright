# Voltwright

Voltwright is a desktop electronics suite written in Rust. You draw a schematic,
run it live, and lay out the board in one window.

It is early work. Expect rough edges and file-format changes.

## Features

- Schematic editor: place parts, wire pin to pin, and let the router find an
  orthogonal path around component bodies. Wires follow parts when you move
  them, and junction dots appear where three or more wire ends meet.
- Live simulation: press Run and watch current arrows move along the wires.
  The solver handles resistors, capacitors, inductors, sources, diodes, LEDs,
  NPN/PNP transistors, op-amps, a 555 timer, switches and lamps. Overloaded LEDs and
  resistors burn out.
- Analyses: transient and AC sweep.
- PCB layout with design-rule checks.
- Exports: bill of materials, Gerber and pick-and-place files.
- Undo and redo, property assignment and search tools.
- Parts library of about 180 parts with a small symbol preview beside each
  one: diodes, LEDs, transistors, op-amps, 74-series and 4000-series logic,
  regulators, drivers, microcontrollers, sensors, displays and connectors.
  Diodes, LEDs, transistors and dual op-amps are simulated; the rest are
  pin-accurate blocks for schematic and PCB work.
- Source code editor with syntax colours for C/C++/Arduino, Python and
  assembly, line numbers, and file open and save. You can load a compiled
  `.bin` or Intel HEX file to check its checksums and browse its bytes.
  Voltwright has no compiler or CPU emulator yet, so firmware is not run in
  the simulation.
- Built-in example circuits (Examples menu or the Home page): LED, voltage
  divider, transistor switch, RC charging, op-amp amplifier, diode
  rectifier, 555 timer LED blinker, op-amp voltage follower, RC low-pass
  filter, and logic gate circuits (NAND truth table, AND/OR/XOR, half adder,
  SR latch, D flip-flop divider, clickable logic inputs), plus a smoothing rectifier, inverting amplifier, series RLC
  resonance and common-emitter amplifier. AND, OR, XOR, NAND, NOR, XNOR, NOT and buffer gates are simulated
  as 5 V logic.
- Schematic editing: mirror (M), rotate, copy/cut/paste with wires kept,
  arrow-key nudge, a thick bus tool, net labels that count up (A0, A1, ...),
  several sheets side by side (View > Add sheet; nets join across sheets by
  label name), automatic annotation (Tools > Annotate), and an electrical
  rules check (Tools > Electrical Rules Check) that marks problems on the sheet.
- PCB autorouter (Tools > Autoroute PCB, or the Autoroute button on the PCB tab): a two-layer maze router with vias that keeps clearances to pads, tracks and the board edge, leaves already-connected nets alone, and reports any net it could not finish. It is one undo step; "Rip up" removes all tracks. The ratsnest now shows only the connections still open.
- CNC export (File > Export): G-code for GRBL-style routers in millimetres -
  isolation routing around top copper, drilling with tool-change pauses, and a
  multi-pass board outline cut. Bit sizes, depths and feeds are set in the
  dialog. Round pads are assumed for isolation, and only the top layer is cut.
- Instruments: voltmeter and ammeter parts with live readouts, a signal
  generator, and a frequency counter, a logic probe, and a four-channel oscilloscope with a floating CRT-style window (knobs for time/div, volts/div, position and trigger level, RUN/STOP, AC coupling)
  (Tools > Oscilloscope; it opens when a simulation with a scope part runs).
- Import of `.pdsprj` projects. Voltwright brings across the parts it has
  models for, rebuilds the nets and reroutes the wires; it lists anything it
  had to skip.

## Build

You need a recent stable Rust toolchain.

```sh
cargo run --release
```

Start with a running simulation:

```sh
cargo run --release -- --simulate
```

Run the tests:

```sh
cargo test
```

## Project files

Voltwright saves projects as plain text with the `.volt` extension.

## Licence

MIT. See [LICENSE](LICENSE).
