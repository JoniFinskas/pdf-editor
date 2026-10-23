use crate::{
    icons::Icon,
    theme::{self, Palette},
    widgets::{self, Btn, Look},
    worker::Request,
};
use eframe::egui::{self, Frame, Margin, RichText};
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
        // Header in the same layout as the Pages sidebar.
        Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 8,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Bookmarks").font(theme::semibold(15.0)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if Btn::icon(Icon::RotateRight)
                            .height(30.0)
                            .tip("Refresh bookmarks")
                            .enabled(ready)
                            .show(ui, p)
                            .clicked()
                        {
                            action = Some(Action::Request(Request::Bookmarks));
                        }
                        if Btn::icon(Icon::ChevronLeft)
                            .height(30.0)
                            .tip("Hide bookmarks")
                            .show(ui, p)
                            .clicked()
                        {
                            self.open = false;
                        }
                    });
                });
            });
        widgets::rule(ui, p);
        ui.add_enabled_ui(ready, |ui| {
            let editor_height = if self.selected.is_some() {
                250.0
            } else {
                190.0
            };
            Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - editor_height).max(80.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if self.items.items.is_empty() {
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                widgets::icon(ui, Icon::Bookmark, 24.0, p.muted);
                                widgets::hint(ui, p, "No bookmarks yet.");
                            });
                        }
                        for bookmark in &self.items.items {
                            ui.push_id(bookmark.id, |ui| {
                                let detail =
                                    bookmark.page.map_or(String::new(), |p| (p + 1).to_string());
                                if widgets::list_row(
                                    ui,
                                    p,
                                    self.selected == Some(bookmark.id),
                                    (bookmark.depth.min(8) * 14) as f32,
                                    Icon::Bookmark,
                                    &bookmark.title,
                                    &detail,
                                )
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
            });
            widgets::rule(ui, p);
            Frame::new().inner_margin(Margin::same(14)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let width = ui.available_width();
                widgets::field(ui, p, "Title", |ui| {
                    ui.add(widgets::text_field(&mut self.title, width));
                });
                let selected = self
                    .items
                    .items
                    .iter()
                    .find(|b| Some(b.id) == self.selected);
                ui.add_enabled_ui(selected.is_some(), |ui| {
                    widgets::checkbox(ui, &mut self.child, "Add inside selected bookmark");
                });
                let mut edit = None;
                if Btn::new("Add current page")
                    .with_icon(Icon::Plus)
                    .look(Look::Primary)
                    .min_width(width)
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
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if Btn::new("Rename")
                            .with_icon(Icon::Edit)
                            .look(Look::Outline)
                            .enabled(!self.title.trim().is_empty())
                            .show(ui, p)
                            .clicked()
                        {
                            edit = Some(BookmarkEdit::Rename {
                                id: selected.id,
                                title: self.title.clone(),
                            });
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if Btn::icon(Icon::Trash)
                                .look(Look::Outline)
                                .tip("Delete bookmark")
                                .show(ui, p)
                                .clicked()
                            {
                                edit = Some(BookmarkEdit::Delete { id: selected.id });
                            }
                            if Btn::icon(Icon::ArrowDown)
                                .look(Look::Outline)
                                .tip("Move down")
                                .enabled(selected.sibling + 1 < selected.siblings)
                                .show(ui, p)
                                .clicked()
                            {
                                edit = Some(BookmarkEdit::Move {
                                    id: selected.id,
                                    position: selected.sibling + 1,
                                });
                            }
                            if Btn::icon(Icon::ArrowUp)
                                .look(Look::Outline)
                                .tip("Move up")
                                .enabled(selected.sibling > 0)
                                .show(ui, p)
                                .clicked()
                            {
                                edit = Some(BookmarkEdit::Move {
                                    id: selected.id,
                                    position: selected.sibling - 1,
                                });
                            }
                        });
                    });
                    widgets::hint(
                        ui,
                        p,
                        "Delete includes child bookmarks. Undo restores them.",
                    );
                }
                if let Some(edit) = edit {
                    action = Some(Action::Request(Request::EditBookmarks(
                        self.items.revision,
                        edit,
                    )));
                }
            });
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
        theme::install(&ctx);
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
