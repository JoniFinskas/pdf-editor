use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use pdf_editor_core::{BookmarkEdit, BookmarkSet};
#[derive(Default)]
pub struct Bookmarks {
    pub open: bool,
    pub items: BookmarkSet,
    selected: Option<(u32, u16)>,
    title: String,
    child: bool,
}
pub enum Action {
    Request(Request),
    Navigate(u16),
}
impl Bookmarks {
    pub fn receive(&mut self, items: BookmarkSet) {
        if !items.items.iter().any(|b| Some(b.id) == self.selected) {
            self.selected = None;
        }
        self.items = items;
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        page: u16,
        ready: bool,
    ) -> Option<Action> {
        let mut action = None;
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.heading("Bookmarks");
            if Btn::new("Close").show(ui, p).clicked() {
                self.open = false;
            }
        });
        ui.add_enabled_ui(ready, |ui| {
            if Btn::new("Refresh").show(ui, p).clicked() {
                action = Some(Action::Request(Request::Bookmarks));
            }
            egui::ScrollArea::vertical()
                .max_height((ui.available_height() - 230.0).max(80.0))
                .show(ui, |ui| {
                    if self.items.items.is_empty() {
                        ui.label("No bookmarks yet.");
                    }
                    for bookmark in &self.items.items {
                        ui.horizontal(|ui| {
                            ui.add_space((bookmark.depth.min(8) * 12) as f32);
                            let label = match bookmark.page {
                                Some(page) => format!("{} · {}", bookmark.title, page + 1),
                                None => bookmark.title.clone(),
                            };
                            if ui
                                .selectable_label(self.selected == Some(bookmark.id), label)
                                .clicked()
                            {
                                self.selected = Some(bookmark.id);
                                self.title = bookmark.title.clone();
                                if let Some(page) = bookmark.page {
                                    action = Some(Action::Navigate(page));
                                }
                            }
                        });
                    }
                });
            ui.separator();
            ui.label("Title");
            ui.text_edit_singleline(&mut self.title);
            let selected = self
                .items
                .items
                .iter()
                .find(|b| Some(b.id) == self.selected);
            ui.add_enabled_ui(selected.is_some(), |ui| {
                ui.checkbox(&mut self.child, "Add inside selected bookmark");
            });
            let mut edit = None;
            if Btn::new("Add current page")
                .enabled(!self.title.trim().is_empty())
                .show(ui, p)
                .clicked()
            {
                edit = Some(BookmarkEdit::Add {
                    title: self.title.clone(),
                    page,
                    parent: if self.child { self.selected } else { None },
                });
            }
            if let Some(selected) = selected {
                ui.horizontal(|ui| {
                    if Btn::new("Rename")
                        .enabled(!self.title.trim().is_empty())
                        .show(ui, p)
                        .clicked()
                    {
                        edit = Some(BookmarkEdit::Rename {
                            id: selected.id,
                            title: self.title.clone(),
                        });
                    }
                    if Btn::new("Delete").show(ui, p).clicked() {
                        edit = Some(BookmarkEdit::Delete { id: selected.id });
                    }
                });
                ui.horizontal(|ui| {
                    if Btn::new("Move up")
                        .enabled(selected.sibling > 0)
                        .show(ui, p)
                        .clicked()
                    {
                        edit = Some(BookmarkEdit::Move {
                            id: selected.id,
                            position: selected.sibling - 1,
                        });
                    }
                    if Btn::new("Move down")
                        .enabled(selected.sibling + 1 < selected.siblings)
                        .show(ui, p)
                        .clicked()
                    {
                        edit = Some(BookmarkEdit::Move {
                            id: selected.id,
                            position: selected.sibling + 1,
                        });
                    }
                });
                ui.small("Delete includes child bookmarks. Undo restores them.");
            }
            if let Some(edit) = edit {
                action = Some(Action::Request(Request::EditBookmarks(
                    self.items.revision,
                    edit,
                )));
            }
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bookmarks_sidebar_keeps_valid_selection_and_renders_both_themes() {
        let mut sidebar = Bookmarks {
            open: true,
            selected: Some((3, 0)),
            ..Default::default()
        };
        sidebar.receive(BookmarkSet {
            revision: 4,
            items: vec![pdf_editor_core::Bookmark {
                id: (3, 0),
                title: "Chapter ä".into(),
                page: Some(0),
                depth: 1,
                sibling: 0,
                siblings: 1,
            }],
        });
        assert_eq!(sidebar.selected, Some((3, 0)));
        let ctx = egui::Context::default();
        for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
            ctx.set_visuals(visuals);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(280.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let p = crate::theme::palette(ui);
                    assert!(sidebar.show(ui, &p, 0, true).is_none());
                },
            );
            output.textures_delta.clear();
        }
        sidebar.receive(BookmarkSet::default());
        assert_eq!(sidebar.selected, None);
    }
}
