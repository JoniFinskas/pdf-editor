use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
enum Selection {
    Paths(Vec<std::path::PathBuf>),
    Export(Request),
}
#[derive(Default)]
pub struct Organize {
    pub open: bool,
    dialog: crate::dialogs::ToolDialog<Selection>,
    range: String,
    groups: String,
    paths: Vec<std::path::PathBuf>,
    shell_collecting: bool,
}
impl Organize {
    pub fn collect_shell_selection(&mut self) {
        self.open = true;
        self.shell_collecting = true;
    }
    pub fn collecting_shell_selection(&self) -> bool {
        self.open && self.shell_collecting
    }

    pub fn add_path(&mut self, path: std::path::PathBuf) -> bool {
        self.open = true;
        if self.paths.contains(&path) {
            return true;
        }
        if self.paths.len() >= 100 {
            return false;
        }
        self.paths.push(path);
        true
    }
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        if let Some(selection) = self.dialog.poll(ctx, self.open, idle) {
            match selection {
                Selection::Paths(paths) => self.paths = paths.into_iter().take(100).collect(),
                Selection::Export(value) => request = Some(value),
            }
        }
        let idle = idle && !self.dialog.busy() && request.is_none();
        ToolWindow::new("Merge, split and extract", Icon::Layers)
            .width(420.0)
            .show(ctx, p, &mut self.open, |ui| {
                self.dialog.feedback(ui,p);
                let width = ui.available_width();
                widgets::subheading(ui, p, "Merge");
                if self.shell_collecting { widgets::hint(ui, p, "Pending Explorer selection. Additional selections join this list until Merge or Close. Review the order below."); }
                widgets::hint(ui, p, "Append PDFs after the current document.");
                let mut movement = None;
                for (i, path) in self.paths.iter().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            widgets::icon(ui, Icon::File, 16.0, p.muted);
                            ui.add(
                                egui::Label::new(
                                    path.file_name().unwrap_or_default().to_string_lossy(),
                                )
                                .truncate(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    if Btn::icon(Icon::ArrowDown)
                                        .height(28.0)
                                        .tip("Move down")
                                        .enabled(i + 1 < self.paths.len())
                                        .show(ui, p)
                                        .clicked()
                                    {
                                        movement = Some((i, i + 1));
                                    }
                                    if Btn::icon(Icon::ArrowUp)
                                        .height(28.0)
                                        .tip("Move up")
                                        .enabled(i > 0)
                                        .show(ui, p)
                                        .clicked()
                                    {
                                        movement = Some((i, i - 1));
                                    }
                                },
                            );
                        });
                    });
                }
                if let Some((a, b)) = movement {
                    self.paths.swap(a, b);
                }
                widgets::actions(ui, |ui| {
                    if Btn::new("Merge in listed order")
                        .look(Look::Primary)
                        .enabled(idle && !self.paths.is_empty())
                        .show(ui, p)
                        .clicked()
                    {
                        self.shell_collecting = false;
                        request = Some(Request::Merge(self.paths.clone()));
                    }
                    if Btn::new("Choose PDFs…")
                        .with_icon(Icon::FilePlus)
                        .look(Look::Outline)
                        .enabled(idle)
                        .show(ui, p)
                        .clicked()
                    {
                        self.dialog.launch(|| rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_files().map(Selection::Paths));
                    }
                });
                ui.add_space(4.0);
                widgets::rule(ui, p);
                widgets::subheading(ui, p, "Extract pages");
                widgets::field(ui, p, "Pages, for example 1-3,5", |ui| {
                    ui.add(widgets::text_field(&mut self.range, width));
                });
                widgets::actions(ui, |ui| {
                    if Btn::new("Extract to new PDF…")
                        .with_icon(Icon::Export)
                        .look(Look::Outline)
                        .enabled(idle && !self.range.is_empty())
                        .show(ui, p)
                        .clicked()
                    {
                        let range=self.range.clone();
                        self.dialog.launch(move || rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name("extracted.pdf").save_file()
                            .map(|path| Selection::Export(Request::Extract(range,path))));
                    }
                });
                ui.add_space(4.0);
                widgets::rule(ui, p);
                widgets::subheading(ui, p, "Split");
                widgets::field(ui, p, "Groups, for example 1-3;4-6;7", |ui| {
                    ui.add(widgets::text_field(&mut self.groups, width));
                });
                widgets::actions(ui, |ui| {
                    if Btn::new("Split into folder…")
                        .with_icon(Icon::Folder)
                        .look(Look::Outline)
                        .enabled(idle && !self.groups.is_empty())
                        .show(ui, p)
                        .clicked()
                    {
                        let groups=self.groups.clone();
                        self.dialog.launch(move || rfd::FileDialog::new().pick_folder()
                            .map(|folder| Selection::Export(Request::Split(groups,folder))));
                    }
                });
                widgets::hint(
                    ui,
                    p,
                    "Exports use new filenames and do not alter the open document. Exported PDFs have no password.",
                );
            });
        if !self.open {
            self.shell_collecting = false;
        }
        if !self.open { self.dialog.cancel(); return None; }
        request
    }
}

#[cfg(test)]
mod shell_tests {
    use super::*;
    #[test]
    fn shell_selection_keeps_order_deduplicates_and_is_bounded() {
        let mut tool = Organize::default();
        tool.collect_shell_selection();
        assert!(tool.collecting_shell_selection());
        tool.add_path("second.pdf".into());
        tool.add_path("third.pdf".into());
        tool.add_path("second.pdf".into());
        assert!(tool.open);
        assert_eq!(
            tool.paths,
            vec![
                std::path::PathBuf::from("second.pdf"),
                std::path::PathBuf::from("third.pdf")
            ]
        );
        for i in 0..200 {
            tool.add_path(format!("generated-{i}.pdf").into());
        }
        assert_eq!(tool.paths.len(), 100);
        assert!(!tool.add_path("overflow.pdf".into()));
        tool.open = false;
        assert!(!tool.collecting_shell_selection());
    }
}
