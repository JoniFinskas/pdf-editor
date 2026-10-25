use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
#[derive(Default)]
pub struct WordExport {
    pub open: bool,
    dialog: crate::dialogs::ToolDialog<Request>,
}
impl WordExport {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let request = self.dialog.poll(ctx, self.open, idle);
        let idle = idle && !self.dialog.busy() && request.is_none();
        ToolWindow::new("Export to Word",Icon::Export).width(430.).show(ctx,p,&mut self.open,|ui|{
   self.dialog.feedback(ui,p);
   widgets::hint(ui,p,"Layout may change. Check complex layouts and merged cells after export.");
   widgets::hint(ui,p,"Images: max 2048 px per side. Pages with complex transparency export as images; their text is not editable.");
   if Btn::new("Export Word document...").look(Look::Primary).enabled(idle).show(ui,p).clicked() {self.dialog.launch(|| rfd::FileDialog::new().add_filter("Word document",&["docx"]).set_file_name("document.docx").save_file().map(Request::ExportWord));}
  });
        if !self.open { self.dialog.cancel(); return None; }
        request
    }
}
