use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::{PageTransform, Request},
};
use eframe::egui::{self, Color32, Stroke, Vec2};
use pdf_editor_core::{Drawing, DrawingKind};
pub struct DrawingTool {
    pub open: bool,
    pub active: bool,
    kind: DrawingKind,
    color: [u8; 3],
    width: f32,
    opacity: f32,
    points: Vec<[f32; 2]>,
    page: Option<u16>,
}
impl Default for DrawingTool {
    fn default() -> Self {
        Self {
            open: false,
            active: false,
            kind: DrawingKind::Freehand,
            color: [40, 100, 220],
            width: 2.0,
            opacity: 1.0,
            points: vec![],
            page: None,
        }
    }
}
impl DrawingTool {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette) {
        if !self.open {
            self.active = false;
            self.points.clear();
            return;
        }
        ToolWindow::new("Draw and shapes", Icon::Pen)
            .width(340.0)
            .show(ctx, p, &mut self.open, |ui| {
                widgets::field(ui, p, "Shape", |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (kind, icon, tip) in [
                            (DrawingKind::Freehand, Icon::Pen, "Freehand"),
                            (DrawingKind::Rectangle, Icon::Square, "Rectangle"),
                            (DrawingKind::Ellipse, Icon::Circle, "Ellipse"),
                            (DrawingKind::Line, Icon::Line, "Line"),
                            (DrawingKind::Arrow, Icon::ArrowDiagonal, "Arrow"),
                        ] {
                            if Btn::icon(icon)
                                .look(if self.kind == kind {
                                    Look::Active
                                } else {
                                    Look::Outline
                                })
                                .height(36.0)
                                .tip(tip)
                                .show(ui, p)
                                .clicked()
                            {
                                self.kind = kind;
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    widgets::field(ui, p, "Color", |ui| {
                        ui.color_edit_button_srgb(&mut self.color)
                    });
                    widgets::number(
                        ui,
                        p,
                        "Width",
                        true,
                        egui::DragValue::new(&mut self.width).range(0.1..=100.0),
                    );
                });
                widgets::slider(
                    ui,
                    p,
                    "Opacity",
                    egui::Slider::new(&mut self.opacity, 0.0..=1.0),
                );
                widgets::hint(
                    ui,
                    p,
                    "Drag on a page to draw. Each stroke or shape becomes a PDF annotation and one undo step.",
                );
                widgets::actions(ui, |ui| {
                    if Btn::new(if self.active {
                        "Finish drawing"
                    } else {
                        "Draw on page"
                    })
                    .look(if self.active {
                        Look::OutlineAccent
                    } else {
                        Look::Primary
                    })
                    .show(ui, p)
                    .clicked()
                    {
                        self.active = !self.active;
                        self.points.clear();
                    }
                });
            });
    }
    pub fn on_page(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        page: u16,
        rect: egui::Rect,
        scale: f32,
        transform: PageTransform,
    ) -> Option<Request> {
        if !self.active {
            return None;
        }
        if response.drag_started() {
            self.page = Some(page);
            self.points.clear();
        }
        if self.page != Some(page) {
            return None;
        }
        if (response.dragged() || response.drag_stopped())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let pos = rect.clamp(pos);
            let local = (pos - rect.min) / scale;
            if let Some((x, y)) = transform.inverse(local.x, local.y) {
                if self.kind == DrawingKind::Freehand || self.points.len() < 2 {
                    if self.points.len() < 10000 {
                        self.points.push([x, y]);
                    }
                } else {
                    self.points[1] = [x, y];
                }
            }
        }
        let points: Vec<_> = self
            .points
            .iter()
            .map(|p| {
                let (x, y) = transform.apply(p[0], p[1]);
                rect.min + Vec2::new(x, y) * scale
            })
            .collect();
        let stroke = Stroke::new(
            self.width * scale,
            Color32::from_rgb(self.color[0], self.color[1], self.color[2]),
        );
        if let (Some(first), Some(last)) = (points.first(), points.last()) {
            let bounds = egui::Rect::from_two_pos(*first, *last);
            match self.kind {
                DrawingKind::Freehand => {
                    painter.add(egui::Shape::line(points, stroke));
                }
                DrawingKind::Rectangle => {
                    painter.rect_stroke(bounds, 0, stroke, egui::StrokeKind::Middle);
                }
                DrawingKind::Ellipse => {
                    painter.add(egui::Shape::ellipse_stroke(
                        bounds.center(),
                        bounds.size() / 2.0,
                        stroke,
                    ));
                }
                DrawingKind::Line => {
                    painter.line_segment([*first, *last], stroke);
                }
                DrawingKind::Arrow => {
                    painter.arrow(*first, *last - *first, stroke);
                }
            }
        }
        if response.drag_stopped() && self.points.len() >= 2 {
            self.page = None;
            return Some(Request::Draw(Drawing {
                page,
                kind: self.kind,
                points: std::mem::take(&mut self.points),
                color: self.color,
                width: self.width,
                opacity: self.opacity,
            }));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_drag_dispatches_a_real_shape_request() {
        let ctx = egui::Context::default();
        let mut tool = DrawingTool {
            active: true,
            kind: DrawingKind::Rectangle,
            ..Default::default()
        };
        let mut request = None;
        let mut warmup = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(400.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.allocate_exact_size(Vec2::new(300.0, 200.0), egui::Sense::click_and_drag());
            },
        );
        warmup.textures_delta.clear();
        for (position, pressed) in [
            (egui::pos2(40.0, 40.0), Some(true)),
            (egui::pos2(100.0, 80.0), None),
            (egui::pos2(160.0, 120.0), None),
            (egui::pos2(160.0, 120.0), Some(false)),
        ] {
            let mut events = vec![egui::Event::PointerMoved(position)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(300.0, 200.0),
                        egui::Sense::click_and_drag(),
                    );
                    if let Some(r) = tool.on_page(
                        &response,
                        ui.painter(),
                        0,
                        rect,
                        1.0,
                        PageTransform::identity(),
                    ) {
                        request = Some(r);
                    }
                },
            );
            output.textures_delta.clear();
        }
        let Some(Request::Draw(drawing)) = request else {
            panic!("Drawing request missing")
        };
        assert_eq!(drawing.kind, DrawingKind::Rectangle);
        assert_eq!(drawing.points.len(), 2);
        assert_eq!(drawing.page, 0);
    }
}
