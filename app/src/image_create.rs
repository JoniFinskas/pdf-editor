use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::ImagePageSize;
use std::path::PathBuf;

enum Selection {
    Images(Vec<PathBuf>),
    Create(Request),
}
#[derive(Default)]
pub struct ImageCreate {
    pub open: bool,
    dialog: crate::dialogs::ToolDialog<Selection>,
    paths: Vec<PathBuf>,
    size: ImagePageSize,
    error: Option<String>,
}
impl ImageCreate {
    pub fn accepts(path: &std::path::Path) -> bool {
        path.extension().is_some_and(|e| {
            ["png", "jpg", "jpeg", "heic", "heif"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        })
    }
    pub fn add(&mut self, paths: Vec<PathBuf>) {
        self.open = true;
        for path in paths {
            if self.paths.len() >= 100 {
                self.error = Some("Choose at most 100 images".into());
                break;
            }
            if Self::accepts(&path) {
                self.paths.push(path);
            }
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        if let Some(selection) = self.dialog.poll(ctx, self.open, idle) {
            match selection {
                Selection::Images(paths) => self.add(paths),
                Selection::Create(value) => request = Some(value),
            }
        }
        let idle = idle && !self.dialog.busy() && request.is_none();
        ToolWindow::new("Images to PDF",Icon::Image).width(480.0).show(ctx,p,&mut self.open,|ui| {
            self.dialog.feedback(ui,p);
            widgets::hint(ui,p,"Drop JPG, PNG or HEIC images here. One image per page, in listed order. HEIC requires an installed Windows codec.");
            let mut movement=None;
            let mut remove=None;
            egui::ScrollArea::vertical().max_height(260.0).show(ui,|ui| {
                for (i,path) in self.paths.iter().enumerate() { ui.push_id(i,|ui| { ui.horizontal(|ui| {
                    if Btn::icon(Icon::ArrowUp).enabled(i>0).show(ui,p).clicked() { movement=Some((i,i-1)); }
                    if Btn::icon(Icon::ArrowDown).enabled(i+1<self.paths.len()).show(ui,p).clicked() { movement=Some((i,i+1)); }
                    if Btn::new("Remove").show(ui,p).clicked() { remove=Some(i); }
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                }); }); }
            });
            if let Some((a,b))=movement { self.paths.swap(a,b); }
            if let Some(i)=remove { self.paths.remove(i); }
            ui.horizontal(|ui| { ui.label("Page size"); for (value,label) in [(ImagePageSize::A4,"A4"),(ImagePageSize::Letter,"Letter"),(ImagePageSize::Image,"Image at 96 DPI")] { ui.selectable_value(&mut self.size,value,label); } });
            if let Some(error)=&self.error { ui.label(error); }
            widgets::actions(ui,|ui| {
                if Btn::new("Add images…").enabled(idle).show(ui,p).clicked() { self.dialog.launch(|| rfd::FileDialog::new().add_filter("Images",&["jpg","jpeg","png","heic","heif"]).pick_files().map(Selection::Images)); }
                if Btn::new("Create PDF…").look(Look::Primary).enabled(idle && !self.paths.is_empty()).show(ui,p).clicked() { let (paths,size)=(self.paths.clone(),self.size); self.dialog.launch(move || rfd::FileDialog::new().add_filter("PDF",&["pdf"]).set_file_name("images.pdf").save_file().map(|path| Selection::Create(Request::ImagesToPdf(paths,size,path)))); }
            });
            widgets::hint(ui,p,"Photos are encoded at JPEG quality 95 on white. Existing files are never overwritten.");
        });
        if !self.open { self.dialog.cancel(); return None; }
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_queue_accepts_multiple_formats_and_is_bounded() {
        let mut queue = ImageCreate::default();
        queue.add(vec![
            "one.JPG".into(),
            "two.heic".into(),
            "three.png".into(),
            "ignore.pdf".into(),
        ]);
        assert_eq!(queue.paths.len(), 3);
        queue.paths.swap(0, 2);
        assert_eq!(queue.paths[0], PathBuf::from("three.png"));
        queue.add(
            (0..101)
                .map(|i| PathBuf::from(format!("{i}.jpg")))
                .collect(),
        );
        assert_eq!(queue.paths.len(), 100);
        assert!(queue.error.is_some());
    }
}
