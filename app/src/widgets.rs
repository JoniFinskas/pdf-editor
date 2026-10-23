//! Small painted controls shared by the toolbars, the home page and the
//! settings page: icon buttons, keycaps, dividers and settings rows.

use crate::icons::{self, Icon};
use crate::theme::{self, Palette};
use eframe::egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, WidgetInfo, WidgetType, pos2, vec2,
};

/// Visual weight of a [`Btn`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// No frame until hovered; toolbar default.
    Ghost,
    /// Tinted with the accent; a toggled tool such as an open find bar.
    Active,
    Outline,
    OutlineAccent,
    Primary,
    Danger,
}

/// Button with an optional painted icon and an optional label.
pub struct Btn<'a> {
    id: Option<Id>,
    icon: Option<Icon>,
    label: &'a str,
    look: Look,
    enabled: bool,
    tip: &'a str,
    height: f32,
    min_width: f32,
    align_left: bool,
    trailing: Option<Icon>,
}

impl<'a> Btn<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            id: None,
            icon: None,
            label,
            look: Look::Ghost,
            enabled: true,
            tip: "",
            height: 34.0,
            min_width: 0.0,
            align_left: false,
            trailing: None,
        }
    }

    pub fn icon(icon: Icon) -> Self {
        Self::new("").with_icon(icon)
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Stable id, so tests and popups can find the button.
    pub fn id(mut self, key: &str) -> Self {
        self.id = Some(tool_id(key));
        self
    }

    pub fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = tip;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }

    /// Aligns the content to the left edge, for menu and navigation rows.
    pub fn align_left(mut self) -> Self {
        self.align_left = true;
        self
    }

    /// Small icon after the label, such as a dropdown chevron.
    pub fn trailing(mut self, icon: Icon) -> Self {
        self.trailing = Some(icon);
        self
    }

    pub fn show(self, ui: &mut Ui, p: &Palette) -> Response {
        let enabled = self.enabled && ui.is_enabled();
        let icon_side = (self.height * 0.53).round().clamp(14.0, 20.0);
        let galley = (!self.label.is_empty()).then(|| {
            ui.painter().layout_no_wrap(
                self.label.to_string(),
                FontId::proportional(14.0),
                Color32::PLACEHOLDER,
            )
        });
        let pad = if galley.is_some() { 12.0 } else { 0.0 };
        let mut width = 2.0 * pad;
        if self.icon.is_some() {
            width += icon_side;
        }
        if let Some(galley) = &galley {
            width += galley.size().x;
            if self.icon.is_some() {
                width += 8.0;
            }
        }
        if self.trailing.is_some() {
            width += TRAILING + 6.0;
        }
        let width = width.max(self.height).max(self.min_width);
        let (auto_id, rect) = ui.allocate_space(vec2(width, self.height));
        let id = self.id.unwrap_or(auto_id);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, id, sense);
        let label = if self.label.is_empty() {
            self.tip
        } else {
            self.label
        };
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));

        if ui.is_rect_visible(rect) {
            let hot = enabled && (response.hovered() || response.has_focus());
            let pressed = enabled && response.is_pointer_button_down_on();
            let (fill, stroke, fg) = match self.look {
                Look::Ghost => (
                    if pressed || hot {
                        p.hover
                    } else {
                        Color32::TRANSPARENT
                    },
                    Color32::TRANSPARENT,
                    p.text,
                ),
                Look::Active => (p.accent_soft, Color32::TRANSPARENT, p.accent_strong),
                Look::Outline => (
                    if hot { p.hover } else { Color32::TRANSPARENT },
                    p.border_strong,
                    p.text,
                ),
                Look::OutlineAccent => (
                    if hot {
                        p.accent_soft
                    } else {
                        Color32::TRANSPARENT
                    },
                    p.accent,
                    p.accent_strong,
                ),
                Look::Primary => (
                    if hot { p.accent_strong } else { p.accent },
                    Color32::TRANSPARENT,
                    p.accent_text,
                ),
                Look::Danger => (
                    if hot {
                        p.danger.gamma_multiply(0.85)
                    } else {
                        p.danger
                    },
                    Color32::TRANSPARENT,
                    Color32::WHITE,
                ),
            };
            let fg = if enabled {
                fg
            } else {
                p.muted.gamma_multiply(0.6)
            };
            let radius = CornerRadius::same(theme::RADIUS);
            let painter = ui.painter();
            painter.rect(
                rect,
                radius,
                fill,
                Stroke::new(1.0, stroke),
                StrokeKind::Inside,
            );
            if response.has_focus() {
                painter.rect_stroke(
                    rect.expand(1.0),
                    radius,
                    Stroke::new(1.5, p.accent_strong),
                    StrokeKind::Outside,
                );
            }
            let icon_width = if self.icon.is_some() { icon_side } else { 0.0 };
            let gap = if self.icon.is_some() && galley.is_some() {
                8.0
            } else {
                0.0
            };
            let trailing = if self.trailing.is_some() {
                TRAILING + 6.0
            } else {
                0.0
            };
            let content = icon_width + gap + galley.as_ref().map_or(0.0, |g| g.size().x) + trailing;
            let mut x = if self.align_left {
                rect.left() + 10.0
            } else {
                rect.center().x - content / 2.0
            };
            if let Some(icon) = self.icon {
                let at = Rect::from_min_size(
                    pos2(x, rect.center().y - icon_side / 2.0),
                    vec2(icon_side, icon_side),
                );
                icons::paint(painter, at, icon, fg);
                x += icon_side + gap;
            }
            if let Some(galley) = galley {
                let y = rect.center().y - galley.size().y / 2.0;
                x += galley.size().x;
                painter.galley(pos2(x - galley.size().x, y), galley, fg);
            }
            if let Some(icon) = self.trailing {
                let at = Rect::from_min_size(
                    pos2(x + 6.0, rect.center().y - TRAILING / 2.0),
                    vec2(TRAILING, TRAILING),
                );
                icons::paint(painter, at, icon, fg);
            }
            if enabled {
                response.clone().on_hover_cursor(CursorIcon::PointingHand);
            }
        }
        if self.tip.is_empty() {
            response
        } else {
            response
                .on_hover_text(self.tip)
                .on_disabled_hover_text(self.tip)
        }
    }
}

