//! Editor window: toolbar, page canvas, in-place edit box, banners, modals
//! and the settings page.

use crate::recovery::{self, AutoSave, Orphan, Session};
use crate::theme::{self, Palette};
use crate::worker::{ChangeKind, PageTransform, Reply, Request, Worker};
use eframe::egui::{
    self, Align, Align2, Button, Color32, Context, CornerRadius, CursorIcon, FontId, Frame, Id,
    Key, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Shadow, Stroke, StrokeKind,
    TextEdit, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand, pos2, vec2,
};
use pdf_editor_core::{PageInfo, Substitution, TextObject};
use std::path::PathBuf;

/// Workspace version from the root Cargo.toml; see VERSIONING.md.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// UI points per PDF point at 100% zoom (96 dpi screen, 72 dpi PDF).
const PT_TO_UI: f32 = 96.0 / 72.0;
const ZOOM_STEPS: [f32; 15] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0,
];
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const PAGE_GAP: f32 = 24.0;
const CANVAS_PAD: f32 = 32.0;
/// Largest rendered texture side in pixels; higher zoom levels upscale.
const MAX_TEXTURE_SIDE: f32 = 8192.0;
/// Stays below the core's 16 megapixel render limit, leaving room for its
/// rounding of page dimensions.
const MAX_RENDER_PIXELS: f32 = 15_500_000.0;
/// Pages farther than this from the viewport release their textures.
const KEEP_PAGES: usize = 2;

pub struct EditorApp {
    dragging_page: Option<usize>,
    text_tool: TextTool,
    find: FindState,
    worker: Worker,
    engine_alive: bool,
    setup_error: Option<String>,
    policy: Substitution,
    config_path: Option<PathBuf>,
    doc: Option<DocView>,
    opening: bool,
    busy: u32,
    banners: Vec<Banner>,
    next_banner: u64,
    /// Shows the settings page instead of the document.
    settings_open: bool,
    confirm: Option<Pending>,
    allow_close: bool,
    title: String,
    autosave: AutoSave,
    autosave_path: Option<PathBuf>,
    /// This editor's recovery folder; None when it could not be created.
    session: Option<Session>,
    session_error: Option<String>,
    /// Recovery copies left by editors that did not exit normally.
    orphans: Vec<Orphan>,
    /// The copy being opened for restore.
    restoring: Option<Orphan>,
}

struct TextTool {
    selection: Vec<TextObject>,
    selected: usize,
    adding: bool,
    text: String,
    font: String,
    fonts: Vec<String>,
    size: f32,
    color: [u8; 3],
    at: Option<(u16, Pos2)>,
}
impl Default for TextTool {
    fn default() -> Self {
        Self {
            selection: vec![],
            selected: 0,
            adding: false,
            text: "New text".into(),
            font: "ArialMT".into(),
            fonts: vec![],
            size: 16.0,
            color: [0, 0, 0],
            at: None,
        }
    }
}

#[derive(Default)]
struct FindState {
    open: bool,
    focus: bool,
    query: String,
    matches: Vec<(u16, Vec<[f32; 4]>)>,
    active: usize,
}

struct DocView {
    name: String,
    /// Path the document was opened from.
    source: PathBuf,
    /// Recovery copy this document was restored from; removed once the
    /// edits are protected again (new copy or save) or discarded.
    restored_from: Option<Orphan>,
    /// Generation of the last recovery copy, when one is current.
    copied_generation: Option<u64>,
    next_copy: Option<f64>,
    copying: bool,
    generation: u64,
    pages: Vec<PageView>,
    suggested: PathBuf,
    zoom: f32,
    can_undo: bool,
    can_redo: bool,
    /// History position and the position last saved; None when unreachable.
    position: i64,
    saved: Option<i64>,
    edit: Option<EditState>,
    /// Page and raster point of a line clicked while the previous edit was
    /// applied; it opens once that page's objects are refreshed.
    reopen: Option<(usize, Pos2)>,
    /// Page, raster rect and object count of an edit being applied.
    applying: Option<(usize, Rect, usize)>,
    scroll_to: Option<Vec2>,
    offset: Vec2,
    viewport: Rect,
    current_page: usize,
}

struct PageView {
    thumbnail: Option<TextureHandle>,
    pending_thumbnail: bool,
    info: PageInfo,
    texture: Option<PageTexture>,
    pending_render: Option<(f32, u64)>,
    objects: Option<PageObjects>,
    pending_objects: Option<u64>,
    error: Option<String>,
}

struct PageTexture {
    handle: TextureHandle,
    scale: f32,
    generation: u64,
}

struct PageObjects {
    generation: u64,
    /// Editable lines in paint order.
    lines: Vec<Line>,
}

/// Adjacent text objects that read as one line, edited together. Browser
/// print output often places every glyph in its own object.
#[derive(Clone)]
struct Line {
    objects: Vec<TextObject>,
    /// Union of the objects' extents in unscaled raster points.
    rect: Rect,
    text: String,
}

