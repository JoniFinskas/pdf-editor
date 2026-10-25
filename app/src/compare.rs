//! Read-only comparison session with bounded background page processing.
use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{Btn, Look, ToolWindow},
};
use eframe::egui::{self, Color32, TextureHandle};
use pdf_editor_core::{ChangeKind, ComparePage, Engine, PagePair};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
struct Session {
    tx: mpsc::SyncSender<(u16, bool)>,
    rx: mpsc::Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

struct Region {
    kind: ChangeKind,
    bounds: [Option<[f32; 4]>; 2],
    before: Option<String>,
    after: Option<String>,
}
fn change_style(kind: ChangeKind) -> (&'static str, Color32) {
    match kind {
        ChangeKind::Added => ("Added", Color32::from_rgb(18, 130, 70)),
        ChangeKind::Removed => ("Removed", Color32::from_rgb(205, 45, 65)),
        ChangeKind::Changed => ("Changed", Color32::from_rgb(170, 105, 0)),
    }
}
fn regions(page: &ComparePage) -> Vec<Region> {
    let mut left = page.left_marks.iter();
    let mut right = page.right_marks.iter();
    page.changes
        .iter()
        .map(|change| Region {
            kind: change.kind,
            before: change.before.clone(),
            after: change.after.clone(),
            bounds: [
                change.left.and_then(|_| left.next().map(|(_, b)| *b)),
                change.right.and_then(|_| right.next().map(|(_, b)| *b)),
            ],
        })
        .collect()
}
fn fitted_scale(width: f32, sizes: [egui::Vec2; 2], zoom: f32) -> f32 {
    ((width - 16.0).max(1.0) / (sizes[0].x + sizes[1].x).max(1.0)).min(1.0) * zoom.clamp(0.25, 3.0)
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
enum Event {
    Aligned(Vec<PagePair>),
    Aligning(usize, usize),
    AlignmentNote(String),
    Summary(usize, usize, usize),
    ScanError(usize, String),
    Page(u16, bool, pdf_editor_core::Result<ComparePage>),
    Error(String),
}

#[derive(Default)]
struct PreviewCache(Option<(u16, ComparePage)>);

impl PreviewCache {
    fn page(
        &mut self,
        row: u16,
        overlay: bool,
        load: impl FnOnce() -> pdf_editor_core::Result<ComparePage>,
    ) -> pdf_editor_core::Result<ComparePage> {
        if self.0.as_ref().is_none_or(|(cached, _)| *cached != row) {
            self.0 = Some((row, load()?));
        }
        let mut page = self.0.as_ref().unwrap().1.clone();
        if overlay {
            pdf_editor_core::compare_pixels(&mut page.left, &mut page.right, true)?;
        }
        Ok(page)
    }
}
fn start(
    paths: [PathBuf; 2],
    passwords: [String; 2],
    ctx: egui::Context,
    by_number: bool,
    documents: Option<[Arc<pdf_editor_core::Document>; 2]>,
    origin: Option<PathBuf>,
) -> Session {
    let (tx, commands) = mpsc::sync_channel::<(u16, bool)>(1);
    let (events, rx) = mpsc::sync_channel(2);
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let send = |event| {
            let delivered = events.send(event).is_ok();
            ctx.request_repaint();
            delivered
        };
        let setup = || -> pdf_editor_core::Result<_> {
            #[cfg(not(test))]
            let exe = std::env::current_exe()?;
            #[cfg(test)]
            let exe =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/pdf-editor.exe");
            let lib = exe
                .parent()
                .ok_or_else(|| pdf_editor_core::Error("Program folder unavailable".into()))?
                .join("pdfium.dll");
            let engine = Engine::new(&lib)?;
            let left = match &documents {
                Some(docs) if origin.is_none() => docs[0].clone(),
                _ => Arc::new(
                    engine
                        .open_with_password(origin.as_ref().unwrap_or(&paths[0]), &passwords[0])?,
                ),
            };
            let right = match &documents {
                Some(docs) => docs[1].clone(),
                None => Arc::new(engine.open_with_password(&paths[1], &passwords[1])?),
            };
            let counts = [engine.pages(&left)?.len(), engine.pages(&right)?.len()];
            Ok((engine, left, right, counts))
        };
        let (engine, left, right, counts) = match setup() {
            Ok(v) => v,
            Err(e) => {
                send(Event::Error(e.to_string()));
                return;
            }
        };
        let numbered = || {
            (0..counts[0].max(counts[1]))
                .map(|i| PagePair {
                    left: (i < counts[0]).then_some(i as u16),
                    right: (i < counts[1]).then_some(i as u16),
                })
                .collect::<Vec<_>>()
        };
        let mut fingerprints = [Vec::new(), Vec::new()];
        let mut failed = false;
        if !by_number {
            let total = counts[0] + counts[1];
            let mut done = 0;
            for (side, doc) in [&left, &right].into_iter().enumerate() {
                for page in 0..counts[side] {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if done % 16 == 0 && !send(Event::Aligning(done, total)) {
                        return;
                    }
                    match engine.comparison_fingerprint(doc, page as u16) {
                        Ok(value) => fingerprints[side].push(value),
                        Err(_) => {
                            failed = true;
                            break;
                        }
                    }
                    done += 1;
                }
                if failed {
                    break;
                }
            }
        }
        let pairs = if by_number || failed {
            numbered()
        } else {
            match pdf_editor_core::align_comparison_pages(&fingerprints[0], &fingerprints[1]) {
                Ok(pairs) => pairs,
                Err(_) => {
                    failed = true;
                    numbered()
                }
            }
        };
        if failed
            && !send(Event::AlignmentNote(
                "Some pages could not be matched. Comparing by page number.".into(),
            ))
        {
            return;
        }
        let count = pairs.len();
        if !send(Event::Aligned(pairs.clone())) {
            return;
        }
        let compare = |row: u16, overlay| {
            let pair = pairs
                .get(row as usize)
                .ok_or_else(|| pdf_editor_core::Error("Comparison pair out of range".into()))?;
            engine.compare_page_pair(&left, &right, *pair, overlay)
        };
        let mut scan = 0;
        let mut preview = PreviewCache::default();
        while !stop.load(Ordering::Relaxed) {
            let request = if scan < count {
                commands.try_recv().ok()
            } else {
                commands
                    .recv_timeout(std::time::Duration::from_millis(100))
                    .ok()
            };
            if let Some((page, overlay)) = request {
                let result = preview.page(page, overlay, || compare(page, false));
                if !send(Event::Page(page, overlay, result)) {
                    break;
                }
            } else if scan < count {
                let event = match compare(scan as u16, false) {
                    Ok(page) => Event::Summary(scan, page.changes.len(), page.changed_pixels),
                    Err(e) => Event::ScanError(scan, e.to_string()),
                };
                if !send(event) {
                    break;
                }
                scan += 1;
            }
        }
    });
    Session { tx, rx, cancel }
}
#[derive(Default)]
pub struct Compare {
    pub open: bool,
    current_mode: bool,
    current_available: bool,
    preparing: bool,
    serial: u64,
    refresh_current: bool,
    documents: Option<[Arc<pdf_editor_core::Document>; 2]>,
    origin: Option<PathBuf>,
    suggested_origin: Option<PathBuf>,
    paths: [Option<PathBuf>; 2],
    picker: crate::dialogs::ToolDialog<PathBuf>,
    picker_side: usize,
    passwords: [String; 2],
    session: Option<Session>,
    summaries: Vec<Option<(usize, usize)>>,
    pairs: Vec<PagePair>,
    by_number: bool,
    alignment_progress: Option<(usize, usize)>,
    alignment_note: Option<String>,
    errors: Vec<(usize, String)>,
    selected: u16,
    overlay: bool,
    zoom: f32,
    waiting: bool,
    requested: Option<(u16, bool)>,
    textures: [Option<TextureHandle>; 2],
    regions: Vec<Region>,
    focused_region: Option<usize>,
    jump_region: bool,
    displayed: Option<(u16, bool)>,
    changed_pixels: usize,
    show_all_pages: bool,
    error: Option<String>,
}
impl Compare {
    #[cfg(test)]
    pub(crate) fn test_totals(&self) -> Option<(usize, usize)> {
        if self.displayed.is_none()
            || self.summaries.is_empty()
            || self.summaries.iter().any(Option::is_none)
        {
            return None;
        }
        Some(
            self.summaries
                .iter()
                .flatten()
                .fold((0, 0), |(a, b), (x, y)| (a + x, b + y)),
        )
    }
    #[cfg(test)]
    pub(crate) fn test_error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    #[cfg(test)]
    pub(crate) fn test_origin(&self) -> Option<&std::path::Path> {
        self.paths[0].as_deref()
    }
    pub fn open_current(
        &mut self,
        source: PathBuf,
        origin: Option<PathBuf>,
        suggestion: Option<PathBuf>,
    ) -> u64 {
        self.picker.cancel();
        self.clear_results();
        self.documents = None;
        self.paths = [
            Some(origin.clone().unwrap_or_else(|| source.clone())),
            Some(source),
        ];
        self.origin = origin;
        self.suggested_origin = suggestion;
        self.current_mode = true;
        self.current_available = true;
        self.open = true;
        self.passwords = Default::default();
        self.prepare_current()
    }
    pub fn prepare_current(&mut self) -> u64 {
        self.clear_results();
        self.documents = None;
        self.preparing = true;
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }
    pub fn is_current(&self) -> bool {
        self.open && self.current_mode
    }
    pub fn accepts_current(&self, id: u64) -> bool {
        self.is_current() && self.serial == id
    }
    pub fn take_refresh(&mut self) -> bool {
        std::mem::take(&mut self.refresh_current)
    }
    pub fn receive_current(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        documents: [Arc<pdf_editor_core::Document>; 2],
    ) {
        if !self.accepts_current(id) {
            return;
        }
        self.documents = Some(documents);
        self.preparing = false;
        self.begin(ctx);
    }
    pub fn preparation_failed(&mut self, message: &str) {
        if self.preparing {
            self.current_failed(self.serial, message);
        }
    }
    pub fn current_failed(&mut self, id: u64, message: &str) {
        if self.accepts_current(id) {
            self.preparing = false;
            self.error = Some(message.into());
        }
    }
    pub fn reset_document(&mut self) {
        self.open = false;
        self.close_session();
        self.paths = [None, None];
        self.current_available = false;
    }
    pub fn open_for(&mut self, source: Option<PathBuf>) {
        self.current_mode = false;
        self.current_available = false;
        self.documents = None;
        self.origin = None;
        self.preparing = false;
        if !self.open
            && let Some(source) = source
            && self.paths[0].as_ref() != Some(&source)
        {
            self.clear_results();
            self.paths = [Some(source), None];
        }
        self.open = true;
    }

