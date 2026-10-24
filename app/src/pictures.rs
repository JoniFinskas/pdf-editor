use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, Tone, ToolWindow},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::{Picture, PlacedPicture, Placement};
pub struct Pictures {
    pub open: bool,
    dialog: crate::dialogs::ToolDialog<pdf_editor_core::Result<Picture>>,
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
            dialog: Default::default(),
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
        let mut request = None;
        if let Some(result) = self.dialog.poll(ctx, self.open, idle) {
            match result {
                Ok(image) => {
                    self.at.height = self.at.width * image.height as f32 / image.width as f32;
                    self.image = Some(image);
                    self.selected = None;
                    self.error.clear();
                }
                Err(error) => self.error = error.to_string(),
            }
        }
        let idle = idle && !self.dialog.busy();
        ToolWindow::new("Images", Icon::Image)
            .width(420.0)
            .show(ctx, p, &mut self.open, |ui| {
                self.dialog.feedback(ui, p);
                ui.horizontal(|ui| {
                    widgets::subheading(ui, p, "Images in this document");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Btn::icon(Icon::RotateRight)
                            .height(30.0)
                            .tip("Refresh images")
                            .enabled(idle)
                            .show(ui, p)
                            .clicked()
                        {
                            request = Some(Request::Pictures);
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if self.items.is_empty() {
                            widgets::hint(ui, p, "No inserted images or signatures found.");
                        }
                        for (i, item) in self.items.iter().enumerate() {
                            if widgets::list_row(
                                ui,
                                p,
                                self.selected == Some(i),
                                0.0,
                                Icon::Image,
                                &item.name,
                                &format!("Page {}", item.page + 1),
                            )
                            .clicked()
                            {
                                self.selected = Some(i);
                                self.at = item.placement;
                            }
                        }
                    });
                ui.add_space(4.0);
                widgets::rule(ui, p);
                widgets::subheading(ui, p, "Position and size");
                widgets::hint(
                    ui,
                    p,
                    "PDF points, from the bottom-left corner of the page.",
                );
                let mut page = usize::from(self.at.page) + 1;
                widgets::number(
                    ui,
                    p,
                    "Page",
                    self.selected.is_none(),
                    egui::DragValue::new(&mut page).range(1..=pages.max(1)),
                );
                self.at.page = (page - 1) as u16;
                widgets::numbers(
                    ui,
                    p,
                    vec![
                        ("Left", egui::DragValue::new(&mut self.at.left)),
                        ("Bottom", egui::DragValue::new(&mut self.at.bottom)),
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
                if !self.error.is_empty() {
                    widgets::notice(ui, p, Tone::Error, &self.error);
                }
                widgets::hint(
                    ui,
                    p,
                    "Refresh after changes to select an inserted image or signature.",
                );
                widgets::actions(ui, |ui| {
                    if let Some(item) = self.selected.and_then(|i| self.items.get(i)) {
                        if Btn::new("Move and resize")
                            .look(Look::Primary)
                            .enabled(idle)
                            .show(ui, p)
                            .clicked()
                        {
                            request = Some(Request::MovePicture(item.clone(), self.at));
                        }
                    } else if Btn::new("Insert image")
                        .look(Look::Primary)
                        .enabled(idle && self.image.is_some() && pages > 0)
                        .show(ui, p)
                        .clicked()
                        && let Some(image) = &self.image
                    {
                        request = Some(Request::PlacePicture(image.clone(), self.at));
                    }
                    if Btn::new("Import PNG or JPG…")
                        .with_icon(Icon::FilePlus)
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
            });
        if !self.open { self.dialog.cancel(); return None; }
        request
    }
}