struct EditState {
    page: usize,
    line: Line,
    text: String,
    focus: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BannerKind {
    Warning,
    Error,
    Success,
}

struct Banner {
    id: u64,
    kind: BannerKind,
    text: String,
    expires: Option<f64>,
}

enum Pending {
    Open(Option<PathBuf>),
    Close,
}

impl DocView {
    fn dirty(&self) -> bool {
        self.saved != Some(self.position)
    }
    fn scale(&self) -> f32 {
        self.zoom * PT_TO_UI
    }
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut banners = Vec::new();
        let config_path = Substitution::config_path().ok();
        let policy = match config_path.as_deref().map(Substitution::load) {
            Some(Ok(policy)) => policy,
            Some(Err(e)) => {
                banners.push(Banner {
                    id: 0,
                    kind: BannerKind::Warning,
                    text: format!("Settings could not be read ({e}). Using defaults."),
                    expires: None,
                });
                Substitution::default()
            }
            None => Substitution::default(),
        };
        let library = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("pdfium.dll")));
        let setup_error = match &library {
            Some(path) if path.is_file() => None,
            _ => Some(
                "pdfium.dll was not found next to the program. Run scripts/setup-pdfium.ps1 \
                 and start the editor again."
                    .to_string(),
            ),
        };
        let autosave_path = AutoSave::path();
        let autosave = match autosave_path.as_deref().map(AutoSave::load) {
            Some(Ok(autosave)) => autosave,
            Some(Err(e)) => {
                banners.push(Banner {
                    id: 1,
                    kind: BannerKind::Warning,
                    text: format!("Auto-save settings could not be read ({e}). Using defaults."),
                    expires: None,
                });
                AutoSave::default()
            }
            None => AutoSave::default(),
        };
        let (session, session_error) = match Session::start() {
            Ok(session) => (Some(session), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let orphans = recovery::orphans();
        let worker = Worker::spawn(cc.egui_ctx.clone(), library.unwrap_or_default(), policy);
        let mut app = Self {
            dragging_page: None,
            text_tool: TextTool::default(),
            find: FindState::default(),
            worker,
            engine_alive: true,
            setup_error,
            policy,
            config_path,
            doc: None,
            opening: false,
            busy: 0,
            next_banner: banners.len() as u64,
            banners,
            settings_open: false,
            confirm: None,
            allow_close: false,
            title: String::new(),
            autosave,
            autosave_path,
            session,
            session_error,
            orphans,
            restoring: None,
        };
        if let Some(path) = initial {
            app.open_path(path);
        }
        app
    }

    // ----- Commands -----

    fn send(&mut self, request: Request) {
        if !self.worker.send(request) {
            self.engine_stopped();
        }
    }

    fn engine_stopped(&mut self) {
        if self.engine_alive {
            self.engine_alive = false;
            self.busy = 0;
            self.opening = false;
            self.banner(
                BannerKind::Error,
                "The PDF engine stopped. Restart the editor to continue.",
            );
        }
    }

    fn banner(&mut self, kind: BannerKind, text: impl Into<String>) {
        let text = text.into();
        self.banners.retain(|b| b.text != text);
        self.next_banner += 1;
        self.banners.push(Banner {
            id: self.next_banner,
            kind,
            text,
            expires: None,
        });
        if self.banners.len() > 4 {
            self.banners.remove(0);
        }
    }

    fn request_open(&mut self, path: Option<PathBuf>) {
        let committed = self.commit_edit();
        if committed || self.doc.as_ref().is_some_and(DocView::dirty) {
            self.confirm = Some(Pending::Open(path));
        } else {
            self.open_dialog_or_path(path);
        }
    }

    fn open_dialog_or_path(&mut self, path: Option<PathBuf>) {
        let path = path.or_else(|| {
            rfd::FileDialog::new()
                .set_title("Open PDF")
                .add_filter("PDF document", &["pdf"])
                .pick_file()
        });
        if let Some(path) = path {
            self.open_path(path);
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        if self.setup_error.is_some() || self.busy > 0 {
            return;
        }
        self.opening = true;
        self.busy += 1;
        self.send(Request::Open(path));
    }

    fn save_as(&mut self) {
        // The worker handles requests in order, so a committed edit is
        // applied before the save.
        let committed = self.commit_edit();
        let Some(doc) = &self.doc else { return };
        if self.busy > u32::from(committed) {
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save As")
            .add_filter("PDF document", &["pdf"]);
        if let Some(dir) = doc.suggested.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = doc.suggested.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(mut path) = dialog.save_file() {
            if path.extension().is_none() {
                path.set_extension("pdf");
            }
            self.busy += 1;
            self.send(Request::SaveAs(path));
        }
    }

    fn undo(&mut self) {
        // With uncommitted typing, Undo discards the typing only.
        if self
            .doc
            .as_ref()
            .and_then(|d| d.edit.as_ref())
            .is_some_and(|e| e.text != e.line.text)
        {
            self.cancel_edit();
            return;
        }
        if self.busy == 0 && self.doc.as_ref().is_some_and(|d| d.can_undo) {
            self.cancel_edit();
            self.busy += 1;
            self.send(Request::Undo);
        }
    }

    fn redo(&mut self) {
        if self.busy == 0 && self.doc.as_ref().is_some_and(|d| d.can_redo) {
            self.cancel_edit();
            self.busy += 1;
            self.send(Request::Redo);
        }
    }

    fn cancel_edit(&mut self) {
        if let Some(doc) = &mut self.doc {
            doc.edit = None;
        }
    }

    /// Applies the open edit box, if its text changed, and closes it.
    /// Returns true when an edit was sent.
    fn commit_edit(&mut self) -> bool {
        let Some(doc) = &mut self.doc else {
            return false;
        };
        let Some(edit) = doc.edit.take() else {
            return false;
        };
        if edit.text == edit.line.text {
            return false;
        }
        doc.applying = Some((edit.page, edit.line.rect, edit.line.objects.len()));
        self.busy += 1;
        self.send(Request::Edit {
            objects: edit.line.objects,
            text: edit.text,
        });
        true
    }

    fn set_policy(&mut self, policy: Substitution) {
        self.policy = policy;
        self.send(Request::SetSubstitution(policy));
        let saved = match &self.config_path {
            Some(path) => policy.save(path).map_err(|e| e.to_string()),
            None => Err("the settings folder is not available".to_string()),
        };
        if let Err(e) = saved {
            self.banner(
                BannerKind::Error,
                format!("The setting could not be saved: {e}"),
            );
        }
    }

    fn set_zoom(&mut self, zoom: f32, anchor: Option<Pos2>) {
        let Some(doc) = &mut self.doc else { return };
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (zoom - doc.zoom).abs() < f32::EPSILON {
            return;
        }
        // Keep the content point under the anchor fixed on screen.
        let anchor = anchor.unwrap_or(doc.viewport.center());
        let in_view = anchor - doc.viewport.min;
        let content = doc.offset + in_view;
        let ratio = zoom / doc.zoom;
        doc.scroll_to = Some((content * ratio - in_view).max(Vec2::ZERO));
        doc.zoom = zoom;
    }

    fn step_zoom(&mut self, up: bool) {
        let Some(doc) = &self.doc else { return };
        let z = doc.zoom;
        let next = if up {
            ZOOM_STEPS
                .iter()
                .copied()
                .find(|s| *s > z + 0.001)
                .unwrap_or(MAX_ZOOM)
        } else {
            ZOOM_STEPS
                .iter()
                .rev()
                .copied()
                .find(|s| *s < z - 0.001)
                .unwrap_or(MIN_ZOOM)
        };
        self.set_zoom(next, None);
    }

    fn fit_zoom(&mut self, whole_page: bool) {
        let Some(doc) = &self.doc else { return };
        let Some(page) = doc.pages.get(doc.current_page) else {
            return;
        };
        let avail = doc.viewport.size() - Vec2::splat(2.0 * CANVAS_PAD);
        let mut zoom = avail.x / (page.info.width * PT_TO_UI);
        if whole_page {
            zoom = zoom.min(avail.y / (page.info.height * PT_TO_UI));
        }
        if zoom.is_finite() && zoom > 0.0 {
            self.set_zoom(zoom, None);
        }
    }

    // ----- Worker replies -----

    fn poll(&mut self, ctx: &Context) {
        let Some(replies) = self.worker.poll() else {
            self.engine_stopped();
            return;
        };
        for reply in replies {
            self.handle(ctx, reply);
        }
    }

    fn handle(&mut self, ctx: &Context, reply: Reply) {
        match reply {
            Reply::Thumbnail {
                generation,
                page,
                image,
            } => {
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.generation == generation)
                {
                    if let Some(p) = self.page_mut(page) {
                        p.pending_thumbnail = false;
                        p.thumbnail = Some(ctx.load_texture(
                            format!("thumb-{generation}-{page}"),
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.width as usize, image.height as usize],
                                &image.rgba,
                            ),
                            TextureOptions::LINEAR,
                        ));
                    }
                }
            }
            Reply::Fonts(fonts) => self.text_tool.fonts = fonts,
            Reply::Found {
                generation,
                query,
                matches,
            } => {
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.generation == generation)
                    && self.find.query == query
                {
                    self.find.matches = matches;
                    self.find.active = 0;
                    self.go_match();
                }
            }
            Reply::Opened {
                generation,
                path,
                name,
                pages,
                suggested,
            } => {
                self.text_tool.selection.clear();
                self.text_tool.adding = false;
                self.text_tool.at = None;
                self.find.matches.clear();
                self.find.query.clear();
                self.busy = self.busy.saturating_sub(1);
                self.opening = false;
                self.banners.retain(|b| b.kind != BannerKind::Error);
                let zoom = self.doc.as_ref().map_or(1.0, |d| d.zoom);
                // The previous document's edits were saved or discarded.
                if let Some(old) = self.doc.take()
                    && let Some(orphan) = old.restored_from
                {
                    orphan.discard();
                }
                if let Some(session) = &mut self.session {
                    session.clear();
                }
                let restored = self.restoring.take();
                let (name, source, suggested) = match &restored {
                    Some(orphan) => {
                        let source = orphan.source.clone().unwrap_or(path);
                        let suggested = edited_name(&source).unwrap_or(suggested);
                        (format!("{} (recovered)", orphan.name()), source, suggested)
                    }
                    None => (name, path, suggested),
                };
                self.doc = Some(DocView {
                    name,
                    source,
                    copied_generation: None,
                    next_copy: None,
                    copying: false,
                    generation,
                    pages: pages
                        .into_iter()
                        .map(|info| PageView {
                            thumbnail: None,
                            pending_thumbnail: false,
                            info,
                            texture: None,
                            pending_render: None,
                            objects: None,
                            pending_objects: None,
                            error: None,
                        })
                        .collect(),
                    suggested,
                    zoom,
                    can_undo: false,
                    can_redo: false,
                    position: 0,
                    // A restored document starts with unsaved edits.
                    saved: if restored.is_some() { None } else { Some(0) },
                    restored_from: restored,
                    edit: None,
                    reopen: None,
                    applying: None,
                    scroll_to: Some(Vec2::ZERO),
                    offset: Vec2::ZERO,
                    viewport: Rect::NOTHING,
                    current_page: 0,
                });
            }
            Reply::Rendered {
                generation,
                page,
                scale,
                image,
            } => {
                let Some(view) = self.page_mut(page) else {
                    return;
                };
                if view
                    .pending_render
                    .is_some_and(|(s, g)| s == scale && g == generation)
                {
                    view.pending_render = None;
                }
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.generation != generation)
                {
                    return;
                }
                let size = [image.width as usize, image.height as usize];
                if image.rgba.len() != size[0] * size[1] * 4 {
                    return;
                }
                let color = egui::ColorImage::from_rgba_unmultiplied(size, &image.rgba);
                let handle =
                    ctx.load_texture(format!("page-{page}"), color, TextureOptions::LINEAR);
                if let Some(view) = self.page_mut(page) {
                    view.texture = Some(PageTexture {
                        handle,
                        scale,
                        generation,
                    });
                    view.error = None;
                }
            }
            Reply::Objects {
                generation,
                page,
                transform,
                objects,
            } => {
                let current = self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.generation == generation);
                let Some(view) = self.page_mut(page) else {
                    return;
                };
                if view.pending_objects == Some(generation) {
                    view.pending_objects = None;
                }
                if current {
                    let items = objects
                        .into_iter()
                        .map(|o| {
                            let rect = raster_rect(&transform, &o);
                            (o, rect)
                        })
                        .collect();
                    view.objects = Some(PageObjects {
                        generation,
                        lines: group_lines(items),
                    });
                }
            }
            Reply::Changed {
                pages,
                generation,
                kind,
                can_undo,
                can_redo,
                warning,
                objects_changed,
            } => {
                self.busy = self.busy.saturating_sub(1);
                let Some(doc) = &mut self.doc else { return };
                let selected = doc.applying.take().map_or(1, |(_, _, n)| n);
                doc.can_undo = can_undo;
                doc.can_redo = can_redo;
                if objects_changed > 0 {
                    match kind {
                        ChangeKind::Edit => {
                            if doc.saved.is_some_and(|s| s > doc.position) {
                                doc.saved = None;
                            }
                            doc.position += 1;
                        }
                        ChangeKind::Undo => doc.position -= 1,
                        ChangeKind::Redo => doc.position += 1,
                    }
                }
                if !doc.dirty() {
                    doc.copied_generation = None;
                    doc.next_copy = None;
                    if let Some(session) = &mut self.session {
                        session.clear();
                    }
                }
                if doc.generation != generation {
                    self.text_tool.selection.clear();
                    doc.generation = generation;
                    self.find.matches.clear();
                    if !self.find.query.is_empty() {
                        self.worker.send(Request::Find(self.find.query.clone()));
                    }
                    doc.edit = None;
                    if !pages.is_empty() {
                        doc.pages = pages
                            .into_iter()
                            .map(|info| PageView {
                                info,
                                thumbnail: None,
                                pending_thumbnail: false,
                                texture: None,
                                pending_render: None,
                                objects: None,
                                pending_objects: None,
                                error: None,
                            })
                            .collect();
                        doc.current_page = doc.current_page.min(doc.pages.len().saturating_sub(1));
                        self.text_tool.at = None;
                    }
                    for page in &mut doc.pages {
                        page.thumbnail = None;
                        page.pending_thumbnail = false;
                        page.pending_render = None;
                        page.pending_objects = None;
                        page.objects = None;
                    }
                }
                if let Some(warning) = warning {
                    self.banner(BannerKind::Warning, warning);
                } else if kind == ChangeKind::Edit && objects_changed > selected {
                    self.banner(
                        BannerKind::Success,
                        format!(
                            "Updated {objects_changed} text objects that use the missing font."
                        ),
                    );
                }
            }
            Reply::Saved(path) => {
                self.busy = self.busy.saturating_sub(1);
                if let Some(doc) = &mut self.doc {
                    doc.saved = Some(doc.position);
                    doc.copied_generation = None;
                    doc.next_copy = None;
                    if let Some(orphan) = doc.restored_from.take() {
                        orphan.discard();
                    }
                }
                if let Some(session) = &mut self.session {
                    session.clear();
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.banner(BannerKind::Success, format!("Saved as {name}"));
                let now = ctx.input(|i| i.time);
                if let Some(b) = self.banners.last_mut() {
                    b.expires = Some(now + 4.0);
                }
            }
            Reply::Snapshot {
                generation,
                path,
                error,
            } => {
                let Some(doc) = &mut self.doc else {
                    let _ = std::fs::remove_file(&path);
                    return;
                };
                doc.copying = false;
                let written = match (error, &mut self.session) {
                    (None, Some(session)) if doc.dirty() => session
                        .written(path.clone(), &doc.source)
                        .map_err(|e| e.to_string()),
                    (None, _) => {
                        // Saved or reverted while the copy was written.
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                    (Some(e), _) => Err(e),
                };
                match written {
                    Ok(()) => {
                        doc.copied_generation = Some(generation);
                        if let Some(orphan) = doc.restored_from.take() {
                            orphan.discard();
                        }
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&path);
                        doc.copied_generation = Some(generation);
                        self.banner(
                            BannerKind::Warning,
                            format!("Auto-save could not write a recovery copy: {e}"),
                        );
                    }
                }
            }
            Reply::Failed {
                message,
                blocking,
                page,
            } => {
                if let Some(page) = page {
                    if let Some(view) = self.page_mut(page) {
                        view.error = Some(message);
                        view.pending_render = None;
                        view.pending_objects = None;
                    }
                    return;
                }
                if blocking {
                    self.busy = self.busy.saturating_sub(1);
                }
                self.restoring = None;
                let message = if std::mem::take(&mut self.opening) {
                    format!(
                        "The file could not be opened. {}.",
                        message.trim_end_matches('.')
                    )
                } else {
                    message
                };
                if let Some(doc) = &mut self.doc {
                    doc.applying = None;
                }
                self.banner(BannerKind::Error, message);
            }
        }
    }

    fn thumbnails(&mut self, ui: &mut Ui) {
        ui.heading("Pages");
        let mut action = None;
        if let Some(doc) = &self.doc {
            let page = doc.current_page as u16;
            ui.add_enabled_ui(self.busy == 0, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Rotate").clicked() {
                        action = Some(pdf_editor_core::PageEdit::Rotate { page });
                    }
                    if ui
                        .add_enabled(doc.pages.len() > 1, Button::new("Delete"))
                        .clicked()
                    {
                        action = Some(pdf_editor_core::PageEdit::Delete { page });
                    }
                });
                if ui
                    .button("Insert PDF…")
                    .on_hover_text("Insert all pages before the current page")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PDF", &["pdf"])
                        .pick_file()
                    {
                        action = Some(pdf_editor_core::PageEdit::Insert { path, before: page });
                    }
                }
            });
        }
        let Some(doc) = &mut self.doc else { return };
        let mut requests = Vec::new();
        let mut jump = None;
        ScrollArea::vertical().show_rows(ui, 180.0, doc.pages.len(), |ui, range| {
            for index in range.clone() {
                let page = &mut doc.pages[index];
                ui.vertical_centered(|ui| {
                    let response = if let Some(texture) = &page.thumbnail {
                        ui.add(
                            egui::Image::new((texture.id(), texture.size_vec2()))
                                .sense(Sense::click_and_drag()),
                        )
                    } else {
                        if !page.pending_thumbnail && page.error.is_none() {
                            page.pending_thumbnail = true;
                            requests.push(Request::Thumbnail(index as u16));
                        }
                        ui.add_sized(
                            vec2(120.0, 150.0),
                            Button::new(if page.error.is_some() {
                                "Unavailable"
                            } else {
                                "Loading…"
                            }),
                        )
                    };
                    if response.clicked() {
                        jump = Some(index);
                    }
                    if self.busy == 0 && response.drag_started() {
                        self.dragging_page = Some(index);
                    }
                    if let Some(from) = self.dragging_page {
                        if response.contains_pointer() {
                            ui.painter().rect_stroke(
                                response.rect,
                                0.0,
                                Stroke::new(2.0, ui.visuals().selection.stroke.color),
                                StrokeKind::Outside,
                            );
                            if ui.input(|i| i.pointer.any_released()) && from != index {
                                action = Some(pdf_editor_core::PageEdit::Move {
                                    from: from as u16,
                                    to: index as u16,
                                });
                            }
                        }
                    }
                    ui.selectable_label(doc.current_page == index, format!("Page {}", index + 1))
                        .clicked()
                        .then(|| jump = Some(index));
                });
            }
            for (i, page) in doc.pages.iter_mut().enumerate() {
                if i + 2 < range.start || i > range.end + 2 {
                    page.thumbnail = None;
                }
            }
        });
        if let Some(index) = jump {
            let y = CANVAS_PAD
                + doc
                    .pages
                    .iter()
                    .take(index)
                    .map(|p| p.info.height * doc.scale() + PAGE_GAP)
                    .sum::<f32>();
            doc.scroll_to = Some(vec2(doc.offset.x, y));
        }
        for request in requests {
            self.send(request);
        }
        if ui.input(|i| i.pointer.any_released()) {
            self.dragging_page = None;
        }
        if let Some(action) = action {
            self.commit_edit();
            self.busy += 1;
            self.send(Request::Pages(action));
        }
    }

    fn object_tools(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Text object");
            let previous = self.text_tool.selected;
            egui::ComboBox::from_id_salt("selected-run")
                .selected_text(format!("Run {}", self.text_tool.selected + 1))
                .show_ui(ui, |ui| {
                    for (index, object) in self.text_tool.selection.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.text_tool.selected,
                            index,
                            format!("{}: {}", index + 1, object.text),
                        );
                    }
                });
            if previous != self.text_tool.selected
                && let Some(o) = self.text_tool.selection.get(self.text_tool.selected)
            {
                self.text_tool.size = o.size;
                self.text_tool.color = [o.color[0], o.color[1], o.color[2]];
            }
            ui.add(
                egui::DragValue::new(&mut self.text_tool.size)
                    .range(1.0..=1000.0)
                    .suffix(" pt"),
            );
            ui.color_edit_button_srgb(&mut self.text_tool.color);
            if ui
                .add_enabled(self.busy == 0, Button::new("Apply style"))
                .clicked()
            {
                if let Some(object) = self
                    .text_tool
                    .selection
                    .get(self.text_tool.selected)
                    .cloned()
                {
                    self.cancel_edit();
                    self.busy += 1;
                    self.send(Request::StyleText {
                        object,
                        size: self.text_tool.size,
                        color: self.text_tool.color,
                    });
                }
            }
            if ui
                .add_enabled(self.busy == 0, Button::new("Delete object"))
                .clicked()
            {
                if let Some(object) = self
                    .text_tool
                    .selection
                    .get(self.text_tool.selected)
                    .cloned()
                {
                    self.cancel_edit();
                    self.busy += 1;
                    self.send(Request::DeleteText(object));
                }
            }
            if ui.button("Deselect").clicked() {
                self.commit_edit();
                self.text_tool.selection.clear();
            }
        });
    }

    fn text_tools(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Click a page to choose the text baseline.");
            ui.text_edit_singleline(&mut self.text_tool.text);
            egui::ComboBox::from_id_salt("system-font")
                .selected_text(&self.text_tool.font)
                .show_ui(ui, |ui| {
                    for name in &self.text_tool.fonts {
                        ui.selectable_value(&mut self.text_tool.font, name.clone(), name);
                    }
                });
            ui.add(
                egui::DragValue::new(&mut self.text_tool.size)
                    .range(1.0..=1000.0)
                    .suffix(" pt"),
            );
            ui.color_edit_button_srgb(&mut self.text_tool.color);
            if ui
                .add_enabled(
                    self.busy == 0 && self.text_tool.at.is_some(),
                    Button::new("Place text"),
                )
                .clicked()
            {
                if let Some((page, at)) = self.text_tool.at {
                    self.busy += 1;
                    self.send(Request::AddText {
                        text: pdf_editor_core::NewText {
                            page,
                            x: 0.0,
                            y: 0.0,
                            text: self.text_tool.text.clone(),
                            font: self.text_tool.font.clone(),
                            size: self.text_tool.size,
                            color: self.text_tool.color,
                        },
                        raster: (at.x, at.y),
                    });
                    self.text_tool.adding = false;
                }
            }
            if ui.button("Cancel").clicked() {
                self.text_tool.adding = false;
            }
        });
    }

    fn go_match(&mut self) {
        if let Some((page, boxes)) = self.find.matches.get(self.find.active) {
            if let Some(doc) = &mut self.doc {
                let y = CANVAS_PAD
                    + doc
                        .pages
                        .iter()
                        .take(*page as usize)
                        .map(|p| p.info.height * doc.scale() + PAGE_GAP)
                        .sum::<f32>()
                    + boxes.first().map_or(0.0, |b| b[1] * doc.scale());
                doc.scroll_to = Some(vec2(doc.offset.x, (y - 80.0).max(0.0)));
            }
        }
    }
    fn find_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Find");
            let field = ui.text_edit_singleline(&mut self.find.query);
            if self.find.focus {
                field.request_focus();
                self.find.focus = false;
            }
            if field.changed() {
                self.find.matches.clear();
                self.send(Request::Find(self.find.query.clone()));
            }
            let count = self.find.matches.len();
            ui.label(format!(
                "{} / {count}",
                if count == 0 { 0 } else { self.find.active + 1 }
            ));
            if ui.add_enabled(count > 0, Button::new("Previous")).clicked() {
                self.find.active = (self.find.active + count - 1) % count;
                self.go_match();
            }
            if ui.add_enabled(count > 0, Button::new("Next")).clicked() {
                self.find.active = (self.find.active + 1) % count;
                self.go_match();
            }
            if ui.button("Close").clicked() {
                self.find.open = false;
            }
        });
    }

    fn page_mut(&mut self, page: u16) -> Option<&mut PageView> {
        self.doc.as_mut()?.pages.get_mut(usize::from(page))
    }

    // ----- Input -----

    fn shortcuts(&mut self, ctx: &Context) {
        if self.confirm.is_some() || self.settings_open {
            return;
        }
        for command in crate::shortcuts::consume(ctx, ctx.egui_wants_keyboard_input()) {
            use crate::shortcuts::Command;
            match command {
                Command::Open => self.request_open(None),
                Command::SaveAs => self.save_as(),
                Command::Undo => self.undo(),
                Command::Redo => self.redo(),
                Command::ZoomIn => self.step_zoom(true),
                Command::ZoomOut => self.step_zoom(false),
                Command::ActualSize => self.set_zoom(1.0, None),
                Command::Find => {
                    self.find.open = true;
                    self.find.focus = true;
                }
            }
        }
    }

    fn dropped_files(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.request_open(Some(path));
        }
    }

    fn close_guard(&mut self, ctx: &Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        let committed = self.commit_edit();
        if committed || self.doc.as_ref().is_some_and(DocView::dirty) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let title = match &self.doc {
            Some(doc) if doc.dirty() => format!("• {} — PDF Editor", doc.name),
            Some(doc) => format!("{} — PDF Editor", doc.name),
            None => "PDF Editor".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // ----- Layout -----

    fn toolbar(&mut self, ui: &mut Ui, p: &Palette) {
        let has_doc = self.doc.is_some();
        let idle = self.busy == 0 && self.engine_alive;
        let ready = self.setup_error.is_none() && self.engine_alive;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if tool_button(ui, "Open", ready && idle, "Open a PDF (Ctrl+O)").clicked() {
                self.request_open(None);
            }
            let dirty = self.doc.as_ref().is_some_and(DocView::dirty);
            let save = if dirty {
                ui.add_enabled(
                    has_doc && idle,
                    Button::new(RichText::new("Save As").color(p.accent_text)).fill(p.accent),
                )
            } else {
                ui.add_enabled(has_doc && idle, Button::new("Save As"))
            };
            if save
                .on_hover_text("Save a copy; the original is never changed (Ctrl+S)")
                .clicked()
            {
                self.save_as();
            }
            divider(ui, p);
            let (can_undo, can_redo) = self
                .doc
                .as_ref()
                .map_or((false, false), |d| (d.can_undo, d.can_redo));
            if tool_button(ui, "Undo", can_undo && idle, "Undo (Ctrl+Z)").clicked() {
                self.undo();
            }
            if tool_button(ui, "Redo", can_redo && idle, "Redo (Ctrl+Y)").clicked() {
                self.redo();
            }
            divider(ui, p);
            if tool_button(ui, "Add text", has_doc && idle, "Place a new text box").clicked() {
                self.commit_edit();
                self.text_tool.adding = true;
                self.text_tool.at = None;
                if self.text_tool.fonts.is_empty() {
                    self.send(Request::Fonts);
                }
            }
            if tool_button(ui, "Find", has_doc, "Find text (Ctrl+F)").clicked() {
                self.find.open = true;
                self.find.focus = true;
            }
            self.zoom_controls(ui, has_doc);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if tool_button(
                    ui,
                    "Settings",
                    true,
                    "Font substitution, auto-save and version",
                )
                .clicked()
                {
                    self.settings_open = !self.settings_open;
                    self.commit_edit();
                }
                if self.busy > 0 {
                    ui.add(egui::Spinner::new().size(14.0).color(p.muted));
                }
                if let Some(doc) = &self.doc {
                    ui.add_space(4.0);
                    let name = RichText::new(&doc.name)
                        .font(theme::semibold(13.5))
                        .color(p.text);
                    ui.add(egui::Label::new(name).truncate());
                    if doc.dirty() {
                        ui.label(RichText::new("Edited").small().color(p.muted));
                    }
                }
            });
        });
    }

    fn zoom_controls(&mut self, ui: &mut Ui, has_doc: bool) {
        let zoom = self.doc.as_ref().map_or(1.0, |d| d.zoom);
        if tool_button(
            ui,
            "\u{2212}",
            has_doc && zoom > MIN_ZOOM,
            "Zoom out (Ctrl+−)",
        )
        .clicked()
        {
            self.step_zoom(false);
        }
        let label = format!("{:.0}%", zoom * 100.0);
        ui.add_enabled_ui(has_doc, |ui| {
            ui.menu_button(label, |ui| {
                ui.set_min_width(140.0);
                for step in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0] {
                    if ui.button(format!("{:.0}%", step * 100.0)).clicked() {
                        self.set_zoom(step, None);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Fit width").clicked() {
                    self.fit_zoom(false);
                    ui.close();
                }
                if ui.button("Fit page").clicked() {
                    self.fit_zoom(true);
                    ui.close();
                }
            })
            .response
            .on_hover_text("Zoom presets (Ctrl+0 resets to 100%)");
        });
        if tool_button(ui, "+", has_doc && zoom < MAX_ZOOM, "Zoom in (Ctrl++)").clicked() {
            self.step_zoom(true);
        }
    }

    fn status_bar(&self, ui: &mut Ui, p: &Palette) {
        ui.horizontal_centered(|ui| {
            let muted = |t: &str| RichText::new(t).small().color(p.muted);
            match &self.doc {
                Some(doc) => {
                    ui.label(muted(&format!(
                        "Page {} of {}",
                        doc.current_page + 1,
                        doc.pages.len()
                    )));
                }
                None => {
                    ui.label(muted("No document"));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let hint = if self.doc.as_ref().is_some_and(|d| d.edit.is_some()) {
                    "Enter or click away applies · Esc cancels"
                } else if self.doc.is_some() {
                    "Click text to edit it in place"
                } else {
                    "Ctrl+O to open · Drop a PDF anywhere"
                };
                ui.label(muted(&version_label()))
                    .on_hover_text("PDF Editor version");
                ui.label(muted("·"));
                ui.label(muted(hint));
            });
        });
    }

    fn empty_state(&mut self, ui: &mut Ui, p: &Palette) {
        let size = vec2(420.0, 220.0);
        let rect = Rect::from_center_size(ui.max_rect().center(), size);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.vertical_centered(|ui| {
                if let Some(error) = &self.setup_error {
                    ui.label(RichText::new("PDF engine missing").font(theme::semibold(20.0)));
                    ui.add_space(4.0);
                    ui.label(RichText::new(error).color(p.muted));
                    return;
                }
                if self.opening {
                    ui.add_space(40.0);
                    ui.add(egui::Spinner::new().size(22.0).color(p.accent));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Opening…").color(p.muted));
                    return;
                }
                ui.label(RichText::new("Edit PDF text in place").font(theme::semibold(22.0)));
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Open a document, click a line of text and type. Fonts, size and color stay as they were.")
                        .color(p.muted),
                );
                ui.add_space(14.0);
                let button = Button::new(RichText::new("Open PDF…").color(p.accent_text))
                    .fill(p.accent)
                    .min_size(vec2(132.0, 36.0));
                if ui.add_enabled(self.engine_alive, button).clicked() {
                    self.request_open(None);
                }
                ui.add_space(8.0);
                ui.label(RichText::new("or drop a file anywhere").small().color(p.muted));
            });
        });
    }

    fn canvas(&mut self, ui: &mut Ui, p: &Palette) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let canvas_rect = ui.max_rect();

        // Ctrl+wheel zoom anchored at the pointer.
        let zoom_delta = ui.input(|i| i.zoom_delta());
        let pointer = ui.input(|i| i.pointer.hover_pos());
        if zoom_delta != 1.0
            && self.confirm.is_none()
            && !self.settings_open
            && pointer.is_some_and(|pos| canvas_rect.contains(pos))
        {
            let zoom = self.doc.as_ref().map_or(1.0, |d| d.zoom);
            self.set_zoom(zoom * zoom_delta, pointer);
        }

        let busy = self.busy > 0;
        let Some(doc) = &mut self.doc else { return };
        let scale = doc.scale();
        let widest = doc.pages.iter().map(|pg| pg.info.width).fold(0.0, f32::max) * scale;
        let total_h: f32 = doc
            .pages
            .iter()
            .map(|pg| pg.info.height * scale)
            .sum::<f32>()
            + PAGE_GAP * doc.pages.len().saturating_sub(1) as f32
            + 2.0 * CANVAS_PAD;
        let content_w = (widest + 2.0 * CANVAS_PAD).max(canvas_rect.width());

        let mut area = ScrollArea::both().auto_shrink(false);
        if let Some(offset) = doc.scroll_to.take() {
            area = area.scroll_offset(offset);
        }
        let mut requests = Vec::new();
        let mut start_edit: Option<EditState> = None;
        let mut clicked_at: Option<(usize, Pos2)> = None;

        let output = area.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            ui.allocate_rect(
                Rect::from_min_size(origin, vec2(content_w, total_h)),
                Sense::hover(),
            );
            let painter = ui.painter().clone();
            let near = viewport.expand2(vec2(0.0, viewport.height()));
            let center_y = viewport.center().y;
            let mut y = CANVAS_PAD;
            let mut page_rects = Vec::with_capacity(doc.pages.len());
            let mut first_visible: Option<usize> = None;
            let mut last_visible = 0;

            for (index, page) in doc.pages.iter_mut().enumerate() {
                let size = vec2(page.info.width, page.info.height) * scale;
                let local = Rect::from_min_size(pos2((content_w - size.x) / 2.0, y), size);
                y += size.y + PAGE_GAP;
                let rect = round_rect(local.translate(origin.to_vec2()), ppp);
                page_rects.push(rect);
                if local.min.y <= center_y {
                    doc.current_page = index;
                }
                if !local.intersects(near) {
                    continue;
                }
                first_visible.get_or_insert(index);
                last_visible = index;
                let Ok(page_no) = u16::try_from(index) else {
                    continue;
                };

                // Request a sharp render and the text objects when stale.
                let wanted = render_scale(&page.info, scale * ppp);
                let stale = page.texture.as_ref().is_none_or(|t| {
                    t.generation != doc.generation || (t.scale - wanted).abs() > wanted * 0.01
                });
                if stale
                    && page.error.is_none()
                    && page.pending_render != Some((wanted, doc.generation))
                {
                    page.pending_render = Some((wanted, doc.generation));
                    requests.push(Request::Render {
                        page: page_no,
                        scale: wanted,
                    });
                }
                let objects_stale = page
                    .objects
                    .as_ref()
                    .is_none_or(|o| o.generation != doc.generation);
                if objects_stale
                    && page.error.is_none()
                    && page.pending_objects != Some(doc.generation)
                {
                    page.pending_objects = Some(doc.generation);
                    requests.push(Request::Objects { page: page_no });
                }

                if !local.intersects(viewport) {
                    continue;
                }
                paint_page(&painter, ui, rect, page, p);
                if self.find.open {
                    for (n, (_, boxes)) in self
                        .find
                        .matches
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| usize::from(m.0) == index)
                    {
                        for b in boxes {
                            let r = to_screen(
                                Rect::from_min_max(pos2(b[0], b[1]), pos2(b[2], b[3])),
                                rect,
                                scale,
                            );
                            painter.rect_filled(
                                r,
                                0.0,
                                Color32::from_rgba_unmultiplied(
                                    255,
                                    185,
                                    30,
                                    if n == self.find.active { 140 } else { 65 },
                                ),
                            );
                        }
                    }
                }

                // Hover and click hit testing against cached object extents.
                let response = ui.interact(rect, Id::new(("pdf-page", index)), Sense::click());
                // A line clicked while the previous edit was applied opens
                // once this page's objects reflect that edit.
                if let Some((reopen_page, at)) = doc.reopen
                    && reopen_page == index
                    && !busy
                    && let Some(objects) = &page.objects
                    && objects.generation == doc.generation
                {
                    doc.reopen = None;
                    if let Some(line) = objects.lines.iter().rev().find(|l| l.rect.contains(at)) {
                        start_edit = Some(EditState {
                            page: index,
                            line: line.clone(),
                            text: line.text.clone(),
                            focus: true,
                        });
                    }
                }
                let hit = response.hover_pos().and_then(|pos| {
                    let at = ((pos - rect.min) / scale).to_pos2();
                    page.objects.as_ref().and_then(|o| {
                        o.lines
                            .iter()
                            .rev()
                            .find(|l| l.rect.expand(1.0).contains(at))
                    })
                });
                let editing_this = |line: &Line| {
                    doc.edit.as_ref().is_some_and(|e| {
                        e.page == index && e.line.objects[0].index == line.objects[0].index
                    })
                };
                if self.text_tool.adding && response.clicked() && !busy {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.text_tool.at = Some((page_no, ((pos - rect.min) / scale).to_pos2()));
                    }
                }
                if let Some((pg, at)) = self.text_tool.at
                    && pg == page_no
                    && self.text_tool.adding
                {
                    let at = rect.min + at.to_vec2() * scale;
                    painter.circle_stroke(at, 5.0, Stroke::new(1.5, p.accent));
                }
                if let Some(line) = hit.filter(|_| !self.text_tool.adding) {
                    if !editing_this(line) && !busy {
                        let screen = to_screen(line.rect, rect, scale);
                        painter.rect_stroke(
                            screen.expand(2.0),
                            CornerRadius::same(3),
                            Stroke::new(1.0, p.accent),
                            StrokeKind::Outside,
                        );
                        ctx.set_cursor_icon(CursorIcon::Text);
                    }
                    if response.clicked() && !busy && !editing_this(line) {
                        start_edit = Some(EditState {
                            page: index,
                            line: line.clone(),
                            text: line.text.clone(),
                            focus: true,
                        });
                        clicked_at = response
                            .interact_pointer_pos()
                            .map(|pos| (index, ((pos - rect.min) / scale).to_pos2()));
                    }
                }
                if let Some((applying_page, raster, _)) = doc.applying
                    && applying_page == index
                {
                    let screen = to_screen(raster, rect, scale).expand(2.0);
                    painter.rect_filled(screen, CornerRadius::same(3), p.tint);
                    painter.rect_stroke(
                        screen,
                        CornerRadius::same(3),
                        Stroke::new(1.5, p.accent),
                        StrokeKind::Outside,
                    );
                }
            }

            // Release textures of pages far outside the viewport.
            if let Some(first) = first_visible {
                for (index, page) in doc.pages.iter_mut().enumerate() {
                    if index + KEEP_PAGES < first || index > last_visible + KEEP_PAGES {
                        page.texture = None;
                    }
                }
            }

            // Clicking elsewhere ends the edit box through its lost focus,
            // which applies the text. Clicking another line applies the open
            // edit first, then opens the clicked line from refreshed objects.
            let mut outcome = None;
            if let Some(edit) = start_edit.take() {
                self.text_tool.selection = edit.line.objects.clone();
                self.text_tool.selected = 0;
                if let Some(o) = self.text_tool.selection.first() {
                    self.text_tool.size = o.size;
                    self.text_tool.color = [o.color[0], o.color[1], o.color[2]];
                }
                let changed = doc.edit.as_ref().is_some_and(|e| e.text != e.line.text);
                if changed {
                    outcome = Some(true);
                    doc.reopen = clicked_at;
                } else {
                    doc.edit = Some(edit);
                }
            }
            match outcome {
                Some(apply) => Some(apply),
                None => edit_box(ui, doc, &page_rects, scale, p),
            }
        });

        doc.offset = output.state.offset;
        doc.viewport = output.inner_rect;
        let apply = output.inner;
        for request in requests {
            self.send(request);
        }
        match apply {
            Some(true) => {
                self.commit_edit();
            }
            Some(false) => self.cancel_edit(),
            None => {}
        }
    }

    fn banners(&mut self, ctx: &Context, top: f32, p: &Palette) {
        let now = ctx.input(|i| i.time);
        self.banners.retain(|b| b.expires.is_none_or(|t| t > now));
        if let Some(next) = self
            .banners
            .iter()
            .filter_map(|b| b.expires)
            .reduce(f64::min)
        {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((next - now).max(0.05)));
        }
        if self.banners.is_empty() {
            return;
        }
        let mut dismissed = Vec::new();
        egui::Area::new(Id::new("banners"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, top + 12.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(620.0);
                for banner in &self.banners {
                    let (bg, fg) = match banner.kind {
                        BannerKind::Warning => (p.warn_bg, p.warn_fg),
                        BannerKind::Error => (p.error_bg, p.error_fg),
                        BannerKind::Success => (p.ok_bg, p.ok_fg),
                    };
                    Frame::new()
                        .fill(bg)
                        .stroke(Stroke::new(1.0, fg.gamma_multiply(0.25)))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin {
                            left: 14,
                            right: 6,
                            top: 6,
                            bottom: 6,
                        })
                        .shadow(Shadow {
                            offset: [0, 4],
                            blur: 14,
                            spread: 0,
                            color: p.page_shadow,
                        })
                        .show(ui, |ui| {
                            ui.set_width(560.0);
                            ui.horizontal(|ui| {
                                let (dot, _) =
                                    ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                                ui.painter().circle_filled(dot.center(), 3.5, fg);
                                ui.add(
                                    egui::Label::new(RichText::new(&banner.text).color(fg)).wrap(),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let close =
                                        Button::new(RichText::new("×").size(16.0).color(fg))
                                            .frame(false)
                                            .min_size(vec2(26.0, 26.0));
                                    if ui.add(close).on_hover_text("Dismiss").clicked() {
                                        dismissed.push(banner.id);
                                    }
                                });
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        self.banners.retain(|b| !dismissed.contains(&b.id));
    }

    fn confirm_modal(&mut self, ctx: &Context, p: &Palette) {
        let Some(pending) = &self.confirm else { return };
        let action = match pending {
            Pending::Open(_) => "Open another file",
            Pending::Close => "Close",
        };
        let mut choice: Option<bool> = None;
        let modal = egui::Modal::new(Id::new("confirm-discard")).frame(modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Discard unsaved edits?").font(theme::semibold(17.0)));
            ui.add_space(2.0);
            ui.label(
                RichText::new("Your edits have not been saved. Use Save As first to keep them; the original file stays untouched either way.")
                    .color(p.muted),
            );
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let discard = Button::new(RichText::new(format!("Discard and {}", action.to_lowercase())).color(p.accent_text))
                    .fill(p.error_fg)
                    .min_size(vec2(0.0, 32.0));
                if ui.add(discard).clicked() {
                    choice = Some(true);
                }
                if ui.add(secondary_button("Cancel", p)).clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match (choice, self.confirm.take()) {
            (Some(true), Some(Pending::Open(path))) => {
                self.discard_recovery();
                if let Some(doc) = &mut self.doc {
                    doc.saved = Some(doc.position);
                }
                self.open_dialog_or_path(path);
            }
            (Some(true), Some(Pending::Close)) => {
                self.discard_recovery();
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            (Some(_), _) => {}
            (None, pending) => self.confirm = pending,
        }
    }

    /// Removes recovery copies after edits were explicitly discarded.
    fn discard_recovery(&mut self) {
        if let Some(session) = &mut self.session {
            session.clear();
        }
        if let Some(doc) = &mut self.doc {
            doc.copied_generation = None;
            doc.next_copy = None;
            if let Some(orphan) = doc.restored_from.take() {
                orphan.discard();
            }
        }
    }

    /// Writes a recovery copy once unsaved edits are one interval old.
    fn autosave_tick(&mut self, ctx: &Context) {
        let (Some(minutes), Some(session), Some(doc)) =
            (self.autosave.0, &mut self.session, &mut self.doc)
        else {
            return;
        };
        if !doc.dirty() || doc.copying || doc.copied_generation == Some(doc.generation) {
            doc.next_copy = None;
            return;
        }
        let now = ctx.input(|i| i.time);
        let due = *doc.next_copy.get_or_insert(now + f64::from(minutes) * 60.0);
        if now < due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            return;
        }
        if self.busy > 0 {
            return;
        }
        doc.next_copy = None;
        doc.copying = true;
        let path = session.next_path();
        self.send(Request::Snapshot(path));
    }

    fn set_autosave(&mut self, autosave: AutoSave) {
        self.autosave = autosave;
        if autosave.0.is_none() {
            if let Some(session) = &mut self.session {
                session.clear();
            }
            if let Some(doc) = &mut self.doc {
                doc.copied_generation = None;
            }
        }
        if let Some(doc) = &mut self.doc {
            doc.next_copy = None;
        }
        let saved = match &self.autosave_path {
            Some(path) => autosave.save(path).map_err(|e| e.to_string()),
            None => Err("the settings folder is not available".to_string()),
        };
        if let Err(e) = saved {
            self.banner(
                BannerKind::Error,
                format!("The setting could not be saved: {e}"),
            );
        }
    }

    fn restore_modal(&mut self, ctx: &Context, p: &Palette) {
        if self.orphans.is_empty()
            || self.confirm.is_some()
            || self.busy > 0
            || self.restoring.is_some()
        {
            return;
        }
        let orphan = &self.orphans[0];
        let mut action: Option<u8> = None;
        let modal = egui::Modal::new(Id::new("restore"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new("Restore unsaved edits?").font(theme::semibold(17.0)));
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!(
                        "The editor closed before edits to {} were saved. A recovery copy from {} is available.",
                        orphan.name(),
                        time_ago(orphan.modified)
                    ))
                    .color(p.muted),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let restore = Button::new(RichText::new("Restore").color(p.accent_text))
                        .fill(p.accent)
                        .min_size(vec2(96.0, 32.0));
                    if ui.add(restore).clicked() {
                        action = Some(0);
                    }
                    if ui.add(secondary_button("Discard", p)).clicked() {
                        action = Some(1);
                    }
                    if ui
                        .add(secondary_button("Later", p))
                        .on_hover_text("Keep the copy and ask again next time")
                        .clicked()
                    {
                        action = Some(2);
                    }
                });
            });
        if modal.should_close() && action.is_none() {
            action = Some(2);
        }
        match action {
            Some(0) => {
                if self.doc.as_ref().is_some_and(DocView::dirty) {
                    self.banner(
                        BannerKind::Warning,
                        "Save or discard the open document before restoring.",
                    );
                    return;
                }
                let orphan = self.orphans.remove(0);
                let copy = orphan.copy.clone();
                self.restoring = Some(orphan);
                self.open_path(copy);
                if !self.opening {
                    self.restoring = None;
                }
            }
            Some(1) => self.orphans.remove(0).discard(),
            Some(_) => self.orphans.clear(),
            None => {}
        }
    }

    fn settings_page(&mut self, ui: &mut Ui, p: &Palette) {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.settings_open = false;
        }
        let mut policy = self.policy;
        let mut autosave = self.autosave;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let width = (ui.available_width() - 48.0).clamp(280.0, 640.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(width);
                ui.add_space(28.0);
                ui.horizontal(|ui| {
                    let back = if self.doc.is_some() { "← Back to document" } else { "← Back" };
                    if ui.add(Button::new(back).min_size(vec2(0.0, 30.0))).clicked() {
                        self.settings_open = false;
                    }
                });
                ui.add_space(8.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.label(RichText::new("Settings").font(theme::semibold(24.0)));
                    ui.label(RichText::new("Changes are saved automatically.").color(p.muted));
                    ui.add_space(12.0);

                    section(ui, p, "Font substitution", |ui| {
                        ui.label(
                            RichText::new(
                                "When the original font lacks a character you type and is not installed, \
                                 the closest installed font is used and a warning is shown.",
                            )
                            .color(p.muted),
                        );
                        ui.add_space(8.0);
                        choice(ui, p, &mut policy, Substitution::Line, "Substitute this line only",
                            "Only the edited line uses the replacement font.");
                        choice(ui, p, &mut policy, Substitution::Document, "Substitute whole document",
                            "Every line using the missing font switches, for a consistent look.");
                    });

                    section(ui, p, "Auto-save", |ui| {
                        ui.label(
                            RichText::new(
                                "Unsaved edits are copied to a private recovery folder on this computer. \
                                 Your original file and your saved copies are never overwritten. If the \
                                 editor closes unexpectedly, it offers to restore the edits on next start.",
                            )
                            .color(p.muted),
                        );
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let mut option = |ui: &mut Ui, value: AutoSave, label: &str| {
                                let selected = autosave == value;
                                let text = if selected {
                                    RichText::new(label).color(p.accent_text)
                                } else {
                                    RichText::new(label)
                                };
                                let mut button = Button::new(text).min_size(vec2(72.0, 32.0));
                                button = if selected {
                                    button.fill(p.accent)
                                } else {
                                    button.stroke(Stroke::new(1.0, p.border))
                                };
                                if ui.add(button).clicked() {
                                    autosave = value;
                                }
                            };
                            option(ui, AutoSave(None), "Off");
                            for minutes in recovery::INTERVALS {
                                option(ui, AutoSave(Some(minutes)), &format!("Every {minutes} min"));
                            }
                        });
                        ui.add_space(8.0);
                        let status = match (&self.session_error, autosave.0) {
                            (Some(e), Some(_)) => format!("Auto-save is unavailable: {e}"),
                            (_, None) => "Auto-save is off. Unsaved edits are lost if the editor closes unexpectedly.".to_string(),
                            (None, Some(_)) => {
                                let folder = recovery::root()
                                    .map(|r| r.display().to_string())
                                    .unwrap_or_default();
                                format!(
                                    "Copies are kept in {folder} and deleted when you save, discard your edits or close normally."
                                )
                            }
                        };
                        ui.label(RichText::new(status).small().color(p.muted));
                    });

                    section(ui, p, "About", |ui| {
                        ui.label(
                            RichText::new(format!("PDF Editor {}", version_label()))
                                .font(theme::semibold(14.0)),
                        );
                        ui.label(
                            RichText::new(
                                "Works offline. Documents stay on this computer and the editor opens \
                                 no network connections.",
                            )
                            .color(p.muted),
                        );
                        ui.label(
                            RichText::new(
                                "Licensed GPL-3.0-only. Interface font: Inter, SIL Open Font License 1.1.",
                            )
                            .small()
                            .color(p.muted),
                        );
                    });
                    ui.add_space(28.0);
                });
            });
        });
        if policy != self.policy {
            self.set_policy(policy);
        }
        if autosave != self.autosave {
            self.set_autosave(autosave);
        }
    }

    fn drop_overlay(&self, ctx: &Context, p: &Palette) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let rect = ctx.content_rect().shrink(14.0);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            Id::new("drop-overlay"),
        ));
        painter.rect_filled(rect, CornerRadius::same(14), p.chrome.gamma_multiply(0.92));
        painter.rect_stroke(
            rect,
            CornerRadius::same(14),
            Stroke::new(2.0, p.accent),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Drop PDF to open",
            theme::semibold(18.0),
            p.text,
        );
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(ui);
        self.poll(&ctx);
        self.autosave_tick(&ctx);
        self.close_guard(&ctx);
        self.dropped_files(&ctx);
        self.shortcuts(&ctx);

        let toolbar = egui::Panel::top("toolbar")
            .frame(
                Frame::new()
                    .fill(p.chrome)
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show_separator_line(false)
            .exact_size(48.0)
            .show(ui, |ui| self.toolbar(ui, &p));
        let toolbar_bottom = toolbar.response.rect.bottom();

        egui::Panel::bottom("status")
            .frame(
                Frame::new()
                    .fill(p.chrome)
                    .inner_margin(Margin::symmetric(14, 4))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show_separator_line(false)
            .exact_size(28.0)
            .show(ui, |ui| self.status_bar(ui, &p));

        if !self.text_tool.selection.is_empty() && !self.text_tool.adding && !self.settings_open {
            egui::Panel::top("selected-object").show(ui, |ui| self.object_tools(ui));
        }
        if self.text_tool.adding && !self.settings_open {
            egui::Panel::top("text-tools").show(ui, |ui| self.text_tools(ui));
        }
        if self.find.open && !self.settings_open {
            egui::Panel::top("find").show(ui, |ui| self.find_bar(ui));
        }
        if self.doc.is_some() && !self.settings_open {
            egui::Panel::left("thumbnails")
                .default_size(180.0)
                .resizable(true)
                .show(ui, |ui| self.thumbnails(ui));
        }
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(p.canvas))
            .show(ui, |ui| {
                if self.settings_open {
                    self.settings_page(ui, &p);
                } else if self.doc.is_some() {
                    self.canvas(ui, &p);
                } else {
                    self.empty_state(ui, &p);
                }
            });

        self.banners(&ctx, toolbar_bottom, &p);
        self.confirm_modal(&ctx, &p);
        self.restore_modal(&ctx, &p);
        self.drop_overlay(&ctx, &p);
        self.update_title(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // An exit with unsaved edits that were not discarded keeps the copy
        // for the next start.
        let unsaved = self.doc.as_ref().is_some_and(DocView::dirty) && !self.allow_close;
        if let Some(session) = self.session.take()
            && !unsaved
        {
            session.close();
        }
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}

// ----- Drawing helpers -----

/// Version shown in the interface; debug builds are marked for testers.
fn version_label() -> String {
    if cfg!(debug_assertions) {
        format!("v{VERSION} (debug)")
    } else {
        format!("v{VERSION}")
    }
}

fn section(ui: &mut Ui, p: &Palette, title: &str, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(p.chrome)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).font(theme::semibold(15.0)));
            ui.add_space(4.0);
            add(ui);
        });
    ui.add_space(14.0);
}

