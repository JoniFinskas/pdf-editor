//! Custom light and dark theme, Inter fonts and the shared color palette.

use eframe::egui::{
    self, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin,
    Shadow, Stroke, Style, TextStyle, Theme, Visuals,
};
use std::sync::Arc;

pub const SEMIBOLD: &str = "Inter SemiBold";
pub const RADIUS: u8 = 8;

/// Colors not covered by egui's `Visuals`, chosen per theme.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Area behind the pages.
    pub canvas: Color32,
    /// Top bar, sidebars and status bar.
    pub chrome: Color32,
    /// Cards, menus and floating bars.
    pub surface: Color32,
    /// Text fields and select boxes.
    pub field: Color32,
    pub hover: Color32,
    pub border: Color32,
    /// Outlines of fields and outlined buttons.
    pub border_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    /// Accent used for text and icons on the chrome.
    pub accent_strong: Color32,
    pub accent_text: Color32,
    /// Background of selected and active items.
    pub accent_soft: Color32,
    pub tint: Color32,
    pub page_shadow: Color32,
    /// Find matches on the page, and the current match.
    pub highlight: Color32,
    pub highlight_active: Color32,
    pub warn_bg: Color32,
    pub warn_fg: Color32,
    pub error_bg: Color32,
    pub error_fg: Color32,
    pub ok_bg: Color32,
    pub ok_fg: Color32,
    /// Fill of destructive buttons.
    pub danger: Color32,
}

const LIGHT: Palette = Palette {
    canvas: Color32::from_rgb(0xEF, 0xF1, 0xF4),
    chrome: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    field: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    hover: Color32::from_rgb(0xF1, 0xF3, 0xF6),
    border: Color32::from_rgb(0xE3, 0xE6, 0xEB),
    border_strong: Color32::from_rgb(0xCB, 0xD1, 0xDA),
    text: Color32::from_rgb(0x11, 0x18, 0x27),
    muted: Color32::from_rgb(0x67, 0x6E, 0x7B),
    accent: Color32::from_rgb(0x25, 0x63, 0xEB),
    accent_strong: Color32::from_rgb(0x1D, 0x5B, 0xE0),
    accent_text: Color32::WHITE,
    accent_soft: Color32::from_rgba_premultiplied(0x04, 0x0A, 0x18, 0x1A),
    tint: Color32::from_rgba_premultiplied(0x04, 0x0A, 0x18, 0x1A),
    page_shadow: Color32::from_black_alpha(34),
    highlight: Color32::from_rgba_premultiplied(0x71, 0x5C, 0x09, 0x73),
    highlight_active: Color32::from_rgba_premultiplied(0x1B, 0x3B, 0x6F, 0x73),
    warn_bg: Color32::from_rgb(0xFF, 0xF6, 0xE0),
    warn_fg: Color32::from_rgb(0x7A, 0x4E, 0x00),
    error_bg: Color32::from_rgb(0xFD, 0xEC, 0xEC),
    error_fg: Color32::from_rgb(0xA3, 0x1F, 0x1F),
    ok_bg: Color32::from_rgb(0xE8, 0xF6, 0xEE),
    ok_fg: Color32::from_rgb(0x1E, 0x6B, 0x3F),
    danger: Color32::from_rgb(0xDC, 0x26, 0x26),
};

