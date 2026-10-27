use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look},
    worker::Request,
};
use eframe::egui;
use pdf_editor_core::{Bounds, LinkSpec, LinkTarget, PdfLink};

pub struct Links {
    pub items: Vec<PdfLink>,
    pub selected: Option<usize>,
    pub draft: LinkSpec,
    pub selecting: bool,
    pub jump: Option<u16>,
    start: Option<(u16, egui::Pos2)>,
    pending_update: Option<(PdfLink, LinkSpec)>,
    pending_delete: bool,
}
impl Default for Links {
    fn default() -> Self {
        Self {
            items: vec![],
            selected: None,
            draft: LinkSpec {
                page: 0,
                bounds: Bounds {
                    left: 72.,
                    bottom: 72.,
                    right: 220.,
                    top: 94.,
                },
                target: LinkTarget::Uri(String::new()),
            },
            selecting: false,
            jump: None,
            start: None,
            pending_update: None,
            pending_delete: false,
        }
    }
}
impl Links {
    pub fn receive(&mut self, items: Vec<PdfLink>) {
        if std::mem::take(&mut self.pending_delete) {
            self.selected = None;
        }
        let pending = self.pending_update.take();
        let old = self.selected.and_then(|i| self.items.get(i));
        let expected = pending.as_ref().filter(|(source, _)| Some(source) == old);
        self.selected = old.and_then(|old| {
            items.iter().position(|new| {
                new.index == old.index
                    && new.link.page == old.link.page
                    && ((new.link == old.link && new.object_id == old.object_id)
                        || expected.is_some_and(|(_, spec)| {
                            new.link == *spec && new.revision >= old.revision
                        }))
            })
        });
        if expected
            .is_some_and(|(source, desired)| self.draft == source.link || self.draft == *desired)
            && let Some(item) = self.selected.and_then(|i| items.get(i))
        {
            self.draft = item.link.clone();
        }
        self.items = items;
    }
    pub fn prepare_request(&mut self, request: &Request) {
        match request {
            Request::UpdateLink(old, spec) => {
                self.pending_update = Some((old.clone(), spec.clone()))
            }
            Request::DeleteLink(_) => {
                self.pending_delete = true;
                self.pending_update = None;
            }
            _ => {}
        }
    }
    pub fn failed(&mut self) {
        self.pending_update = None;
        self.pending_delete = false;
    }
    pub fn cancel_selection(&mut self) {
        self.selecting = false;
        self.start = None;
    }
    pub fn finish_frame(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || (self.start.is_some() && i.pointer.button_released(egui::PointerButton::Primary))
        }) {
            self.cancel_selection();
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        idle: bool,
        pages: usize,
    ) -> Option<Request> {
        let mut request = None;
        ui.horizontal(|ui| {
            if Btn::new("New link")
                .id("link-new")
                .enabled(idle)
                .show(ui, p)
                .clicked()
            {
                self.selected = None;
                self.pending_update = None;
                self.cancel_selection();
            }
            if Btn::icon(Icon::RotateRight)
                .id("link-refresh")
                .tip("Refresh links")
                .enabled(idle)
                .show(ui, p)
                .clicked()
            {
                request = Some(Request::Links);
            }
        });
        if self.items.is_empty() {
            widgets::hint(ui, p, "No links.");
        }
        egui::ScrollArea::vertical()
            .id_salt("links-list")
            .max_height(160.)
            .show_rows(ui, 32., self.items.len(), |ui, rows| {
                for i in rows {
                    let item = &self.items[i];
                    let title = match &item.link.target {
                        LinkTarget::Uri(uri) => uri.chars().take(80).collect(),
                        LinkTarget::Page(page) => format!("Go to page {}", page + 1),
                        LinkTarget::Unsupported => "Unsupported action".into(),
                    };
                    if ui
                        .add_enabled_ui(idle, |ui| {
                            widgets::list_row(
                                ui,
                                p,
                                self.selected == Some(i),
                                0.,
                                Icon::File,
                                &title,
                                &format!("Page {}", item.link.page + 1),
                            )
                        })
                        .inner
                        .clicked()
                    {
                        self.selected = Some(i);
                        self.draft = item.link.clone();
                        self.jump = Some(item.link.page);
                        self.cancel_selection();
                    }
                }
            });
        widgets::rule(ui, p);
        ui.add_enabled_ui(idle, |ui| {
            let mut page = usize::from(self.draft.page) + 1;
            widgets::number(
                ui,
                p,
                "On page",
                self.selected.is_none(),
                egui::DragValue::new(&mut page).range(1..=pages.max(1)),
            );
            self.draft.page = (page - 1) as u16;
            let mut kind = match self.draft.target {
                LinkTarget::Uri(_) => 0,
                LinkTarget::Page(_) => 1,
                LinkTarget::Unsupported => 2,
            };
            egui::ComboBox::from_id_salt("link-target-kind")
                .selected_text(match kind {
                    0 => "Website or email",
                    1 => "Page",
                    _ => "Unsupported action",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut kind, 0, "Website or email");
                    ui.selectable_value(&mut kind, 1, "Page");
                });
            match (&mut self.draft.target, kind) {
                (LinkTarget::Uri(_), 0)
                | (LinkTarget::Page(_), 1)
                | (LinkTarget::Unsupported, 2) => {}
                (_, 0) => self.draft.target = LinkTarget::Uri(String::new()),
                (_, 1) => self.draft.target = LinkTarget::Page(0),
                _ => {}
            }
            match &mut self.draft.target {
                LinkTarget::Uri(uri) => {
                    let width = ui.available_width();
                    ui.add(widgets::text_field(uri, width).hint_text("https://… or mailto:…"));
                }
                LinkTarget::Page(page) => {
                    let mut target = usize::from(*page) + 1;
                    widgets::number(
                        ui,
                        p,
                        "Go to page",
                        true,
                        egui::DragValue::new(&mut target).range(1..=pages.max(1)),
                    );
                    *page = (target - 1) as u16;
                }
                LinkTarget::Unsupported => {}
            }
            if Btn::new(if self.selecting {
                "Cancel area selection"
            } else {
                "Select area on page"
            })
            .id("link-area")
            .show(ui, p)
            .clicked()
            {
                self.selecting = !self.selecting;
                self.start = None;
            }
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
        });
        let mut spec = self.draft.clone();
        let valid = match &spec.target {
            LinkTarget::Uri(uri) => LinkTarget::uri(uri)
                .map(|target| spec.target = target)
                .is_ok(),
            LinkTarget::Page(page) => usize::from(*page) < pages,
            LinkTarget::Unsupported => false,
        };
        widgets::actions(ui, |ui| {
            if Btn::new(if self.selected.is_some() {
                "Update link"
            } else {
                "Add link"
            })
            .id("link-apply")
            .look(Look::Primary)
            .enabled(idle && pages > 0 && valid && !self.selecting)
            .show(ui, p)
            .clicked()
            {
                request = Some(match self.selected.and_then(|i| self.items.get(i)) {
                    Some(item) => Request::UpdateLink(item.clone(), spec),
                    None => Request::AddLink(spec),
                });
            }
            if Btn::new("Delete")
                .id("link-delete")
                .with_icon(Icon::Trash)
                .enabled(idle && self.selected.is_some())
                .show(ui, p)
                .clicked()
                && let Some(item) = self.selected.and_then(|i| self.items.get(i))
            {
                self.pending_update = None;
                request = Some(Request::DeleteLink(item.clone()));
            }
        });
        request
    }
    pub fn on_page(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        page: u16,
        rect: egui::Rect,
        scale: f32,
        color: egui::Color32,
    ) -> Option<egui::Rect> {
        if !self.selecting {
            return None;
        }
        if response.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_selection();
            return None;
        }
        if self.selected.is_some() && page != self.draft.page {
            return None;
        }
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.ctx.input(|i| i.pointer.latest_pos()))?;
        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = response
                .ctx
                .input(|i| i.pointer.press_origin())
                .unwrap_or(pointer);
            self.start = Some((page, ((origin - rect.min) / scale).to_pos2()));
        }
        if let Some((start_page, start)) = self.start
            && start_page == page
        {
            let local = ((pointer - rect.min) / scale).to_pos2();
            let bounds = egui::Rect::from_two_pos(start, local).intersect(
                egui::Rect::from_min_size(egui::Pos2::ZERO, rect.size() / scale),
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    rect.min + bounds.min.to_vec2() * scale,
                    rect.min + bounds.max.to_vec2() * scale,
                ),
                0.,
                egui::Stroke::new(1.5, color),
                egui::StrokeKind::Inside,
            );
            if response
                .ctx
                .input(|i| i.pointer.button_released(egui::PointerButton::Primary))
            {
                self.cancel_selection();
                if bounds.width() > 1. && bounds.height() > 1. {
                    self.draft.page = page;
                    return Some(bounds);
                }
            }
        }
        None
    }
}
