use super::*;
use pdf_editor_core::{Bounds, ObjectLayout, PageBatch, PageItem, SearchOptions, TextCharacter};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Qol {
    pub text_mode: bool,
    pub box_select: bool,
    pub arrange_open: bool,
    pub pages_open: bool,
    pub search_open: bool,
    pub format_open: bool,
    pub objects: Vec<PageItem>,
    pub restore_objects: Option<(u16, Vec<usize>)>,
    pub pages: BTreeSet<usize>,
    pub page_anchor: usize,
    pub characters: BTreeMap<u16, Vec<TextCharacter>>,
    pub pending_characters: BTreeSet<u16>,
    pub anchor: Option<(u16, usize)>,
    pub end: Option<(u16, usize)>,
    pub text_dragging: bool,
    pub marquee: Option<(u16, Pos2)>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub scope: usize,
    pub hits: Vec<pdf_editor_core::SearchHit>,
    pub family: String,
    pub format_targets: Vec<TextObject>,
    pub replace_context: Option<(u64, SearchOptions)>,
    pub fonts_requested: bool,
    pub bold: bool,
    pub italic: bool,
    pub working_copy: Option<PathBuf>,
    pub copy_encrypted: bool,
    pub original_protected: bool,
    pub remember_session: bool,
    pub remember_changed: bool,
    pub pending_view: Option<crate::session_state::SessionTab>,
    pub cancelling: bool,
    pub blank_width: f32,
    pub blank_height: f32,
    pub insert_before: usize,
}

impl Qol {
    pub fn document_changed(&mut self) {
        self.objects.clear();
        self.pages.clear();
        self.characters.clear();
        self.pending_characters.clear();
        self.anchor = None;
        self.end = None;
        self.text_dragging = false;
        self.marquee = None;
        self.hits.clear();
        self.format_targets.clear();
    }
    pub fn items(&self, objects: &PageObjects, item: &PageItem) -> Vec<PageItem> {
        if !self.objects.is_empty()
            && self
                .objects
                .iter()
                .all(|o| o.page == item.page && o.revision == item.revision)
        {
            self.objects.clone()
        } else {
            selection_items(objects, item)
        }
    }
    pub fn select(
        &mut self,
        objects: &PageObjects,
        item: Option<&PageItem>,
        extend: bool,
    ) -> Option<PageItem> {
        let Some(item) = item else {
            if !extend {
                self.objects.clear();
            }
            return self.primary();
        };
        let picked = selection_items(objects, item);
        if self
            .objects
            .first()
            .is_some_and(|o| o.page != item.page || o.revision != item.revision)
        {
            self.objects.clear();
        }
        if extend {
            if picked
                .iter()
                .all(|p| self.objects.iter().any(|o| o.index == p.index))
            {
                self.objects
                    .retain(|o| !picked.iter().any(|p| p.index == o.index));
            } else {
                for item in picked {
                    if !self.objects.iter().any(|o| o.index == item.index) {
                        self.objects.push(item);
                    }
                }
            }
        } else {
            self.objects = picked;
        }
        self.primary()
    }
    pub fn primary(&self) -> Option<PageItem> {
        let mut first = self.objects.first()?.clone();
        for item in &self.objects {
            first.bounds = union(first.bounds, item.bounds);
        }
        Some(first)
    }
    pub fn page_click(&mut self, index: usize, shift: bool, ctrl: bool) {
        if shift {
            if !ctrl {
                self.pages.clear();
            }
            self.pages
                .extend(self.page_anchor.min(index)..=self.page_anchor.max(index));
        } else if ctrl {
            if !self.pages.remove(&index) {
                self.pages.insert(index);
            }
            self.page_anchor = index;
        } else {
            self.pages.clear();
            self.pages.insert(index);
            self.page_anchor = index;
        }
    }
    pub fn page_indices(&self, current: usize) -> Vec<u16> {
        if self.pages.is_empty() {
            vec![current as u16]
        } else {
            self.pages.iter().map(|p| *p as u16).collect()
        }
    }
    fn endpoints(&self) -> Option<((u16, usize), (u16, usize))> {
        let a = self.anchor?;
        let b = self.end?;
        Some((a.min(b), a.max(b)))
    }
    pub fn selection(&self) -> Option<(String, Vec<(u16, Bounds)>)> {
        let (start, end) = self.endpoints()?;
        if start.1 == usize::MAX || end.1 == usize::MAX {
            return None;
        }
        let mut text = String::new();
        let mut regions = Vec::new();
        for page in start.0..=end.0 {
            let chars = self.characters.get(&page)?;
            if chars.is_empty() {
                continue;
            }
            let from = if page == start.0 { start.1 } else { 0 };
            let to = if page == end.0 {
                end.1
            } else {
                chars.len() - 1
            };
            let selection = pdf_editor_core::selected_text(chars, from, to);
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&selection.text);
            let mut line: Option<Bounds> = None;
            for b in selection.bounds {
                if let Some(previous) = line
                    && (previous.bottom - b.bottom).abs() < (b.top - b.bottom).max(1.0) * 0.5
                {
                    line = Some(union(previous, b));
                } else {
                    if let Some(previous) = line {
                        regions.push((page, previous));
                    }
                    line = Some(b);
                }
            }
            if let Some(line) = line {
                regions.push((page, line));
            }
        }
        Some((text, regions))
    }
    pub fn search_options(&self, current: usize) -> SearchOptions {
        let (pages, regions) = match self.scope {
            1 => (Some(vec![current as u16]), vec![]),
            2 => (Some(self.page_indices(current)), vec![]),
            3 => {
                let regions = self.selection().map(|(_, r)| r).unwrap_or_default();
                let pages = regions
                    .iter()
                    .map(|(p, _)| *p)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                (Some(pages), regions)
            }
            _ => (None, vec![]),
        };
        SearchOptions {
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            pages,
            regions,
        }
    }
    pub fn advanced(&self) -> bool {
        self.search_open || self.case_sensitive || self.whole_word || self.scope != 0
    }
}