/// Side of a trailing icon.
const TRAILING: f32 = 14.0;

/// Id of a [`Btn`] given a stable key.
pub fn tool_id(key: &str) -> Id {
    Id::new(("tool", key))
}

/// Short vertical rule between toolbar groups.
pub fn divider(ui: &mut Ui, p: &Palette, height: f32) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, p.border_strong);
    ui.add_space(6.0);
}

/// Full-width horizontal rule.
pub fn rule(ui: &mut Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.border);
}

/// Keyboard key drawn as a small bordered cap.
pub fn keycap(ui: &mut Ui, p: &Palette, key: &str) {
    let font = FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(key.to_string(), font, p.muted);
    let size = vec2((galley.size().x + 14.0).max(24.0), 24.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(5),
        p.field,
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, p.muted);
}

/// Key combination such as Ctrl + O, as keycaps joined by plus signs.
pub fn keys(ui: &mut Ui, p: &Palette, keys: &[&str]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = KEY_GAP;
        for (index, key) in keys.iter().enumerate() {
            if index > 0 {
                ui.label(egui_text("+", FontId::proportional(13.0), p.muted));
            }
            keycap(ui, p, key);
        }
    });
}

/// Width taken by [`keys`], for centering.
pub fn keys_width(ui: &Ui, keys: &[&str]) -> f32 {
    let measure = |text: &str, size: f32| {
        ui.painter()
            .layout_no_wrap(text.to_string(), FontId::proportional(size), Color32::WHITE)
            .size()
            .x
    };
    let caps: f32 = keys
        .iter()
        .map(|k| (measure(k, 12.0) + 14.0).max(24.0))
        .sum();
    let joins = keys.len().saturating_sub(1) as f32;
    caps + joins * (measure("+", 13.0) + 2.0 * KEY_GAP)
}

