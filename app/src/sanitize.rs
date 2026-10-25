use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
pub struct Sanitize {
    pub open: bool,
    options: pdf_editor_core::SanitizeOptions,
    export: crate::dialogs::ToolDialog<Request>,
    unprotected_export: bool,
}
impl Default for Sanitize {
    fn default() -> Self {
        Self {
            open: false,
            options: pdf_editor_core::SanitizeOptions {
                flatten_visible: true,
                ..Default::default()
            },
            export: Default::default(),
            unprotected_export: false,
        }
    }
}

impl Sanitize {
    pub fn close(&mut self) {
        self.open = false;
        self.export.cancel();
        self.unprotected_export = false;
    }
    #[cfg(test)]
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        self.show_for(ctx, p, idle, false)
    }
    pub fn show_for(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        protected: bool,
    ) -> Option<Request> {
        let mut request = self.export.poll(ctx, self.open, idle);
        let idle = idle && !self.export.busy() && request.is_none();
        ToolWindow::new("Sanitize document", Icon::Redact).width(430.0).show(ctx, p, &mut self.open, |ui| {
            self.export.feedback(ui,p);
            widgets::hint(ui, p, "Remove PDF metadata, hidden text, attachments and actions.");
            ui.checkbox(&mut self.options.flatten_visible, "Rebuild visible pages");
            if self.options.flatten_visible {
                widgets::notice(ui,p,widgets::Tone::Warn,"Image-only copy, up to 100 pages. Text, forms and links become uneditable. Undo and recovery can retain originals.");
            }
            ui.checkbox(&mut self.options.remove_comments, if self.options.flatten_visible {"Remove comment and drawing appearances"} else {"Remove comments and drawings"});
            ui.checkbox(&mut self.options.remove_links, if self.options.flatten_visible {"Remove link appearances"} else {"Remove links"});
            if !self.options.flatten_visible { widgets::hint(ui, p, "Editable cleanup removes PDF metadata and supported hidden text. Use Rebuild visible pages for sharing after removing private data. Undo and recovery can retain originals."); }
            if Btn::new("Sanitize").look(Look::Outline).enabled(idle).show(ui,p).clicked() { request = Some(Request::Sanitize(self.options)); }
            if protected {ui.checkbox(&mut self.unprotected_export,"Export without password");}
            let export_enabled=idle && (!protected || self.unprotected_export);
            if Btn::new("Sanitize & save…").id("sanitize-export").tip("Verify the selected cleanup in a new copy. Visible content is not assessed.").look(Look::Primary)
                .enabled(export_enabled).show(ui,p).clicked() && export_enabled {
                let options = self.options;
                self.export.launch(move || rfd::FileDialog::new().set_title("Save sanitized copy")
                    .add_filter("PDF document",&["pdf"]).set_file_name("sanitized.pdf").save_file()
                    .map(|path|Request::ExportSanitized(options,path)));
            }
        });
        if !self.open {
            self.export.cancel();
        }
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_export_requires_an_explicit_unencrypted_choice() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = Sanitize {
            open: true,
            ..Default::default()
        };
        let render = |tool: &mut Sanitize| {
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900., 700.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        tool.show_for(ui.ctx(), &crate::theme::palette(ui), true, true);
                    },
                );
                output.textures_delta.clear();
            }
        };
        render(&mut tool);
        assert!(
            !ctx.read_response(widgets::tool_id("sanitize-export"))
                .unwrap()
                .sense.senses_click()
        );
        tool.unprotected_export = true;
        render(&mut tool);
        assert!(
            ctx.read_response(widgets::tool_id("sanitize-export"))
                .unwrap()
                .sense.senses_click()
        );
        tool.close();
        assert!(!tool.unprotected_export);
    }
}
