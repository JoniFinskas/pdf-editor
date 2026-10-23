use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, Tone, ToolWindow},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::Bounds;
pub struct Redaction {
    pub open: bool,
    pub page: u16,
    pub bounds: Bounds,
    acknowledged: bool,
}
impl Default for Redaction {
    fn default() -> Self {
        Self {
            open: false,
            page: 0,
            bounds: Bounds {
                left: 72.0,
                bottom: 72.0,
                right: 220.0,
                top: 100.0,
            },
            acknowledged: false,
        }
    }
}
impl Redaction {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        pages: usize,
    ) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Secure redaction", Icon::Redact)
            .width(420.0)
            .show(ctx, p, &mut self.open, |ui| {
                widgets::hint(
                    ui,
                    p,
                    "Remove intersecting text, images and vector objects, then draw a black box.",
                );
                widgets::notice(
                    ui,
                    p,
                    Tone::Warn,
                    "Whole intersecting text runs, images and groups are removed, including portions outside the box. Other page text stays editable. Forms, annotations, bookmarks, attachments and metadata are removed from the document.",
                );
                let mut page = usize::from(self.page) + 1;
                widgets::number(
                    ui,
                    p,
                    "Page",
                    true,
                    egui::DragValue::new(&mut page).range(1..=pages.max(1)),
                );
                self.page = (page - 1) as u16;
                widgets::caption(ui, p, "Box in PDF points, from the bottom-left corner");
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
                ui.add_space(2.0);
                widgets::checkbox(ui,
                    &mut self.acknowledged,
                    "Remove intersecting objects and document extras",
                );
                widgets::hint(
                    ui,
                    p,
                    "Undo is available in this session. Save As creates the sanitized file; the original remains unchanged.",
                );
                widgets::actions(ui, |ui| {
                    if Btn::new("Apply redaction")
                        .look(Look::Danger)
                        .enabled(idle && pages > 0 && self.acknowledged)
                        .show(ui, p)
                        .clicked()
                    {
                        request = Some(Request::Redact(self.page, self.bounds));
                        self.acknowledged = false;
                    }
                });
            });
        request
    }
}
