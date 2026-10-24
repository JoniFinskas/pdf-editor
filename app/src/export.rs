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
    dialog: crate::dialogs::ToolDialog<Request>,
    ranges: String,
    dpi: u16,
    format: PageImageFormat,
}
impl Default for Export {
    fn default() -> Self {
        Self {
            open: false,
            dialog: Default::default(),
            ranges: "1".into(),
            dpi: 150,
            format: PageImageFormat::Png,
        }
    }
}
impl Export {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let request = self.dialog.poll(ctx, self.open, idle);
        let idle = idle && !self.dialog.busy() && request.is_none();
        ToolWindow::new("Export page images", Icon::Export)
            .width(380.0)
            .show(ctx, p, &mut self.open, |ui| {
                self.dialog.feedback(ui, p);
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
                    {
                        let (range, dpi, format) = (self.ranges.clone(), self.dpi, self.format);
                        self.dialog.launch(move || {
                            rfd::FileDialog::new()
                                .pick_folder()
                                .map(|folder| Request::ExportImages(range, dpi, format, folder))
                        });
                    }
                });
            });
        if !self.open { self.dialog.cancel(); return None; }
        request
    }
}
