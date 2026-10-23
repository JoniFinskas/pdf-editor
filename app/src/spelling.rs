//! Debounced local spelling feedback; no document text leaves the process.
use eframe::egui::{self, Color32, FontId, Id, Response, Stroke, TextBuffer, Ui};
use pdf_editor_core::{SpellingIssue, check_spelling, replace_spelling};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct State {
    text: String,
    language: String,
    changed: Instant,
    busy: bool,
    checked: bool,
    issues: Vec<SpellingIssue>,
    error: Option<String>,
}

pub struct Feedback {
    state: Arc<Mutex<State>>,
    text: String,
    issues: Vec<SpellingIssue>,
}

pub fn prepare(ui: &Ui, id: Id, text: &str) -> Feedback {
    let state = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_insert_with(id.with("spelling"), || {
            Arc::new(Mutex::new(State {
                text: text.to_owned(),
                language: "en-US".into(),
                changed: Instant::now(),
                busy: false,
                checked: false,
                issues: vec![],
                error: None,
            }))
        })
        .clone()
    });
    let mut current = state.lock().unwrap_or_else(|e| e.into_inner());
    if current.text != text {
        current.text = text.to_owned();
        current.changed = Instant::now();
        current.checked = false;
        current.issues.clear();
        current.error = None;
    }
    if !current.checked && !current.busy && current.changed.elapsed() >= Duration::from_millis(300)
    {
        current.busy = true;
        let state = state.clone();
        let request = text.to_owned();
        let language = current.language.clone();
        let ctx = ui.ctx().clone();
        let started = std::thread::Builder::new()
            .name("spell-check".into())
            .spawn(move || {
                let result = check_spelling(&request, &language);
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                state.busy = false;
                if state.text == request && state.language == language {
                    state.checked = true;
                    match result {
                        Ok(issues) => {
                            state.issues = issues;
                            state.error = None;
                        }
                        Err(e) => {
                            state.error = Some(e.to_string());
                            state.issues.clear();
                        }
                    }
                }
                ctx.request_repaint();
            });
        if let Err(error) = started {
            current.busy = false;
            current.checked = true;
            current.error = Some(format!("Cannot start spell checking: {error}"));
        }
    }
    if !current.checked {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
    let issues = current.issues.clone();
    drop(current);
    Feedback {
        state,
        text: text.to_owned(),
        issues,
    }
}

impl Feedback {
    pub fn layout(
        &self,
        ui: &Ui,
        text: &dyn TextBuffer,
        width: f32,
        font: &FontId,
        color: Color32,
    ) -> Arc<egui::Galley> {
        let text = text.as_str();
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = width;
        let mut offset = 0;
        if text == self.text {
            for issue in &self.issues {
                if issue.range.start < offset || text.get(issue.range.clone()).is_none() {
                    continue;
                }
                job.append(
                    &text[offset..issue.range.start],
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[issue.range.clone()],
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color,
                        underline: Stroke::new(1.0, Color32::from_rgb(220, 70, 70)),
                        ..Default::default()
                    },
                );
                offset = issue.range.end;
            }
        }
        job.append(
            &text[offset..],
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    }
    pub fn menu(&self, response: &Response, text: &mut String) {
        response.context_menu(|ui| {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            ui.label("Spelling language");
            for (tag, label) in [("en-US", "English"), ("fi-FI", "Finnish")] {
                if ui.selectable_label(state.language == tag, label).clicked() {
                    state.language = tag.into();
                    state.checked = false;
                    state.issues.clear();
                    state.error = None;
                    state.changed = Instant::now();
                    ui.ctx().request_repaint();
                }
            }
            ui.separator();
            if let Some(error) = &state.error {
                ui.label(error);
            } else if !state.checked {
                ui.label("Checking spelling…");
            } else if state.issues.is_empty() {
                ui.label("No spelling errors");
            }
            if text.as_str() == self.text {
                for issue in &self.issues {
                    ui.menu_button(&issue.word, |ui| {
                        if issue.suggestions.is_empty() {
                            ui.label("No suggestions");
                        }
                        for replacement in &issue.suggestions {
                            if ui.button(replacement).clicked()
                                && replace_spelling(text, issue, replacement).is_ok()
                            {
                                response.request_focus();
                                ui.close();
                            }
                        }
                    });
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_text_never_uses_stale_underlines() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let feedback = prepare(ui, Id::new("test"), "wrong");
            feedback.state.lock().unwrap().issues = vec![SpellingIssue {
                range: 0..5,
                word: "wrong".into(),
                suggestions: vec!["right".into()],
            }];
            let feedback = prepare(ui, Id::new("test"), "wrong");
            let galley = feedback.layout(
                ui,
                &"wrong".to_owned(),
                100.0,
                &FontId::proportional(14.0),
                Color32::WHITE,
            );
            assert!(
                galley
                    .job
                    .sections
                    .iter()
                    .any(|s| s.format.underline.width == 1.0)
            );
            let feedback = prepare(ui, Id::new("test"), "😀");
            assert!(feedback.issues.is_empty());
            let galley = feedback.layout(
                ui,
                &"😀".to_owned(),
                100.0,
                &FontId::proportional(14.0),
                Color32::WHITE,
            );
            assert!(
                galley
                    .job
                    .sections
                    .iter()
                    .all(|s| s.format.underline == Stroke::NONE)
            );
        });
        output.textures_delta.clear();
    }
}
