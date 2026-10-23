use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
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
        let title = if self.saving {
            "Save options"
        } else {
            "Open encrypted PDF"
        };
        let icon = if self.saving { Icon::Save } else { Icon::Lock };
        ToolWindow::new(title, icon)
            .width(360.0)
            .centered()
            .show(ctx, p, &mut open, |ui| {
                if let Some(name) = self.path.as_ref().and_then(|path| path.file_name()) {
                    ui.label(name.to_string_lossy());
                }
                widgets::hint(
                    ui,
                    p,
                    if self.saving {
                        "Optional password. Leave blank to save without encryption."
                    } else {
                        "Enter the PDF password. Recovery copies are disabled for encrypted sources."
                    },
                );
                let width = ui.available_width();
                widgets::field(ui, p, "Password", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password).margin(egui::Margin::symmetric(10, 7))
                            .password(true)
                            .desired_width(width)
                            .hint_text("Password"),
                    );
                });
                if self.saving {
                    widgets::field(ui, p, "Confirm password", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.confirm).margin(egui::Margin::symmetric(10, 7))
                                .password(true)
                                .desired_width(width)
                                .hint_text("Confirm password"),
                        );
                    });
                    if self.password != self.confirm && !self.confirm.is_empty() {
                        widgets::notice(ui, p, widgets::Tone::Warn, "The passwords do not match.");
                    }
                }
                widgets::actions(ui, |ui| {
                    if Btn::new(if self.saving { "Save" } else { "Open" })
                        .look(Look::Primary)
                        .min_width(84.0)
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
            });
        if !open {
            self.path = None;
            self.password.clear();
            self.confirm.clear();
        }
        request
    }
}
