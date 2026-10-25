// Opt-in real eframe window; no normal startup, settings, instance or recovery state.
use super::*;
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
const TITLE: &str = "PDF Editor generated desktop acceptance";
#[derive(Debug)]
struct GeneratedDrop(PathBuf);
impl egui::DroppedFile for GeneratedDrop {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}
struct Acceptance {
    editor: EditorApp,
    folder: PathBuf,
    input: PathBuf,
    dropped: PathBuf,
    output: PathBuf,
    use_pickers: bool,
    stage: u8,
    cycle: usize,
    frame: usize,
    started: Instant,
    dialog: Option<mpsc::Receiver<Result<(), String>>>,
    events: Vec<egui::Event>,
    drop_pending: bool,
    shot: Option<usize>,
    shots: usize,
    done: Arc<Mutex<bool>>,
}
impl Acceptance {
    fn select(&mut self, path: PathBuf) {
        let (sender, receiver) = mpsc::channel();
        self.dialog = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(crate::native_controls::select_file(&path));
        });
    }
    fn text(&mut self) {
        self.editor.busy += 1;
        self.editor.send(Request::AddText {
            text: pdf_editor_core::NewText {
                page: 0,
                x: 30.,
                y: 30.,
                text: "AB".into(),
                font: "Arial".into(),
                size: 12.,
                color: [0, 0, 0],
            },
            raster: (30., 30.),
        });
    }
}
impl eframe::App for Acceptance {
    fn raw_input_hook(&mut self, _ctx: &Context, raw: &mut egui::RawInput) {
        if !self.events.is_empty() {
            raw.focused = true;
            if let Some(viewport) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
                viewport.focused = Some(true);
            }
        }
        raw.events.append(&mut self.events);
        if self.drop_pending {
            raw.dropped_files
                .push(Arc::new(GeneratedDrop(self.dropped.clone())));
            self.drop_pending = false;
        }
        for event in &raw.events {
            if let egui::Event::Screenshot { image, .. } = event
                && let Some(scale) = self.shot.take()
            {
                assert!(image.size[0] >= 500 && image.size[1] >= 300);
                let picture = pdf_editor_core::Picture {
                    width: image.size[0] as u32,
                    height: image.size[1] as u32,
                    rgba: image
                        .pixels
                        .iter()
                        .flat_map(|pixel| pixel.to_array())
                        .collect(),
                };
                let png = pdf_editor_core::store_signature(&self.folder, &picture).unwrap();
                std::fs::rename(png, self.folder.join(format!("native-scale-{scale}.png")))
                    .unwrap();
                self.shots += 1;
            }
        }
    }
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        assert!(
            self.started.elapsed() < Duration::from_secs(55),
            "Native acceptance timed out at stage {}",
            self.stage
        );
        if let Some(receiver) = &self.dialog
            && let Ok(result) = receiver.try_recv()
        {
            result.unwrap();
            self.dialog = None;
        }
        self.editor.ui(ui, frame);
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Title(TITLE.into()));
        let ctx = ui.ctx();
        self.frame += 1;
        ctx.request_repaint_after(Duration::from_millis(15));
        if self.stage == 4 && self.frame.is_multiple_of(120) {
            eprintln!(
                "native drop state: busy={}, source_match={}, objects={}, confirm={}",
                self.editor.busy,
                self.editor.doc.as_ref().is_some_and(
                    |doc| doc.source.canonicalize().ok() == self.dropped.canonicalize().ok()
                ),
                self.editor
                    .doc
                    .as_ref()
                    .is_some_and(|doc| doc.pages[0].objects.is_some()),
                self.editor.confirm.is_some()
            );
        }
        match self.stage {
            0 => {
                if self.use_pickers {
                    self.editor.open_dialog_or_path(None);
                    self.select(self.input.clone());
                } else {
                    self.editor.open_path(self.input.clone());
                }
                self.stage = 1;
            }
            1 if self.editor.busy == 0
                && self
                    .editor
                    .doc
                    .as_ref()
                    .is_some_and(|doc| doc.source == self.input) =>
            {
                self.text();
                self.stage = 2;
            }
            2 if self.editor.busy == 0 => {
                assert!(self.editor.doc.as_ref().unwrap().dirty());
                if self.use_pickers {
                    self.editor.save_as();
                    self.select(self.output.clone());
                } else {
                    self.editor
                        .document_action(Request::SaveAs(self.output.clone()));
                }
                self.stage = 3;
            }
            3 if self.editor.busy == 0 && self.output.is_file() => {
                assert!(!self.editor.doc.as_ref().unwrap().dirty());
                crate::native_controls::drop_generated_file(TITLE, &self.dropped).unwrap();
                self.stage = 4;
            }
            4 if self.editor.busy == 0
                && self.editor.doc.as_ref().is_some_and(|doc| {
                    doc.source.canonicalize().ok() == self.dropped.canonicalize().ok()
                        && doc.pages[0].objects.is_some()
                }) =>
            {
                let doc = self.editor.doc.as_mut().unwrap();
                let line = doc.pages[0].objects.as_ref().unwrap().lines[0].clone();
                doc.edit = Some(EditState {
                    page: 0,
                    text: line.text.clone(),
                    line,
                    focus: true,
                    rejected_text: None,
                });
                self.stage = 5;
                self.frame = 0;
                ctx.set_pixels_per_point(1.);
            }
            5 => {
                if self.shot.is_none() && self.shots == self.cycle && self.frame >= 20 {
                    let scale = [100, 125, 150, 200][self.cycle];
                    self.shot = Some(scale);
                    ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
                }
                if self.shots > self.cycle {
                    self.cycle += 1;
                    self.frame = 0;
                    if self.cycle == 4 {
                        self.stage = 6;
                        self.cycle = 0;
                        self.editor.doc.as_mut().unwrap().edit = None;
                        ctx.set_pixels_per_point(1.);
                    } else {
                        ctx.set_visuals(if self.cycle.is_multiple_of(2) {
                            egui::Visuals::dark()
                        } else {
                            egui::Visuals::light()
                        });
                        ctx.set_pixels_per_point([1., 1.25, 1.5, 2.][self.cycle]);
                    }
                }
            }
            6 if self.editor.busy == 0 => {
                if self.cycle == 40 {
                    self.text();
                    self.stage = 7;
                } else {
                    if self.cycle.is_multiple_of(2) {
                        self.text();
                    } else {
                        self.editor.undo();
                    }
                    self.cycle += 1;
                }
            }
            7 if self.editor.busy == 0 => {
                assert!(self.editor.doc.as_ref().unwrap().dirty());
                crate::native_controls::close_window(TITLE).unwrap();
                self.stage = 8;
                self.frame = 0;
            }
            8 if self.frame >= 10 && matches!(self.editor.confirm, Some(Pending::Close)) => {
                // Deliver a pointer click through the native frame's input boundary.
                if let Some(response) = ctx.read_response(widgets::tool_id("unsaved-cancel")) {
                    let pos = response.rect.center();
                    self.events = vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: Default::default(),
                        },
                    ];
                    self.stage = 9;
                    self.frame = 0;
                }
            }
            9 if self.editor.confirm.is_none() => {
                assert!(self.editor.doc.as_ref().unwrap().dirty());
                assert_eq!(self.shots, 4);
                *self.done.lock().unwrap() = true;
                self.editor.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            9 if self.frame == 1 => {
                let pos = ctx
                    .read_response(widgets::tool_id("unsaved-cancel"))
                    .unwrap()
                    .rect
                    .center();
                self.events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ];
            }
            _ => (),
        }
    }
}

