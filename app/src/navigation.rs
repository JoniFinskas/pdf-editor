use crate::{icons::Icon, theme::Palette, widgets::{Btn, ToolWindow}};
use eframe::egui::{self, Key};

#[derive(Default)]
pub struct PageJump {
    pub open: bool,
    value: String,
    focus: bool,
}
fn page_index(value: &str, count: usize) -> Option<usize> {
    value.trim().parse::<usize>().ok().filter(|page| (1..=count).contains(page)).map(|page| page - 1)
}
impl PageJump {
    pub fn open_at(&mut self, current: usize) {
        self.open = true;
        self.value = (current + 1).to_string();
        self.focus = true;
    }
    pub fn show(&mut self, ctx: &egui::Context, palette: &Palette, count: usize) -> Option<usize> {
        if !self.open { return None; }
        let mut open = self.open;
        let mut selected = None;
        ToolWindow::new("Go to page", Icon::File).width(280.0).show(ctx, palette, &mut open, |ui| {
            ui.label(format!("Page number (1 to {count})"));
            let edit = ui.add(egui::TextEdit::singleline(&mut self.value).id(egui::Id::new("jump-page-input")));
            if self.focus { edit.request_focus(); self.focus = false; }
            let target = page_index(&self.value, count);
            if target.is_none() { ui.colored_label(palette.error_fg, "Enter a page number within the document."); }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if Btn::new("Go").id("jump-page-go").enabled(target.is_some()).show(ui,palette).clicked() || enter {
                selected = target;
            }
            ui.horizontal(|ui| {
                if Btn::new("First page").enabled(count > 0).show(ui,palette).clicked() { selected = Some(0); }
                if Btn::new("Last page").enabled(count > 0).show(ui,palette).clicked() { selected = count.checked_sub(1); }
            });
        });
        self.open = open && selected.is_none() && !ctx.input(|i| i.key_pressed(Key::Escape));
        selected
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_jump_accepts_only_existing_one_based_pages() {
        for input in ["", "0", "-1", "2.5", "abc", "1001", "999999999999999999999999999"] {
            assert_eq!(page_index(input, 1000), None);
        }
        assert_eq!(page_index(" 1 ", 1000), Some(0));
        assert_eq!(page_index("1000", 1000), Some(999));
        assert_eq!(page_index("1", 0), None);
    }
}
