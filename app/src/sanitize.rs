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
            widgets::hint(ui, p, "Remove metadata, hidden text, attachments and actions.");
            ui.checkbox(&mut self.options.flatten_visible, "Rebuild visible pages");
            if self.options.flatten_visible {
                widgets::notice(ui,p,widgets::Tone::Warn,"Image-only output, up to 100 pages. Visible edits remain; text, forms and links become uneditable. Local Undo and recovery copies can still contain originals.");
            }
            ui.checkbox(&mut self.options.remove_comments, if self.options.flatten_visible {"Remove comment and drawing appearances"} else {"Remove comments and drawings"});
            ui.checkbox(&mut self.options.remove_links, if self.options.flatten_visible {"Remove link appearances"} else {"Remove links"});
            if !self.options.flatten_visible { widgets::hint(ui, p, "Removes invisible and zero-opacity text. Covered, same-color and off-page text may remain. Complex content is rejected without changes; use Rebuild visible pages instead. Visible edits remain in the saved output; local Undo and recovery copies can contain originals."); }
            if Btn::new("Sanitize").look(Look::Primary).enabled(idle).show(ui,p).clicked() { request = Some(Request::Sanitize(self.options)); }
        });
        request
    }
}
