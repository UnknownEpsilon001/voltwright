// Source code editor: syntax colours, line numbers, file open/save, and a
// loader for compiled firmware (.bin and Intel HEX).
//
// There is no compiler or CPU emulator built in. Build your firmware with
// your own toolchain, then load the .bin or .hex here to check it and look
// at its bytes.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use eframe::egui::{
    self, text::LayoutJob, Color32, FontFamily, FontId, RichText, ScrollArea, TextFormat,
};

// ------------------------------------------------------------------ languages

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    C,
    Python,
    Asm,
    Plain,
}

impl Lang {
    pub fn from_path(path: &str) -> Lang {
        let ext = Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "c" | "h" | "cpp" | "hpp" | "cc" | "ino" | "pde" => Lang::C,
            "py" | "pyw" => Lang::Python,
            "asm" | "s" | "inc" => Lang::Asm,
            _ => Lang::Plain,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::C => "C / C++ / Arduino",
            Lang::Python => "Python",
            Lang::Asm => "Assembly",
            Lang::Plain => "Plain text",
        }
    }
}

// ------------------------------------------------------------------ tokenizer

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    Type,
    Builtin,
    Function,
    Str,
    Number,
    Comment,
    Preproc,
    Label,
}

const C_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return",
    "goto", "typedef", "struct", "union", "enum", "static", "const", "volatile", "extern", "inline",
    "sizeof", "new", "delete", "class", "public", "private", "protected", "namespace", "using",
    "template", "true", "false", "nullptr", "NULL", "register", "auto",
];
const C_TYPES: &[&str] = &[
    "void", "int", "char", "short", "long", "float", "double", "unsigned", "signed", "bool",
    "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t", "int32_t", "int64_t",
    "size_t", "byte", "word", "String",
];
const C_BUILTINS: &[&str] = &[
    "setup", "loop", "main", "pinMode", "digitalWrite", "digitalRead", "analogRead", "analogWrite",
    "delay", "delayMicroseconds", "millis", "micros", "Serial", "Wire", "SPI", "HIGH", "LOW",
    "INPUT", "OUTPUT", "INPUT_PULLUP", "LED_BUILTIN", "attachInterrupt", "tone", "noTone",
];
const PY_KEYWORDS: &[&str] = &[
    "def", "class", "if", "elif", "else", "for", "while", "return", "import", "from", "as", "with",
    "try", "except", "finally", "raise", "pass", "break", "continue", "lambda", "yield", "in", "is",
    "not", "and", "or", "None", "True", "False", "global", "nonlocal", "assert", "del", "async",
    "await",
];
const PY_BUILTINS: &[&str] = &[
    "print", "len", "range", "int", "float", "str", "list", "dict", "set", "tuple", "bool", "open",
    "machine", "Pin", "time", "sleep", "sleep_ms", "utime", "self",
];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Split `text` into coloured spans that tile it exactly, in order.
pub fn tokenize(text: &str, lang: Lang) -> Vec<(Range<usize>, Kind)> {
    let mut out: Vec<(Range<usize>, Kind)> = Vec::new();
    let mut push = |start: usize, end: usize, kind: Kind| {
        if end > start {
            out.push((start..end, kind));
        }
    };
    if lang == Lang::Plain {
        push(0, text.len(), Kind::Plain);
        return out;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let pos = |i: usize| chars.get(i).map(|&(p, _)| p).unwrap_or(text.len());
    let mut i = 0;
    let mut line_start = true; // only whitespace seen on this line so far
    let mut first_word = true; // for assembly mnemonics
    while i < chars.len() {
        let c = chars[i].1;
        let start = pos(i);
        if c == '\n' {
            push(start, pos(i + 1), Kind::Plain);
            i += 1;
            line_start = true;
            first_word = true;
            continue;
        }
        if c.is_whitespace() {
            push(start, pos(i + 1), Kind::Plain);
            i += 1;
            continue;
        }
        // Comments.
        let comment = match lang {
            Lang::C => (c == '/' && at(i + 1) == Some('/')) || false,
            Lang::Python => c == '#',
            Lang::Asm => c == ';' || (c == '/' && at(i + 1) == Some('/')),
            Lang::Plain => false,
        };
        if comment {
            let mut j = i;
            while j < chars.len() && chars[j].1 != '\n' {
                j += 1;
            }
            push(start, pos(j), Kind::Comment);
            i = j;
            continue;
        }
        if lang == Lang::C && c == '/' && at(i + 1) == Some('*') {
            let mut j = i + 2;
            while j < chars.len() && !(chars[j].1 == '*' && at(j + 1) == Some('/')) {
                j += 1;
            }
            let end = (j + 2).min(chars.len());
            push(start, pos(end), Kind::Comment);
            i = end;
            line_start = false;
            continue;
        }
        // Preprocessor lines.
        if lang == Lang::C && c == '#' && line_start {
            let mut j = i;
            while j < chars.len() && chars[j].1 != '\n' {
                j += 1;
            }
            push(start, pos(j), Kind::Preproc);
            i = j;
            continue;
        }
        line_start = false;
        // Strings.
        if c == '"' || c == '\'' {
            let triple = lang == Lang::Python
                && at(i + 1) == Some(c)
                && at(i + 2) == Some(c);
            let mut j = i + if triple { 3 } else { 1 };
            while j < chars.len() {
                if chars[j].1 == '\\' {
                    j += 2;
                    continue;
                }
                if triple {
                    if chars[j].1 == c && at(j + 1) == Some(c) && at(j + 2) == Some(c) {
                        j += 3;
                        break;
                    }
                } else if chars[j].1 == c {
                    j += 1;
                    break;
                } else if chars[j].1 == '\n' {
                    break;
                }
                j += 1;
            }
            let j = j.min(chars.len());
            push(start, pos(j), Kind::Str);
            i = j;
            first_word = false;
            continue;
        }
        // Numbers: decimal, 0x hex, 0b binary, $hex (assembly), with suffixes.
        if c.is_ascii_digit() || (lang == Lang::Asm && c == '$' && at(i + 1).is_some_and(|d| d.is_ascii_hexdigit())) {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].1.is_ascii_alphanumeric() || chars[j].1 == '.' || chars[j].1 == '_') {
                j += 1;
            }
            push(start, pos(j), Kind::Number);
            i = j;
            first_word = false;
            continue;
        }
        // Words.
        if is_ident_start(c) || (lang == Lang::Asm && c == '.') || (lang == Lang::Python && c == '@') {
            let mut j = i + 1;
            while j < chars.len() && is_ident(chars[j].1) {
                j += 1;
            }
            let word = &text[start..pos(j)];
            let next = {
                let mut k = j;
                while at(k) == Some(' ') {
                    k += 1;
                }
                at(k)
            };
            let kind = match lang {
                Lang::C => {
                    if C_KEYWORDS.contains(&word) {
                        Kind::Keyword
                    } else if C_TYPES.contains(&word) || word.ends_with("_t") {
                        Kind::Type
                    } else if C_BUILTINS.contains(&word) {
                        Kind::Builtin
                    } else if next == Some('(') {
                        Kind::Function
                    } else {
                        Kind::Plain
                    }
                }
                Lang::Python => {
                    if word.starts_with('@') {
                        Kind::Preproc
                    } else if PY_KEYWORDS.contains(&word) {
                        Kind::Keyword
                    } else if PY_BUILTINS.contains(&word) {
                        Kind::Builtin
                    } else if next == Some('(') {
                        Kind::Function
                    } else {
                        Kind::Plain
                    }
                }
                Lang::Asm => {
                    if word.starts_with('.') {
                        Kind::Preproc
                    } else if next == Some(':') {
                        Kind::Label
                    } else if is_register(word) {
                        Kind::Type
                    } else if first_word {
                        Kind::Keyword
                    } else {
                        Kind::Plain
                    }
                }
                Lang::Plain => Kind::Plain,
            };
            if kind != Kind::Label || next == Some(':') {
                first_word = false;
            }
            push(start, pos(j), kind);
            i = j;
            continue;
        }
        // Anything else: operators and punctuation.
        push(start, pos(i + 1), Kind::Plain);
        i += 1;
    }
    out
}

