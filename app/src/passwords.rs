use crate::{theme::Palette, widgets::Btn, worker::Request};
use eframe::egui;
use std::path::PathBuf;
#[derive(Default)]
pub struct PasswordDialog {
    path: Option<PathBuf>,
    saving: bool,
    password: String,
    confirm: String,
}
impl PasswordDialog {
    pub fn open(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saving = false;
        self.password.clear();
        self.confirm.clear();
    }
    pub fn save(&mut self, path: PathBuf) {
        self.open(path);
        self.saving = true;
    }
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        self.path.as_ref()?;
        let mut open = true;
        let mut request = None;
        egui::Window::new(if self.saving {
            "Save options"
        } else {
            "Open encrypted PDF"
        })
        .open(&mut open)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(if self.saving {
                "Optional password. Leave blank to save without encryption."
            } else {
                "Enter the PDF password. Recovery copies are disabled for encrypted sources."
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .hint_text("Password"),
            );
            if self.saving {
                ui.add(
                    egui::TextEdit::singleline(&mut self.confirm)
                        .password(true)
                        .hint_text("Confirm password"),
                );
            }
            if Btn::new(if self.saving { "Save" } else { "Open" })
                .enabled(idle && (!self.saving || self.password == self.confirm))
                .show(ui, p)
                .clicked()
                && let Some(path) = self.path.take()
            {
                let password = std::mem::take(&mut self.password);
                self.confirm.clear();
                request = Some(if self.saving {
                    if password.is_empty() {
                        Request::SaveAs(path)
                    } else {
                        Request::SavePassword(path, password)
                    }
                } else {
                    Request::OpenPassword(path, password)
                });
            }
        });
        if !open {
            self.path = None;
            self.password.clear();
            self.confirm.clear();
        }
        request
    }
}
