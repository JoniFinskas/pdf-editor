//! In-place paragraph affordances use the same canvas editor and palette.
use eframe::egui::{self, Color32, Rect, TextEdit, Ui};
pub fn text_edit(text: &mut String, paragraph: bool) -> TextEdit<'_> {
    if paragraph {
        TextEdit::multiline(text)
    } else {
        TextEdit::singleline(text)
    }
}
pub fn hint(
    ui: &Ui,
    rect: Rect,
    paragraph: Option<&pdf_editor_core::Paragraph>,
    fallback: Option<&str>,
    color: Color32,
) {
    let message = paragraph.map(|p| {
        format!(
            "{:?} paragraph · Ctrl+Enter applies · Escape cancels · Alt+click edits a line",
            p.alignment
        )
    });
    if let Some(text) = message.as_deref().or(fallback) {
        ui.painter().text(
            rect.left_top() - egui::vec2(0.0, 5.0),
            egui::Align2::LEFT_BOTTOM,
            text,
            egui::FontId::proportional(11.0),
            color,
        );
    }
}