fn union(a: Bounds, b: Bounds) -> Bounds {
    Bounds {
        left: a.left.min(b.left),
        bottom: a.bottom.min(b.bottom),
        right: a.right.max(b.right),
        top: a.top.max(b.top),
    }
}

fn visual_layout(transform: PageTransform, layout: ObjectLayout) -> ObjectLayout {
    use ObjectLayout::*;
    let origin = transform.apply(0.0, 0.0);
    let x = transform.apply(1.0, 0.0);
    let y = transform.apply(0.0, 1.0);
    let horizontal_x = (x.0 - origin.0).abs() > (y.0 - origin.0).abs();
    let edge = |horizontal: bool, minimum: bool| {
        let pdf_x = if horizontal {
            horizontal_x
        } else {
            !horizontal_x
        };
        let coefficient = match (horizontal, pdf_x) {
            (true, true) => x.0 - origin.0,
            (true, false) => y.0 - origin.0,
            (false, true) => x.1 - origin.1,
            (false, false) => y.1 - origin.1,
        };
        let native_min = minimum == (coefficient > 0.0);
        match (pdf_x, native_min) {
            (true, true) => AlignLeft,
            (true, false) => AlignRight,
            (false, true) => AlignBottom,
            (false, false) => AlignTop,
        }
    };
    match layout {
        AlignLeft => edge(true, true),
        AlignRight => edge(true, false),
        AlignTop => edge(false, true),
        AlignBottom => edge(false, false),
        AlignHorizontalCenter => {
            if horizontal_x {
                AlignHorizontalCenter
            } else {
                AlignVerticalCenter
            }
        }
        AlignVerticalCenter => {
            if horizontal_x {
                AlignVerticalCenter
            } else {
                AlignHorizontalCenter
            }
        }
        DistributeHorizontal => {
            if horizontal_x {
                DistributeHorizontal
            } else {
                DistributeVertical
            }
        }
        DistributeVertical => {
            if horizontal_x {
                DistributeVertical
            } else {
                DistributeHorizontal
            }
        }
    }
}

impl EditorApp {
    pub(super) fn qol_toolbar(&mut self, ui: &mut Ui, p: &Palette) {
        let was = self.qol.text_mode;
        if ui.selectable_label(was,"Select text").on_hover_text("Drag words or lines to copy or highlight; double-click selects a word, triple-click a line").clicked() {self.qol.text_mode= !was;}
        if was != self.qol.text_mode {
            self.commit_edit();
            self.drawing.active = false;
            self.tables.selecting = false;
            self.text_tool.adding = false;
        }
        if Btn::new("Selection tools")
            .id("selection-tools")
            .height(30.0)
            .show(ui, p)
            .clicked()
        {
            self.qol.arrange_open = !self.qol.arrange_open;
        }
        if self.qol.text_mode {
            let selection = self.qol.selection();
            if Btn::new("Copy text")
                .id("copy-selected-text")
                .height(30.0)
                .enabled(selection.as_ref().is_some_and(|(s, _)| !s.is_empty()))
                .show(ui, p)
                .clicked()
                && let Some((text, _)) = &selection
            {
                ui.ctx().copy_text(text.clone());
            }
            if Btn::new("Highlight selection")
                .id("highlight-selected-text")
                .height(30.0)
                .enabled(self.busy == 0 && selection.as_ref().is_some_and(|(_, r)| !r.is_empty()))
                .show(ui, p)
                .clicked()
                && let Some((_, regions)) = selection
            {
                self.document_action(Request::HighlightText(regions));
            }
        }
    }

