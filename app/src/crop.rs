use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use pdf_editor_core::Bounds;
pub struct Crop {
    pub open: bool,
    page: u16,
    bounds: Bounds,
}
impl Default for Crop {
    fn default() -> Self {
        Self {
            open: false,
            page: 0,
            bounds: Bounds {
                left: 20.0,
                bottom: 20.0,
                right: 480.0,
                top: 280.0,
            },
        }
    }
}
impl Crop {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        pages: usize,
    ) -> Option<Request> {
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Crop page").open(&mut self.open).show(ctx,|ui| {
            let mut page=usize::from(self.page)+1;
            ui.horizontal(|ui| {ui.label("Page");ui.add(egui::DragValue::new(&mut page).range(1..=pages.max(1)));self.page=(page-1) as u16;});
            ui.label("Visible bounds in PDF points from the bottom-left");
            ui.horizontal(|ui| {ui.label("Left");ui.add(egui::DragValue::new(&mut self.bounds.left));ui.label("Right");ui.add(egui::DragValue::new(&mut self.bounds.right));});
            ui.horizontal(|ui| {ui.label("Bottom");ui.add(egui::DragValue::new(&mut self.bounds.bottom));ui.label("Top");ui.add(egui::DragValue::new(&mut self.bounds.top));});
            ui.label("Cropping hides content outside the visible box. Use Redact when content must be removed.");
            if Btn::new("Apply crop").enabled(idle && pages>0).show(ui,p).clicked() {request=Some(Request::Crop(self.page,self.bounds));}
        });
        request
    }
}
