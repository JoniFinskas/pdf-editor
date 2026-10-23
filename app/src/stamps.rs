use crate::{theme::Palette, widgets::Btn, worker::Request};
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
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Page numbers and watermark")
            .open(&mut self.open)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if Btn::new("Page numbers").show(ui, p).clicked() {
                        self.stamp.template = "{page} / {pages}".into();
                        self.stamp.size = 12.0;
                        self.stamp.opacity = 1.0;
                        self.stamp.y = 24.0;
                    }
                    if Btn::new("Watermark").show(ui, p).clicked() {
                        self.stamp.template = "DRAFT".into();
                        self.stamp.size = 48.0;
                        self.stamp.opacity = 0.25;
                        self.stamp.y = 300.0;
                    }
                });
                ui.label("Text — {page} is the page number; {pages} is the total");
                ui.text_edit_singleline(&mut self.stamp.template);
                ui.horizontal(|ui| {
                    ui.label("Font");
                    ui.text_edit_singleline(&mut self.stamp.font);
                });
                ui.horizontal(|ui| {
                    ui.label("Size");
                    ui.add(egui::DragValue::new(&mut self.stamp.size).range(1.0..=1000.0));
                    ui.color_edit_button_srgb(&mut self.stamp.color);
                });
                ui.add(egui::Slider::new(&mut self.stamp.opacity, 0.0..=1.0).text("Opacity"));
                ui.label("Position in PDF points from the bottom-left corner");
                ui.horizontal(|ui| {
                    ui.label("Left");
                    ui.add(egui::DragValue::new(&mut self.stamp.x));
                    ui.label("Baseline");
                    ui.add(egui::DragValue::new(&mut self.stamp.y));
                });
                if Btn::new("Apply to all pages")
                    .enabled(idle && !self.stamp.template.is_empty())
                    .show(ui, p)
                    .clicked()
                {
                    request = Some(Request::Stamp(self.stamp.clone()));
                }
            });
        request
    }
}
