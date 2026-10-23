use crate::{icons::Icon, theme::Palette, widgets::ToolWindow, worker::Request};
use eframe::egui;
use pdf_editor_core::{PrivacyQuery, RedactionPreview};

#[derive(Default)]
pub struct Privacy {
    pub open: bool,
    pub jump: Option<usize>,
    mode: usize,
    text: String,
    pub preview: Option<RedactionPreview>,
}
impl Privacy {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Search and redact", Icon::Redact)
            .width(440.0)
            .show(ctx, p, &mut self.open, |ui| {
                let modes = ["Text", "Email", "Phone", "IBAN", "Finnish personal identity code", "Credit card pattern"];
                egui::ComboBox::from_label("Match")
                    .selected_text(modes[self.mode])
                    .show_ui(ui, |ui| {
                        for (i, label) in modes.iter().enumerate() {
                            if ui.selectable_value(&mut self.mode, i, *label).changed() {
                                self.preview = None;
                            }
                        }
                    });
                if self.mode == 0 && ui.text_edit_singleline(&mut self.text).changed() {
                    self.preview = None;
                }
                ui.label("Patterns may include false positives. Review every match. Entire intersecting text runs or grouped artwork may be removed. Applying also removes document-wide annotations, forms, bookmarks, attachments, tags, metadata and actions.");
                if ui.add_enabled(idle && (self.mode != 0 || !self.text.trim().is_empty()), egui::Button::new("Preview matches")).clicked() {
                    let query = match self.mode {
                        0 => PrivacyQuery::Text(self.text.clone()),
                        1 => PrivacyQuery::Email,
                        2 => PrivacyQuery::Phone,
                        3 => PrivacyQuery::Iban,
                        4 => PrivacyQuery::FinnishId,
                        _ => PrivacyQuery::CreditCard,
                    };
                    self.preview = None;
                    request = Some(Request::PrivacyPreview(query));
                }
                if let Some(preview) = &self.preview {
                    ui.label(format!("{} matches", preview.matches.len()));
                    egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                        for (index, m) in preview.matches.iter().enumerate() {
                            for b in &m.bounds {
                                if ui.button(format!("Page {}: ({:.1}, {:.1})–({:.1}, {:.1})", m.page + 1, b.left, b.bottom, b.right, b.top)).clicked() {
                                    self.jump = Some(index);
                                }
                            }
                        }
                    });
                    if ui.add_enabled(idle && !preview.matches.is_empty(), egui::Button::new("Apply true redaction")).clicked() {
                        request = Some(Request::PrivacyApply(preview.clone()));
                        self.preview = None;
                    }
                }
            });
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_preview_button_dispatches_selected_pattern() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tool = Privacy {
            open: true,
            mode: 1,
            ..Default::default()
        };
        let mut button = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    assert!(tool.show(&ctx, &crate::theme::palette(ui), true).is_none());
                },
            );
            for clipped in &output.shapes {
                if let egui::epaint::Shape::Text(text) = &clipped.shape
                    && text.galley.text() == "Preview matches"
                {
                    button = Some(text.pos + text.galley.size() * 0.5);
                }
            }
            output.textures_delta.clear();
        }
        let pos = button.expect("Preview button is visible");
        let mut request = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| {
                request = tool.show(&ctx, &crate::theme::palette(ui), true);
            },
        );
        output.textures_delta.clear();
        assert!(matches!(
            request,
            Some(Request::PrivacyPreview(PrivacyQuery::Email))
        ));
    }
}
