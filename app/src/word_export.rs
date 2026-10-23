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
}
impl WordExport {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Export to Word",Icon::Export).width(430.).show(ctx,p,&mut self.open,|ui|{
   widgets::hint(ui,p,"Export paragraphs, font names, sizes, bold/italic, images and simple ruled tables to an editable Word document. Layout is best effort; review the exported document before sharing.");
   widgets::hint(ui,p,"Images are limited to 2048 pixels per side. Ordinary groups are expanded automatically; unsupported transparency groups return an error. Complex layouts and merged table cells may need corrections in Word.");
   if Btn::new("Export Word document...").look(Look::Primary).enabled(idle).show(ui,p).clicked()&&let Some(path)=rfd::FileDialog::new().add_filter("Word document",&["docx"]).set_file_name("document.docx").save_file(){request=Some(Request::ExportWord(path));}
  });
        request
    }
}