/// `<stem>_edited.pdf` next to the original file.
fn edited_name(source: &std::path::Path) -> Option<PathBuf> {
    let stem = source.file_stem()?.to_string_lossy();
    Some(source.with_file_name(format!("{stem}_edited.pdf")))
}

fn time_ago(when: std::time::SystemTime) -> String {
    let secs = when.elapsed().map_or(0, |d| d.as_secs());
    match secs {
        0..60 => "less than a minute ago".into(),
        60..3600 => format!("{} minutes ago", secs / 60),
        3600..86_400 => format!("{} hours ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn modal_frame(ctx: &Context) -> Frame {
    Frame::window(&ctx.global_style()).inner_margin(Margin::same(22))
}

fn secondary_button(label: &str, p: &Palette) -> Button<'static> {
    Button::new(label.to_string())
        .stroke(Stroke::new(1.0, p.border))
        .min_size(vec2(84.0, 32.0))
}

fn tool_button(ui: &mut Ui, label: &str, enabled: bool, tip: &str) -> egui::Response {
    ui.add_enabled(enabled, Button::new(label).min_size(vec2(30.0, 30.0)))
        .on_hover_text(tip)
        .on_disabled_hover_text(tip)
}

fn divider(ui: &mut Ui, p: &Palette) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.border);
    ui.add_space(6.0);
}