fn is_register(word: &str) -> bool {
    let w = word.to_lowercase();
    if let Some(rest) = w.strip_prefix('r') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    matches!(
        w.as_str(),
        "a" | "b" | "x" | "y" | "z" | "sp" | "pc" | "acc" | "dptr" | "ax" | "bx" | "cx" | "dx" | "si" | "di" | "lr"
    )
}

fn color_of(kind: Kind) -> Color32 {
    match kind {
        Kind::Plain => Color32::from_rgb(0xD4, 0xD4, 0xD4),
        Kind::Keyword => Color32::from_rgb(0x56, 0x9C, 0xD6),
        Kind::Type => Color32::from_rgb(0x4E, 0xC9, 0xB0),
        Kind::Builtin => Color32::from_rgb(0x4F, 0xC1, 0xFF),
        Kind::Function => Color32::from_rgb(0xDC, 0xDC, 0xAA),
        Kind::Str => Color32::from_rgb(0xCE, 0x91, 0x78),
        Kind::Number => Color32::from_rgb(0xB5, 0xCE, 0xA8),
        Kind::Comment => Color32::from_rgb(0x6A, 0x99, 0x55),
        Kind::Preproc => Color32::from_rgb(0xC5, 0x86, 0xC0),
        Kind::Label => Color32::from_rgb(0xFF, 0xD7, 0x00),
    }
}

