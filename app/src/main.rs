#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icons;
mod recovery;
mod shortcuts;
mod signatures;
mod theme;
mod widgets;
mod worker;

use eframe::egui;

fn main() -> eframe::Result {
    let initial = pdf_editor_core::startup_path(std::env::args_os().skip(1));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PDF Editor")
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([640.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "PDF Editor",
        options,
        Box::new(|cc| Ok(Box::new(app::EditorApp::new(cc, initial)))),
    )
}