const DARK: Palette = Palette {
    canvas: Color32::from_rgb(0x1A, 0x1C, 0x21),
    chrome: Color32::from_rgb(0x14, 0x16, 0x1A),
    surface: Color32::from_rgb(0x1E, 0x21, 0x27),
    field: Color32::from_rgb(0x16, 0x18, 0x1D),
    hover: Color32::from_rgb(0x22, 0x25, 0x2C),
    border: Color32::from_rgb(0x26, 0x2A, 0x31),
    border_strong: Color32::from_rgb(0x38, 0x3E, 0x48),
    text: Color32::from_rgb(0xE7, 0xE9, 0xEC),
    muted: Color32::from_rgb(0x91, 0x98, 0xA3),
    accent: Color32::from_rgb(0x3B, 0x82, 0xF6),
    accent_strong: Color32::from_rgb(0x60, 0xA0, 0xF7),
    accent_text: Color32::WHITE,
    accent_soft: Color32::from_rgba_premultiplied(0x09, 0x14, 0x27, 0x28),
    tint: Color32::from_rgba_premultiplied(0x09, 0x14, 0x27, 0x28),
    page_shadow: Color32::from_black_alpha(120),
    highlight: Color32::from_rgba_premultiplied(0x71, 0x5C, 0x09, 0x73),
    highlight_active: Color32::from_rgba_premultiplied(0x1B, 0x3B, 0x6F, 0x73),
    warn_bg: Color32::from_rgb(0x33, 0x2A, 0x14),
    warn_fg: Color32::from_rgb(0xF2, 0xC9, 0x6B),
    error_bg: Color32::from_rgb(0x3A, 0x1D, 0x1F),
    error_fg: Color32::from_rgb(0xF4, 0x9A, 0x9A),
    ok_bg: Color32::from_rgb(0x16, 0x30, 0x22),
    ok_fg: Color32::from_rgb(0x8F, 0xDB, 0xAE),
    danger: Color32::from_rgb(0xDC, 0x26, 0x26),
};

pub fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

/// Palette for code that paints without a `Ui`.
pub fn palette_of(ctx: &Context) -> Palette {
    if ctx.global_style().visuals.dark_mode {
        DARK
    } else {
        LIGHT
    }
}

pub fn install(ctx: &Context) {
    install_fonts(ctx);
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, Visuals::light()));
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, Visuals::dark()));
    ctx.all_styles_mut(style);
    // Ctrl+plus/minus zoom the document, not the interface.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        SEMIBOLD.into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    let mut fallbacks = Vec::new();
    // System fallbacks keep non-Latin PDF text visible in the edit box.
    if let Some(windir) = std::env::var_os("WINDIR") {
        let dir = std::path::Path::new(&windir).join("Fonts");
        for (name, file) in [
            ("Segoe UI", "segoeui.ttf"),
            ("Segoe UI Symbol", "seguisym.ttf"),
        ] {
            if let Ok(bytes) = std::fs::read(dir.join(file)) {
                fonts
                    .font_data
                    .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
                fallbacks.push(name.to_string());
            }
        }
    }
    let regular: Vec<String> = std::iter::once("Inter".to_string())
        .chain(fallbacks.clone())
        .collect();
    let semibold: Vec<String> = std::iter::once(SEMIBOLD.to_string())
        .chain(fallbacks)
        .collect();
    fonts
        .families
        .insert(FontFamily::Proportional, regular.clone());
    fonts.families.insert(FontFamily::Monospace, regular);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

fn visuals(p: &Palette, mut v: Visuals) -> Visuals {
    let radius = CornerRadius::same(RADIUS);
    v.override_text_color = Some(p.text);
    v.panel_fill = p.chrome;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border_strong);
    v.window_corner_radius = CornerRadius::same(12);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: p.page_shadow,
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: p.page_shadow,
    };
    v.menu_corner_radius = CornerRadius::same(10);
    v.extreme_bg_color = p.field;
    v.text_edit_bg_color = Some(p.field);
    v.faint_bg_color = p.canvas;
    v.hyperlink_color = p.accent_strong;
    v.selection.bg_fill = p.accent.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(1.5, p.accent_strong);
    v.button_frame = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill, stroke) in [
        (&mut w.inactive, Color32::TRANSPARENT, Color32::TRANSPARENT),
        (&mut w.hovered, p.hover, p.border_strong),
        (&mut w.active, p.hover, p.border_strong),
        (&mut w.open, p.hover, p.border_strong),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    // Text fields, select boxes and number fields draw their frame from the
    // inactive stroke; menus clear it for their items.
    w.inactive.bg_fill = p.field;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border_strong);
    v
}

fn style(s: &mut Style) {
    use FontFamily::Proportional;
    s.text_styles = [
        (TextStyle::Heading, semibold(18.0)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Proportional)),
    ]
    .into();
    s.spacing.item_spacing = egui::vec2(8.0, 8.0);
    s.spacing.button_padding = egui::vec2(12.0, 6.0);
    s.spacing.interact_size.y = 30.0;
    s.spacing.window_margin = Margin::same(20);
    s.spacing.menu_margin = Margin::same(6);
    s.spacing.text_edit_width = 220.0;
    s.interaction.selectable_labels = false;
}
