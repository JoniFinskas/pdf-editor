use crate::{
    icons::Icon,
    theme::{self, Palette},
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui::{self, RichText};
use pdf_editor_core::{FieldValue, FormField};
#[derive(Default)]
pub struct Forms {
    pub open: bool,
    pub fields: Vec<FormField>,
    pub designer: crate::form_design::Designer,
    design: bool,
}
impl Forms {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Forms", Icon::Form)
            .width(if self.design { 560.0 } else { 420.0 })
            .show(ctx, p, &mut self.open, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.design, false, "Fill");
                    ui.selectable_value(&mut self.design, true, "Design");
                    if ui
                        .add_enabled(
                            idle && !self.fields.is_empty(),
                            egui::Button::new("Flatten forms"),
                        )
                        .on_hover_text(
                            "Turn filled fields into page content. Undo restores editable fields.",
                        )
                        .clicked()
                    {
                        request = Some(Request::FlattenForms);
                    }
                });
                if self.design {
                    if let Some(design_request) = self.designer.show(ui, p, idle) {
                        request = Some(design_request);
                    }
                    return;
                }
                ui.horizontal(|ui| {
                    widgets::hint(ui, p, "Refresh after applying to load updated values.");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Btn::icon(Icon::RotateRight)
                            .height(30.0)
                            .tip("Refresh fields")
                            .enabled(idle)
                            .show(ui, p)
                            .clicked()
                        {
                            request = Some(Request::FormFields);
                        }
                    });
                });
                if self.fields.is_empty() {
                    widgets::hint(ui, p, "No supported form fields in this document.");
                }
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for field in &mut self.fields {
                            ui.push_id((field.page, field.index), |ui| {
                                widgets::rule(ui, p);
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&field.name)
                                            .font(theme::semibold(13.5))
                                            .color(p.text),
                                    );
                                    ui.label(
                                        RichText::new(format!("Page {}", field.page + 1))
                                            .size(12.5)
                                            .color(p.muted),
                                    );
                                    if field.read_only {
                                        ui.label(
                                            RichText::new("Read-only").size(12.5).color(p.muted),
                                        );
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(idle && !field.read_only, |ui| {
                                        match &mut field.value {
                                            FieldValue::Text(value) => {
                                                ui.add(
                                                    egui::TextEdit::singleline(value)
                                                        .margin(egui::Margin::symmetric(10, 7))
                                                        .desired_width(260.0),
                                                );
                                            }
                                            FieldValue::Check(value) => {
                                                widgets::checkbox(ui, value, "Checked");
                                            }
                                            FieldValue::Radio(value) => {
                                                if ui.radio(*value, "Selected").clicked() {
                                                    *value = true;
                                                }
                                            }
                                            FieldValue::Choice { options, selected } => {
                                                egui::ComboBox::from_id_salt("choice")
                                                    .width(244.0)
                                                    .selected_text(
                                                        selected
                                                            .and_then(|i| options.get(i))
                                                            .map_or("Select…", String::as_str),
                                                    )
                                                    .show_ui(ui, |ui| {
                                                        for (i, option) in
                                                            options.iter().enumerate()
                                                        {
                                                            ui.selectable_value(
                                                                selected,
                                                                Some(i),
                                                                option,
                                                            );
                                                        }
                                                    });
                                            }
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if Btn::new("Apply")
                                                    .look(Look::OutlineAccent)
                                                    .height(30.0)
                                                    .tip("Apply field")
                                                    .show(ui, p)
                                                    .clicked()
                                                {
                                                    request = Some(Request::FillField(
                                                        field.clone(),
                                                        field.value.clone(),
                                                    ));
                                                }
                                            },
                                        );
                                    });
                                });
                                ui.add_space(4.0);
                            });
                        }
                    });
            });
        request
    }
}