    fn begin(&mut self, ctx: &egui::Context) {
        let [Some(a), Some(b)] = &self.paths else {
            return;
        };
        if self.current_mode && self.documents.is_none() {
            return;
        }
        if !self.current_mode
            && (a == b
                || a.canonicalize()
                    .ok()
                    .zip(b.canonicalize().ok())
                    .is_some_and(|(a, b)| a == b))
        {
            self.clear_results();
            self.error = Some("Choose two different files.".into());
            return;
        }
        let paths = [a.clone(), b.clone()];
        self.clear_results();
        self.selected = 0;
        self.session = Some(start(
            paths,
            self.passwords.clone(),
            ctx.clone(),
            self.by_number,
            self.documents.clone(),
            self.origin.clone(),
        ));
    }

    fn clear_results(&mut self) {
        self.session = None;
        self.textures = [None, None];
        self.requested = None;
        self.waiting = false;
        self.summaries.clear();
        self.pairs.clear();
        self.errors.clear();
        self.error = None;
        self.displayed = None;
        self.regions.clear();
        self.focused_region = None;
        self.alignment_progress = None;
        self.alignment_note = None;
    }

    fn close_session(&mut self) {
        self.picker.cancel();
        self.clear_results();
        self.passwords = Default::default();
        self.documents = None;
        self.preparing = false;
        self.refresh_current = false;
        self.serial = self.serial.wrapping_add(1);
    }

