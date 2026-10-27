use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::{Annotation, Bounds, Markup, MarkupKind};
pub struct MarkupTool {
    pub open: bool,
    pub links_mode: bool,
    pub links: crate::links::Links,
    pending_update: Option<(Annotation, Markup)>,
    pending_delete: bool,
    pub items: Vec<Annotation>,
    pub selected: Option<usize>,
    pub draft: Markup,
}
impl Default for MarkupTool {
    fn default() -> Self {
        Self {
            open: false,
            links_mode: false,
            links: Default::default(),
            pending_update: None,
            pending_delete: false,
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
    pub fn prepare_request(&mut self, request: &Request) {
        self.links.prepare_request(request);
        match request {
            Request::UpdateAnnotation(old, desired) => {
                self.pending_update = Some((old.clone(), desired.clone()))
            }
            Request::DeleteAnnotation(_) => {
                self.pending_delete = true;
                self.pending_update = None;
            }
            _ => {}
        }
    }
    pub fn failed(&mut self) {
        self.pending_update = None;
        self.pending_delete = false;
        self.links.failed();
    }
    pub fn receive(&mut self, items: Vec<Annotation>) {
        if std::mem::take(&mut self.pending_delete) {
            self.selected = None;
        }
        let pending = self.pending_update.take();
        let old = self.selected.and_then(|i| self.items.get(i));
        let expected = pending.as_ref().filter(|(source, _)| {
            old.is_some_and(|old| {
                old.index == source.index
                    && old.revision == source.revision
                    && old.markup == source.markup
            })
        });
        self.selected = old.and_then(|old| {
            items.iter().position(|new| {
                new.index == old.index
                    && new.markup.page == old.markup.page
                    && (new.markup == old.markup
                        || expected.is_some_and(|(_, desired)| {
                            new.markup == *desired && new.revision >= old.revision
                        }))
            })
        });
        if expected
            .is_some_and(|(source, desired)| self.draft == source.markup || self.draft == *desired)
            && let Some(item) = self.selected.and_then(|i| items.get(i))
        {
            self.draft = item.markup.clone();
        }
        self.items = items;
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        pages: usize,
    ) -> Option<Request> {
        let mut request = None;
        let mut open = self.open;
        ToolWindow::new("Markup and links", Icon::Highlighter)
            .width(450.0)
            .show(ctx, p, &mut open, |ui| {
                let previous = self.links_mode;
                ui.add_enabled_ui(idle, |ui| {
                    widgets::segmented(
                        ui,
                        p,
                        "markup-mode",
                        &mut self.links_mode,
                        &[(false, "Markup"), (true, "Links")],
                    )
                });
                if previous != self.links_mode {
                    self.links.cancel_selection();
                    request = Some(if self.links_mode {
                        Request::Links
                    } else {
                        Request::Annotations
                    });
                }
                if self.links_mode {
                    if let Some(action) = self.links.show(ui, p, idle, pages) {
                        request = Some(action);
                    }
                    return;
                }
                ui.horizontal(|ui| {
                    widgets::subheading(ui, p, "Annotations");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if Btn::icon(Icon::Plus)
                            .height(30.0)
                            .tip("New annotation")
                            .show(ui, p)
                            .clicked()
                        {
                            self.selected = None;
                        }
                        if Btn::icon(Icon::RotateRight)
                            .height(30.0)
                            .tip("Refresh annotations")
                            .enabled(idle)
                            .show(ui, p)
                            .clicked()
                        {
                            request = Some(Request::Annotations);
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, 32.0, self.items.len(), |ui, rows| {
                        if self.items.is_empty() {
                            widgets::hint(ui, p, "No annotations.");
                        }
                        for i in rows {
                            let item = &self.items[i];
                            let text: String = item.markup.text.chars().take(32).collect();
                            let title = if text.is_empty() {
                                format!("{:?}", item.markup.kind)
                            } else {
                                format!("{:?} · {text}", item.markup.kind)
                            };
                            let icon = if item.markup.kind == MarkupKind::Note {
                                Icon::Edit
                            } else {
                                Icon::Highlighter
                            };
                            if widgets::list_row(
                                ui,
                                p,
                                self.selected == Some(i),
                                0.0,
                                icon,
                                &title,
                                &format!("Page {}", item.markup.page + 1),
                            )
                            .clicked()
                            {
                                self.selected = Some(i);
                                self.draft = item.markup.clone();
                            }
                        }
                    });
                ui.add_space(4.0);
                widgets::rule(ui, p);
                widgets::subheading(
                    ui,
                    p,
                    if self.selected.is_some() {
                        "Edit annotation"
                    } else {
                        "New annotation"
                    },
                );
                widgets::segmented(
                    ui,
                    p,
                    "markup-kind",
                    &mut self.draft.kind,
                    &[
                        (MarkupKind::Highlight, "Highlight"),
                        (MarkupKind::Underline, "Underline"),
                        (MarkupKind::Strikethrough, "Strikethrough"),
                        (MarkupKind::Note, "Note"),
                    ],
                );
                let mut page = usize::from(self.draft.page) + 1;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    widgets::number(
                        ui,
                        p,
                        "Page",
                        self.selected.is_none(),
                        egui::DragValue::new(&mut page).range(1..=pages.max(1)),
                    );
                    widgets::field(ui, p, "Color", |ui| {
                        ui.color_edit_button_srgb(&mut self.draft.color)
                    });
                });
                self.draft.page = (page - 1) as u16;
                widgets::slider(
                    ui,
                    p,
                    "Opacity",
                    egui::Slider::new(&mut self.draft.opacity, 0.0..=1.0),
                );
                ui.collapsing("Position", |ui| {
                    widgets::numbers(
                        ui,
                        p,
                        vec![
                            ("Left", egui::DragValue::new(&mut self.draft.bounds.left)),
                            (
                                "Bottom",
                                egui::DragValue::new(&mut self.draft.bounds.bottom),
                            ),
                            ("Right", egui::DragValue::new(&mut self.draft.bounds.right)),
                            ("Top", egui::DragValue::new(&mut self.draft.bounds.top)),
                        ],
                    )
                });
                let width = ui.available_width();
                widgets::field(ui, p, "Note or comment", |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.draft.text)
                            .margin(egui::Margin::symmetric(10, 7))
                            .desired_width(width)
                            .desired_rows(3),
                    );
                });
                widgets::hint(ui, p, "Selected text supplies the bounds.");
                widgets::actions(ui, |ui| {
                    if Btn::new(if self.selected.is_some() {
                        "Update annotation"
                    } else {
                        "Add annotation"
                    })
                    .look(Look::Primary)
                    .enabled(idle && pages > 0)
                    .show(ui, p)
                    .clicked()
                    {
                        request = Some(match self.selected.and_then(|i| self.items.get(i)) {
                            Some(item) => {
                                Request::UpdateAnnotation(item.clone(), self.draft.clone())
                            }
                            None => Request::AddAnnotation(self.draft.clone()),
                        });
                    }
                    if Btn::new("Delete")
                        .with_icon(Icon::Trash)
                        .look(Look::Outline)
                        .enabled(idle && self.selected.is_some())
                        .show(ui, p)
                        .clicked()
                        && let Some(item) = self.selected.and_then(|i| self.items.get(i))
                    {
                        request = Some(Request::DeleteAnnotation(item.clone()));
                    }
                });
            });
        self.open = open;
        if !self.open {
            self.links.cancel_selection();
        }
        request
    }
}
