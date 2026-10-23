use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use pdf_editor_core::PageImageFormat;
pub struct Export {
    pub open: bool,
    ranges: String,
    dpi: u16,
    format: PageImageFormat,
}
impl Default for Export {
    fn default() -> Self {
        Self {
            open: false,
            ranges: "1".into(),
            dpi: 150,
            format: PageImageFormat::Png,
        }
    }
}
impl Export {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Export page images")
            .open(&mut self.open)
            .show(ctx, |ui| {
                ui.label("Pages — for example 1-3,5");
                ui.text_edit_singleline(&mut self.ranges);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.format, PageImageFormat::Png, "PNG");
                    ui.selectable_value(&mut self.format, PageImageFormat::Jpeg, "JPG");
                });
                ui.add(egui::Slider::new(&mut self.dpi, 36..=600).text("DPI"));
                ui.label(
                    "Each page becomes a new image file. Very large pages may require a lower DPI.",
                );
                if Btn::new("Export to folder…")
                    .enabled(idle && !self.ranges.is_empty())
                    .show(ui, p)
                    .clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    request = Some(Request::ExportImages(
                        self.ranges.clone(),
                        self.dpi,
                        self.format,
                        folder,
                    ));
                }
            });
        request
    }
}
