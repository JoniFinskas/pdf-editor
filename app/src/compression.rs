use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
pub struct Compression {
    pub open: bool,
    edge: u32,
    quality: u8,
}
impl Default for Compression {
    fn default() -> Self {
        Self {
            open: false,
            edge: 1600,
            quality: 75,
        }
    }
}
impl Compression {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Compress images", Icon::Compress)
            .width(380.0)
            .show(ctx, p, &mut self.open, |ui| {
                widgets::hint(
                    ui,
                    p,
                    "Compresses RGB images; text stays editable.",
                );
                widgets::slider(
                    ui,
                    p,
                    "Maximum image edge (pixels)",
                    egui::Slider::new(&mut self.edge, 128..=8192),
                );
                widgets::slider(
                    ui,
                    p,
                    "JPEG quality",
                    egui::Slider::new(&mut self.quality, 10..=95),
                );
                widgets::hint(
                    ui,
                    p,
                    "No change if compression saves no space.",
                );
                widgets::actions(ui, |ui| {
                    if Btn::new("Compress")
                        .look(Look::Primary)
                        .min_width(96.0)
                        .enabled(idle)
                        .show(ui, p)
                        .clicked()
                    {
                        request = Some(Request::Compress(self.edge, self.quality));
                    }
                });
            });
        request
    }
}
