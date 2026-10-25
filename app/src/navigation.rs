use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{Btn, ToolWindow},
};
use eframe::egui::{self, Key};

#[derive(Default)]
pub struct PageJump {
    pub open: bool,
    value: String,
    focus: bool,
}
fn page_index(value: &str, count: usize) -> Option<usize> {
    value
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|page| (1..=count).contains(page))
        .map(|page| page - 1)
}
impl PageJump {
    pub fn open_at(&mut self, current: usize) {
        self.open = true;
        self.value = (current + 1).to_string();
        self.focus = true;
    }
    pub fn show(&mut self, ctx: &egui::Context, palette: &Palette, count: usize) -> Option<usize> {
        if !self.open {
            return None;
        }
        let mut open = self.open;
        let mut selected = None;
        ToolWindow::new("Go to page", Icon::File).width(280.0).show(
            ctx,
            palette,
            &mut open,
            |ui| {
                ui.label(format!("Page number (1 to {count})"));
                let mut edit = egui::TextEdit::singleline(&mut self.value)
                    .id(egui::Id::new("jump-page-input"))
                    .show(ui);
                if self.focus {
                    edit.response.request_focus();
                    edit.state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(self.value.chars().count()),
                        )));
                    edit.state.store(ctx, edit.response.id);
                    self.focus = false;
                }
                let target = page_index(&self.value, count);
                if target.is_none() {
                    ui.colored_label(palette.error_fg, "Enter a page number within the document.");
                }
                let enter = edit.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if enter && target.is_none() {
                    edit.response.request_focus();
                }
                if Btn::new("Go")
                    .id("jump-page-go")
                    .enabled(target.is_some())
                    .show(ui, palette)
                    .clicked()
                    || enter
                {
                    selected = target;
                }
                ui.horizontal(|ui| {
                    if Btn::new("First page")
                        .enabled(count > 0)
                        .show(ui, palette)
                        .clicked()
                    {
                        selected = Some(0);
                    }
                    if Btn::new("Last page")
                        .enabled(count > 0)
                        .show(ui, palette)
                        .clicked()
                    {
                        selected = count.checked_sub(1);
                    }
                });
            },
        );
        self.open = open && selected.is_none() && !ctx.input(|i| i.key_pressed(Key::Escape));
        selected
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_page_enter_keeps_focus_for_keyboard_correction() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut jump = PageJump::default();
        jump.open_at(0);
        let mut frame = |events| {
            let mut selected = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| selected = jump.show(ui.ctx(), &crate::theme::palette(ui), 100),
            );
            output.textures_delta.clear();
            selected
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        frame(vec![egui::Event::Text("999".into())]);
        let enter = || egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        assert_eq!(frame(vec![enter()]), None);
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("jump-page-input"))));
        frame(vec![
            egui::Event::Key {
                key: Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
            egui::Event::Text("5".into()),
        ]);
        assert_eq!(frame(vec![enter()]), Some(4));
    }
    #[test]
    fn opening_page_jump_selects_current_number_for_replacement() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut jump = PageJump::default();
        jump.open_at(11);
        let mut frame = |events| {
            let mut selected = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| selected = jump.show(ui.ctx(), &crate::theme::palette(ui), 100),
            );
            output.textures_delta.clear();
            selected
        };
        for _ in 0..3 {
            assert_eq!(frame(vec![]), None);
        }
        frame(vec![egui::Event::Text("5".into())]);
        assert_eq!(
            frame(vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default()
            }]),
            Some(4)
        );
    }
    #[test]
    fn page_jump_accepts_only_existing_one_based_pages() {
        for input in [
            "",
            "0",
            "-1",
            "2.5",
            "abc",
            "1001",
            "999999999999999999999999999",
        ] {
            assert_eq!(page_index(input, 1000), None);
        }
        assert_eq!(page_index(" 1 ", 1000), Some(0));
        assert_eq!(page_index("1000", 1000), Some(999));
        assert_eq!(page_index("1", 0), None);
    }
}
