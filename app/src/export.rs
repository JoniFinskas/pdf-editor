use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
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
        let mut request = None;
        ToolWindow::new("Export page images", Icon::Export)
            .width(380.0)
            .show(ctx, p, &mut self.open, |ui| {
                let width = ui.available_width();
                widgets::field(ui, p, "Pages, for example 1-3,5", |ui| {
                    ui.add(widgets::text_field(&mut self.ranges, width));
                });
                widgets::field(ui, p, "Format", |ui| {
                    widgets::segmented(
                        ui,
                        p,
                        "export-format",
                        &mut self.format,
                        &[
                            (PageImageFormat::Png, "PNG"),
                            (PageImageFormat::Jpeg, "JPG"),
                        ],
                    );
                });
                widgets::slider(ui, p, "DPI", egui::Slider::new(&mut self.dpi, 36..=600));
                widgets::hint(
                    ui,
                    p,
                    "Each page becomes a new image file. Very large pages may require a lower DPI.",
                );
                widgets::actions(ui, |ui| {
                    if Btn::new("Export to folder…")
                        .with_icon(Icon::Folder)
                        .look(Look::Primary)
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
            });
        request
    }
}
