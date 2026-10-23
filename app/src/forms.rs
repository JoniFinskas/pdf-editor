use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use pdf_editor_core::{FieldValue, FormField};
#[derive(Default)]
pub struct Forms {
    pub open: bool,
    pub fields: Vec<FormField>,
}
impl Forms {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Fill form")
            .open(&mut self.open)
            .default_width(430.0)
            .show(ctx, |ui| {
                if Btn::new("Refresh fields")
                    .enabled(idle)
                    .show(ui, p)
                    .clicked()
                {
                    request = Some(Request::FormFields);
                }
                if self.fields.is_empty() {
                    ui.label("No supported form fields in this document.");
                }
                egui::ScrollArea::vertical()
                    .max_height(500.0)
                    .show(ui, |ui| {
                        for field in &mut self.fields {
                            ui.push_id((field.page, field.index), |ui| {
                                ui.label(format!("{} · Page {}", field.name, field.page + 1));
                                ui.add_enabled_ui(idle && !field.read_only, |ui| {
                                    match &mut field.value {
                                        FieldValue::Text(value) => {
                                            ui.text_edit_singleline(value);
                                        }
                                        FieldValue::Check(value) => {
                                            ui.checkbox(value, "Checked");
                                        }
                                        FieldValue::Radio(value) => {
                                            if ui.radio(*value, "Selected").clicked() {
                                                *value = true;
                                            }
                                        }
                                        FieldValue::Choice { options, selected } => {
                                            egui::ComboBox::from_id_salt("choice")
                                                .selected_text(
                                                    selected
                                                        .and_then(|i| options.get(i))
                                                        .map_or("Select…", String::as_str),
                                                )
                                                .show_ui(ui, |ui| {
                                                    for (i, option) in options.iter().enumerate() {
                                                        ui.selectable_value(
                                                            selected,
                                                            Some(i),
                                                            option,
                                                        );
                                                    }
                                                });
                                        }
                                    }
                                    if Btn::new("Apply field").show(ui, p).clicked() {
                                        request = Some(Request::FillField(
                                            field.clone(),
                                            field.value.clone(),
                                        ));
                                    }
                                });
                                if field.read_only {
                                    ui.label("Read-only field");
                                }
                                ui.separator();
                            });
                        }
                    });
                ui.label("Refresh after applying to load updated field values.");
            });
        request
    }
}