    pub(super) fn qol_windows(&mut self, ctx: &Context, p: &Palette) {
        self.arrange_window(ctx, p);
        self.search_window(ctx, p);
        self.format_window(ctx, p);
        self.page_window(ctx, p);
        if self.busy > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            if let Some(progress) = self.worker.progress() {
                egui::Area::new(Id::new("operation-progress"))
                    .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -38.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0, p.border))
                            .corner_radius(8)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(if self.qol.cancelling {
                                        "Cancellation requested; finishing the current step…"
                                            .to_owned()
                                    } else {
                                        format!(
                                            "{} · {} / {}",
                                            progress.label, progress.completed, progress.total
                                        )
                                    });
                                    if progress.label != "Processing document"
                                        && Btn::new("Cancel")
                                            .id("cancel-operation")
                                            .enabled(!self.qol.cancelling)
                                            .show(ui, p)
                                            .clicked()
                                    {
                                        self.qol.cancelling = self.worker.cancel();
                                    }
                                });
                            });
                    });
            }
        } else {
            self.qol.cancelling = false;
        }
    }

    fn arrange_window(&mut self, ctx: &Context, p: &Palette) {
        let mut open = self.qol.arrange_open;
        let mut request = None;
        crate::widgets::ToolWindow::new("Selection tools",Icon::Edit).width(390.0).show(ctx,p,&mut open,|ui| {
            ui.checkbox(&mut self.qol.box_select,"Box selection").on_hover_text("Drag a rectangle around objects. Shift-drag on empty page space also selects a rectangle.");
            ui.label(format!("{} objects selected",self.qol.objects.len()));
            let items=self.qol.objects.clone();
            ui.add_enabled_ui(self.busy==0&&!items.is_empty(),|ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label,action) in [("Copy",None),("Duplicate",Some(pdf_editor_core::ObjectAction::Duplicate{dx:12.0,dy:-12.0})),("Delete",Some(pdf_editor_core::ObjectAction::Delete))] {
                        if ui.button(label).clicked() {request=Some(action.map_or_else(||Request::CopyItems(items.clone()),|a|Request::EditItems(items.clone(),a)));}
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for (label,layout) in [("Left",ObjectLayout::AlignLeft),("H center",ObjectLayout::AlignHorizontalCenter),("Right",ObjectLayout::AlignRight),
                        ("Top",ObjectLayout::AlignTop),("V center",ObjectLayout::AlignVerticalCenter),("Bottom",ObjectLayout::AlignBottom),
                        ("Equal horizontal gaps",ObjectLayout::DistributeHorizontal),("Equal vertical gaps",ObjectLayout::DistributeVertical)] {
                        if ui.button(label).clicked() {let transform=self.doc.as_ref().and_then(|d|items.first().and_then(|i|d.pages.get(usize::from(i.page)))).and_then(|p|p.objects.as_ref()).map(|o|o.transform);request=Some(Request::LayoutItems(items.clone(),transform.map_or(layout,|t|visual_layout(t,layout))));}
                    }
                });
                if ui.button("Format selected text…").clicked() {self.qol.format_targets=self.formatting_targets();self.qol.format_open=true;}
            });
        });
        self.qol.arrange_open = open;
        if let Some(request) = request {
            self.document_action(request);
        }
    }

    pub(super) fn search_options_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            changed |= ui
                .checkbox(&mut self.qol.case_sensitive, "Match case")
                .changed();
            changed |= ui
                .checkbox(&mut self.qol.whole_word, "Whole words")
                .changed();
            let old = self.qol.scope;
            egui::ComboBox::from_id_salt("search-scope")
                .selected_text(
                    [
                        "Document",
                        "Current page",
                        "Selected pages",
                        "Selected text",
                    ][self.qol.scope.min(3)],
                )
                .show_ui(ui, |ui| {
                    for (index, label) in [
                        "Document",
                        "Current page",
                        "Selected pages",
                        "Selected text",
                    ]
                    .iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut self.qol.scope, index, *label);
                    }
                });
            changed |= old != self.qol.scope;
        });
        changed
    }
    fn search_window(&mut self, ctx: &Context, p: &Palette) {
        let mut open = self.qol.search_open;
        let mut search = false;
        let mut jump = None;
        crate::widgets::ToolWindow::new("Search results", Icon::Search)
            .width(430.0)
            .show(ctx, p, &mut open, |ui| {
                let response = ui.add(
                    TextEdit::singleline(&mut self.find.query)
                        .hint_text("Find text")
                        .desired_width(f32::INFINITY),
                );
                search |= response.changed();
                search |= self.search_options_ui(ui);
                if ui.button("Search").clicked() {
                    search = true;
                }
                ui.label(format!("{} matches", self.qol.hits.len()));
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .show(ui, |ui| {
                        for (index, hit) in self.qol.hits.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    index == self.find.active,
                                    format!("Page {} · {}", hit.page + 1, hit.excerpt),
                                )
                                .clicked()
                            {
                                jump = Some(index);
                            }
                        }
                    });
            });
        self.qol.search_open = open;
        if search {
            self.find.open = true;
            self.send(Request::FindOptions(
                self.find.query.clone(),
                self.current_search_options(),
            ));
        }
        if let Some(index) = jump {
            self.find.active = index;
            self.go_match();
        }
    }

    pub(super) fn current_search_options(&self) -> SearchOptions {
        self.qol
            .search_options(self.doc.as_ref().map_or(0, |d| d.current_page))
    }
    pub(super) fn formatting_targets(&self) -> Vec<TextObject> {
        if !self.text_tool.selection.is_empty() {
            return self.text_tool.selection.clone();
        }
        self.doc
            .as_ref()
            .into_iter()
            .flat_map(|d| &d.pages)
            .filter_map(|p| p.objects.as_ref())
            .flat_map(|p| &p.lines)
            .flat_map(|l| &l.objects)
            .filter(|o| {
                self.qol
                    .objects
                    .iter()
                    .any(|i| i.page == o.page && i.index == o.index)
            })
            .cloned()
            .collect()
    }
    fn format_window(&mut self, ctx: &Context, p: &Palette) {
        let mut open = self.qol.format_open;
        let mut apply = false;
        if open && self.text_tool.fonts.is_empty() && !self.qol.fonts_requested {
            self.qol.fonts_requested = true;
            self.send(Request::Fonts);
        }
        crate::widgets::ToolWindow::new("Text formatting",Icon::Edit).width(410.0).show(ctx,p,&mut open,|ui| {
            ui.label("Font family");
            ui.add(TextEdit::singleline(&mut self.qol.family).hint_text("For example Arial or Inter").desired_width(f32::INFINITY));
            egui::ComboBox::from_id_salt("format-font").selected_text("Installed fonts").show_ui(ui,|ui| {
                for font in &self.text_tool.fonts {ui.selectable_value(&mut self.qol.family,font.clone(),font);}
            });
            ui.horizontal(|ui| {ui.checkbox(&mut self.qol.bold,"Bold");ui.checkbox(&mut self.qol.italic,"Italic");});
            ui.add(egui::DragValue::new(&mut self.text_tool.size).range(1.0..=1000.0).suffix(" pt"));
            ui.color_edit_button_srgb(&mut self.text_tool.color);
            apply=Btn::new("Apply font and style").id("apply-font-format").look(Look::Primary).enabled(self.busy==0&&!self.qol.family.trim().is_empty()).show(ui,p).clicked();
        });
        self.qol.format_open = open;
        if apply {
            let objects = if self.qol.format_targets.is_empty() {
                self.formatting_targets()
            } else {
                self.qol.format_targets.clone()
            };
            self.commit_edit();
            if self.busy > 0 {
                self.qol.format_targets.clear();
                self.banner(
                    BannerKind::Warning,
                    "The text edit is applying. Select the updated text before changing its font.",
                );
                return;
            }
            if objects.is_empty() {
                self.banner(BannerKind::Warning, "Select text before applying a font.");
                return;
            }
            self.document_action(Request::FormatText(
                objects,
                pdf_editor_core::TextFormat {
                    font: self.qol.family.trim().into(),
                    bold: self.qol.bold,
                    italic: self.qol.italic,
                    size: self.text_tool.size,
                    color: self.text_tool.color,
                },
            ));
        }
    }

    pub(super) fn page_actions(&mut self, ui: &mut Ui, p: &Palette, idle: bool) {
        let Some(doc) = &self.doc else {
            return;
        };
        let current = doc.current_page;
        let pages = self.qol.page_indices(current);
        let mut request = None;
        let mut extract = false;
        ui.label(format!("{} selected",pages.len())).on_hover_text("Ctrl-click toggles; Shift-click selects a range");
        ui.add_enabled_ui(idle, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Rotate").clicked() {
                    request = Some(Request::PageBatch(PageBatch::Rotate(pages.clone())));
                }
                if ui.button("Delete").clicked() {
                    request = Some(Request::PageBatch(PageBatch::Delete(pages.clone())));
                }
                if ui.button("Duplicate").clicked() {
                    request = Some(Request::PageBatch(PageBatch::Duplicate {
                        pages: pages.clone(),
                        before: pages.last().copied().unwrap_or(current as u16) + 1,
                    }));
                }
                if ui.button("Extract…").clicked() {
                    extract = true;
                }
                if ui.button("Blank page…").clicked() {
                    self.qol.pages_open = true;
                    self.qol.insert_before = current + 1;
                    self.qol.blank_width = doc.pages[current].info.width;
                    self.qol.blank_height = doc.pages[current].info.height;
                }
            });
        });
        if extract {
            let ranges = pages
                .iter()
                .map(|p| (usize::from(*p) + 1).to_string())
                .collect::<Vec<_>>()
                .join(",");
            self.insert_dialog.launch(move || {
                rfd::FileDialog::new()
                    .set_title("Extract selected pages")
                    .add_filter("PDF document", &["pdf"])
                    .save_file()
                    .map(|mut path| {
                        if path.extension().is_none() {
                            path.set_extension("pdf");
                        }
                        Request::Extract(ranges, path)
                    })
            });
        }
        if let Some(request) = request {
            self.document_action(request);
        }
        let _ = p;
    }
    fn page_window(&mut self, ctx: &Context, p: &Palette) {
        let mut open = self.qol.pages_open;
        let mut apply = false;
        crate::widgets::ToolWindow::new("Insert blank page", Icon::File)
            .width(320.0)
            .show(ctx, p, &mut open, |ui| {
                let count = self.doc.as_ref().map_or(0, |d| d.pages.len());
                ui.label("Insert after page (0 means the beginning)");
                ui.add(egui::DragValue::new(&mut self.qol.insert_before).range(0..=count));
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(
                        egui::DragValue::new(&mut self.qol.blank_width)
                            .range(1.0..=14400.0)
                            .suffix(" pt"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Height");
                    ui.add(
                        egui::DragValue::new(&mut self.qol.blank_height)
                            .range(1.0..=14400.0)
                            .suffix(" pt"),
                    );
                });
                apply = ui
                    .add_enabled(self.busy == 0, egui::Button::new("Insert blank page"))
                    .clicked();
            });
        self.qol.pages_open = open;
        if apply {
            self.document_action(Request::PageBatch(PageBatch::Blank {
                before: self.qol.insert_before as u16,
                width: self.qol.blank_width,
                height: self.qol.blank_height,
            }));
        }
    }

    pub(super) fn save(&mut self) {
        if self.qol.working_copy.is_some() {
            self.document_action(Request::SaveWorkingCopy);
        } else {
            self.save_as();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrangement_labels_follow_display_axes_at_each_rotation() {
        use ObjectLayout::*;
        for (matrix, left, top, horizontal) in [
            (
                [1., 0., 0., -1., 0., 500.],
                AlignLeft,
                AlignTop,
                DistributeHorizontal,
            ),
            (
                [0., 1., 1., 0., 0., 0.],
                AlignBottom,
                AlignLeft,
                DistributeVertical,
            ),
            (
                [-1., 0., 0., 1., 400., 0.],
                AlignRight,
                AlignBottom,
                DistributeHorizontal,
            ),
            (
                [0., -1., -1., 0., 500., 400.],
                AlignTop,
                AlignRight,
                DistributeVertical,
            ),
        ] {
            let transform = PageTransform::for_test(matrix);
            assert_eq!(visual_layout(transform, AlignLeft), left);
            assert_eq!(visual_layout(transform, AlignTop), top);
            assert_eq!(visual_layout(transform, DistributeHorizontal), horizontal);
        }
    }
    #[test]
    fn page_ranges_toggle_and_extend_without_losing_document_order() {
        let mut q = Qol::default();
        q.page_click(3, false, false);
        q.page_click(1, true, false);
        assert_eq!(q.page_indices(0), vec![1, 2, 3]);
        q.page_click(2, false, true);
        assert_eq!(q.page_indices(0), vec![1, 3]);
        q.page_click(5, true, true);
        assert_eq!(q.page_indices(0), vec![1, 2, 3, 4, 5]);
    }
}
