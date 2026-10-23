//! Small painted controls shared by the toolbars, the home page and the
//! settings page and the tool windows: icon buttons, keycaps, dividers,
//! settings rows and form fields.

use crate::icons::{self, Icon};
use crate::theme::{self, Palette};
use eframe::egui::{
    Align, Align2, Color32, Context, CornerRadius, CursorIcon, DragValue, FontId, Frame, Id, Label,
    Layout, Margin, Painter, Rect, Response, Sense, Shadow, Shape, Slider, Stroke, StrokeKind,
    TextEdit, TextFormat, Ui, WidgetInfo, WidgetType, Window, pos2,
    text::{LayoutJob, TextWrapping},
    vec2,
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
            // Filled buttons fade to a neutral fill while disabled, so the
            // label stays readable.
            let fill = if !enabled && matches!(self.look, Look::Primary | Look::Danger) {
                p.hover
            } else {
                fill
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
    dashed_rect_on(ui.painter(), rect, stroke);
}

/// [`dashed_rect`] on a given painter.
pub fn dashed_rect_on(painter: &Painter, rect: Rect, stroke: Stroke) {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(Shape::dashed_line(&corners, stroke, 5.0, 4.0));
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

/// Floating, movable tool window in the menu style: a title row with an
/// icon and a close button above the tool's own content.
pub struct ToolWindow<'a> {
    title: &'a str,
    icon: Icon,
    width: f32,
    centered: bool,
}

impl<'a> ToolWindow<'a> {
    pub fn new(title: &'a str, icon: Icon) -> Self {
        Self {
            title,
            icon,
            width: 400.0,
            centered: false,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Opens in the middle of the window instead of beside the page.
    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }

    /// Shows the window while `open` is set; the close button clears it.
    pub fn show(self, ctx: &Context, p: &Palette, open: &mut bool, add: impl FnOnce(&mut Ui)) {
        if !*open {
            return;
        }
        let screen = ctx.content_rect();
        let (pivot, at) = if self.centered {
            (Align2::CENTER_CENTER, screen.center())
        } else {
            // Below the top bars and the find bar, beside the page.
            (
                Align2::RIGHT_TOP,
                pos2(screen.right() - 24.0, screen.top() + 172.0),
            )
        };
        let frame = Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border_strong))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(18))
            .shadow(Shadow {
                offset: [0, 10],
                blur: 32,
                spread: 0,
                color: p.page_shadow,
            });
        let mut close = false;
        Window::new(self.title)
            .id(Id::new(("tool-window", self.title)))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .pivot(pivot)
            .default_pos(at)
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(self.width);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    icon(ui, self.icon, 20.0, p.accent_strong);
                    ui.add_space(2.0);
                    ui.label(egui_text(self.title, theme::semibold(16.0), p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if Btn::icon(Icon::Close)
                            .height(28.0)
                            .tip("Close")
                            .show(ui, p)
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(6.0);
                rule(ui, p);
                ui.add_space(8.0);
                add(ui);
            });
        if close {
            *open = false;
        }
    }
}

/// Small muted label above a field.
pub fn caption(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(egui_text(text, FontId::proportional(12.5), p.muted));
}

/// Muted explanatory text that wraps to the available width.
pub fn hint(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add(Label::new(egui_text(text, FontId::proportional(12.5), p.muted)).wrap());
}

/// Group title inside a tool window.
pub fn subheading(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(4.0);
    ui.label(egui_text(text, theme::semibold(14.0), p.text));
}

/// Caption above a control.
pub fn field<R>(ui: &mut Ui, p: &Palette, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        caption(ui, p, label);
        add(ui)
    })
    .inner
}

/// Width of one column when a row is split into four, so number fields
/// line up across rows.
pub fn column_width(ui: &Ui) -> f32 {
    ((ui.available_width() - 3.0 * COLUMN_GAP) / 4.0).floor()
}

const COLUMN_GAP: f32 = 10.0;