fn choice(
    ui: &mut Ui,
    p: &Palette,
    value: &mut Substitution,
    option: Substitution,
    title: &str,
    detail: &str,
) {
    let selected = *value == option;
    let stroke = if selected {
        Stroke::new(1.5, p.accent)
    } else {
        Stroke::new(1.0, p.border)
    };
    let response = Frame::new()
        .stroke(stroke)
        .fill(if selected {
            p.tint
        } else {
            Color32::TRANSPARENT
        })
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                ui.painter().circle_stroke(
                    rect.center(),
                    7.0,
                    Stroke::new(1.5, if selected { p.accent } else { p.muted }),
                );
                if selected {
                    ui.painter().circle_filled(rect.center(), 3.5, p.accent);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(title).font(theme::semibold(13.5)));
                    ui.label(RichText::new(detail).small().color(p.muted));
                });
            });
        })
        .response;
    let response = ui.interact(
        response.rect,
        Id::new(("substitution", title)),
        Sense::click(),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if response.clicked() {
        *value = option;
    }
    ui.add_space(4.0);
}

fn paint_page(painter: &egui::Painter, ui: &Ui, rect: Rect, page: &PageView, p: &Palette) {
    let radius = CornerRadius::same(2);
    painter.add(
        Shadow {
            offset: [0, 3],
            blur: 18,
            spread: 0,
            color: p.page_shadow,
        }
        .as_shape(rect, radius),
    );
    painter.rect_filled(rect, radius, Color32::WHITE);
    match (&page.texture, &page.error) {
        (Some(texture), _) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.handle.id(), rect, uv, Color32::WHITE);
        }
        (None, Some(error)) => {
            let font = FontId::proportional(13.0);
            let text = format!("This page could not be displayed.\n{error}");
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                font,
                Color32::from_rgb(0x8C, 0x2F, 0x2F),
            );
        }
        (None, None) => {
            let spinner = Rect::from_center_size(rect.center(), vec2(20.0, 20.0));
            if ui.is_rect_visible(spinner) {
                egui::Spinner::new()
                    .size(20.0)
                    .color(Color32::from_gray(170))
                    .paint_at(ui, spinner);
            }
        }
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_black_alpha(18)),
        StrokeKind::Outside,
    );
}

