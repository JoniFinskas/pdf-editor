use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
#[derive(Default)]
pub struct Replace {
    pub open: bool,
    pub query: String,
    text: String,
    pub count: Option<usize>,
    occurrence: usize,
}
impl Replace {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Find and replace")
            .open(&mut self.open)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.label("Find");
                if ui.text_edit_singleline(&mut self.query).changed() {
                    self.count = None;
                }
                ui.label("Replace with");
                ui.text_edit_singleline(&mut self.text);
                if Btn::new("Count matches")
                    .enabled(idle && !self.query.is_empty())
                    .show(ui, p)
                    .clicked()
                {
                    request = Some(Request::ReplacementCount(self.query.clone()));
                }
                if let Some(count) = self.count {
                    ui.label(format!("{count} editable matches in the document"));
                    ui.horizontal(|ui| {
                        ui.label("Occurrence");
                        self.occurrence = self.occurrence.clamp(1, count.max(1));
                        ui.add(egui::DragValue::new(&mut self.occurrence).range(1..=count.max(1)));
                    });
                    ui.horizontal(|ui| {
                        for (label, occurrence) in [
                            ("Replace one", Some(self.occurrence.saturating_sub(1))),
                            ("Replace all", None),
                        ] {
                            if Btn::new(label)
                                .enabled(idle && count > 0)
                                .show(ui, p)
                                .clicked()
                            {
                                request = Some(Request::Replace {
                                    query: self.query.clone(),
                                    text: self.text.clone(),
                                    occurrence,
                                });
                                self.count = None;
                            }
                        }
                    });
                }
                ui.label(
                    "Literal, case-insensitive text. Each replacement action is one undo step.",
                );
            });
        request
    }
}
