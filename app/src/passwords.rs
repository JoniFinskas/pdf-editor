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
    focus: bool,
}

impl PasswordDialog {
    pub fn open(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saving = false;
        self.password.clear();
        self.confirm.clear();
        self.focus = true;
    }
    pub fn save(&mut self, path: PathBuf) {
        self.open(path);
        self.saving = true;
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        idle: bool,
        scope: egui::Id,
    ) -> Option<Request> {
        self.path.as_ref()?;
        let first = self.focus;
        let ids = [
            scope.with("pdf-password-input"),
            scope.with("pdf-password-confirm"),
        ];
        if first {
            ctx.data_mut(|data| {
                for id in ids {
                    data.remove::<egui::text_edit::TextEditState>(id);
                }
            });
        }
        let escape = !first
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
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
                let field = widgets::field(ui, p, "Password", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password).id(ids[0]).margin(egui::Margin::symmetric(10, 7))
                            .password(true)
                            .desired_width(width)
                            .hint_text("Password"),
                    )
                });
                if first {field.request_focus();self.focus=false;}
                let mut enter = !first && field.lost_focus() && ui.input(|input|input.key_pressed(egui::Key::Enter));
                let mut confirmation = None;
                if self.saving {
                    let response = widgets::field(ui, p, "Confirm password", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.confirm).id(ids[1]).margin(egui::Margin::symmetric(10, 7))
                                .password(true)
                                .desired_width(width)
                                .hint_text("Confirm password"),
                        )
                    });
                    enter |= !first && response.lost_focus() && ui.input(|input|input.key_pressed(egui::Key::Enter));
                    confirmation = Some(response);
                    if self.password != self.confirm && !self.confirm.is_empty() {
                        widgets::notice(ui, p, widgets::Tone::Warn, "The passwords do not match.");
                    }
                }
                let ready = idle && (!self.saving || self.password == self.confirm);
                if enter && !ready {
                    confirmation.as_ref().unwrap_or(&field).request_focus();
                }
                widgets::actions(ui, |ui| {
                    let clicked = Btn::new(if self.saving { "Save" } else { "Open" })
                        .look(Look::Primary)
                        .min_width(84.0)
                        .enabled(ready)
                        .show(ui, p)
                        .clicked();
                    if !first && !escape && ready && (clicked || enter)
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
        if !open || escape {
            request = None;
        }
        if !open || escape || request.is_some() {
            self.path = None;
            self.password.clear();
            self.confirm.clear();
            self.focus = false;
            ctx.data_mut(|data| {
                for id in ids {
                    data.remove::<egui::text_edit::TextEditState>(id);
                }
            });
        }
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        dialog: &mut PasswordDialog,
        ctx: &egui::Context,
        idle: bool,
        events: Vec<egui::Event>,
    ) -> Option<Request> {
        let mut request = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                request = dialog.show(
                    ui.ctx(),
                    &crate::theme::palette(ui),
                    idle,
                    egui::Id::new("generated-password-dialog"),
                )
            },
        );
        output.textures_delta.clear();
        request
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn keyboard_save_requires_matching_passwords_and_idle_worker() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut dialog = PasswordDialog::default();
        dialog.save("generated-copy.pdf".into());
        assert!(frame(&mut dialog, &ctx, true, vec![key(egui::Key::Enter)]).is_none());
        for _ in 0..2 {
            frame(&mut dialog, &ctx, true, vec![]);
        }
        frame(
            &mut dialog,
            &ctx,
            true,
            vec![egui::Event::Text("generated-password".into())],
        );
        assert!(frame(&mut dialog, &ctx, true, vec![key(egui::Key::Enter)]).is_none());
        assert!(
            ctx.memory(|memory| memory.has_focus(
                egui::Id::new("generated-password-dialog").with("pdf-password-confirm")
            ))
        );
        frame(
            &mut dialog,
            &ctx,
            true,
            vec![egui::Event::Text("generated-password".into())],
        );
        assert!(frame(&mut dialog, &ctx, false, vec![key(egui::Key::Enter)]).is_none());
        assert!(matches!(
            frame(&mut dialog, &ctx, true, vec![key(egui::Key::Enter)]),
            Some(Request::SavePassword(..))
        ));
        assert!(dialog.password.is_empty() && dialog.confirm.is_empty() && dialog.path.is_none());
        assert!(
            egui::text_edit::TextEditState::load(
                &ctx,
                egui::Id::new("generated-password-dialog").with("pdf-password-input")
            )
            .is_none()
        );
        assert!(
            egui::text_edit::TextEditState::load(
                &ctx,
                egui::Id::new("generated-password-dialog").with("pdf-password-confirm")
            )
            .is_none()
        );
    }
    #[test]
    fn escape_clears_password_and_widget_undo_before_reopen() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut dialog = PasswordDialog::default();
        dialog.open("generated.pdf".into());
        for _ in 0..3 {
            frame(&mut dialog, &ctx, true, vec![]);
        }
        frame(
            &mut dialog,
            &ctx,
            true,
            vec![egui::Event::Text("generated-password".into())],
        );
        assert!(frame(&mut dialog, &ctx, true, vec![key(egui::Key::Escape)]).is_none());
        assert!(dialog.password.is_empty() && dialog.confirm.is_empty() && dialog.path.is_none());
        assert!(
            egui::text_edit::TextEditState::load(
                &ctx,
                egui::Id::new("generated-password-dialog").with("pdf-password-input")
            )
            .is_none()
        );
        dialog.open("other-generated.pdf".into());
        for _ in 0..3 {
            frame(&mut dialog, &ctx, true, vec![]);
        }
        frame(
            &mut dialog,
            &ctx,
            true,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        assert!(dialog.password.is_empty());
    }
    #[test]
    fn keyboard_opens_encrypted_pdf_without_mouse_input() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut dialog = PasswordDialog::default();
        dialog.open("generated.pdf".into());
        let mut frame = |events| {
            let mut request = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    request = dialog.show(
                        ui.ctx(),
                        &crate::theme::palette(ui),
                        true,
                        egui::Id::new("generated-password-dialog"),
                    )
                },
            );
            output.textures_delta.clear();
            request
        };
        for _ in 0..3 {
            assert!(frame(vec![]).is_none());
        }
        frame(vec![egui::Event::Text("generated-password".into())]);
        let request = frame(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        assert!(
            matches!(request,Some(Request::OpenPassword(path,password)) if path==std::path::Path::new("generated.pdf") && password=="generated-password")
        );
    }
}