/// Draws the in-place edit box. Returns Some(true) to apply (Enter or focus
/// moved elsewhere) and Some(false) to cancel (Escape).
fn edit_box(
    ui: &mut Ui,
    doc: &mut DocView,
    page_rects: &[Rect],
    scale: f32,
    p: &Palette,
) -> Option<bool> {
    let edit = doc.edit.as_mut()?;
    let page_rect = *page_rects.get(edit.page)?;
    let screen = to_screen(edit.line.rect, page_rect, scale);
    let first = &edit.line.objects[0];

    // Match the visible glyph size: the object's font size scaled by its
    // matrix. Bounds are glyph-tight (cap height without descenders is about
    // 0.7 em), so they only serve as a fallback when the content stream adds
    // its own scaling.
    let [a, b, c, d, _, _] = first.matrix;
    let from_matrix = first.size * (a * d - b * c).abs().sqrt() * scale;
    let height = screen.height().max(4.0);
    let font_px = if from_matrix > height * 0.6 && from_matrix < height * 2.4 {
        from_matrix
    } else {
        height * 1.2
    }
    .clamp(6.0, 240.0);
    let font = FontId::proportional(font_px);
    let [r, g, bl, alpha] = first.color;
    let color = if alpha < 32 {
        Color32::BLACK
    } else {
        Color32::from_rgb(r, g, bl)
    };

    let text_width = ui.fonts_mut(|f| {
        f.layout_no_wrap(edit.text.clone(), font.clone(), color)
            .size()
            .x
    });
    let box_height = screen.height().max(font_px * 1.25);
    let mut rect = Rect::from_center_size(screen.center(), vec2(screen.width(), box_height))
        .expand2(vec2(3.0, 1.0));
    rect.max.x = rect.max.x.max(rect.min.x + text_width + 10.0);

    let painter = ui.painter();
    painter.add(
        Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 0,
            color: p.page_shadow,
        }
        .as_shape(rect, CornerRadius::same(3)),
    );
    painter.rect_filled(rect, CornerRadius::same(3), Color32::WHITE);
    painter.rect_filled(rect, CornerRadius::same(3), p.tint.gamma_multiply(0.5));
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.5, p.accent),
        StrokeKind::Outside,
    );

    let id = Id::new(("edit-box", edit.page, first.index, first.revision));
    let editor = TextEdit::singleline(&mut edit.text)
        .id(id)
        .font(font)
        .text_color(color)
        .frame(Frame::NONE)
        .margin(Margin::symmetric(3, 0))
        .desired_width(rect.width() - 6.0)
        .vertical_align(Align::Center)
        .cursor_at_end(true)
        .char_limit(16_384);
    let response = ui.put(rect, editor);
    if edit.focus {
        response.request_focus();
        edit.focus = false;
        return None;
    }
    if response.lost_focus() {
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        return Some(!escape);
    }
    None
}

