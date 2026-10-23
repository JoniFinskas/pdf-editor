use crate::{theme::Palette, widgets::Btn, worker::Request};
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
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Compress images").open(&mut self.open).show(ctx,|ui| {
            ui.label("Downscale and JPEG-compress supported RGB images. Text stays editable.");
            ui.add(egui::Slider::new(&mut self.edge,128..=8192).text("Maximum image edge (pixels)"));
            ui.add(egui::Slider::new(&mut self.quality,10..=95).text("JPEG quality"));
            ui.label("The result banner shows size before and after. If no space is saved, the document stays unchanged. Undo restores image quality.");
            if Btn::new("Compress").enabled(idle).show(ui,p).clicked() {request=Some(Request::Compress(self.edge,self.quality));}
        });
        request
    }
}
