use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
#[derive(Default)]
pub struct Sanitize {
    pub open: bool,
    options: pdf_editor_core::SanitizeOptions,
}
impl Sanitize {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Sanitize document", Icon::Redact).width(430.0).show(ctx, p, &mut self.open, |ui| {
            widgets::hint(ui, p, "Remove metadata, hidden text, attachments, JavaScript and actions. Save As writes a cleaned copy; Undo restores the original.");
            ui.checkbox(&mut self.options.flatten_visible, "Rebuild visible pages (supports complex PDFs)");
            if self.options.flatten_visible {
                widgets::notice(ui,p,widgets::Tone::Warn,"Keeps visible page appearance as images. Removes all searchable/editable text, forms and interactive links. Up to 100 pages; Undo restores the original.");
            }
            ui.checkbox(&mut self.options.remove_comments, "Also remove comments and drawing annotations");
            ui.checkbox(&mut self.options.remove_links, "Also remove link annotations");
            if !self.options.flatten_visible { widgets::hint(ui, p, "Invisible rendering and zero-opacity text are removed. Complex visibility (layers, soft masks, clipping text or mixed hidden/visible blocks) is rejected without changes. XFA forms, retained appearances, unused Forms, inherited Form resources and aliased payloads are also rejected. Same-color, covered and off-page text are not detected. Ordinary web/mail and internal links can be retained. For complex documents, choose Rebuild visible pages above."); }
            if Btn::new("Sanitize").look(Look::Primary).enabled(idle).show(ui,p).clicked() { request = Some(Request::Sanitize(self.options)); }
        });
        request
    }
}