fn to_screen(raster: Rect, page: Rect, scale: f32) -> Rect {
    Rect::from_min_max(
        page.min + raster.min.to_vec2() * scale,
        page.min + raster.max.to_vec2() * scale,
    )
}

/// Joins consecutive objects into lines: same font, size, color and
/// horizontal orientation, on one baseline, advancing left to right with at
/// most a word gap between them. Space-only objects never start or end a line.
fn group_lines(items: Vec<(TextObject, Rect)>) -> Vec<Line> {
    fn em(o: &TextObject) -> f32 {
        (o.size * o.matrix[3].abs()).max(0.1)
    }
    fn joins(a: &TextObject, b: &TextObject) -> bool {
        let horizontal = |o: &TextObject| {
            o.matrix[1].abs() < 1e-3 && o.matrix[2].abs() < 1e-3 && o.matrix[0] > 0.0
        };
        let size = em(a);
        horizontal(a)
            && horizontal(b)
            && a.font == b.font
            && a.color == b.color
            && (a.size - b.size).abs() < 0.01
            && a.matrix[..4]
                .iter()
                .zip(&b.matrix[..4])
                .all(|(x, y)| (x - y).abs() < 1e-3)
            && (a.matrix[5] - b.matrix[5]).abs() < 0.2 * size
            && b.bounds.left >= a.bounds.left - 0.1 * size
            && b.bounds.left - a.bounds.right <= 0.8 * size
    }
    let blank = |o: &TextObject| o.text.trim().is_empty();
    let mut groups: Vec<Vec<(TextObject, Rect)>> = vec![];
    for item in items {
        match groups.last_mut() {
            Some(group)
                if group.last().is_some_and(|(last, _)| joins(last, &item.0))
                    && !(group.len() == 1 && blank(&group[0].0)) =>
            {
                group.push(item)
            }
            _ => groups.push(vec![item]),
        }
    }
    let mut lines = vec![];
    for mut group in groups {
        // Trailing spaces become their own entries.
        let mut tail = vec![];
        while group.len() > 1 && group.last().is_some_and(|(o, _)| blank(o)) {
            tail.extend(group.pop());
        }
        lines.push(line_from(group));
        lines.extend(tail.into_iter().rev().map(|item| line_from(vec![item])));
    }
    lines
}

