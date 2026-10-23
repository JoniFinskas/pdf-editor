use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use pdf_editor_core::{Picture, PlacedPicture, Placement};
pub struct Pictures {
    pub open: bool,
    pub items: Vec<PlacedPicture>,
    pub selected: Option<usize>,
    image: Option<Picture>,
    at: Placement,
    error: String,
}
impl Default for Pictures {
    fn default() -> Self {
        Self {
            open: false,
            items: vec![],
            selected: None,
            image: None,
            at: Placement {
                page: 0,
                left: 72.0,
                bottom: 72.0,
                width: 180.0,
                height: 120.0,
            },
            error: String::new(),
        }
    }
}
impl Pictures {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        pages: usize,
    ) -> Option<Request> {
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Images")
            .open(&mut self.open)
            .default_width(410.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if Btn::new("Import PNG / JPG…").show(ui, p).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg"])
                            .pick_file()
                    {
                        match Picture::open(&path) {
                            Ok(image) => {
                                self.at.height =
                                    self.at.width * image.height as f32 / image.width as f32;
                                self.image = Some(image);
                                self.selected = None;
                                self.error.clear();
                            }
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                    if Btn::new("Refresh images")
                        .enabled(idle)
                        .show(ui, p)
                        .clicked()
                    {
                        request = Some(Request::Pictures);
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for (i, item) in self.items.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.selected == Some(i),
                                    format!("Page {} · {}", item.page + 1, item.name),
                                )
                                .clicked()
                            {
                                self.selected = Some(i);
                                self.at = item.placement;
                            }
                        }
                    });
                ui.separator();
                ui.label("Position and size in PDF points, from the bottom-left corner");
                let mut page = usize::from(self.at.page) + 1;
                ui.horizontal(|ui| {
                    ui.label("Page");
                    ui.add_enabled(
                        self.selected.is_none(),
                        egui::DragValue::new(&mut page).range(1..=pages.max(1)),
                    );
                    self.at.page = (page - 1) as u16;
                });
                ui.horizontal(|ui| {
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
                if let Some(item) = self.selected.and_then(|i| self.items.get(i)) {
                    if Btn::new("Move / resize image")
                        .enabled(idle)
                        .show(ui, p)
                        .clicked()
                    {
                        request = Some(Request::MovePicture(item.clone(), self.at));
                    }
                } else if Btn::new("Insert image")
                    .enabled(idle && self.image.is_some() && pages > 0)
                    .show(ui, p)
                    .clicked()
                    && let Some(image) = &self.image
                {
                    request = Some(Request::PlacePicture(image.clone(), self.at));
                }
                ui.label("Refresh after changes to select an inserted image or signature.");
                if !self.error.is_empty() {
                    ui.label(&self.error);
                }
            });
        request
    }
}
