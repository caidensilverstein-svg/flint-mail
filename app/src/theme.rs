use eframe::egui::{self, Color32, Rounding, Stroke};

// Warm editorial palette: paper and ink, one deep green accent.
pub const SIDEBAR_BG: Color32 = Color32::from_rgb(0x1e, 0x1d, 0x1b);
pub const SIDEBAR_TEXT: Color32 = Color32::from_rgb(0xb9, 0xb3, 0xa8);
pub const SIDEBAR_SUB: Color32 = Color32::from_rgb(0x7c, 0x77, 0x6d);
pub const SIDEBAR_SEL: Color32 = Color32::from_rgb(0x2f, 0x5d, 0x50);
pub const SIDEBAR_HOVER: Color32 = Color32::from_rgba_premultiplied(24, 24, 22, 24);
pub const SIDEBAR_LINE: Color32 = Color32::from_rgb(0x33, 0x31, 0x2e);
pub const ACCENT: Color32 = Color32::from_rgb(0x2c, 0x5e, 0x4f);
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(0x1e, 0x42, 0x37);
pub const ORANGE: Color32 = Color32::from_rgb(0xb7, 0x79, 0x1f);
pub const RED: Color32 = Color32::from_rgb(0xb5, 0x48, 0x3f);
pub const GREEN: Color32 = Color32::from_rgb(0x3d, 0x7a, 0x5e);
pub const PURPLE: Color32 = Color32::from_rgb(0x6b, 0x5b, 0x95);

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
                bg: Color32::from_rgb(0x17, 0x16, 0x14),
                alt: Color32::from_rgb(0x1f, 0x1d, 0x1b),
                text: Color32::from_rgb(0xe8, 0xe4, 0xdc),
                sub: Color32::from_rgb(0x8f, 0x89, 0x7e),
                sel: Color32::from_rgb(0x24, 0x33, 0x2d),
                hover: Color32::from_rgb(0x25, 0x23, 0x20),
                border: Color32::from_rgb(0x2e, 0x2b, 0x28),
                card: Color32::from_rgb(0x20, 0x1e, 0x1c),
            }
        } else {
            Pal {
                bg: Color32::from_rgb(0xfa, 0xf8, 0xf4),
                alt: Color32::from_rgb(0xf2, 0xef, 0xe9),
                text: Color32::from_rgb(0x1b, 0x1a, 0x18),
                sub: Color32::from_rgb(0x7a, 0x75, 0x6c),
                sel: Color32::from_rgb(0xe6, 0xee, 0xe9),
                hover: Color32::from_rgb(0xf1, 0xee, 0xe8),
                border: Color32::from_rgb(0xe6, 0xe1, 0xd8),
                card: Color32::from_rgb(0xff, 0xfe, 0xfc),
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
    v.selection.bg_fill = if dark { Color32::from_rgb(0x2f, 0x5d, 0x50) } else { Color32::from_rgb(0xc9, 0xdd, 0xd3) };
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0_f32, pal.border);
    v.override_text_color = Some(pal.text);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = Rounding::same(7.0);
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
    Color32::from_rgb(0x2c, 0x5e, 0x4f),
    Color32::from_rgb(0x8a, 0x5a, 0x44),
    Color32::from_rgb(0x4f, 0x6d, 0x8c),
    Color32::from_rgb(0x6b, 0x5b, 0x95),
    Color32::from_rgb(0xa0, 0x52, 0x4a),
    Color32::from_rgb(0x3f, 0x80, 0x82),
    Color32::from_rgb(0x9a, 0x6f, 0x2e),
    Color32::from_rgb(0x5b, 0x64, 0x72),
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

static SERIF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// A system serif for headings. Every candidate is parsed first because egui panics on a bad font file.
pub fn install_fonts(ctx: &egui::Context) {
    let candidates = [
        "/System/Library/Fonts/NewYork.ttf",
        "/System/Library/Fonts/Supplemental/Georgia.ttf",
        "/Library/Fonts/Georgia.ttf",
        "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
    ];
    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else { continue };
        if ab_glyph::FontVec::try_from_vec_and_index(bytes.clone(), 0).is_err() {
            continue;
        }
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("flint-serif".into(), egui::FontData::from_owned(bytes));
        let mut fam = vec!["flint-serif".to_string()];
        fam.extend(fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
        fonts.families.insert(egui::FontFamily::Name("serif".into()), fam);
        ctx.set_fonts(fonts);
        SERIF.store(true, std::sync::atomic::Ordering::Relaxed);
        return;
    }
}

pub fn serif(size: f32) -> egui::FontId {
    if SERIF.load(std::sync::atomic::Ordering::Relaxed) {
        egui::FontId::new(size, egui::FontFamily::Name("serif".into()))
    } else {
        egui::FontId::proportional(size)
    }
}
