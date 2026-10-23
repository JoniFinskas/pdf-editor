use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use pdf_editor_core::{Annotation, Bounds, Markup, MarkupKind};
pub struct MarkupTool {
    pub open: bool,
    pub items: Vec<Annotation>,
    pub selected: Option<usize>,
    pub draft: Markup,
}
impl Default for MarkupTool {
    fn default() -> Self {
        Self {
            open: false,
            items: vec![],
            selected: None,
            draft: Markup {
                page: 0,
                kind: MarkupKind::Highlight,
                bounds: Bounds {
                    left: 72.0,
                    bottom: 72.0,
                    right: 220.0,
                    top: 94.0,
                },
                color: [255, 210, 40],
                opacity: 0.45,
                text: String::new(),
            },
        }
    }
}
impl MarkupTool {
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
        egui::Window::new("Markup and notes").open(&mut self.open).default_width(450.0).show(ctx,|ui| {
            ui.horizontal(|ui| {
                if Btn::new("Refresh annotations").enabled(idle).show(ui,p).clicked() {request=Some(Request::Annotations);}
                if Btn::new("New annotation").show(ui,p).clicked() {self.selected=None;}
            });
            egui::ScrollArea::vertical().max_height(160.0).show(ui,|ui| {
                for (i,item) in self.items.iter().enumerate() {
                    if ui.selectable_label(self.selected==Some(i),format!("Page {} · {:?} · {}",item.markup.page+1,item.markup.kind,item.markup.text.chars().take(32).collect::<String>())).clicked() {self.selected=Some(i);self.draft=item.markup.clone();}
                }
            });
            ui.separator();
            egui::ComboBox::from_id_salt("markup-kind").selected_text(format!("{:?}",self.draft.kind)).show_ui(ui,|ui| {
                for kind in [MarkupKind::Highlight,MarkupKind::Underline,MarkupKind::Strikethrough,MarkupKind::Note] {ui.selectable_value(&mut self.draft.kind,kind,format!("{kind:?}"));}
            });
            let mut page=usize::from(self.draft.page)+1;
            ui.horizontal(|ui| {ui.label("Page");ui.add_enabled(self.selected.is_none(),egui::DragValue::new(&mut page).range(1..=pages.max(1)));self.draft.page=(page-1) as u16;ui.color_edit_button_srgb(&mut self.draft.color);});
            ui.add(egui::Slider::new(&mut self.draft.opacity,0.0..=1.0).text("Opacity"));
            ui.label("Bounds in PDF points, from the bottom-left corner");
            ui.horizontal(|ui| {ui.label("Left");ui.add(egui::DragValue::new(&mut self.draft.bounds.left));ui.label("Right");ui.add(egui::DragValue::new(&mut self.draft.bounds.right));});
            ui.horizontal(|ui| {ui.label("Bottom");ui.add(egui::DragValue::new(&mut self.draft.bounds.bottom));ui.label("Top");ui.add(egui::DragValue::new(&mut self.draft.bounds.top));});
            ui.label("Note / comment");ui.text_edit_multiline(&mut self.draft.text);
            ui.horizontal(|ui| {
                if Btn::new(if self.selected.is_some(){"Update annotation"}else{"Add annotation"}).enabled(idle && pages>0).show(ui,p).clicked() {
                    request=Some(match self.selected.and_then(|i|self.items.get(i)) {Some(item)=>Request::UpdateAnnotation(item.clone(),self.draft.clone()),None=>Request::AddAnnotation(self.draft.clone())});
                }
                if Btn::new("Delete").enabled(idle && self.selected.is_some()).show(ui,p).clicked()
                    && let Some(item)=self.selected.and_then(|i|self.items.get(i)) {request=Some(Request::DeleteAnnotation(item.clone()));}
            });
            ui.label("Select text before opening this window to use its bounds. Refresh after changes.");
        });
        request
    }
}