    pub fn show(&mut self, ctx: &egui::Context, p: &Palette) {
        if let Some(path) = self.picker.poll(ctx, self.open, true) {
            if self.current_mode && self.picker_side == 0 {
                self.origin = Some(path.clone());
            }
            self.passwords[self.picker_side].clear();
            self.paths[self.picker_side] = Some(path);
            self.clear_results();
            self.begin(ctx);
        }
        if !self.open {
            self.close_session();
            return;
        }
        if self.zoom == 0. {
            self.zoom = 1.;
        }
        let mut failed_session = false;
        if let Some(session) = &self.session {
            while let Ok(event) = session.rx.try_recv() {
                match event {
                    Event::Aligned(pairs) => {
                        self.summaries = vec![None; pairs.len()];
                        self.pairs = pairs;
                        self.alignment_progress = None;
                    }
                    Event::Aligning(done, total) => self.alignment_progress = Some((done, total)),
                    Event::AlignmentNote(note) => self.alignment_note = Some(note),
                    Event::Summary(i, t, v) => {
                        if let Some(s) = self.summaries.get_mut(i) {
                            *s = Some((t, v));
                        }
                    }
                    Event::ScanError(i, e) => {
                        if let Some(s) = self.summaries.get_mut(i) {
                            *s = Some((0, 0));
                        }
                        self.errors.push((i, e));
                    }
                    Event::Error(e) => {
                        self.error = Some(e);
                        self.waiting = false;
                        failed_session = true;
                    }
                    Event::Page(page, overlay, result) => {
                        self.waiting = false;
                        if page != self.selected || overlay != self.overlay {
                            self.requested = None;
                            continue;
                        }
                        match result {
                            Err(e) => {
                                self.error = Some(e.to_string());
                                self.textures = [None, None];
                                self.displayed = None;
                            }
                            Ok(page) => {
                                self.error = None;
                                if let Some(summary) =
                                    self.summaries.get_mut(self.selected as usize)
                                {
                                    *summary = Some((page.changes.len(), page.changed_pixels));
                                }
                                self.errors
                                    .retain(|(index, _)| *index != self.selected as usize);
                                self.regions = regions(&page);
                                self.changed_pixels = page.changed_pixels;
                                self.displayed = Some((self.selected, overlay));
                                self.textures = [page.left, page.right].map(|image| {
                                    image.map(|im| {
                                        ctx.load_texture(
                                            "compare-page",
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [im.width as usize, im.height as usize],
                                                &im.rgba,
                                            ),
                                            egui::TextureOptions::LINEAR,
                                        )
                                    })
                                });
                            }
                        }
                    }
                }
            }
        }
        if failed_session {
            self.session = None;
        }
        let mut open = self.open;
        let mut window = ToolWindow::new("Compare PDFs", Icon::Layers)
            .width((ctx.content_rect().width() - 48.0).clamp(280.0, 1300.0));
        if self.current_mode || !self.summaries.is_empty() {
            window = window.height(ctx.content_rect().height() - 80.0);
        }
        window.centered().show(ctx, p, &mut open, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.current_available
                    && Btn::new("Current edits")
                        .id("compare-current")
                        .look(if self.current_mode {
                            Look::Active
                        } else {
                            Look::Ghost
                        })
                        .show(ui, p)
                        .clicked()
                    && !self.current_mode
                {
                    self.refresh_current = true;
                }
                if Btn::new("Two files")
                    .id("compare-files")
                    .look(if !self.current_mode {
                        Look::Active
                    } else {
                        Look::Ghost
                    })
                    .show(ui, p)
                    .clicked()
                    && self.current_mode
                {
                    self.current_mode = false;
                    self.preparing = false;
                    self.serial = self.serial.wrapping_add(1);
                    self.documents = None;
                    self.origin = None;
                    self.paths[1] = None;
                    self.clear_results();
                }
                let options = Btn::icon(Icon::Gear)
                    .id("compare-options")
                    .tip("Comparison options")
                    .show(ui, p);
                egui::Popup::menu(&options).show(|ui| {
                    if ui
                        .checkbox(&mut self.by_number, "Match page numbers")
                        .changed()
                    {
                        self.begin(ctx);
                    }
                    if !self.current_mode || self.origin.is_some() {
                        for i in 0..if self.current_mode { 1 } else { 2 } {
                            ui.label(if i == 0 {
                                "Original password"
                            } else {
                                "Revised password"
                            });
                            ui.add(
                                egui::TextEdit::singleline(&mut self.passwords[i])
                                    .password(true)
                                    .desired_width(180.0),
                            );
                        }
                    }
                    if Btn::new("Compare again")
                        .id("compare-start")
                        .show(ui, p)
                        .clicked()
                    {
                        self.begin(ctx);
                        ui.close();
                    }
                });
                if Btn::icon(Icon::RotateRight)
                    .id("compare-refresh")
                    .tip("Refresh comparison")
                    .enabled(!self.preparing)
                    .show(ui, p)
                    .clicked()
                {
                    if self.current_mode {
                        self.refresh_current = true;
                    } else {
                        self.begin(ctx);
                    }
                }
            });
            ui.columns(2, |columns| {
                for (i, ui) in columns.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.strong(if i == 0 { "Original" } else { "Revised" });
                        let name = if self.current_mode && i == 1 {
                            "Current edits".into()
                        } else {
                            self.paths[i]
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Choose PDF".into())
                        };
                        if (!self.current_mode || i == 0)
                            && Btn::icon(Icon::Folder)
                                .id(if i == 0 {
                                    "compare-original"
                                } else {
                                    "compare-revised"
                                })
                                .tip(if i == 0 {
                                    "Choose original"
                                } else {
                                    "Choose revised"
                                })
                                .enabled(!self.picker.busy() && !self.preparing)
                                .show(ui, p)
                                .clicked()
                        {
                            self.picker_side = i;
                            self.picker.launch(|| {
                                rfd::FileDialog::new()
                                    .add_filter("PDF", &["pdf"])
                                    .pick_file()
                            });
                        }
                        ui.add(egui::Label::new(name).truncate());
                    });
                }
            });
            if self.current_mode
                && self.origin.is_none()
                && let Some(path) = self.suggested_origin.clone()
                && Btn::new("Use suggested original")
                    .id("compare-suggested-original")
                    .tip(&format!(
                        "Filename match: {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ))
                    .enabled(!self.preparing)
                    .show(ui, p)
                    .clicked()
            {
                self.paths[0] = Some(path.clone());
                self.origin = Some(path);
                self.begin(ctx);
            }
            self.picker.feedback(ui, p);
            if let Some(error) = self.error.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(p.error_fg, error);
                    if Btn::new("Dismiss").show(ui, p).clicked() {
                        self.error = None;
                    }
                    if Btn::new("Retry page")
                        .enabled(self.session.is_some() && !self.waiting)
                        .show(ui, p)
                        .clicked()
                    {
                        self.requested = None;
                    }
                });
            }
            if let Some((done, total)) = self.alignment_progress {
                ui.label(format!("Matching pages {done} / {total}…"));
            }
            if let Some(note) = &self.alignment_note {
                ui.colored_label(p.warn_fg, note);
            }
            if self.summaries.is_empty() {
                if self.preparing || self.session.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Comparing…");
                    });
                } else if self.error.is_none() {
                    ui.label("Choose two PDFs.");
                }
                return;
            }
            ui.separator();
            let count = self.summaries.len();
            ui.horizontal_wrapped(|ui| {
                for (label, overlay) in [("Text", false), ("Visual", true)] {
                    if Btn::new(label)
                        .id(if overlay {
                            "compare-visual"
                        } else {
                            "compare-text"
                        })
                        .look(if self.overlay == overlay {
                            Look::Active
                        } else {
                            Look::Ghost
                        })
                        .tip(if overlay {
                            "Highlight changed pixels in magenta"
                        } else {
                            "Highlight text changes"
                        })
                        .show(ui, p)
                        .clicked()
                    {
                        self.overlay = overlay;
                    }
                }
                if Btn::new("Fit").id("compare-fit").show(ui, p).clicked() {
                    self.zoom = 1.0;
                }
                ui.add(
                    egui::DragValue::new(&mut self.zoom)
                        .speed(0.05)
                        .range(0.25..=3.0)
                        .custom_formatter(|n, _| format!("{}%", (n * 100.0).round() as u32))
                        .custom_parser(|s| {
                            s.trim_end_matches('%')
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .map(|n| n / 100.0)
                        }),
                );
                ui.separator();
                if Btn::icon(Icon::ArrowLeft)
                    .id("compare-page-prev")
                    .tip("Previous page")
                    .enabled(self.selected > 0)
                    .show(ui, p)
                    .clicked()
                {
                    self.selected -= 1;
                }
                ui.label(format!("{} / {count}", self.selected as usize + 1))
                    .on_hover_text(self.pair_label(self.selected as usize));
                if Btn::icon(Icon::ArrowRight)
                    .id("compare-page-next")
                    .tip("Next page")
                    .enabled((self.selected as usize + 1) < count)
                    .show(ui, p)
                    .clicked()
                {
                    self.selected += 1;
                }
                if !self.regions.is_empty()
                    || self.changed_page(true).is_some()
                    || self.changed_page(false).is_some()
                {
                    ui.separator();
                    if Btn::new("‹ Change")
                        .id("compare-change-prev")
                        .show(ui, p)
                        .clicked()
                    {
                        if self.regions.is_empty() {
                            if let Some(page) = self.changed_page(false) {
                                self.selected = page;
                            }
                        } else {
                            self.step_change(false);
                        }
                    }
                    if Btn::new("Change ›")
                        .id("compare-change-next")
                        .show(ui, p)
                        .clicked()
                    {
                        if self.regions.is_empty() {
                            if let Some(page) = self.changed_page(true) {
                                self.selected = page;
                            }
                        } else {
                            self.step_change(true);
                        }
                    }
                }
                if self.waiting {
                    ui.spinner();
                }
            });
            let height = (ctx.content_rect().height() - 235.0).clamp(120.0, 900.0);
            let wide = ui.available_width() >= 650.0;
            if wide {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(175.0);
                        self.change_list(ui, p, height);
                    });
                    self.previews(ui, p, height);
                });
            } else {
                ui.collapsing("Pages and changes", |ui| self.change_list(ui, p, 130.0));
                self.previews(ui, p, height);
            }
        });
        self.open = open;
        if !open {
            self.close_session();
            return;
        }
        let key = (self.selected, self.overlay);
        if !self.waiting
            && !self.summaries.is_empty()
            && self.requested != Some(key)
            && let Some(session) = &self.session
            && session.tx.try_send(key).is_ok()
        {
            self.waiting = true;
            self.requested = Some(key);
            if self.displayed.is_none_or(|(page, _)| page != self.selected) {
                self.textures = [None, None];
                self.displayed = None;
                self.regions.clear();
                self.focused_region = None;
            }
        }
    }

    fn pair_label(&self, index: usize) -> String {
        let pair = self.pairs.get(index).copied().unwrap_or(PagePair {
            left: Some(index as u16),
            right: Some(index as u16),
        });
        let number = |page: Option<u16>| {
            page.map_or_else(|| "—".into(), |n| (usize::from(n) + 1).to_string())
        };
        format!(
            "Original {} → Revised {}",
            number(pair.left),
            number(pair.right)
        )
    }

    fn step_change(&mut self, forward: bool) {
        let count = self.regions.len();
        if count == 0 {
            return;
        }
        self.focused_region = Some(match self.focused_region.filter(|i| *i < count) {
            Some(i) if forward => (i + 1) % count,
            Some(i) => (i + count - 1) % count,
            None if forward => 0,
            None => count - 1,
        });
        self.jump_region = true;
    }

    fn changed_page(&self, forward: bool) -> Option<u16> {
        let changed = |i: usize| {
            self.summaries[i].is_some_and(|(text, pixels)| text > 0 || pixels > 0)
                || self.errors.iter().any(|(page, _)| *page == i)
        };
        if forward {
            ((self.selected as usize + 1)..self.summaries.len())
                .find(|i| changed(*i))
                .map(|i| i as u16)
        } else {
            (0..self.selected as usize)
                .rev()
                .find(|i| changed(*i))
                .map(|i| i as u16)
        }
    }

    fn change_list(&mut self, ui: &mut egui::Ui, p: &Palette, height: f32) {
        let scanned = self.summaries.iter().filter(|s| s.is_some()).count();
        let changed = self
            .summaries
            .iter()
            .flatten()
            .filter(|(t, v)| *t > 0 || *v > 0)
            .count();
        if changed > 0 {
            ui.strong(format!("{changed} changed pages"));
        }
        if scanned < self.summaries.len() {
            ui.label(format!("Checking {scanned} / {}", self.summaries.len()));
        }
        if !self.errors.is_empty() {
            ui.colored_label(
                p.error_fg,
                format!("{} pages could not be checked", self.errors.len()),
            );
        }
        ui.checkbox(&mut self.show_all_pages, "All pages");
        if scanned == self.summaries.len() && changed == 0 && self.errors.is_empty() {
            ui.colored_label(p.ok_fg, "No differences found");
        }
        egui::ScrollArea::vertical()
            .id_salt("comparison-changes")
            .max_height(height)
            .show(ui, |ui| {
                for (i, summary) in self.summaries.iter().enumerate() {
                    if let Some((text, pixels)) = summary
                        && (self.show_all_pages || *text > 0 || *pixels > 0)
                        && !self.errors.iter().any(|(page, _)| *page == i)
                    {
                        let label = format!(
                            "{} · {text} text changes{}",
                            self.pair_label(i),
                            if *pixels > 0 { " · visual" } else { "" }
                        );
                        if ui
                            .selectable_label(self.selected as usize == i, label)
                            .clicked()
                        {
                            self.selected = i as u16;
                        }
                    }
                }
                for (i, error) in &self.errors {
                    if ui
                        .selectable_label(
                            self.selected as usize == *i,
                            format!("{} · Could not compare", self.pair_label(*i)),
                        )
                        .on_hover_text(error)
                        .clicked()
                    {
                        self.selected = *i as u16;
                    }
                }
                if self
                    .displayed
                    .is_some_and(|(page, _)| page == self.selected)
                {
                    ui.separator();
                    if !self.regions.is_empty() {
                        ui.strong(format!("{} text changes", self.regions.len()));
                    }
                    if self.regions.is_empty()
                        && self.changed_pixels > 0
                        && !self.overlay
                        && Btn::new("Show visual changes").show(ui, p).clicked()
                    {
                        self.overlay = true;
                    }
                    for (i, region) in self.regions.iter().enumerate() {
                        let (label, color) = change_style(region.kind);
                        if ui
                            .selectable_label(
                                self.focused_region == Some(i),
                                egui::RichText::new(format!("{} · {label}", i + 1)).color(color),
                            )
                            .clicked()
                        {
                            self.focused_region = Some(i);
                            self.jump_region = true;
                        }
                        if let Some(before) = &region.before {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("− {before}"))
                                        .color(change_style(ChangeKind::Removed).1),
                                )
                                .wrap(),
                            );
                        }
                        if let Some(after) = &region.after {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("+ {after}"))
                                        .color(change_style(ChangeKind::Added).1),
                                )
                                .wrap(),
                            );
                        }
                        ui.add_space(8.0);
                    }
                }
            });
    }

    fn previews(&mut self, ui: &mut egui::Ui, p: &Palette, height: f32) {
        let sizes = self.textures.each_ref().map(|t| {
            t.as_ref()
                .map_or(egui::vec2(300.0, 400.0), TextureHandle::size_vec2)
        });
        let scale = fitted_scale(ui.available_width(), sizes, self.zoom);
        let current = self
            .displayed
            .is_some_and(|(page, _)| page == self.selected);
        let visual = self.displayed.is_some_and(|(_, overlay)| overlay);
        let mut jumped = false;
        egui::ScrollArea::both()
            .id_salt(("compare-pages", self.selected))
            .max_height(height)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for (side, size) in sizes.iter().enumerate() {
                        ui.vertical(|ui| {
                            ui.set_width((size.x * scale).max(80.0));
                            let page = self
                                .pairs
                                .get(self.selected as usize)
                                .map(|pair| if side == 0 { pair.left } else { pair.right })
                                .unwrap_or(Some(self.selected));
                            ui.strong(format!(
                                "{} · {}",
                                if side == 0 { "ORIGINAL" } else { "REVISED" },
                                page.map_or_else(
                                    || "No page".into(),
                                    |n| format!("Page {}", n + 1)
                                )
                            ));
                            if current && let Some(texture) = &self.textures[side] {
                                let response = ui.add(
                                    egui::Image::new(texture).fit_to_exact_size(*size * scale),
                                );
                                ui.painter().rect_stroke(
                                    response.rect,
                                    0.0,
                                    egui::Stroke::new(1.0, p.border),
                                    egui::StrokeKind::Outside,
                                );
                                for (i, region) in self.regions.iter().enumerate() {
                                    if let Some(b) = region.bounds[side] {
                                        let rect = egui::Rect::from_min_max(
                                            response.rect.min + egui::vec2(b[0], b[1]) * scale,
                                            response.rect.min + egui::vec2(b[2], b[3]) * scale,
                                        )
                                        .expand(2.0);
                                        let active = self.focused_region == Some(i);
                                        if ui
                                            .interact(
                                                rect,
                                                egui::Id::new((
                                                    "compare-region",
                                                    self.selected,
                                                    side,
                                                    i,
                                                )),
                                                egui::Sense::click(),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.focused_region = Some(i);
                                            self.jump_region = true;
                                        }
                                        let (_, color) = change_style(region.kind);
                                        if !visual || active {
                                            ui.painter().rect_filled(
                                                rect,
                                                2.0,
                                                color.gamma_multiply(if active {
                                                    0.3
                                                } else {
                                                    0.12
                                                }),
                                            );
                                            ui.painter().rect_stroke(
                                                rect,
                                                2.0,
                                                egui::Stroke::new(
                                                    if active { 2.5 } else { 1.0 },
                                                    color,
                                                ),
                                                egui::StrokeKind::Outside,
                                            );
                                            let badge = egui::Rect::from_min_size(
                                                rect.left_top(),
                                                egui::vec2(18.0, 16.0),
                                            );
                                            ui.painter().rect_filled(badge, 2.0, color);
                                            ui.painter().text(
                                                badge.center(),
                                                egui::Align2::CENTER_CENTER,
                                                (i + 1).to_string(),
                                                egui::FontId::proportional(11.0),
                                                Color32::WHITE,
                                            );
                                        }
                                        if active && self.jump_region && !jumped {
                                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                                            jumped = true;
                                        }
                                    }
                                }
                            } else {
                                ui.allocate_ui(
                                    egui::vec2((sizes[side].x * scale).max(80.0), 200.0),
                                    |ui| {
                                        ui.label(if !current && self.waiting {
                                            "Loading page…"
                                        } else if !current {
                                            "Preview unavailable"
                                        } else if side == 0 {
                                            "Page added in Revised"
                                        } else {
                                            "Page removed from Revised"
                                        });
                                    },
                                );
                            }
                        });
                    }
                });
            });
        if jumped {
            self.jump_region = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unlocked_original_stays_available_across_current_snapshot_refreshes() {
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let engine=Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let current=Arc::new(engine.open(&root.join("tests/fixtures/generated/unicode-cid.pdf")).unwrap());
        let original=root.join("tests/fixtures/generated/encrypted.pdf");
        let ctx=egui::Context::default();crate::theme::install(&ctx);
        let mut compare=Compare::default();let id=compare.open_current(PathBuf::from("generated.pdf"),Some(original.clone()),None);
        compare.passwords[0]="fixture-reader".into();compare.receive_current(&ctx,id,[current.clone(),current.clone()]);
        for pass in 0..3 {
            if pass>0 {let id=compare.prepare_current();compare.receive_current(&ctx,id,[current.clone(),current.clone()]);}
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
            while compare.test_totals().is_none() {
                let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1200.,900.))),..Default::default()},|ui|compare.show(ui.ctx(),&crate::theme::palette(ui)));out.textures_delta.clear();
                assert!(compare.error.is_none(),"Unlock failed after refresh");assert!(std::time::Instant::now()<deadline);std::thread::yield_now();
            }
            assert_eq!(compare.origin.as_ref(),Some(&original));
        }
        compare.close_session();assert!(compare.passwords.iter().all(String::is_empty));assert!(compare.documents.is_none());
    }
    #[test]
    fn compact_comparison_keeps_key_controls_inside_small_views() {
        for size in [egui::vec2(640., 420.), egui::vec2(1200., 900.)] {
            for dark in [false, true] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let texture = ctx.load_texture(
                    "generated-page",
                    egui::ColorImage::filled([400, 600], Color32::WHITE),
                    egui::TextureOptions::LINEAR,
                );
                let mut compare = Compare {
                    open: true,
                    current_mode: true,
                    current_available: true,
                    paths: [
                        Some(PathBuf::from("long-generated-original-name.pdf")),
                        Some(PathBuf::from("generated-current.pdf")),
                    ],
                    summaries: vec![Some((1, 20)), Some((0, 0))],
                    displayed: Some((0, false)),
                    textures: [Some(texture.clone()), Some(texture)],
                    ..Default::default()
                };
                let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(bounds),
                            ..Default::default()
                        },
                        |ui| compare.show(ui.ctx(), &crate::theme::palette(ui)),
                    );
                    output.textures_delta.clear();
                    for forbidden in [
                        "Unsaved edits are not included",
                        "Numbers match the change list",
                        "changed pixels",
                        "Click a highlight or a change below",
                    ] {
                        assert!(!output.shapes.iter().any(|s|matches!(&s.shape,egui::epaint::Shape::Text(t) if t.galley.text().contains(forbidden))));
                    }
                }
                for id in [
                    "close-tool-Compare PDFs",
                    "compare-current",
                    "compare-files",
                    "compare-original",
                    "compare-options",
                    "compare-text",
                    "compare-visual",
                    "compare-page-next",
                ] {
                    let rect = ctx.read_response(crate::widgets::tool_id(id)).unwrap().rect;
                    assert!(bounds.contains_rect(rect), "{id}: {rect:?}");
                }
            }
        }
    }
    #[test]
    fn same_file_comparison_is_rejected_instead_of_reporting_no_changes() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/generated/unicode-cid.pdf");
        let ctx = egui::Context::default();
        let mut compare = Compare {
            paths: [Some(fixture.clone()), Some(fixture)],
            ..Default::default()
        };
        compare.begin(&ctx);
        assert!(compare.session.is_none());
        assert!(
            compare
                .error
                .as_ref()
                .is_some_and(|e| e.contains("different"))
        );
    }
    #[test]
    fn choosing_other_generated_pdf_starts_comparison_and_displays_results() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let fixture = root.join("tests/fixtures/generated/unicode-cid.pdf");
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut compare = Compare::default();
        compare.open_for(Some(fixture.clone()));
        assert_eq!(compare.paths[0].as_ref(), Some(&fixture));
        assert!(compare.paths[1].is_none());
        let identical = root.join(format!(
            "target/compare-identical-{}.pdf",
            std::process::id()
        ));
        std::fs::copy(&fixture, &identical).unwrap();
        for (other, changed) in [
            (identical.clone(), false),
            (root.join("tests/fixtures/generated/multi-page.pdf"), true),
        ] {
            compare.picker_side = 1;
            compare.picker.launch(move || Some(other));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| compare.show(&ctx, &crate::theme::palette(ui)),
                );
                output.textures_delta.clear();
                assert!(compare.error.is_none(), "Generated comparison failed");
                if !compare.picker.busy()
                    && compare.displayed.is_some()
                    && compare.summaries.iter().all(Option::is_some)
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "Comparison did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(compare.textures.iter().all(Option::is_some));
            assert_eq!(compare.changed_pixels > 0, changed);
            assert_eq!(!compare.regions.is_empty(), changed);
        }
        compare.open = false;
        compare.close_session();
        std::fs::remove_file(identical).unwrap();
    }
    #[test]
    fn preview_mode_switches_reuse_pristine_rasters_and_retry_failed_loads() {
        let make = || ComparePage {
            left: Some(pdf_editor_core::Image {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
            }),
            right: Some(pdf_editor_core::Image {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            }),
            changes: vec![],
            changed_pixels: 1,
            left_marks: vec![],
            right_marks: vec![],
        };
        let mut cache = PreviewCache::default();
        let plain = cache.page(0, false, || Ok(make())).unwrap();
        let overlay = cache
            .page(0, true, || panic!("must reuse cached page"))
            .unwrap();
        assert_ne!(
            plain.left.as_ref().unwrap().rgba,
            overlay.left.unwrap().rgba
        );
        let again = cache
            .page(0, false, || panic!("must reuse pristine page"))
            .unwrap();
        assert_eq!(plain.left.unwrap().rgba, again.left.unwrap().rgba);
        assert!(
            cache
                .page(1, false, || Err(pdf_editor_core::Error(
                    "generated failure".into()
                )))
                .is_err()
        );
        assert!(cache.page(1, false, || Ok(make())).is_ok());
        assert_eq!(cache.0.as_ref().unwrap().0, 1);
    }
    #[test]
    fn change_buttons_and_page_highlights_select_the_same_change() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let texture = ctx.load_texture(
            "generated",
            egui::ColorImage::filled([600, 800], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        let mut compare = Compare {
            open: true,
            displayed: Some((0, false)),
            summaries: vec![Some((2, 20))],
            textures: [Some(texture.clone()), Some(texture)],
            regions: (0..2)
                .map(|i| Region {
                    kind: ChangeKind::Changed,
                    bounds: [Some([40.0, 40.0 + i as f32 * 80.0, 200.0, 60.0 + i as f32 * 80.0]);
                        2],
                    before: Some(format!("Original line {i}")),
                    after: Some(format!("Revised line {i}")),
                })
                .collect(),
            ..Default::default()
        };
        let frame = |compare: &mut Compare, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 1000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| compare.show(&ctx, &crate::theme::palette(ui)),
            );
            output.textures_delta.clear();
        };
        let click = |compare: &mut Compare, id| {
            let response = ctx.read_response(id).unwrap();
            assert!(
                response.interact_rect.is_positive(),
                "Control clipped: {:?} / {:?}",
                response.rect,
                response.interact_rect
            );
            let at = response.interact_rect.center();
            for pressed in [true, false] {
                frame(
                    compare,
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };
        for _ in 0..3 {
            frame(&mut compare, vec![]);
        }
        click(
            &mut compare,
            egui::Id::new(("compare-region", 0u16, 1usize, 1usize)),
        );
        assert_eq!(compare.focused_region, Some(1));
        click(&mut compare, crate::widgets::tool_id("compare-change-next"));
        assert_eq!(compare.focused_region, Some(0));
        click(&mut compare, crate::widgets::tool_id("compare-change-prev"));
        assert_eq!(compare.focused_region, Some(1));
        compare.regions.clear();
        compare.step_change(true);
    }
    #[test]
    fn aligned_labels_show_real_page_numbers_and_missing_sides() {
        let compare = Compare {
            pairs: vec![
                PagePair {
                    left: None,
                    right: Some(1),
                },
                PagePair {
                    left: Some(1),
                    right: Some(2),
                },
            ],
            ..Default::default()
        };
        assert_eq!(compare.pair_label(0), "Original — → Revised 2");
        assert_eq!(compare.pair_label(1), "Original 2 → Revised 3");
    }
    #[test]
    fn paired_regions_keep_numbers_for_additions_removals_and_changes() {
        let bounds = pdf_editor_core::Bounds {
            left: 0.0,
            bottom: 0.0,
            right: 20.0,
            top: 10.0,
        };
        let page = ComparePage {
            left: None,
            right: None,
            changed_pixels: 3,
            changes: vec![
                pdf_editor_core::TextChange {
                    kind: ChangeKind::Removed,
                    left: Some(bounds),
                    right: None,
                    before: Some("removed".into()),
                    after: None,
                },
                pdf_editor_core::TextChange {
                    kind: ChangeKind::Added,
                    left: None,
                    right: Some(bounds),
                    before: None,
                    after: Some("added".into()),
                },
                pdf_editor_core::TextChange {
                    kind: ChangeKind::Changed,
                    left: Some(bounds),
                    right: Some(bounds),
                    before: Some("before".into()),
                    after: Some("after".into()),
                },
            ],
            left_marks: vec![
                (ChangeKind::Removed, [1.0; 4]),
                (ChangeKind::Changed, [3.0; 4]),
            ],
            right_marks: vec![
                (ChangeKind::Added, [2.0; 4]),
                (ChangeKind::Changed, [4.0; 4]),
            ],
        };
        let regions = regions(&page);
        assert_eq!(regions[0].bounds, [Some([1.0; 4]), None]);
        assert_eq!(regions[1].bounds, [None, Some([2.0; 4])]);
        assert_eq!(regions[2].bounds, [Some([3.0; 4]), Some([4.0; 4])]);
        for width in [240.0, 600.0, 1200.0] {
            let sizes = [egui::vec2(1000.0, 1500.0), egui::vec2(600.0, 800.0)];
            let scale = fitted_scale(width, sizes, 1.0);
            assert!(scale > 0.0 && 1600.0 * scale + 16.0 <= width + 0.01);
        }
    }

    #[test]
    fn changed_page_navigation_skips_identical_and_pending_pages() {
        let mut compare = Compare {
            summaries: vec![
                Some((0, 0)),
                Some((1, 0)),
                None,
                Some((0, 30)),
                Some((0, 0)),
            ],
            errors: vec![(4, "test error".into())],
            ..Default::default()
        };
        assert_eq!(compare.changed_page(false), None);
        assert_eq!(compare.changed_page(true), Some(1));
        compare.selected = 3;
        assert_eq!(compare.changed_page(false), Some(1));
        assert_eq!(compare.changed_page(true), Some(4));
    }

    #[test]
    fn visual_mode_keeps_current_preview_until_replacement_arrives() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let texture = ctx.load_texture(
            "generated",
            egui::ColorImage::filled([100, 140], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        let original = texture.id();
        let (tx, commands) = mpsc::sync_channel(1);
        let (events, rx) = mpsc::sync_channel(2);
        let mut compare = Compare {
            open: true,
            overlay: true,
            displayed: Some((0, false)),
            textures: [Some(texture.clone()), Some(texture)],
            summaries: vec![Some((1, 10))],
            session: Some(Session {
                tx,
                rx,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            ..Default::default()
        };
        let render = |compare: &mut Compare| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1180.0, 820.0),
                    )),
                    ..Default::default()
                },
                |ui| compare.show(&ctx, &crate::theme::palette(ui)),
            );
            output.textures_delta.clear();
        };
        render(&mut compare);
        assert_eq!(commands.try_recv().unwrap(), (0, true));
        assert!(compare.waiting);
        assert_eq!(compare.textures[0].as_ref().unwrap().id(), original);
        events
            .send(Event::Page(
                0,
                true,
                Err(pdf_editor_core::Error("test error".into())),
            ))
            .unwrap();
        render(&mut compare);
        assert!(!compare.waiting);
        assert!(compare.textures[0].is_none());
        assert!(compare.displayed.is_none());
        assert!(compare.error.is_some());
    }
    #[test]
    fn closing_comparison_discards_late_picker_and_worker_results() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut compare = Compare {
            open: true,
            ..Default::default()
        };
        let render = |compare: &mut Compare| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1180., 820.),
                    )),
                    ..Default::default()
                },
                |ui| compare.show(&ctx, &crate::theme::palette(ui)),
            );
            output.textures_delta.clear();
        };
        let (release, wait) = mpsc::channel();
        compare.picker.launch(move || {
            wait.recv().unwrap();
            Some(PathBuf::from("late-generated.pdf"))
        });
        let (tx, _commands) = mpsc::sync_channel(1);
        let (events, rx) = mpsc::sync_channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        compare.session = Some(Session {
            tx,
            rx,
            cancel: cancelled.clone(),
        });
        compare.summaries.push(Some((1, 12)));
        compare.alignment_progress = Some((16, 32));
        compare.passwords = ["generated-secret".into(), String::new()];
        compare.open = false;
        render(&mut compare);
        assert!(cancelled.load(Ordering::Relaxed));
        assert!(events.send(Event::Summary(0, 99, 99)).is_err());
        assert!(compare.summaries.is_empty());
        assert!(compare.alignment_progress.is_none());
        assert!(compare.passwords.iter().all(String::is_empty));
        compare.open = true;
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while compare.picker.busy() {
            render(&mut compare);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(compare.paths.iter().all(Option::is_none));
        compare.picker_side = 1;
        compare
            .picker
            .launch(|| Some(PathBuf::from("fresh-generated.pdf")));
        while compare.picker.busy() {
            render(&mut compare);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(compare.paths[0].is_none());
        assert_eq!(compare.paths[1], Some(PathBuf::from("fresh-generated.pdf")));
    }

    #[test]
    fn comparison_window_draws_without_loading_a_document() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut compare = Compare {
            open: true,
            ..Default::default()
        };
        for dark in [false, true] {
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1180., 820.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    compare.show(&ctx, &crate::theme::palette(ui));
                },
            );
            output.textures_delta.clear();
            assert!(compare.session.is_none());
            assert!(compare.paths.iter().all(Option::is_none));
        }
    }
}
