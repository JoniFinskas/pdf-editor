use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
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
        let mut request = None;
        ToolWindow::new("Find and replace", Icon::Replace)
            .width(380.0)
            .show(ctx, p, &mut self.open, |ui| {
                let width = ui.available_width();
                widgets::field(ui, p, "Find", |ui| {
                    if ui
                        .add(widgets::text_field(&mut self.query, width))
                        .changed()
                    {
                        self.count = None;
                    }
                });
                widgets::field(ui, p, "Replace with", |ui| {
                    ui.add(widgets::text_field(&mut self.text, width));
                });
                widgets::hint(
                    ui,
                    p,
                    "Literal, case-insensitive text. Each replacement action is one undo step.",
                );
                match self.count {
                    Some(count) => {
                        ui.horizontal(|ui| {
                            widgets::dot(ui, if count > 0 { p.accent_strong } else { p.muted });
                            ui.label(format!("{count} editable matches in the document"));
                        });
                        self.occurrence = self.occurrence.clamp(1, count.max(1));
                        widgets::number(
                            ui,
                            p,
                            "Occurrence",
                            true,
                            egui::DragValue::new(&mut self.occurrence).range(1..=count.max(1)),
                        );
                        widgets::actions(ui, |ui| {
                            for (label, occurrence, look) in [
                                ("Replace all", None, Look::Primary),
                                (
                                    "Replace one",
                                    Some(self.occurrence.saturating_sub(1)),
                                    Look::Outline,
                                ),
                            ] {
                                if Btn::new(label)
                                    .look(look)
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
                            if Btn::icon(Icon::RotateRight)
                                .height(34.0)
                                .tip("Count matches again")
                                .enabled(idle && !self.query.is_empty())
                                .show(ui, p)
                                .clicked()
                            {
                                request = Some(Request::ReplacementCount(self.query.clone()));
                            }
                        });
                    }
                    None => widgets::actions(ui, |ui| {
                        if Btn::new("Count matches")
                            .with_icon(Icon::Search)
                            .look(Look::Primary)
                            .enabled(idle && !self.query.is_empty())
                            .show(ui, p)
                            .clicked()
                        {
                            request = Some(Request::ReplacementCount(self.query.clone()));
                        }
                    }),
                }
            });
        request
    }
}
