use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
};
use eframe::egui;
use pdf_editor_core::{BlankPdf, CvCheck, CvSection, CvSpec};

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
    preset: u8,
    landscape: bool,
    pages: u16,
    width_mm: f32,
    height_mm: f32,
}
impl CreateTool {
    pub fn open(&mut self, cv: bool) {
        self.open = true;
        self.cv_mode = cv;
        if self.pages == 0 {
            self.pages = 1;
            self.width_mm = 210.;
            self.height_mm = 297.;
        }
    }
    pub fn remember(&mut self, creation: &Creation) {
        if let Creation::Cv(spec) = creation {
            self.cv = spec.clone();
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
        let mut creation = None;
        let mut check = false;
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
        window.show(ctx, p, &mut self.open, |ui| {
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
            ui.add_enabled_ui(idle, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("creation-fields")
                    .auto_shrink([false, !self.cv_mode])
                    .max_height((ui.clip_rect().bottom() - ui.cursor().top() - 56.).max(60.))
                    .show(ui, |ui| {
                        if self.cv_mode {
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
                            for (index, section) in self.cv.sections.iter_mut().enumerate() {
                                ui.push_id(index, |ui| {
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
                                                (ui.available_width() - 110.).max(80.),
                                            ));
                                            if Btn::icon(Icon::ArrowUp)
                                                .height(28.)
                                                .enabled(index > 0)
                                                .tip("Move section up")
                                                .show(ui, p)
                                                .clicked()
                                            {
                                                movement = Some((index, index - 1));
                                            }
                                            if Btn::icon(Icon::Trash)
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
                            }
                            if let Some(index) = remove {
                                self.cv.sections.remove(index);
                            }
                            if Btn::new("Add section")
                                .id("cv-add-section")
                                .enabled(self.cv.sections.len() < 20)
                                .show(ui, p)
                                .clicked()
                            {
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
                        creation = Some(if self.cv_mode {
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
                            let (mut width, mut height) = match self.preset {
                                0 => (595.276, 841.89),
                                1 => (612., 792.),
                                2 => (419.528, 595.276),
                                _ => (self.width_mm * 72. / 25.4, self.height_mm * 72. / 25.4),
                            };
                            if self.landscape {
                                std::mem::swap(&mut width, &mut height);
                            }
                            Creation::Blank(BlankPdf {
                                width,
                                height,
                                pages: self.pages,
                            })
                        });
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
        if creation.is_some() {
            self.open = false;
            self.check = None;
        }
        (creation, check)
    }
}