const KEY_GAP: f32 = 6.0;

/// Paints a dashed outline along `rect`.
pub fn dashed_rect(ui: &Ui, rect: Rect, stroke: Stroke) {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    ui.painter()
        .extend(Shape::dashed_line(&corners, stroke, 5.0, 4.0));
}

/// Small filled dot, as in the "Edited" marker.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// Icon at a fixed size that takes part in layout.
pub fn icon(ui: &mut Ui, icon: Icon, side: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    icons::paint(ui.painter(), rect, icon, color);
    response
}

/// Settings row: title and description on the left, `control` on the right.
pub fn setting_row(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    detail: &str,
    control: impl FnOnce(&mut Ui),
) {
    let width = ui.available_width();
    let label_width = (width * 0.55).clamp(200.0, 420.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(label_width);
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(egui_text(title, theme::semibold(14.5), p.text));
            if !detail.is_empty() {
                ui.label(egui_text(detail, FontId::proportional(12.5), p.muted));
            }
        });
        ui.add_space(16.0);
        ui.vertical(|ui| control(ui));
    });
    ui.add_space(14.0);
}

/// Segmented control with one selected option.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    p: &Palette,
    key: &str,
    value: &mut T,
    options: &[(T, &str)],
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let font = FontId::proportional(14.0);
        let (outer_id, start) = (Id::new(("segmented", key)), ui.cursor().min);
        let mut rects = Vec::new();
        for (index, (option, label)) in options.iter().enumerate() {
            let galley = ui
                .painter()
                .layout_no_wrap(label.to_string(), font.clone(), p.text);
            let size = vec2((galley.size().x + 36.0).max(96.0), 40.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let response = ui.interact(rect, outer_id.with(index), Sense::click());
            let selected = *value == *option;
            response.widget_info(|| {
                WidgetInfo::selected(WidgetType::RadioButton, true, selected, *label)
            });
            if response.clicked() {
                *value = *option;
            }
            let painter = ui.painter();
            if selected {
                painter.rect(
                    rect.shrink(1.0),
                    CornerRadius::same(theme::RADIUS - 2),
                    p.accent_soft,
                    Stroke::new(1.0, p.accent),
                    StrokeKind::Inside,
                );
            } else if response.hovered() || response.has_focus() {
                painter.rect_filled(
                    rect.shrink(1.0),
                    CornerRadius::same(theme::RADIUS - 2),
                    p.hover,
                );
            }
            if response.has_focus() {
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(theme::RADIUS),
                    Stroke::new(1.5, p.accent_strong),
                    StrokeKind::Inside,
                );
            }
            let color = if selected { p.text } else { p.muted };
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                *label,
                font.clone(),
                color,
            );
            response.on_hover_cursor(CursorIcon::PointingHand);
            rects.push(rect);
        }
        let outer = rects
            .iter()
            .fold(Rect::from_min_size(start, vec2(0.0, 0.0)), |a, r| {
                a.union(*r)
            });
        let painter = ui.painter();
        painter.rect_stroke(
            outer,
            CornerRadius::same(theme::RADIUS),
            Stroke::new(1.0, p.border_strong),
            StrokeKind::Inside,
        );
        for rect in rects.iter().skip(1) {
            painter.vline(
                rect.left(),
                outer.y_range().shrink(1.0),
                Stroke::new(1.0, p.border_strong),
            );
        }
    });
}

fn egui_text(text: &str, font: FontId, color: Color32) -> eframe::egui::RichText {
    eframe::egui::RichText::new(text).font(font).color(color)
}
