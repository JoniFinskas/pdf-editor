use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::TextStamp;
pub struct Stamps {
    pub open: bool,
    stamp: TextStamp,
}
impl Default for Stamps {
    fn default() -> Self {
        Self {
            open: false,
            stamp: TextStamp {
                template: "{page} / {pages}".into(),
                font: "Arial".into(),
                size: 12.0,
                color: [80, 80, 80],
                opacity: 1.0,
                x: 72.0,
                y: 24.0,
            },
        }
    }
}
impl Stamps {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Page numbers and watermark", Icon::Hash)
            .width(420.0)
            .show(ctx, p, &mut self.open, |ui| {
                widgets::field(ui, p, "Start from", |ui| {
                    ui.horizontal(|ui| {
                        if Btn::new("Page numbers")
                            .with_icon(Icon::Hash)
                            .look(Look::Outline)
                            .show(ui, p)
                            .clicked()
                        {
                            self.stamp.template = "{page} / {pages}".into();
                            self.stamp.size = 12.0;
                            self.stamp.opacity = 1.0;
                            self.stamp.y = 24.0;
                        }
                        if Btn::new("Watermark")
                            .with_icon(Icon::TextPlus)
                            .look(Look::Outline)
                            .show(ui, p)
                            .clicked()
                        {
                            self.stamp.template = "DRAFT".into();
                            self.stamp.size = 48.0;
                            self.stamp.opacity = 0.25;
                            self.stamp.y = 300.0;
                        }
                    });
                });
                let width = ui.available_width();
                widgets::field(ui, p, "Text", |ui| {
                    ui.add(widgets::text_field(&mut self.stamp.template, width));
                });
                widgets::hint(ui, p, "{page} is the page number; {pages} is the total.");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let font_width = 2.0 * widgets::column_width(ui) + 10.0;
                    widgets::field(ui, p, "Font", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.stamp.font)
                                .margin(egui::Margin::symmetric(10, 7))
                                .desired_width(font_width),
                        );
                    });
                    widgets::number(
                        ui,
                        p,
                        "Size",
                        true,
                        egui::DragValue::new(&mut self.stamp.size).range(1.0..=1000.0),
                    );
                    widgets::field(ui, p, "Color", |ui| {
                        ui.color_edit_button_srgb(&mut self.stamp.color)
                    });
                });
                widgets::slider(
                    ui,
                    p,
                    "Opacity",
                    egui::Slider::new(&mut self.stamp.opacity, 0.0..=1.0),
                );
                widgets::caption(ui, p, "Position in PDF points, from the bottom-left corner");
                widgets::numbers(
                    ui,
                    p,
                    vec![
                        ("Left", egui::DragValue::new(&mut self.stamp.x)),
                        ("Baseline", egui::DragValue::new(&mut self.stamp.y)),
                    ],
                );
                widgets::actions(ui, |ui| {
                    if Btn::new("Apply to all pages")
                        .look(Look::Primary)
                        .enabled(idle && !self.stamp.template.is_empty())
                        .show(ui, p)
                        .clicked()
                    {
                        request = Some(Request::Stamp(self.stamp.clone()));
                    }
                });
            });
        request
    }
}
