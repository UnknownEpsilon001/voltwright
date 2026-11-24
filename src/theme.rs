// Dark workstation palette for the editor and every surrounding UI surface.
// LEO WAS HERE ;3

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2};

// Canvas
pub const CANVAS: Color32 = Color32::from_rgb(0x07, 0x0A, 0x0E);
pub const GRID_DOT: Color32 = Color32::from_rgb(0x25, 0x2C, 0x34);
pub const GRID_DOT_MAJOR: Color32 = Color32::from_rgb(0x3B, 0x46, 0x52);

// Chrome
pub const CHROME: Color32 = Color32::from_rgb(0x14, 0x19, 0x1F);
pub const CHROME_LIGHT: Color32 = Color32::from_rgb(0x1A, 0x21, 0x29);
pub const CHROME_PALE: Color32 = Color32::from_rgb(0x0E, 0x13, 0x18);
pub const CHROME_DARK: Color32 = Color32::from_rgb(0x68, 0x74, 0x81);
pub const CHROME_EDGE: Color32 = Color32::from_rgb(0x35, 0x40, 0x4C);
pub const TEXT: Color32 = Color32::from_rgb(0xDC, 0xE5, 0xED);

// Schematic ink
pub const WIRE: Color32 = Color32::from_rgb(0x00, 0x9F, 0xE3);
pub const WIRE_HOT: Color32 = Color32::from_rgb(0x46, 0xC2, 0xF1);
pub const PIN: Color32 = Color32::from_rgb(0xC8, 0xC8, 0xC8);
pub const BODY_GREEN: Color32 = Color32::from_rgb(0x2C, 0xA7, 0x37);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0xA8, 0xB5);
pub const SELECTION: Color32 = Color32::from_rgb(0xFF, 0xD0, 0x20);
pub const HOVER: Color32 = Color32::from_rgb(0x70, 0xD0, 0xFF);
pub const GHOST: Color32 = Color32::from_rgb(0x80, 0xC0, 0xFF);

// PCB
pub const COPPER_TOP: Color32 = Color32::from_rgb(0xC0, 0x20, 0x20);
pub const COPPER_BOTTOM: Color32 = Color32::from_rgb(0x20, 0x40, 0xC0);
pub const SILK_TOP: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xE8);
pub const SILK_BOTTOM: Color32 = Color32::from_rgb(0x90, 0xC0, 0x90);
pub const MASK_TOP: Color32 = Color32::from_rgb(0x80, 0x30, 0x30);
pub const MASK_BOTTOM: Color32 = Color32::from_rgb(0x30, 0x40, 0x80);
pub const BOARD_EDGE: Color32 = Color32::from_rgb(0xE0, 0xE0, 0x40);
pub const DRILL: Color32 = Color32::from_rgb(0xC0, 0xC0, 0xC0);
pub const VIA_COLOR: Color32 = Color32::from_rgb(0xD0, 0xA0, 0x20);

// Graphs (sampled from Frequency_graph_simulated.png)
pub const GRAPH_BG: Color32 = Color32::from_rgb(0x00, 0x20, 0x00);
pub const GRAPH_GRID: Color32 = Color32::from_rgb(0x00, 0x50, 0x00);
pub const GRAPH_TEXT: Color32 = Color32::from_rgb(0xC0, 0xE0, 0xC0);
pub const TRACE: [Color32; 6] = [
    Color32::from_rgb(0x00, 0xFF, 0x00),
    Color32::from_rgb(0x00, 0x00, 0xFF),
    Color32::from_rgb(0x00, 0xE0, 0x00),
    Color32::from_rgb(0xC0, 0x20, 0x00),
    Color32::from_rgb(0x00, 0xC0, 0xC0),
    Color32::from_rgb(0xFF, 0x00, 0x00),
];

// Status / accents
pub const OK: Color32 = Color32::from_rgb(0x18, 0x8C, 0x30);
pub const WARN: Color32 = Color32::from_rgb(0xC0, 0x80, 0x00);
pub const ERR: Color32 = Color32::from_rgb(0xC0, 0x20, 0x20);
pub const LINK: Color32 = Color32::from_rgb(0x4C, 0xC7, 0xFF);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = Vec2::new(6.0, 4.0);
    style.spacing.button_padding = Vec2::new(8.0, 3.0);
    style.spacing.interact_size = Vec2::new(22.0, 20.0);
    style.spacing.menu_margin = egui::Margin::symmetric(6, 4);
    style.visuals.window_corner_radius = CornerRadius::same(2);
    style.visuals.menu_corner_radius = CornerRadius::same(2);
    style.visuals.widgets.noninteractive.bg_fill = CHROME;
    style.visuals.widgets.noninteractive.weak_bg_fill = CHROME;
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, CHROME_EDGE);
    style.visuals.widgets.inactive.bg_fill = CHROME_LIGHT;
    style.visuals.widgets.inactive.weak_bg_fill = CHROME_LIGHT;
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, CHROME_EDGE);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x26, 0x37, 0x46);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x26, 0x37, 0x46);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, WIRE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(0x18, 0x3B, 0x52);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x18, 0x3B, 0x52);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, WIRE_HOT);
    style.visuals.widgets.open.bg_fill = Color32::from_rgb(0x1D, 0x31, 0x40);
    style.visuals.widgets.open.weak_bg_fill = Color32::from_rgb(0x1D, 0x31, 0x40);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, WIRE);
    style.visuals.selection.bg_fill = Color32::from_rgb(0x1A, 0x58, 0x7A);
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.panel_fill = CHROME;
    style.visuals.window_fill = CHROME_PALE;
    style.visuals.window_stroke = Stroke::new(1.0, CHROME_EDGE);
    style.visuals.faint_bg_color = CHROME_LIGHT;
    style.visuals.extreme_bg_color = Color32::from_rgb(0x08, 0x0C, 0x11);
    style.visuals.code_bg_color = Color32::from_rgb(0x0A, 0x0F, 0x14);
    style.visuals.hyperlink_color = LINK;
    style.visuals.override_text_color = Some(TEXT);
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(16.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// Format a schematic coordinate in th (shown in the status bar).
pub fn format_th(v: f32) -> String {
    if v.abs() >= 1000.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}
