use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::Request,
};
use eframe::egui;
#[derive(Default)]
pub struct Organize {
    pub open: bool,
    range: String,
    groups: String,
    paths: Vec<std::path::PathBuf>,
}
impl Organize {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ToolWindow::new("Merge, split and extract", Icon::Layers)
            .width(420.0)
            .show(ctx, p, &mut self.open, |ui| {
                let width = ui.available_width();
                widgets::subheading(ui, p, "Merge");
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
                        request = Some(Request::Merge(self.paths.clone()));
                    }
                    if Btn::new("Choose PDFs…")
                        .with_icon(Icon::FilePlus)
                        .look(Look::Outline)
                        .enabled(idle)
                        .show(ui, p)
                        .clicked()
                        && let Some(paths) = rfd::FileDialog::new()
                            .add_filter("PDF", &["pdf"])
                            .pick_files()
                    {
                        self.paths = paths;
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
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("PDF", &["pdf"])
                            .set_file_name("extracted.pdf")
                            .save_file()
                    {
                        request = Some(Request::Extract(self.range.clone(), path));
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
                        && let Some(folder) = rfd::FileDialog::new().pick_folder()
                    {
                        request = Some(Request::Split(self.groups.clone(), folder));
                    }
                });
                widgets::hint(
                    ui,
                    p,
                    "Exports use new filenames and do not alter the open document. Exported PDFs have no password.",
                );
            });
        request
    }
}
