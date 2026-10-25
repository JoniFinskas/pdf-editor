use crate::{
    icons::Icon,
    theme::{self, Palette},
    widgets::{self, Btn, Look, Tone, ToolWindow},
    worker::Request,
};
use eframe::egui::{self, Color32, Sense, Stroke, Vec2};
use pdf_editor_core::{Picture, Placement};
use std::path::Path;

pub struct Signatures {
    pub open: bool,
    pub placing: bool,
    dialog: crate::dialogs::ToolDialog<pdf_editor_core::Result<Picture>>,
    strokes: Vec<Vec<[f32; 2]>>,
    picture: Option<Picture>,
    preview: Option<egui::TextureHandle>,
    at: Placement,
    note: Option<(Tone, String)>,
}
impl Default for Signatures {
    fn default() -> Self {
        Self {
            open: false,
            placing: false,
            dialog: Default::default(),
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
            note: None,
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
        self.note = None;
    }
    fn failed(&mut self, error: impl ToString) {
        self.note = Some((Tone::Error, error.to_string()));
    }
    pub fn paint_placement(
        &self,
        painter: &egui::Painter,
        page: u16,
        origin: egui::Pos2,
        scale: f32,
        transform: crate::worker::PageTransform,
    ) {
        if (!self.open && !self.placing) || page != self.at.page {
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
            let accent = theme::palette_of(painter.ctx()).accent;
            widgets::dashed_rect_on(painter, rect.expand(2.0), Stroke::new(1.5, accent));
        }
    }
    pub fn place_at_pointer(
        &mut self, response: &egui::Response, page: u16, origin: egui::Pos2,
        scale: f32, transform: crate::worker::PageTransform,
    ) -> Option<Request> {
        if !self.placing { return None; }
        if response.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.placing = false;
            return None;
        }
        let pointer = response.hover_pos()?;
        let at = (pointer-origin)/scale;
        let (x,y) = transform.inverse(at.x,at.y)?;
        self.at.page = page;
        self.at.left = x-self.at.width*0.5;
        self.at.bottom = y-self.at.height*0.5;
        response.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        response.ctx.request_repaint();
        if response.clicked() {
            self.placing = false;
            return self.picture.clone().map(|picture| Request::PlacePicture(picture,self.at));
        }
        None
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        folder: Option<&Path>,
        pages: usize,
        idle: bool,
    ) -> Option<Request> {
        if let Some(result) = self.dialog.poll(ctx, self.open, idle) {
            match result {
                Ok(pic) => self.set_picture(ctx, pic),
                Err(error) => self.failed(error),
            }
        }
        let idle = idle && !self.dialog.busy();
        let mut start_placing = false;
        let mut open = self.open;
        ToolWindow::new("Signatures", Icon::Signature)
            .width(440.0)
            .show(ctx, p, &mut open, |ui| {
                self.dialog.feedback(ui, p);
                widgets::hint(ui, p, "Draw here or import an image.");
                let size = Vec2::new(ui.available_width(), 140.0);
                let (response, painter) = ui.allocate_painter(size, Sense::drag());
                let pad = response.rect;
                // The pad stays white in both themes, like paper.
                painter.rect(
                    pad,
                    theme::RADIUS,
                    Color32::WHITE,
                    Stroke::new(1.0, p.border_strong),
                    egui::StrokeKind::Inside,
                );
                let base = pad.bottom() - 32.0;
                painter.hline(
                    pad.x_range().shrink(24.0),
                    base,
                    Stroke::new(1.0, Color32::from_gray(200)),
                );
                if self.strokes.is_empty() {
                    painter.text(
                        egui::pos2(pad.left() + 24.0, base + 6.0),
                        egui::Align2::LEFT_TOP,
                        "Sign above the line",
                        egui::FontId::proportional(12.0),
                        Color32::from_gray(150),
                    );
                }
                if response.drag_started() {
                    self.strokes.push(vec![]);
                }
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let local = (pos - pad.min) / pad.size();
                    if let Some(stroke) = self.strokes.last_mut() {
                        stroke.push([local.x.clamp(0.0, 1.0), local.y.clamp(0.0, 1.0)]);
                    }
                }
                for stroke in &self.strokes {
                    for points in stroke.windows(2) {
                        painter.line_segment(
                            [points[0], points[1]]
                                .map(|p| pad.min + Vec2::new(p[0], p[1]) * pad.size()),
                            Stroke::new(2.0, Color32::BLACK),
                        );
                    }
                }
                ui.horizontal(|ui| {
                    if Btn::new("Use drawing")
                        .look(Look::OutlineAccent)
                        .show(ui, p)
                        .clicked()
                    {
                        match pdf_editor_core::signature_image(&self.strokes) {
                            Ok(picture) => self.set_picture(ctx, picture),
                            Err(e) => self.failed(e),
                        }
                    }
                    if Btn::new("Clear").look(Look::Outline).show(ui, p).clicked() {
                        self.strokes.clear();
                    }
                    if Btn::new("Import…")
                        .with_icon(Icon::Image)
                        .look(Look::Outline)
                        .enabled(idle)
                        .show(ui, p)
                        .clicked()
                    {
                        self.dialog.launch(|| {
                            rfd::FileDialog::new()
                                .add_filter("Images", &["png", "jpg", "jpeg"])
                                .pick_file()
                                .map(|path| Picture::open(&path))
                        });
                    }
                });
                widgets::subheading(ui, p, "Saved signatures");
                if let Some(folder) = folder {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("signature-library")
                            .selected_text("Choose a saved signature")
                            .width(220.0)
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
                                                Err(e) => self.failed(e),
                                            }
                                        }
                                    }
                                }
                                Err(e) => self.failed(e),
                            });
                        if Btn::new("Save for reuse")
                            .with_icon(Icon::Save)
                            .look(Look::Outline)
                            .enabled(self.picture.is_some())
                            .show(ui, p)
                            .clicked()
                            && let Some(picture) = &self.picture
                        {
                            self.note =
                                Some(match pdf_editor_core::store_signature(folder, picture) {
                                    Ok(_) => (Tone::Ok, "Signature saved to the library.".into()),
                                    Err(e) => (Tone::Error, e.to_string()),
                                });
                        }
                    });
                } else {
                    widgets::hint(
                        ui,
                        p,
                        "A config folder is needed to save signatures.",
                    );
                }
                if let Some(texture) = &self.preview {
                    widgets::subheading(ui, p, "Preview");
                    egui::Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.0, p.border))
                        .corner_radius(theme::RADIUS)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.image((texture.id(), Vec2::new(240.0, 80.0)));
                        });
                }
                widgets::subheading(ui, p, "Placement");
                widgets::hint(
                    ui,
                    p,
                    "PDF points · bottom-left origin",
                );
                let mut page = usize::from(self.at.page) + 1;
                widgets::numbers(
                    ui,
                    p,
                    vec![
                        (
                            "Page",
                            egui::DragValue::new(&mut page).range(1..=pages.max(1)),
                        ),
                        ("Left", egui::DragValue::new(&mut self.at.left)),
                        ("Bottom", egui::DragValue::new(&mut self.at.bottom)),
                    ],
                );
                self.at.page = (page - 1) as u16;
                widgets::numbers(
                    ui,
                    p,
                    vec![
                        (
                            "Width",
                            egui::DragValue::new(&mut self.at.width).range(1.0..=10000.0),
                        ),
                        (
                            "Height",
                            egui::DragValue::new(&mut self.at.height).range(1.0..=10000.0),
                        ),
                    ],
                );
                if let Some((tone, text)) = &self.note
                    && widgets::dismissible_notice(ui, p, *tone, text, "dismiss-signature-note") {
                    self.note = None;
                }
                widgets::hint(
                    ui,
                    p,
                    "Click the page to place. Esc cancels.",
                );
                widgets::actions(ui, |ui| {
                    if Btn::new("Place signature")
                        .look(Look::Primary)
                        .enabled(idle && pages > 0 && self.picture.is_some())
                        .show(ui, p)
                        .clicked()
                        && let Some(picture) = &self.picture
                    {
                        let _ = picture;
                        start_placing = true;
                    }
                });
            });
        self.open = open && !start_placing;
        if start_placing { self.placing = true; }
        if !self.open { self.dialog.cancel(); }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signature_preview_tracks_pointer_and_places_once_on_the_chosen_page() {
        let ctx=egui::Context::default();
        let mut signatures=Signatures::default();
        signatures.set_picture(&ctx,Picture {width:2,height:1,rgba:vec![255;8]});
        signatures.placing=true;
        let transform=crate::worker::PageTransform::for_test([1.0,0.0,0.0,-1.0,0.0,400.0]);
        let mut requests=vec![];
        let at=egui::pos2(100.0,120.0);
        for events in [vec![egui::Event::PointerMoved(at)],vec![egui::Event::PointerButton {
            pos:at,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()
        }],vec![egui::Event::PointerButton {
            pos:at,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()
        }],vec![]] {
            let mut output=ctx.run_ui(egui::RawInput {events,
                screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(600.0,500.0))),
                ..Default::default()},|ui| {
                let response=ui.interact(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(300.0,400.0)),
                    egui::Id::new("signature-page-test"),Sense::click());
                if let Some(request)=signatures.place_at_pointer(&response,2,egui::Pos2::ZERO,1.0,transform) {
                    requests.push(request);
                }
            });
            output.textures_delta.clear();
        }
        assert_eq!(requests.len(),1);
        let Request::PlacePicture(_,placement)=requests.remove(0) else {panic!()};
        assert_eq!(placement.page,2);
        assert_eq!(placement.left+placement.width*0.5,100.0);
        assert_eq!(placement.bottom+placement.height*0.5,280.0);
        assert!(!signatures.placing);
    }
}