fn layout(text: &str, lang: Lang, font: &FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for (range, kind) in tokenize(text, lang) {
        job.append(
            &text[range],
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: color_of(kind),
                ..Default::default()
            },
        );
    }
    job
}

// ------------------------------------------------------------------ firmware

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FwKind {
    Binary,
    IntelHex,
}

#[derive(Clone, Debug)]
pub struct Firmware {
    pub name: String,
    pub kind: FwKind,
    /// Image bytes from `base`; gaps in a HEX file are filled with 0xFF.
    pub image: Vec<u8>,
    pub base: u32,
    /// Bytes actually present in the file (HEX gaps excluded).
    pub data_bytes: usize,
    pub records: usize,
    pub bad_records: usize,
    pub crc32: u32,
}

impl Firmware {
    pub fn summary(&self) -> String {
        let end = self.base as usize + self.image.len().saturating_sub(1);
        match self.kind {
            FwKind::Binary => format!(
                "{}: raw binary, {} bytes, CRC-32 {:08X}",
                self.name,
                self.image.len(),
                self.crc32
            ),
            FwKind::IntelHex => format!(
                "{}: Intel HEX, {} data bytes at 0x{:04X}-0x{:04X}, {} records, {} bad checksums, CRC-32 {:08X}",
                self.name, self.data_bytes, self.base, end, self.records, self.bad_records, self.crc32
            ),
        }
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn hex_byte(s: &str, at: usize) -> Option<u8> {
    u8::from_str_radix(s.get(at..at + 2)?, 16).ok()
}

fn parse_hex(name: &str, text: &str) -> Result<Firmware, String> {
    let mut bytes: BTreeMap<u32, u8> = BTreeMap::new();
    let mut upper: u32 = 0;
    let (mut records, mut bad) = (0usize, 0usize);
    let mut saw_eof = false;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(body) = line.strip_prefix(':') else {
            return Err(format!("line {} does not start with ':'", n + 1));
        };
        let len = hex_byte(body, 0).ok_or(format!("line {}: bad length", n + 1))? as usize;
        if body.len() < (len + 5) * 2 {
            return Err(format!("line {} is too short", n + 1));
        }
        let all: Option<Vec<u8>> = (0..len + 5).map(|i| hex_byte(body, i * 2)).collect();
        let all = all.ok_or(format!("line {}: bad hex digits", n + 1))?;
        records += 1;
        let sum: u32 = all.iter().map(|&b| b as u32).sum();
        if sum % 256 != 0 {
            bad += 1;
        }
        let addr = ((all[1] as u32) << 8) | all[2] as u32;
        match all[3] {
            0x00 => {
                for (k, &b) in all[4..4 + len].iter().enumerate() {
                    bytes.insert(upper.wrapping_add(addr).wrapping_add(k as u32), b);
                }
            }
            0x01 => {
                saw_eof = true;
                break;
            }
            0x02 if len == 2 => upper = (((all[4] as u32) << 8) | all[5] as u32) << 4,
            0x04 if len == 2 => upper = (((all[4] as u32) << 8) | all[5] as u32) << 16,
            _ => {}
        }
    }
    if !saw_eof {
        return Err("no end-of-file record; the HEX file looks truncated".into());
    }
    let (Some((&lo, _)), Some((&hi, _))) = (bytes.iter().next(), bytes.iter().next_back()) else {
        return Err("the HEX file has no data records".into());
    };
    let mut image = vec![0xFFu8; (hi - lo) as usize + 1];
    for (&a, &b) in &bytes {
        image[(a - lo) as usize] = b;
    }
    let crc = crc32(&image);
    Ok(Firmware {
        name: name.into(),
        kind: FwKind::IntelHex,
        image,
        base: lo,
        data_bytes: bytes.len(),
        records,
        bad_records: bad,
        crc32: crc,
    })
}

/// Parse a firmware file: Intel HEX or a raw binary image.
pub fn parse_firmware(name: &str, data: &[u8]) -> Result<Firmware, String> {
    if data.is_empty() {
        return Err("the file is empty".into());
    }
    if data.starts_with(b"\x7fELF") {
        return Err("this is an ELF executable; convert it first, e.g. objcopy -O binary or -O ihex".into());
    }
    let ext_hex = name.to_lowercase().ends_with(".hex") || name.to_lowercase().ends_with(".ihx");
    let looks_hex = data[0] == b':' && data.iter().all(|&b| b == b'\n' || b == b'\r' || (32..127).contains(&b));
    if ext_hex || looks_hex {
        let text = String::from_utf8_lossy(data);
        return parse_hex(name, &text);
    }
    Ok(Firmware {
        name: name.into(),
        kind: FwKind::Binary,
        image: data.to_vec(),
        base: 0,
        data_bytes: data.len(),
        records: 0,
        bad_records: 0,
        crc32: crc32(data),
    })
}

/// One row of a hex dump: offset, 16 bytes in hex, then ASCII.
pub fn dump_row(base: u32, offset: usize, bytes: &[u8]) -> String {
    let mut hex = String::new();
    let mut ascii = String::new();
    for i in 0..16 {
        match bytes.get(i) {
            Some(&b) => {
                hex.push_str(&format!("{b:02X} "));
                ascii.push(if (32..127).contains(&b) { b as char } else { '.' });
            }
            None => hex.push_str("   "),
        }
        if i == 7 {
            hex.push(' ');
        }
    }
    format!("{:08X}  {hex} |{ascii}|", base as usize + offset)
}

// ------------------------------------------------------------------ the panel

pub struct SourceEditor {
    pub path: String,
    pub lang: Option<Lang>,
    pub fw_path: String,
    pub firmware: Option<Firmware>,
    pub show_dump: bool,
    pub message: String,
    pub cursor: (usize, usize),
}

impl Default for SourceEditor {
    fn default() -> Self {
        SourceEditor {
            path: "main.ino".into(),
            lang: None,
            fw_path: "firmware.hex".into(),
            firmware: None,
            show_dump: false,
            message: String::new(),
            cursor: (1, 1),
        }
    }
}

const BG: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);
const GUTTER: Color32 = Color32::from_rgb(0x85, 0x85, 0x85);

