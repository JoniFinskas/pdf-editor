use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui::{self, Color32, Sense, Stroke, Vec2};
use pdf_editor_core::{Picture, Placement};
use std::path::Path;

pub struct Signatures {
    pub open: bool,
    strokes: Vec<Vec<[f32; 2]>>,
    picture: Option<Picture>,
    preview: Option<egui::TextureHandle>,
    at: Placement,
    error: String,
}
impl Default for Signatures {
    fn default() -> Self {
        Self {
            open: false,
            strokes: vec![],
            picture: None,
            preview: None,
            at: Placement {
                page: 0,
                left: 72.0,
                bottom: 72.0,
                width: 180.0,
                height: 60.0,
            },
            error: String::new(),
        }
    }
}
impl Signatures {
    fn set_picture(&mut self, ctx: &egui::Context, picture: Picture) {
        self.at.height = self.at.width * picture.height as f32 / picture.width as f32;
        self.preview = Some(ctx.load_texture(
            "signature-preview",
            egui::ColorImage::from_rgba_unmultiplied(
                [picture.width as usize, picture.height as usize],
                &picture.rgba,
            ),
            egui::TextureOptions::LINEAR,
        ));
        self.picture = Some(picture);
        self.error.clear();
    }
    pub fn paint_placement(
        &self,
        painter: &egui::Painter,
        page: u16,
        origin: egui::Pos2,
        scale: f32,
        transform: crate::worker::PageTransform,
    ) {
        if !self.open || page != self.at.page {
            return;
        }
        if let Some(texture) = &self.preview {
            let a = transform.apply(self.at.left, self.at.bottom + self.at.height);
            let b = transform.apply(self.at.left + self.at.width, self.at.bottom);
            let rect = egui::Rect::from_two_pos(
                origin + Vec2::new(a.0, a.1) * scale,
                origin + Vec2::new(b.0, b.1) * scale,
            );
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            painter.rect_stroke(
                rect,
                0,
                Stroke::new(1.0, Color32::from_rgb(60, 120, 230)),
                egui::StrokeKind::Outside,
            );
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        folder: Option<&Path>,
        pages: usize,
        idle: bool,
    ) -> Option<Request> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut request = None;
        egui::Window::new("Signatures")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("Draw a signature, or import a PNG or JPG.");
                let (response, painter) =
                    ui.allocate_painter(Vec2::new(420.0, 140.0), Sense::drag());
                painter.rect_filled(response.rect, 6, Color32::WHITE);
                if response.drag_started() {
                    self.strokes.push(vec![]);
                }
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let local = (pos - response.rect.min) / response.rect.size();
                    if let Some(stroke) = self.strokes.last_mut() {
                        stroke.push([local.x.clamp(0.0, 1.0), local.y.clamp(0.0, 1.0)]);
                    }
                }
                for stroke in &self.strokes {
                    for points in stroke.windows(2) {
                        painter.line_segment(
                            [points[0], points[1]].map(|p| {
                                response.rect.min + Vec2::new(p[0], p[1]) * response.rect.size()
                            }),
                            Stroke::new(2.0, Color32::BLACK),
                        );
                    }
                }
                ui.horizontal(|ui| {
                    if Btn::new("Use drawing").show(ui, p).clicked() {
                        match pdf_editor_core::signature_image(&self.strokes) {
                            Ok(picture) => self.set_picture(ctx, picture),
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                    if Btn::new("Clear").show(ui, p).clicked() {
                        self.strokes.clear();
                    }
                    if Btn::new("Import…").show(ui, p).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg"])
                            .pick_file()
                    {
                        match Picture::open(&path) {
                            Ok(pic) => self.set_picture(ctx, pic),
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                });
                if let Some(folder) = folder {
                    ui.horizontal(|ui| {
                        if Btn::new("Save reusable signature")
                            .enabled(self.picture.is_some())
                            .show(ui, p)
                            .clicked()
                            && let Some(picture) = &self.picture
                        {
                            match pdf_editor_core::store_signature(folder, picture) {
                                Ok(_) => self.error = "Signature saved to the library.".into(),
                                Err(e) => self.error = e.to_string(),
                            }
                        }
                        egui::ComboBox::from_id_salt("signature-library")
                            .selected_text("Saved signatures")
                            .show_ui(ui, |ui| match pdf_editor_core::signature_files(folder) {
                                Ok(paths) => {
                                    for path in paths {
                                        if ui
                                            .selectable_label(
                                                false,
                                                path.file_stem()
                                                    .unwrap_or_default()
                                                    .to_string_lossy(),
                                            )
                                            .clicked()
                                        {
                                            match Picture::open(&path) {
                                                Ok(pic) => self.set_picture(ctx, pic),
                                                Err(e) => self.error = e.to_string(),
                                            }
                                        }
                                    }
                                }
                                Err(e) => self.error = e.to_string(),
                            });
                    });
                } else {
                    ui.label("A config folder is required to save reusable signatures.");
                }
                if let Some(texture) = &self.preview {
                    ui.image((texture.id(), Vec2::new(300.0, 100.0)));
                }
                ui.separator();
                ui.label("Placement in PDF points, measured from the bottom-left corner.");
                let mut page = usize::from(self.at.page) + 1;
                ui.horizontal(|ui| {
                    ui.label("Page");
                    ui.add(egui::DragValue::new(&mut page).range(1..=pages.max(1)));
                    self.at.page = (page - 1) as u16;
                    ui.label("Left");
                    ui.add(egui::DragValue::new(&mut self.at.left));
                    ui.label("Bottom");
                    ui.add(egui::DragValue::new(&mut self.at.bottom));
                });
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut self.at.width).range(1.0..=10000.0));
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut self.at.height).range(1.0..=10000.0));
                });
                if Btn::new("Place signature")
                    .enabled(idle && pages > 0 && self.picture.is_some())
                    .show(ui, p)
                    .clicked()
                    && let Some(picture) = &self.picture
                {
                    request = Some(Request::PlacePicture(picture.clone(), self.at));
                }
                ui.label("Signatures become page content. Undo removes the last placement.");
                if !self.error.is_empty() {
                    ui.label(&self.error);
                }
            });
        self.open = open;
        request
    }
}
