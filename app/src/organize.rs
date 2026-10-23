use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
#[derive(Default)]
pub struct Organize {
    pub open: bool,
    range: String,
    groups: String,
    paths: Vec<std::path::PathBuf>,
}
impl Organize {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        if !self.open {
            return None;
        }
        let mut request = None;
        egui::Window::new("Merge, split and extract").open(&mut self.open).default_width(430.0).show(ctx,|ui| {
            ui.label("Merge PDFs after the current document");
            if Btn::new("Choose PDFs…").enabled(idle).show(ui,p).clicked()
                && let Some(paths)=rfd::FileDialog::new().add_filter("PDF",&["pdf"]).pick_files() {self.paths=paths;}
            let mut movement=None;
            for (i,path) in self.paths.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                    if Btn::new("Up").enabled(i>0).show(ui,p).clicked() {movement=Some((i,i-1));}
                    if Btn::new("Down").enabled(i+1<self.paths.len()).show(ui,p).clicked() {movement=Some((i,i+1));}
                });
            }
            if let Some((a,b))=movement {self.paths.swap(a,b);}
            if Btn::new("Merge in listed order").enabled(idle && !self.paths.is_empty()).show(ui,p).clicked() {request=Some(Request::Merge(self.paths.clone()));}
            ui.separator();ui.label("Extract pages — for example 1-3,5");ui.text_edit_singleline(&mut self.range);
            if Btn::new("Extract to new PDF…").enabled(idle && !self.range.is_empty()).show(ui,p).clicked()
                && let Some(path)=rfd::FileDialog::new().add_filter("PDF",&["pdf"]).set_file_name("extracted.pdf").save_file() {
                request=Some(Request::Extract(self.range.clone(),path));
            }
            ui.separator();ui.label("Split groups — for example 1-3;4-6;7");ui.text_edit_singleline(&mut self.groups);
            if Btn::new("Split into folder…").enabled(idle && !self.groups.is_empty()).show(ui,p).clicked()
                && let Some(folder)=rfd::FileDialog::new().pick_folder() {request=Some(Request::Split(self.groups.clone(),folder));}
            ui.label("Exports use new filenames and do not alter the open document. Exported PDFs have no password.");
        });
        request
    }
}