#[test]
#[ignore = "Runs an isolated native desktop window with generated PDFs and owned dialogs"]
fn native_desktop_generated_open_save_drop_close_and_scaled_rendering() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join(format!("target/native-desktop-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let input = folder.join("generated-source.pdf");
    let dropped = folder.join("generated-drop.pdf");
    let output = folder.join("generated-copy.pdf");
    std::fs::copy(
        root.join("tests/fixtures/generated/unicode-cid.pdf"),
        &input,
    )
    .unwrap();
    std::fs::copy(
        root.join("tests/fixtures/generated/transformed.pdf"),
        &dropped,
    )
    .unwrap();
    let source = std::fs::read(&input).unwrap();
    let completed = Arc::new(Mutex::new(false));
    let done = completed.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([900., 700.])
            .with_drag_and_drop(true),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    let library = root.join("target/debug/pdfium.dll");
    let test_folder = folder.clone();
    let test_input = input.clone();
    eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let (mut editor, _, _) = mock_app();
            editor.worker = Worker::spawn(
                cc.egui_ctx.clone(),
                library,
                Substitution::Line,
                Default::default(),
            );
            editor.tabbed = false;
            editor.recent_files = vec![test_input.clone()];
            Ok(Box::new(Acceptance {
                editor,
                folder: test_folder,
                input: test_input,
                dropped,
                output,
                use_pickers: std::env::var_os("PDF_EDITOR_NATIVE_PICKERS").is_some(),
                stage: 0,
                cycle: 0,
                frame: 0,
                started: Instant::now(),
                dialog: None,
                events: Vec::new(),
                drop_pending: false,
                shot: None,
                shots: 0,
                done,
            }))
        }),
    )
    .unwrap();
    assert!(
        *completed.lock().unwrap(),
        "Native acceptance did not complete"
    );
    assert_eq!(std::fs::read(input).unwrap(), source);
    assert!(!folder.join("settings.txt").exists());
}