/// Number field with a caption, one column wide.
pub fn number(ui: &mut Ui, p: &Palette, label: &str, enabled: bool, value: DragValue<'_>) {
    let width = column_width(ui);
    field(ui, p, label, |ui| {
        ui.add_enabled(enabled, |ui: &mut Ui| ui.add_sized([width, 32.0], value))
    });
}

/// Row of captioned number fields, such as page bounds in points.
pub fn numbers(ui: &mut Ui, p: &Palette, fields: Vec<(&str, DragValue<'_>)>) {
    let width = column_width(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = COLUMN_GAP;
        for (label, value) in fields {
            field(ui, p, label, |ui| ui.add_sized([width, 32.0], value));
        }
    });
}

/// Captioned slider that fills the row.
pub fn slider(ui: &mut Ui, p: &Palette, label: &str, slider: Slider<'_>) {
    field(ui, p, label, |ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 82.0).max(80.0);
        // The rail uses the field fill, which matches the window in light mode.
        ui.visuals_mut().widgets.inactive.bg_fill = p.border;
        ui.add(slider.trailing_fill(true))
    });
}

/// Single-line text field with the same height as the number fields.
pub fn text_field(text: &mut String, width: f32) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .desired_width(width)
        .margin(Margin::symmetric(10, 7))
}

/// Checkbox with a square box; the shared widget radius would make it round.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: &str) -> Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            state.corner_radius = CornerRadius::same(4);
        }
        ui.checkbox(checked, text)
    })
    .inner
}

/// Tone of a [`notice`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Warn,
    Error,
    Ok,
}

/// Tinted message box inside a tool window.
pub fn notice(ui: &mut Ui, p: &Palette, tone: Tone, text: &str) {
    let (bg, fg) = match tone {
        Tone::Warn => (p.warn_bg, p.warn_fg),
        Tone::Error => (p.error_bg, p.error_fg),
        Tone::Ok => (p.ok_bg, p.ok_fg),
    };
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                icon(ui, Icon::Info, 16.0, fg);
                ui.add(Label::new(egui_text(text, FontId::proportional(13.0), fg)).wrap());
            });
        });
}

/// Row of buttons aligned to the right edge; add the main action first.
pub fn actions(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 34.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            add(ui);
        },
    );
}

/// Selectable list row with an icon, a title and muted detail on the right.
pub fn list_row(
    ui: &mut Ui,
    p: &Palette,
    selected: bool,
    indent: f32,
    kind: Icon,
    title: &str,
    detail: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, title));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS - 2);
        if selected {
            painter.rect_filled(rect, radius, p.accent_soft);
        } else if response.hovered() || response.has_focus() {
            painter.rect_filled(rect, radius, p.hover);
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                radius,
                Stroke::new(1.5, p.accent_strong),
                StrokeKind::Inside,
            );
        }
        let fg = if selected { p.accent_strong } else { p.text };
        let left = rect.left() + 8.0 + indent;
        icons::paint(
            painter,
            Rect::from_center_size(pos2(left + 8.0, rect.center().y), vec2(16.0, 16.0)),
            kind,
            if selected { p.accent_strong } else { p.muted },
        );
        let detail =
            painter.layout_no_wrap(detail.to_string(), FontId::proportional(12.5), p.muted);
        let room = (rect.right() - 18.0 - detail.size().x - (left + 24.0)).max(20.0);
        let mut job = LayoutJob::single_section(
            title.to_string(),
            TextFormat::simple(FontId::proportional(14.0), fg),
        );
        job.wrap = TextWrapping::truncate_at_width(room);
        let title = painter.layout_job(job);
        painter.galley(
            pos2(left + 24.0, rect.center().y - title.size().y / 2.0),
            title,
            fg,
        );
        painter.galley(
            pos2(
                rect.right() - 10.0 - detail.size().x,
                rect.center().y - detail.size().y / 2.0,
            ),
            detail,
            p.muted,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}
