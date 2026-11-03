//! Task-oriented tool discovery with keyboard navigation.
use super::*;

const CATEGORIES: [&str; 7] = [
    "Basics",
    "Edit text & objects",
    "Annotate & sign",
    "Pages & layout",
    "Create & convert",
    "Protect & clean",
    "Review & print",
];

impl Tool {
    fn category(self) -> &'static str {
        match self {
            Self::Replace | Self::Pictures | Self::Forms => CATEGORIES[1],
            Self::Markup | Self::Drawing | Self::Signatures => CATEGORIES[2],
            Self::Organize | Self::Crop | Self::Stamps | Self::Bookmarks => CATEGORIES[3],
            Self::NewPdf | Self::Cv | Self::ImageCreate
            | Self::ScanCleanup
            | Self::Ocr
            | Self::Export
            | Self::Tables
            | Self::Word
            | Self::Compress => CATEGORIES[4],
            Self::Redact | Self::Privacy | Self::Sanitize => CATEGORIES[5],
            Self::Compare | Self::Print => CATEGORIES[6],
        }
    }

    pub(super) fn aliases(self) -> &'static str {
        match self {
            Self::Cv => "resume résumé curriculum vitae ATS",
            Self::NewPdf => "blank new create document paper",
            Self::Compare => "diff difference differences revisions changes",
            Self::Ocr => "recognition searchable scan scanned",
            Self::Organize => "combine join separate divide chapters bookmarks",
            Self::Stamps => "watermark numbering pagination",
            Self::Pictures => "photo picture jpg jpeg png",
            Self::Sanitize => "privacy remove metadata javascript attachments",
            Self::Forms => "checkbox radio dropdown date field designer flatten fill",
            Self::Drawing => "circle ellipse rectangle arrow pen freehand",
            Self::Export => "png jpg jpeg dpi resolution",
            Self::Word => "docx microsoft word document",
            Self::Tables => "xlsx spreadsheet excel csv rows columns",
            Self::Signatures => "autograph sign reusable",
            _ => "",
        }
    }
}

struct Entry {
    category: &'static str,
    label: &'static str,
    key: String,
    tool: Option<Tool>,
    description: &'static str,
    enabled: bool,
}