fn line_from(group: Vec<(TextObject, Rect)>) -> Line {
    let mut text = String::new();
    let mut rect = Rect::NOTHING;
    let mut previous: Option<&TextObject> = None;
    for (object, r) in &group {
        rect = rect.union(*r);
        if let Some(prev) = previous {
            let gap = object.bounds.left - prev.bounds.right;
            let em = (prev.size * prev.matrix[3].abs()).max(0.1);
            let spaced = text.ends_with(' ') || object.text.starts_with(' ');
            if (object.text.trim().is_empty() || gap > 0.2 * em) && !spaced {
                text.push(' ');
            }
        }
        if !object.text.trim().is_empty() || previous.is_none() {
            text.push_str(&object.text);
        }
        previous = Some(object);
    }
    Line {
        objects: group.into_iter().map(|(o, _)| o).collect(),
        rect,
        text,
    }
}

fn raster_rect(transform: &PageTransform, object: &TextObject) -> Rect {
    let b = object.bounds;
    let corners = [
        (b.left, b.bottom),
        (b.right, b.bottom),
        (b.left, b.top),
        (b.right, b.top),
    ];
    let mut rect = Rect::NOTHING;
    for (x, y) in corners {
        let (rx, ry) = transform.apply(x, y);
        rect.extend_with(pos2(rx, ry));
    }
    rect
}