impl SourceEditor {
    fn lang(&self) -> Lang {
        self.lang.unwrap_or_else(|| Lang::from_path(&self.path))
    }

    fn open_source(&mut self, source: &mut String) {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                *source = text;
                self.lang = None;
                self.message = format!("Opened {}", self.path);
            }
            Err(e) => self.message = format!("Open failed: {e}"),
        }
    }

    fn save_source(&mut self, source: &str) {
        match std::fs::write(&self.path, source) {
            Ok(()) => self.message = format!("Saved {}", self.path),
            Err(e) => self.message = format!("Save failed: {e}"),
        }
    }

    fn load_firmware(&mut self) {
        let path = self.fw_path.trim().to_string();
        let name = Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|d| parse_firmware(&name, &d)) {
            Ok(fw) => {
                self.message = format!("Loaded firmware {}", fw.name);
                self.firmware = Some(fw);
            }
            Err(e) => {
                self.firmware = None;
                self.message = format!("Firmware load failed: {e}");
            }
        }
    }

    /// Draw the editor. Returns true when the source text changed.
    pub fn ui(&mut self, ui: &mut egui::Ui, source: &mut String) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("File:");
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(260.0));
            if ui.button("Open").clicked() {
                self.open_source(source);
                changed = true;
            }
            if ui.button("Save").clicked() {
                self.save_source(source);
            }
            ui.separator();
            egui::ComboBox::from_id_salt("source-lang")
                .selected_text(self.lang().name())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.lang, None, "Auto (from file name)");
                    for l in [Lang::C, Lang::Python, Lang::Asm, Lang::Plain] {
                        ui.selectable_value(&mut self.lang, Some(l), l.name());
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Firmware:");
            ui.add(egui::TextEdit::singleline(&mut self.fw_path).desired_width(260.0));
            if ui.button("Load .bin / .hex").clicked() {
                self.load_firmware();
            }
            if self.firmware.is_some() && ui.button(if self.show_dump { "Hide bytes" } else { "Show bytes" }).clicked() {
                self.show_dump = !self.show_dump;
            }
        });
        if let Some(fw) = &self.firmware {
            let color = if fw.bad_records > 0 { egui::Color32::from_rgb(0xF0, 0x80, 0x40) } else { egui::Color32::from_rgb(0x60, 0xC0, 0x70) };
            ui.colored_label(color, fw.summary());
            ui.weak("Loaded and checked only: Voltwright has no CPU emulator yet, so the firmware does not run in the simulation.");
        } else {
            ui.weak("No compiler is built in. Build with your own toolchain, then load the .bin or .hex here.");
        }
        if !self.message.is_empty() {
            ui.weak(&self.message);
        }
        ui.separator();

        // Reserve room for the byte view and status line under the editor.
        let below = if self.show_dump && self.firmware.is_some() { 230.0 } else { 26.0 };
        let height = (ui.available_height() - below).max(120.0);
        let font = FontId::new(14.0, FontFamily::Monospace);
        let lang = self.lang();

        let lines = source.split('\n').count().max(1);
        let digits = lines.to_string().len().max(2);
        let numbers = (1..=lines)
            .map(|n| format!("{n:>digits$}"))
            .collect::<Vec<_>>()
            .join("\n");

        let mut cursor_char: Option<usize> = None;
        egui::Frame::NONE.fill(BG).show(ui, |ui| {
            ScrollArea::both()
                .id_salt("source-scroll")
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(RichText::new(numbers).font(font.clone()).color(GUTTER))
                                .selectable(false)
                                .wrap_mode(egui::TextWrapMode::Extend),
                        );
                        ui.add_space(6.0);
                        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
                            let job = layout(text.as_str(), lang, &font);
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                        let out = egui::TextEdit::multiline(source)
                            .font(font.clone())
                            .lock_focus(true)
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layouter)
                            .show(ui);
                        if out.response.response.changed() {
                            changed = true;
                        }
                        cursor_char = out.cursor_range.map(|c| c.primary.index.0);
                    });
                });
        });
        if let Some(idx) = cursor_char {
            let before: String = source.chars().take(idx).collect();
            let line = before.matches('\n').count() + 1;
            let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
            self.cursor = (line, col);
        }
        ui.weak(format!(
            "Ln {}, Col {}   |   {} lines   |   {}",
            self.cursor.0,
            self.cursor.1,
            lines,
            lang.name()
        ));

        if self.show_dump {
            if let Some(fw) = &self.firmware {
                ui.separator();
                ScrollArea::vertical()
                    .id_salt("fw-dump")
                    .max_height(190.0)
                    .show(ui, |ui| {
                        let shown = fw.image.len().min(4096);
                        for (row, chunk) in fw.image[..shown].chunks(16).enumerate() {
                            ui.monospace(dump_row(fw.base, row * 16, chunk));
                        }
                        if fw.image.len() > shown {
                            ui.weak(format!("... {} more bytes not shown", fw.image.len() - shown));
                        }
                    });
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, lang: Lang) -> Vec<(String, Kind)> {
        tokenize(text, lang)
            .into_iter()
            .filter(|(r, k)| *k != Kind::Plain || !text[r.clone()].trim().is_empty())
            .map(|(r, k)| (text[r].to_string(), k))
            .collect()
    }

    #[test]
    fn tokens_tile_the_text_exactly() {
        let src = "#include <x.h>\nint main() { /* c */ return 0x1F; } // end\n\"s\\\"q\" é\n";
        for lang in [Lang::C, Lang::Python, Lang::Asm, Lang::Plain] {
            let toks = tokenize(src, lang);
            let mut at = 0;
            for (r, _) in &toks {
                assert_eq!(r.start, at, "gap before {r:?} in {lang:?}");
                at = r.end;
            }
            assert_eq!(at, src.len(), "{lang:?} does not cover the text");
        }
    }

    #[test]
    fn c_highlighting() {
        let k = kinds("#define LED 13\nvoid setup() { digitalWrite(LED, HIGH); } // go", Lang::C);
        assert!(k.contains(&("#define LED 13".into(), Kind::Preproc)));
        assert!(k.contains(&("void".into(), Kind::Type)));
        assert!(k.contains(&("setup".into(), Kind::Builtin)));
        assert!(k.contains(&("digitalWrite".into(), Kind::Builtin)));
        assert!(k.contains(&("// go".into(), Kind::Comment)));
    }

    #[test]
    fn python_and_asm_highlighting() {
        let k = kinds("def run(x):\n    return 'hi' # note", Lang::Python);
        assert!(k.contains(&("def".into(), Kind::Keyword)));
        assert!(k.contains(&("run".into(), Kind::Function)));
        assert!(k.contains(&("'hi'".into(), Kind::Str)));
        let k = kinds("start:\n  ldi r16, 0xFF ; set\n  .org 0", Lang::Asm);
        assert!(k.contains(&("start".into(), Kind::Label)));
        assert!(k.contains(&("ldi".into(), Kind::Keyword)));
        assert!(k.contains(&("r16".into(), Kind::Type)));
        assert!(k.contains(&("0xFF".into(), Kind::Number)));
        assert!(k.contains(&(".org".into(), Kind::Preproc)));
    }

    #[test]
    fn language_from_extension() {
        assert_eq!(Lang::from_path("a/b/main.ino"), Lang::C);
        assert_eq!(Lang::from_path("x.PY"), Lang::Python);
        assert_eq!(Lang::from_path("boot.s"), Lang::Asm);
        assert_eq!(Lang::from_path("notes.txt"), Lang::Plain);
    }

    const HEX: &str = ":10000000000102030405060708090A0B0C0D0E0F78\n:0400100010111213A6\n:00000001FF\n";

    #[test]
    fn intel_hex_parses_and_checks_sums() {
        let fw = parse_firmware("a.hex", HEX.as_bytes()).unwrap();
        assert_eq!(fw.kind, FwKind::IntelHex);
        assert_eq!(fw.base, 0);
        assert_eq!(fw.data_bytes, 20);
        assert_eq!(fw.image.len(), 0x14);
        assert_eq!(fw.image[0x12], 0x12);
        assert_eq!(fw.bad_records, 0);
        let bad = HEX.replace("A6", "A7");
        assert_eq!(parse_firmware("a.hex", bad.as_bytes()).unwrap().bad_records, 1);
        assert!(parse_firmware("a.hex", b":00000002FE\n").is_err());
    }

    #[test]
    fn binary_and_elf() {
        let fw = parse_firmware("a.bin", &[1, 2, 3, 4]).unwrap();
        assert_eq!(fw.kind, FwKind::Binary);
        assert_eq!(fw.image, vec![1, 2, 3, 4]);
        assert!(parse_firmware("a.elf", b"\x7fELF....").is_err());
        assert!(parse_firmware("a.bin", &[]).is_err());
    }

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn dump_row_formats() {
        let row = dump_row(0x100, 16, b"AB\x00");
        assert!(row.starts_with("00000110  41 42 00 "));
        assert!(row.ends_with("|AB.|"));
    }
}
