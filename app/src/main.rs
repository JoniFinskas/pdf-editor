#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod recovery;
mod theme;
mod worker;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
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