impl EditorApp {
    pub(super) fn tool_catalog(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("All tools")
                    .font(theme::semibold(18.0))
                    .color(p.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if Btn::new("Close")
                    .id("close-tools")
                    .height(28.0)
                    .show(ui, p)
                    .clicked()
                {
                    self.tools_open = false;
                }
            });
        });
        let focused = self.tools_focus || ui.memory(|m| m.has_focus(Id::new("tool-search")));
        let enter = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
        let down =
            focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown));
        let up = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp));
        let search = ui.add(
            TextEdit::singleline(&mut self.tools_query)
                .id(Id::new("tool-search"))
                .event_filter(egui::EventFilter {
                    vertical_arrows: true,
                    horizontal_arrows: true,
                    ..Default::default()
                })
                .hint_text("Search tools")
                .desired_width(f32::INFINITY),
        );
        if self.tools_focus {
            if !search.has_focus() {
                search.request_focus();
            }
            self.tools_focus = false;
        }
        if up || down {
            ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }
        if search.changed() {
            self.tools_selected = 0;
            self.tools_category.clear();
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("tool-category")
                .selected_text(if self.tools_category.is_empty() {
                    "All categories"
                } else {
                    &self.tools_category
                })
                .show_ui(ui, |ui| {
                    for category in std::iter::once("").chain(CATEGORIES) {
                        if ui
                            .selectable_value(
                                &mut self.tools_category,
                                category.to_owned(),
                                if category.is_empty() {
                                    "All categories"
                                } else {
                                    category
                                },
                            )
                            .changed()
                        {
                            self.tools_selected = 0;
                        }
                    }
                });
            if ui.small_button("Reset").clicked() {
                self.tools_query.clear();
                self.tools_category.clear();
                self.tools_selected = 0;
                self.tools_focus = true;
            }
        });
        let ready = self.busy == 0 && self.engine_alive;
        let document = self.doc.is_some();
        let mut entries = Vec::new();
        for (label, key) in [
            ("Edit text / spell check", "catalog-edit"),
            ("Move, resize, copy or delete objects", "catalog-move"),
            ("Add text box", "catalog-add-text"),
            ("Find text", "catalog-find"),
            ("Go to page", "catalog-jump"),
            ("Pages: rotate, insert, reorder, delete", "catalog-pages"),
            ("Save As / password options", "catalog-save"),
            ("Crash recovery settings", "catalog-recovery"),
        ] {
            if self
                .tools_query
                .split_whitespace()
                .all(|w| label.to_lowercase().contains(&w.to_lowercase()))
            {
                entries.push(Entry {
                    category: CATEGORIES[0],
                    label,
                    key: key.into(),
                    tool: None,
                    description: "",
                    enabled: (document && ready) || key == "catalog-recovery",
                });
            }
        }
        for (_, tools) in Tool::MENU {
            for &(tool, label, _) in tools {
                if tool.matches(label, &self.tools_query) {
                    entries.push(Entry {
                        category: tool.category(),
                        label,
                        key: format!("catalog-{}", tool.key()),
                        tool: Some(tool),
                        description: tool.description(),
                        enabled: ready
                            && (document || matches!(tool, Tool::NewPdf | Tool::Cv | Tool::ImageCreate | Tool::Compare)),
                    });
                }
            }
        }
        entries.retain(|e| self.tools_category.is_empty() || self.tools_category == e.category);
        entries.sort_by_key(|e| CATEGORIES.iter().position(|c| *c == e.category).unwrap());
        self.tools_selected = self.tools_selected.min(entries.len().saturating_sub(1));
        if !entries.is_empty() {
            if down {
                self.tools_selected = (self.tools_selected + 1) % entries.len();
            }
            if up {
                self.tools_selected = (self.tools_selected + entries.len() - 1) % entries.len();
            }
        }
        ui.label(
            RichText::new(format!("{} tools", entries.len()))
                .small()
                .color(p.muted),
        );
        let mut chosen = None;
        ScrollArea::vertical()
            .id_salt("all-tools-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut previous = "";
                for (index, entry) in entries.iter().enumerate() {
                    if entry.category != previous {
                        menu_caption(ui, p, entry.category);
                        previous = entry.category;
                    }
                    let selected = focused && index == self.tools_selected;
                    let response = Btn::new(entry.label)
                        .id(&entry.key)
                        .align_left()
                        .min_width(ui.available_width())
                        .look(if selected { Look::Active } else { Look::Ghost })
                        .enabled(entry.enabled)
                        .tip(if entry.enabled { entry.description } else if !document { "Open a PDF to use this tool" } else { "Available when the current operation finishes" })
                        .show(ui, p);
                    if selected && (up || down || search.changed()) {
                        response.scroll_to_me(Some(Align::Center));
                    }
                    if response.clicked() || (enter && selected && entry.enabled) {
                        chosen = Some(index);
                    }
                    ui.add_space(2.0);
                }
                if entries.is_empty() {
                    ui.label("No matching tools.");
                }
            });
        if let Some(index) = chosen {
            // Reveal the page or selected tool instead of leaving the catalog over it.
            self.tools_open = false;
            let entry = &entries[index];
            if let Some(tool) = entry.tool {
                self.open_tool(tool);
                if tool == Tool::Bookmarks {
                    self.tools_open = false;
                }
            } else {
                match entry.key.as_str() {
                    "catalog-edit" => self.set_object_mode(false),
                    "catalog-move" => self.set_object_mode(true),
                    "catalog-add-text" => {
                        self.set_object_mode(false);
                        self.text_tool.adding = true;
                    }
                    "catalog-find" => {
                        self.find.open = true;
                        self.find.focus = true;
                    }
                    "catalog-jump" => {
                        self.commit_edit();
                        if let Some(doc) = &self.doc {
                            self.page_jump.open_at(doc.current_page);
                        }
                    }
                    "catalog-pages" => {
                        self.pages_open = true;
                        self.bookmarks.open = false;
                        self.tools_open = false;
                    }
                    "catalog-recovery" => {
                        self.commit_edit();
                        self.settings_open = true;
                        self.settings_tab = SettingsTab::General;
                    }
                    _ => self.save_as(),
                }
            }
        }
    }
}
