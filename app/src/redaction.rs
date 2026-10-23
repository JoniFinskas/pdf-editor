use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use pdf_editor_core::Bounds;
pub struct Redaction {
    pub open: bool,
    pub page: u16,
    pub bounds: Bounds,
    acknowledged: bool,
}
impl Default for Redaction {
    fn default() -> Self {
        Self {
            open: false,
            page: 0,
            bounds: Bounds {
                left: 72.0,
                bottom: 72.0,
                right: 220.0,
                top: 100.0,
            },
            acknowledged: false,
        }
    }
}
impl Redaction {
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
        egui::Window::new("Secure redaction").open(&mut self.open).default_width(440.0).show(ctx,|ui| {
            ui.label("Remove content inside the box and replace it with black pixels.");
            ui.label("This rebuilds every page as an image. Text layers, forms, annotations and bookmarks are flattened or removed. Original PDF objects are not copied into the result.");
            let mut page=usize::from(self.page)+1;
            ui.horizontal(|ui| {ui.label("Page");ui.add(egui::DragValue::new(&mut page).range(1..=pages.max(1)));self.page=(page-1) as u16;});
            ui.label("PDF points, measured from the bottom-left corner");
            ui.horizontal(|ui| {ui.label("Left");ui.add(egui::DragValue::new(&mut self.bounds.left));ui.label("Right");ui.add(egui::DragValue::new(&mut self.bounds.right));});
            ui.horizontal(|ui| {ui.label("Bottom");ui.add(egui::DragValue::new(&mut self.bounds.bottom));ui.label("Top");ui.add(egui::DragValue::new(&mut self.bounds.top));});
            ui.checkbox(&mut self.acknowledged,"Flatten this document and remove the boxed content");
            if Btn::new("Apply redaction").enabled(idle && pages>0 && self.acknowledged).show(ui,p).clicked() {
                request=Some(Request::Redact(self.page,self.bounds));self.acknowledged=false;
            }
            ui.label("Undo is available in this session. Save As creates the sanitized file; the original remains unchanged.");
        });
        request
    }
}