fn render_scale(info: &PageInfo, wanted: f32) -> f32 {
    let area = (info.width * info.height).max(1.0);
    let side = info.width.max(info.height).max(1.0);
    let cap = (MAX_RENDER_PIXELS / area)
        .sqrt()
        .min(MAX_TEXTURE_SIDE / side);
    // Quantize downward so zoom jitter does not trigger re-renders and the
    // cap still holds after rounding.
    (wanted.min(cap).max(0.05) * 64.0).floor().max(1.0) / 64.0
}

fn round_rect(rect: Rect, ppp: f32) -> Rect {
    let round = |v: f32| (v * ppp).round() / ppp;
    Rect::from_min_size(pos2(round(rect.min.x), round(rect.min.y)), rect.size())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the core's dimension check: ceil(points * scale) per side.
    fn pixels(info: &PageInfo, scale: f32) -> f32 {
        (info.width * scale).ceil() * (info.height * scale).ceil()
    }

    fn glyph(text: &str, left: f32, baseline: f32) -> (TextObject, Rect) {
        let object = TextObject {
            page: 0,
            index: (left * 10.0) as usize + baseline as usize * 10_000,
            revision: 0,
            text: text.into(),
            font: "F".into(),
            size: 10.0,
            bounds: pdf_editor_core::Bounds {
                left,
                bottom: baseline - 2.0,
                right: left + if text.is_empty() { 2.5 } else { 5.0 },
                top: baseline + 7.0,
            },
            color: [0, 0, 0, 255],
            matrix: [1.0, 0.0, 0.0, 1.0, left, baseline],
        };
        let rect = Rect::from_min_max(
            pos2(left, 100.0 - baseline - 7.0),
            pos2(left + 5.0, 100.0 - baseline + 2.0),
        );
        (object, rect)
    }

    #[test]
    fn glyph_objects_group_into_lines() {
        let items = vec![
            glyph("R", 10.0, 50.0),
            glyph("e", 15.0, 50.0),
            glyph("", 20.0, 50.0),
            glyph("f", 23.0, 50.0),
            glyph("", 28.0, 50.0),
            // A distant column on the same baseline starts a new line.
            glyph("x", 60.0, 50.0),
            // The next line down.
            glyph("Next line", 10.0, 30.0),
        ];
        let lines = group_lines(items);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Re f", "", "x", "Next line"]);
        assert_eq!(lines[0].objects.len(), 4);
        assert!(lines[0].rect.min.x <= 10.0 && lines[0].rect.max.x >= 28.0);
    }

    #[test]
    fn render_scale_stays_within_core_limits() {
        let pages = [
            PageInfo {
                width: 612.0,
                height: 792.0,
            },
            PageInfo {
                width: 2384.0,
                height: 3370.0,
            },
            PageInfo {
                width: 14400.0,
                height: 200.0,
            },
            PageInfo {
                width: 1.0,
                height: 1.0,
            },
        ];
        for info in &pages {
            for wanted in [0.01, 1.0, 2.0, MAX_ZOOM * PT_TO_UI * 3.0] {
                let scale = render_scale(info, wanted);
                assert!(scale > 0.0 && scale <= wanted.max(0.05));
                assert!(pixels(info, scale) <= 16_000_000.0, "{info:?} at {wanted}");
                assert!(info.width.max(info.height) * scale <= MAX_TEXTURE_SIDE);
            }
        }
    }
}
