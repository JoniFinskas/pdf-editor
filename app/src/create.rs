use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
};
use eframe::egui;
use pdf_editor_core::{BlankPdf, CvCheck, CvSection, CvSpec};
use std::path::PathBuf;

enum DraftAction {
    Open(PathBuf),
    Save(PathBuf, CvSpec),
    Done(Result<Option<CvSpec>, String>),
}

#[derive(Clone, Debug)]
pub enum Creation {
    Blank(BlankPdf),
    Cv(CvSpec),
}
#[derive(Default)]
pub struct CreateTool {
    pub open: bool,
    pub cv_mode: bool,
    pub cv: CvSpec,
    pub check: Option<CvCheck>,
    pub contact: String,
    error: Option<String>,
    pub pending: bool,
    removed: Option<(usize, u64, CvSection)>,
    section_ids: Vec<u64>,
    next_section_id: u64,
    draft_dialog: crate::dialogs::ToolDialog<DraftAction>,
    draft_processing: bool,
    previous_draft: Option<CvSpec>,
    draft_redo: bool,
    preset: u8,
    landscape: bool,
    pages: u16,
    width_mm: f32,
    height_mm: f32,
}
impl CreateTool {
    #[cfg(test)]
    pub(crate) fn draft_pending(&self) -> bool {
        self.draft_dialog.busy()
    }
    fn draft_snapshot(&self) -> CvSpec {
        let mut spec = self.cv.clone();
        spec.contact = if self.contact.is_empty() {
            Vec::new()
        } else {
            self.contact.split('\n').map(str::to_owned).collect()
        };
        spec
    }
    pub(crate) fn choose_draft_open(
        &mut self,
        choose: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    ) {
        if self.draft_dialog.busy() {
            return;
        }
        self.error = None;
        self.draft_processing = false;
        self.draft_dialog
            .launch(move || choose().map(DraftAction::Open));
    }
    pub(crate) fn choose_draft_save(
        &mut self,
        choose: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    ) {
        if self.draft_dialog.busy() {
            return;
        }
        let spec = self.draft_snapshot();
        if let Err(error) = spec.to_draft_json() {
            self.error = Some(error.to_string());
            return;
        }
        self.error = None;
        self.draft_processing = false;
        self.draft_dialog
            .launch(move || choose().map(|path| DraftAction::Save(path, spec)));
    }
    pub fn open(&mut self, cv: bool) {
        self.open = true;
        self.error = None;
        self.cv_mode = cv;
        if self.pages == 0 {
            self.pages = 1;
            self.width_mm = 210.;
            self.height_mm = 297.;
        }
    }
    pub fn failed(&mut self, message: &str) -> bool {
        if !std::mem::take(&mut self.pending) {
            return false;
        }
        self.open = true;
        self.error = Some(message.to_owned());
        true
    }
    pub fn remember(&mut self, creation: &Creation) {
        if let Creation::Blank(spec) = creation {
            self.cv_mode = false;
            self.preset = 3;
            self.width_mm = spec.width * 25.4 / 72.;
            self.height_mm = spec.height * 25.4 / 72.;
            self.pages = spec.pages;
            self.landscape = false;
        }
        if let Creation::Cv(spec) = creation {
            self.cv = spec.clone();
            self.removed = None;
            self.section_ids.clear();
            self.contact = spec.contact.join("\n");
            self.cv_mode = true;
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        document: bool,
    ) -> (Option<Creation>, bool) {
        if let Some(action) = self.draft_dialog.poll(ctx, self.open && self.cv_mode, idle) {
            match action {
                DraftAction::Open(path) => {
                    self.draft_processing = true;
                    self.draft_dialog.launch(move || {
                        Some(DraftAction::Done(
                            CvSpec::load_draft(&path)
                                .map(Some)
                                .map_err(|e| e.to_string()),
                        ))
                    });
                }
                DraftAction::Save(path, spec) => {
                    self.draft_processing = true;
                    self.draft_dialog.launch(move || {
                        Some(DraftAction::Done(
                            spec.save_draft(&path)
                                .map(|()| None)
                                .map_err(|e| e.to_string()),
                        ))
                    });
                }
                DraftAction::Done(result) => {
                    self.draft_processing = false;
                    match result {
                        Ok(Some(spec)) => {
                            self.previous_draft = Some(self.draft_snapshot());
                            self.draft_redo = false;
                            self.remember(&Creation::Cv(spec));
                            self.check = None;
                            self.error = Some("Draft opened.".into());
                        }
                        Ok(None) => self.error = Some("Draft copy saved.".into()),
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }
        let idle = idle && !self.draft_dialog.busy();
        if self.section_ids.len() != self.cv.sections.len() {
            self.section_ids.clear();
            for _ in &self.cv.sections {
                self.next_section_id += 1;
                self.section_ids.push(self.next_section_id);
            }
        }
        let mut creation = None;
        let mut check = false;
        let mut undo_draft = false;
        let window = ToolWindow::new(
            if self.cv_mode {
                "Build CV"
            } else {
                "New blank PDF"
            },
            Icon::File,
        )
        .width(if self.cv_mode { 520. } else { 390. });
        let window = if self.cv_mode {
            window.height((ctx.content_rect().height() - 140.).clamp(260., 760.))
        } else {
            window
        };
        let mut open = self.open;
        window.show(ctx, p, &mut open, |ui| {
            self.draft_dialog.feedback_for(ui,p,"Reading or saving draft…",!self.draft_processing);
            if self.cv_mode
                && let Some(report) = &mut self.check
            {
                widgets::rule(ui, p);
                ui.label(format!(
                    "{} pages · {:.0} KB",
                    report.pages,
                    report.bytes as f32 / 1024.
                ));
                for warning in &report.warnings {
                    widgets::hint(ui, p, warning);
                }
                ui.collapsing("Extracted text · check the reading order", |ui| {
                    let mut text = report.text.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_rows(8)
                            .desired_width(f32::INFINITY),
                    );
                });
                widgets::hint(ui, p, "Local text checks cannot guarantee an ATS result.");
            }
            if let Some(error) = &self.error {
                widgets::hint(ui, p, error);
            }
            ui.add_enabled_ui(idle, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("creation-fields")
                    .auto_shrink([false, !self.cv_mode])
                    .max_height((ui.clip_rect().bottom() - ui.cursor().top() - 56.).max(60.))
                    .show(ui, |ui| {
                        if self.cv_mode {
                            let draft_menu=Btn::new("Draft").id("cv-draft-menu").show(ui,p);
                            egui::Popup::menu(&draft_menu).show(|ui|{
                                if Btn::new("Open draft…").id("cv-draft-open").show(ui,p).clicked(){
                                    self.choose_draft_open(||rfd::FileDialog::new().set_title("Open CV draft").add_filter("CV draft", &["cvdraft"]).pick_file());
                                    ui.close();
                                }
                                if Btn::new("Save draft copy…").id("cv-draft-save").show(ui,p).on_hover_text("Saves entered details in an unencrypted draft file. Existing files are protected.").clicked(){
                                    self.choose_draft_save(||rfd::FileDialog::new().set_title("Save CV draft copy").set_file_name("CV.cvdraft").add_filter("CV draft", &["cvdraft"]).save_file());
                                    ui.close();
                                }
                                if self.previous_draft.is_some() && Btn::new(if self.draft_redo {"Redo open draft"}else{"Undo open draft"}).id("cv-draft-undo").show(ui,p).clicked(){
                                    undo_draft=true;
                                    ui.close();
                                }
                            });
                            widgets::field(ui, p, "Name", |ui| {
                                ui.add(
                                    widgets::text_field(&mut self.cv.name, ui.available_width())
                                        .id(egui::Id::new("cv-name")),
                                )
                            });
                            widgets::field(ui, p, "Professional title", |ui| {
                                ui.add(widgets::text_field(
                                    &mut self.cv.headline,
                                    ui.available_width(),
                                ))
                            });
                            widgets::field(ui, p, "Contact · one line per item", |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.contact)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(2)
                                        .id(egui::Id::new("cv-contact")),
                                )
                            });
                            let mut movement = None;
                            let mut remove = None;
                            let section_count = self.cv.sections.len();
                            for (index, section) in self.cv.sections.iter_mut().enumerate() {
                                ui.push_id(self.section_ids[index], |ui| {
                                    egui::CollapsingHeader::new(if section.heading.is_empty() {
                                        "Section"
                                    } else {
                                        &section.heading
                                    })
                                    .id_salt("cv-section")
                                    .default_open(index == 0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.add(widgets::text_field(
                                                &mut section.heading,
                                                (ui.available_width() - 120.).max(60.),
                                            ));
                                            if Btn::icon(Icon::ArrowUp)
                                                .id(&format!("cv-up-{index}"))
                                                .height(28.)
                                                .enabled(index > 0)
                                                .tip("Move section up")
                                                .show(ui, p)
                                                .clicked()
                                            {
                                                movement = Some((index, index - 1));
                                            }
                                            if Btn::icon(Icon::ArrowDown)
                                                .id(&format!("cv-down-{index}"))
                                                .height(28.)
                                                .enabled(index + 1 < section_count)
                                                .tip("Move section down")
                                                .show(ui, p)
                                                .clicked()
                                            {
                                                movement = Some((index, index + 1));
                                            }
                                            if Btn::icon(Icon::Trash)
                                                .id(&format!("cv-remove-{index}"))
                                                .height(28.)
                                                .tip("Remove section")
                                                .show(ui, p)
                                                .clicked()
                                            {
                                                remove = Some(index);
                                            }
                                        });
                                        ui.add(
                                            egui::TextEdit::multiline(&mut section.body)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(4),
                                        );
                                    });
                                });
                            }
                            if let Some((a, b)) = movement {
                                self.cv.sections.swap(a, b);
                                self.section_ids.swap(a, b);
                            }
                            if let Some(index) = remove {
                                self.removed = Some((
                                    index,
                                    self.section_ids.remove(index),
                                    self.cv.sections.remove(index),
                                ));
                            }
                            if Btn::new("Add section")
                                .id("cv-add-section")
                                .enabled(self.cv.sections.len() < 20)
                                .show(ui, p)
                                .clicked()
                            {
                                self.next_section_id += 1;
                                self.section_ids.push(self.next_section_id);
                                self.cv.sections.push(CvSection {
                                    heading: "Additional information".into(),
                                    body: String::new(),
                                });
                            }
                            ui.horizontal(|ui| {
                                ui.label("Page size");
                                ui.selectable_value(&mut self.cv.letter, false, "A4");
                                ui.selectable_value(&mut self.cv.letter, true, "Letter");
                            });
                            widgets::hint(
                                ui,
                                p,
                                "Selectable text · one column · automatic page breaks",
                            );
                        } else {
                            egui::ComboBox::from_id_salt("new-pdf-size")
                                .selected_text(
                                    ["A4", "Letter", "A5", "Custom"][usize::from(self.preset)],
                                )
                                .show_ui(ui, |ui| {
                                    for (index, label) in
                                        ["A4", "Letter", "A5", "Custom"].iter().enumerate()
                                    {
                                        ui.selectable_value(&mut self.preset, index as u8, *label);
                                    }
                                });
                            if self.preset == 3 {
                                widgets::numbers(
                                    ui,
                                    p,
                                    vec![
                                        (
                                            "Width (mm)",
                                            egui::DragValue::new(&mut self.width_mm)
                                                .range(1.0..=5080.),
                                        ),
                                        (
                                            "Height (mm)",
                                            egui::DragValue::new(&mut self.height_mm)
                                                .range(1.0..=5080.),
                                        ),
                                    ],
                                );
                            }
                            ui.checkbox(&mut self.landscape, "Landscape");
                            widgets::number(
                                ui,
                                p,
                                "Pages",
                                true,
                                egui::DragValue::new(&mut self.pages).range(1..=1000),
                            );
                        }
                    });
                widgets::actions(ui, |ui| {
                    if self.cv_mode
                        && self.removed.is_some()
                        && Btn::new("Undo remove")
                            .id("cv-undo-remove")
                            .show(ui, p)
                            .clicked()
                        && let Some((index, id, section)) = self.removed.take()
                    {
                        let at = index.min(self.cv.sections.len());
                        self.cv.sections.insert(at, section);
                        self.section_ids.insert(at, id);
                    }

                    if Btn::new(if document {
                        "Create in new tab"
                    } else {
                        "Create PDF"
                    })
                    .id("create-pdf")
                    .look(Look::Primary)
                    .enabled(!self.cv_mode || !self.cv.name.trim().is_empty())
                    .show(ui, p)
                    .clicked()
                    {
                        let candidate = if self.cv_mode {
                            let mut spec = self.cv.clone();
                            spec.contact = self
                                .contact
                                .lines()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned)
                                .collect();
                            Creation::Cv(spec)
                        } else {
                            let (width, height) = match self.preset {
                                0 => (595.276, 841.89),
                                1 => (612., 792.),
                                2 => (419.528, 595.276),
                                _ => (self.width_mm * 72. / 25.4, self.height_mm * 72. / 25.4),
                            };
                            Creation::Blank(
                                BlankPdf {
                                    width,
                                    height,
                                    pages: self.pages,
                                }
                                .with_landscape(self.landscape),
                            )
                        };
                        let valid = match &candidate {
                            Creation::Cv(spec) => spec.validate(),
                            Creation::Blank(spec) => spec.validate(),
                        };
                        match valid {
                            Ok(()) => {
                                creation = Some(candidate);
                                self.error = None;
                            }
                            Err(error) => self.error = Some(error.to_string()),
                        }
                    }
                    if self.cv_mode
                        && document
                        && Btn::new("Review PDF text")
                            .id("cv-check")
                            .show(ui, p)
                            .clicked()
                    {
                        check = true;
                    }
                });
            });
        });
        self.open = open;
        if undo_draft && let Some(spec) = self.previous_draft.take() {
            self.previous_draft = Some(self.draft_snapshot());
            self.draft_redo = !self.draft_redo;
            self.remember(&Creation::Cv(spec));
            self.check = None;
            self.error = None;
        }
        if creation.is_some() {
            self.open = false;
            self.check = None;
        }
        (creation, check)
    }
}
