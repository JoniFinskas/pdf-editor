use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui::{self, pos2, vec2};
use pdf_editor_core::{Picture, ScanColor, ScanOptions};

#[derive(Default)]
pub struct ScanCleanup {
    pub open: bool,
    page: u16,
    loaded_page: Option<u16>,
    options: ScanOptions,
    texture: Option<egui::TextureHandle>,
    preview: Option<egui::TextureHandle>,
}
impl ScanCleanup {
    pub fn receive(&mut self, ctx: &egui::Context, page: u16, picture: Picture, corrected: bool) {
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [picture.width as usize, picture.height as usize],
            &picture.rgba,
        );
        let texture = ctx.load_texture("scan-preview", image, egui::TextureOptions::LINEAR);
        if corrected {
            self.preview = Some(texture);
        } else {
            self.options.corners = pdf_editor_core::detect_scan_corners(&picture)
                .unwrap_or(ScanOptions::default().corners);
            self.texture = Some(texture);
            self.preview = None;
            self.loaded_page = Some(page);
            self.page = page;
        }
    }
    pub fn invalidate(&mut self) {
        self.texture = None;
        self.preview = None;
        self.loaded_page = None;
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        pages: usize,
    ) -> Option<Request> {
        let mut request = None;
        let contents = |ui: &mut egui::Ui| {
            widgets::hint(
                ui,
                p,
                "For photo-only pages. Load a page, inspect the detected paper edges and drag the four corners clockwise. The corrected page replaces the photo in one undo step.",
            );
            ui.horizontal(|ui| {
                ui.label("Page");
                let mut number = self.page + 1;
                if ui
                    .add(egui::DragValue::new(&mut number).range(1..=pages.max(1) as u16))
                    .changed()
                {
                    self.page = number - 1;
                    self.loaded_page = None;
                    self.texture = None;
                    self.preview = None;
                }
                if Btn::new("Load / detect edges")
                    .enabled(idle && pages > 0)
                    .show(ui, p)
                    .clicked()
                {
                    request = Some(Request::ScanPreview(self.page, None));
                }
            });
            if let Some(texture) = &self.texture {
                let size = texture.size_vec2();
                let scale = (ui.available_width() / size.x).min(320.0 / size.y).min(1.0);
                let (rect, _) = ui.allocate_exact_size(size * scale, egui::Sense::hover());
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                let points = self
                    .options
                    .corners
                    .map(|c| rect.min + vec2(c[0] * rect.width(), c[1] * rect.height()));
                for i in 0..4 {
                    ui.painter().line_segment(
                        [points[i], points[(i + 1) % 4]],
                        egui::Stroke::new(2.0, p.accent_strong),
                    );
                    let handle = egui::Rect::from_center_size(points[i], vec2(20.0, 20.0));
                    let response = ui.interact(
                        handle,
                        egui::Id::new(("scan-corner", i)),
                        egui::Sense::drag(),
                    );
                    ui.painter().circle_filled(points[i], 6.0, p.accent_strong);
                    ui.painter().circle_stroke(
                        points[i],
                        6.0,
                        egui::Stroke::new(1.0, egui::Color32::WHITE),
                    );
                    if response.dragged()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        self.options.corners[i] = [
                            ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                            ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0),
                        ];
                        self.preview = None;
                    }
                    response.on_hover_cursor(egui::CursorIcon::Grab);
                }
            }
            ui.horizontal(|ui| {
                ui.label("Deskew");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.options.deskew_degrees)
                            .range(-20.0..=20.0)
                            .speed(0.1)
                            .suffix("°"),
                    )
                    .changed()
                {
                    self.preview = None;
                }
                for (value, label) in [
                    (ScanColor::Color, "Color"),
                    (ScanColor::Grayscale, "Grayscale"),
                    (ScanColor::BlackWhite, "Black & white"),
                ] {
                    if ui
                        .selectable_value(&mut self.options.color, value, label)
                        .changed()
                    {
                        self.preview = None;
                    }
                }
            });
            ui.checkbox(&mut self.options.ocr, "Run Windows OCR after cleanup");
            let loaded = self.loaded_page == Some(self.page);
            widgets::actions(ui, |ui| {
                if Btn::new("Preview correction")
                    .enabled(idle && loaded)
                    .show(ui, p)
                    .clicked()
                {
                    request = Some(Request::ScanPreview(self.page, Some(self.options.clone())));
                }
                if Btn::new("Apply cleanup")
                    .look(Look::Primary)
                    .enabled(idle && loaded)
                    .show(ui, p)
                    .clicked()
                {
                    request = Some(Request::CleanupScan(self.page, self.options.clone()));
                }
            });
            if let Some(texture) = &self.preview {
                let size = texture.size_vec2();
                let scale = (ui.available_width() / size.x).min(220.0 / size.y).min(1.0);
                ui.image((texture.id(), size * scale));
            }
            widgets::hint(
                ui,
                p,
                "Edge detection works best with light paper on a darker background. Correct corners manually when needed. Deskew uses the selected edges plus the angle adjustment.",
            );
        };
        ToolWindow::new("Scan cleanup", Icon::Scan)
            .width(640.0)
            .show(ctx, p, &mut self.open, contents);
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corner_handle_drag_updates_normalized_geometry() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = ScanCleanup {
            open: true,
            ..Default::default()
        };
        tool.receive(
            &ctx,
            0,
            Picture {
                width: 200,
                height: 160,
                rgba: vec![255; 200 * 160 * 4],
            },
            false,
        );
        tool.options.corners = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];
        let frame = |tool: &mut ScanCleanup, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(1000.0, 1000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let p = crate::theme::palette(ui);
                    tool.show(ui.ctx(), &p, true, 1);
                },
            );
            output.textures_delta.clear();
        };
        for _ in 0..3 {
            frame(&mut tool, vec![]);
        }
        let at = ctx
            .read_response(egui::Id::new(("scan-corner", 0usize)))
            .unwrap()
            .rect
            .center();
        frame(
            &mut tool,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let to = at + vec2(20.0, 10.0);
        frame(&mut tool, vec![egui::Event::PointerMoved(to)]);
        frame(
            &mut tool,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!((tool.options.corners[0][0] - 0.2).abs() < 0.01);
        assert!((tool.options.corners[0][1] - 0.1625).abs() < 0.01);
    }
}
