#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod snapping;
mod dialogs;
mod navigation;
mod bookmarks;
mod cli;
mod image_create;
mod scan_cleanup;

mod compare;
mod comparison_origin;
#[cfg(all(test,windows))]
mod native_controls;
mod compression;
mod crop;
mod drawing;
mod export;
mod forms;
mod form_design;
mod icons;
mod instance;
mod markup;
mod links;
mod organize;
mod paragraph;
mod passwords;
mod pictures;
mod recovery;
mod session_state;
mod redaction;
mod sanitize;

mod privacy;
mod replace;
mod shortcuts;
mod signatures;
mod stamps;
mod table_export;

mod spelling;
mod theme;
mod widgets;
mod windows_integration;
mod word_export;
mod worker;
mod window_state;

use eframe::egui;

fn main() -> eframe::Result {
    if let Some(code) = cli::dispatch(std::env::args_os().skip(1).collect()) {
        std::process::exit(code);
    }

    let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let action = args
        .first()
        .and_then(|a| a.to_str())
        .and_then(|a| a.strip_prefix("--shell-"))
        .filter(|a| matches!(*a, "open" | "merge" | "compress" | "export"))
        .unwrap_or("open")
        .to_string();
    if args.first().and_then(|a| a.to_str()).is_some_and(|a| {
        matches!(
            a,
            "--shell-open" | "--shell-merge" | "--shell-compress" | "--shell-export"
        )
    }) {
        args.remove(0);
    }
    let initial = pdf_editor_core::startup_path(args);
    if let Err(error) = &initial {
        rfd::MessageDialog::new()
            .set_title("PDF Editor")
            .set_description(error.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
        return Ok(());
    }
    let request = initial
        .as_ref()
        .ok()
        .and_then(|p| p.as_ref())
        .map(|p| instance::Incoming {
            action: action.clone(),
            path: if p.is_absolute() {
                p.clone()
            } else {
                std::env::current_dir().unwrap_or_default().join(p)
            },
        });
    let owner = match instance::Instance::start(request.as_ref()) {
        Ok(instance::Start::Forwarded) => return Ok(()),
        Ok(instance::Start::Owner(owner)) => Some(owner),
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title("PDF Editor")
                .set_description(format!("Cannot contact the running editor: {e}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Ok(());
        }
    };
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
        Box::new(|cc| {
            Ok(Box::new(
                app::TabbedApp::new(&cc.egui_ctx, initial).with_instance(owner, action).with_native_guard(crate::window_state::native_guard(cc)),
            ))
        }),
    )
}
