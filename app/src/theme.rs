use eframe::egui::{self, Color32, Rounding, Stroke};

// Palette from the original's app-colors.css (light and dark tokens).
pub const SIDEBAR_BG: Color32 = Color32::from_rgb(0x28, 0x2a, 0x2e);
pub const SIDEBAR_TEXT: Color32 = Color32::from_rgb(0xae, 0xb8, 0xc5);
pub const SIDEBAR_SUB: Color32 = Color32::from_rgb(0x66, 0x70, 0x7e);
pub const SIDEBAR_SEL: Color32 = Color32::from_rgb(0x00, 0x70, 0xe6);
pub const SIDEBAR_HOVER: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
pub const SIDEBAR_LINE: Color32 = Color32::from_rgb(0x3d, 0x3f, 0x43);
pub const ACCENT: Color32 = Color32::from_rgb(0x15, 0x7e, 0xfb);
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(0x00, 0x53, 0xcf);
pub const ORANGE: Color32 = Color32::from_rgb(0xf5, 0x8a, 0x07);
pub const RED: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
pub const GREEN: Color32 = Color32::from_rgb(0x2f, 0xb3, 0x5c);
pub const PURPLE: Color32 = Color32::from_rgb(0x8e, 0x5c, 0xe6);

#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,
    pub alt: Color32,
    pub text: Color32,
    pub sub: Color32,
    pub sel: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub card: Color32,
}

impl Pal {
    pub fn new(dark: bool) -> Self {
        if dark {
            Pal {
                bg: Color32::from_rgb(0x1c, 0x1d, 0x20),
                alt: Color32::from_rgb(0x23, 0x25, 0x28),
                text: Color32::from_rgb(0xde, 0xe3, 0xeb),
                sub: Color32::from_rgb(0x86, 0x8d, 0x97),
                sel: Color32::from_rgb(0x19, 0x33, 0x50),
                hover: Color32::from_rgb(0x2a, 0x2c, 0x30),
                border: Color32::from_rgb(0x33, 0x36, 0x3b),
                card: Color32::from_rgb(0x26, 0x28, 0x2c),
            }
        } else {
            Pal {
                bg: Color32::WHITE,
                alt: Color32::from_rgb(0xf6, 0xf7, 0xf9),
                text: Color32::from_rgb(0x14, 0x14, 0x14),
                sub: Color32::from_rgb(0x66, 0x70, 0x7e),
                sel: Color32::from_rgb(0xe5, 0xf2, 0xff),
                hover: Color32::from_rgb(0xf3, 0xf5, 0xf8),
                border: Color32::from_rgb(0xe6, 0xe8, 0xec),
                card: Color32::WHITE,
            }
        }
    }
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let pal = Pal::new(dark);
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = pal.bg;
    v.window_fill = pal.bg;
    v.extreme_bg_color = pal.alt;
    v.faint_bg_color = pal.alt;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = if dark { Color32::from_rgb(0x0c, 0x58, 0xa7) } else { Color32::from_rgb(0xb4, 0xdc, 0xff) };
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_rounding = Rounding::same(10.0);
    v.window_stroke = Stroke::new(1.0_f32, pal.border);
    v.override_text_color = Some(pal.text);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = Rounding::same(6.0);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.widgets.inactive.weak_bg_fill = pal.alt;
    v.widgets.inactive.bg_fill = pal.alt;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

const AVATAR: [Color32; 8] = [
    Color32::from_rgb(0x15, 0x7e, 0xfb),
    Color32::from_rgb(0x2f, 0xb3, 0x5c),
    Color32::from_rgb(0xf5, 0x8a, 0x07),
    Color32::from_rgb(0x8e, 0x5c, 0xe6),
    Color32::from_rgb(0xe5, 0x48, 0x4d),
    Color32::from_rgb(0x12, 0xa4, 0xb3),
    Color32::from_rgb(0xd1, 0x4f, 0x9b),
    Color32::from_rgb(0x5b, 0x6b, 0x82),
];

pub fn avatar_color(key: &str) -> Color32 {
    let h = key.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    AVATAR[(h as usize) % AVATAR.len()]
}

pub fn initials(name: &str) -> String {
    let mut out: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .collect();
    if out.is_empty() {
        out.push('?');
    }
    out.to_uppercase()
}

pub const CALENDAR_COLORS: [Color32; 4] = [ACCENT, GREEN, ORANGE, PURPLE];
