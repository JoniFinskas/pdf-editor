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
    pub canvas: Color32,
    pub chrome: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub tint: Color32,
    pub page_shadow: Color32,
    pub warn_bg: Color32,
    pub warn_fg: Color32,
    pub error_bg: Color32,
    pub error_fg: Color32,
    pub ok_bg: Color32,
    pub ok_fg: Color32,
}

const LIGHT: Palette = Palette {
    canvas: Color32::from_rgb(0xEE, 0xEF, 0xF2),
    chrome: Color32::from_rgb(0xFC, 0xFC, 0xFD),
    border: Color32::from_rgb(0xE3, 0xE5, 0xE9),
    text: Color32::from_rgb(0x1C, 0x1D, 0x22),
    muted: Color32::from_rgb(0x6B, 0x6F, 0x78),
    accent: Color32::from_rgb(0x5E, 0x6A, 0xD2),
    accent_text: Color32::WHITE,
    tint: Color32::from_rgba_premultiplied(0x10, 0x12, 0x24, 0x1F),
    page_shadow: Color32::from_black_alpha(30),
    warn_bg: Color32::from_rgb(0xFF, 0xF6, 0xE0),
    warn_fg: Color32::from_rgb(0x7A, 0x4E, 0x00),
    error_bg: Color32::from_rgb(0xFD, 0xEC, 0xEC),
    error_fg: Color32::from_rgb(0xA3, 0x1F, 0x1F),
    ok_bg: Color32::from_rgb(0xE8, 0xF6, 0xEE),
    ok_fg: Color32::from_rgb(0x1E, 0x6B, 0x3F),
};

const DARK: Palette = Palette {
    canvas: Color32::from_rgb(0x14, 0x15, 0x18),
    chrome: Color32::from_rgb(0x1C, 0x1D, 0x21),
    border: Color32::from_rgb(0x2D, 0x2F, 0x35),
    text: Color32::from_rgb(0xE6, 0xE7, 0xEA),
    muted: Color32::from_rgb(0x8C, 0x90, 0x99),
    accent: Color32::from_rgb(0x7C, 0x86, 0xF0),
    accent_text: Color32::from_rgb(0x10, 0x11, 0x1A),
    tint: Color32::from_rgba_premultiplied(0x17, 0x19, 0x2D, 0x2D),
    page_shadow: Color32::from_black_alpha(110),
    warn_bg: Color32::from_rgb(0x33, 0x2A, 0x14),
    warn_fg: Color32::from_rgb(0xF2, 0xC9, 0x6B),
    error_bg: Color32::from_rgb(0x3A, 0x1D, 0x1F),
    error_fg: Color32::from_rgb(0xF4, 0x9A, 0x9A),
    ok_bg: Color32::from_rgb(0x16, 0x30, 0x22),
    ok_fg: Color32::from_rgb(0x8F, 0xDB, 0xAE),
};

pub fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
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
    v.window_fill = p.chrome;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: p.page_shadow,
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: p.page_shadow,
    };
    v.menu_corner_radius = radius;
    v.extreme_bg_color = p.canvas;
    v.faint_bg_color = p.canvas;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.button_frame = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill, stroke) in [
        (&mut w.inactive, Color32::TRANSPARENT, Color32::TRANSPARENT),
        (&mut w.hovered, p.border.gamma_multiply(0.6), p.border),
        (&mut w.active, p.border, p.border),
        (&mut w.open, p.border.gamma_multiply(0.6), p.border),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    v
}

fn style(s: &mut Style) {
    use FontFamily::Proportional;
    s.text_styles = [
        (TextStyle::Heading, semibold(17.0)),
        (TextStyle::Body, FontId::new(13.5, Proportional)),
        (TextStyle::Button, FontId::new(13.5, Proportional)),
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Proportional)),
    ]
    .into();
    s.spacing.item_spacing = egui::vec2(8.0, 8.0);
    s.spacing.button_padding = egui::vec2(12.0, 6.0);
    s.spacing.interact_size.y = 30.0;
    s.spacing.window_margin = Margin::same(20);
    s.spacing.menu_margin = Margin::same(6);
    s.interaction.selectable_labels = false;
}
