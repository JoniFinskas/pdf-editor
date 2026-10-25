use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::Bounds;
pub struct Crop {
    pub open: bool,
    page: u16,
    bounds: Bounds,
}
impl Default for Crop {
    fn default() -> Self {
        Self {
            open: false,
            page: 0,
            bounds: Bounds {
                left: 20.0,
                bottom: 20.0,
                right: 480.0,
                top: 280.0,
            },
        }
    }
}
impl Crop {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        pages: usize,
    ) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Crop page", Icon::Crop)
            .width(380.0)
            .show(ctx, p, &mut self.open, |ui| {
                let mut page = usize::from(self.page) + 1;
                widgets::number(
                    ui,
                    p,
                    "Page",
                    true,
                    egui::DragValue::new(&mut page).range(1..=pages.max(1)),
                );
                self.page = (page - 1) as u16;
                widgets::caption(ui, p, "PDF points · bottom-left origin");
                widgets::numbers(
                    ui,
                    p,
                    vec![
                        ("Left", egui::DragValue::new(&mut self.bounds.left)),
                        ("Bottom", egui::DragValue::new(&mut self.bounds.bottom)),
                        ("Right", egui::DragValue::new(&mut self.bounds.right)),
                        ("Top", egui::DragValue::new(&mut self.bounds.top)),
                    ],
                );
                widgets::hint(
                    ui,
                    p,
                    "Crop hides content. Use Redact to remove it.",
                );
                widgets::actions(ui, |ui| {
                    if Btn::new("Apply crop")
                        .look(Look::Primary)
                        .enabled(idle && pages > 0)
                        .show(ui, p)
                        .clicked()
                    {
                        request = Some(Request::Crop(self.page, self.bounds));
                    }
                });
            });
        request
    }
}
