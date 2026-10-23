//! Editor window: toolbar, page canvas, in-place edit box, banners, modals
//! and the settings page.

use crate::icons::Icon;
use crate::recovery::{self, AutoSave, Orphan, Session};
use crate::theme::{self, Palette};
use crate::widgets::{self, Btn, Look};
use crate::worker::{ChangeKind, PageTransform, Reply, Request, Worker};
use eframe::egui::{
    self, Align, Align2, Button, Color32, Context, CornerRadius, CursorIcon, FontId, Frame, Id,
    Key, Layout, Margin, Popup, Pos2, Rect, RectAlign, RichText, ScrollArea, Sense, Shadow, Shape,
    Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand,
    WidgetInfo, WidgetType, pos2, vec2,
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
/// Largest thumbnail in the pages sidebar, and the height of one row.
const THUMB_BOX: Vec2 = vec2(112.0, 140.0);
const THUMB_ROW: f32 = 186.0;
/// Shortcut list on the settings page: action, keys and a note.
const SHORTCUTS: [(&str, &[&str], &str); 9] = [
    ("Open a PDF", &["Ctrl", "O"], ""),
    ("Save As", &["Ctrl", "S"], "Ctrl + Shift + S also works."),
    ("Undo", &["Ctrl", "Z"], ""),
    ("Redo", &["Ctrl", "Y"], "Ctrl + Shift + Z also works."),
    ("Find text", &["Ctrl", "F"], ""),
    ("Zoom in", &["Ctrl", "+"], ""),
    ("Zoom out", &["Ctrl", "\u{2212}"], ""),
    ("Actual size", &["Ctrl", "0"], "Resets the zoom to 100%."),
    ("Zoom at the pointer", &["Ctrl", "Wheel"], ""),
];

pub struct EditorApp {
    recent_files: Vec<PathBuf>,
    dragging_page: Option<usize>,
    text_tool: TextTool,
    signatures: crate::signatures::Signatures,
    forms: crate::forms::Forms,
    replace: crate::replace::Replace,
    password: crate::passwords::PasswordDialog,
    organize: crate::organize::Organize,
    markup: crate::markup::MarkupTool,
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
    settings_tab: SettingsTab,
    /// Pages sidebar shown next to the document.
    pages_open: bool,
    /// Home page shows only the recent files list.
    home_recent: bool,
    /// Screen rect the floating text toolbar points at: the selected text
    /// object or the new text position.
    tool_anchor: Option<Rect>,
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SettingsTab {
    #[default]
    General,
    Editing,
    Shortcuts,
    About,
}

impl SettingsTab {
    const ALL: [Self; 4] = [Self::General, Self::Editing, Self::Shortcuts, Self::About];

    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Editing => "Editing",
            Self::Shortcuts => "Shortcuts",
            Self::About => "About",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::General => "Auto-save and unsaved changes.",
            Self::Editing => "How text edits handle fonts and characters.",
            Self::Shortcuts => "Keyboard shortcuts for common actions.",
            Self::About => "Version, privacy and licenses.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::General => Icon::Gear,
            Self::Editing => Icon::Edit,
            Self::Shortcuts => Icon::Keyboard,
            Self::About => Icon::Info,
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
    protected: bool,
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
    changes: pdf_editor_core::ChangeState,
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
    transform: PageTransform,
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
        self.changes.is_dirty()
    }
    fn scale(&self) -> f32 {
        self.zoom * PT_TO_UI
    }
}

impl EditorApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial: pdf_editor_core::Result<Option<PathBuf>>,
    ) -> Self {
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
            recent_files: config_path
                .as_deref()
                .and_then(|p| pdf_editor_core::Settings::load(p).ok())
                .map_or_else(Vec::new, |s| s.recent_files),
            dragging_page: None,
            text_tool: TextTool::default(),
            signatures: Default::default(),
            forms: Default::default(),
            replace: Default::default(),
            password: Default::default(),
            organize: Default::default(),
            markup: Default::default(),
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
            settings_tab: SettingsTab::default(),
            pages_open: true,
            home_recent: false,
            tool_anchor: None,
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
        match initial {
            Ok(Some(path)) => app.open_path(path),
            Ok(None) => {}
            Err(error) => app.banner(BannerKind::Error, error.to_string()),
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
        if committed || self.busy > 0 || self.doc.as_ref().is_some_and(DocView::dirty) {
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
            self.password.save(path);
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
            Reply::Annotations(items) => {
                self.busy = self.busy.saturating_sub(1);
                self.markup.items = items;
                self.markup.selected = None;
            }
            Reply::Exported(message) => {
                self.busy = self.busy.saturating_sub(1);
                self.banner(BannerKind::Success, message);
            }
            Reply::PasswordRequired(path) => {
                self.busy = self.busy.saturating_sub(1);
                self.opening = false;
                self.password.open(path);
            }
            Reply::ReplacementCount(query, count) => {
                self.busy = self.busy.saturating_sub(1);
                if self.replace.query == query {
                    self.replace.count = Some(count);
                }
            }
            Reply::FormFields(fields) => {
                self.busy = self.busy.saturating_sub(1);
                self.forms.fields = fields;
            }
            Reply::Printed(printed) => {
                self.busy = self.busy.saturating_sub(1);
                if printed {
                    self.banner(BannerKind::Success, "Document sent to the printer");
                }
            }

            Reply::Thumbnail {
                generation,
                page,
                image,
            } => {
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.generation == generation)
                    && let Some(p) = self.page_mut(page)
                {
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
                protected,
            } => {
                if self.restoring.is_none()
                    && let Some(config) = &self.config_path
                {
                    let result = (|| -> pdf_editor_core::Result<Vec<PathBuf>> {
                        let mut settings = pdf_editor_core::Settings::load(config)?;
                        let absolute = path.canonicalize().unwrap_or_else(|_| path.clone());
                        settings.remember(&absolute);
                        settings.save(config)?;
                        Ok(settings.recent_files)
                    })();
                    match result {
                        Ok(paths) => self.recent_files = paths,
                        Err(e) => self.banner(
                            BannerKind::Warning,
                            format!("Recent files could not be saved: {e}"),
                        ),
                    }
                }
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
                    protected,
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
                    changes: if restored.is_some() {
                        pdf_editor_core::ChangeState::recovered()
                    } else {
                        pdf_editor_core::ChangeState::default()
                    },
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
                        transform,
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
                        ChangeKind::Edit => doc.changes.edited(),
                        ChangeKind::Undo => doc.changes.undone(),
                        ChangeKind::Redo => doc.changes.redone(),
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
                    self.replace.count = None;
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
                    doc.changes.mark_saved();
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
                if !blocking {
                    self.banner(BannerKind::Error, message);
                    return;
                }
                self.busy = self.busy.saturating_sub(1);
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

    fn thumbnails(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(doc) = &self.doc else { return };
        let idle = self.busy == 0;
        let count = doc.pages.len();
        let current = u16::try_from(doc.current_page).unwrap_or(0);
        let mut action = None;
        Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 8,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Pages").font(theme::semibold(15.0)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let more = Btn::icon(Icon::Dots)
                            .id("page-menu")
                            .height(30.0)
                            .tip("Page actions")
                            .show(ui, p);
                        Popup::menu(&more).show(|ui| {
                            if let Some(edit) = page_menu(ui, p, current, count, idle) {
                                action = Some(edit);
                            }
                        });
                        if Btn::icon(Icon::Plus)
                            .id("insert-pages")
                            .height(30.0)
                            .enabled(idle)
                            .tip("Insert pages from a PDF before the current page")
                            .show(ui, p)
                            .clicked()
                        {
                            action = pick_insert(current);
                        }
                        if Btn::icon(Icon::ChevronLeft)
                            .id("hide-pages")
                            .height(30.0)
                            .tip("Hide pages")
                            .show(ui, p)
                            .clicked()
                        {
                            self.pages_open = false;
                        }
                    });
                });
            });
        widgets::rule(ui, p);

        let Some(doc) = &mut self.doc else { return };
        let mut requests = Vec::new();
        let mut jump = None;
        ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            THUMB_ROW,
            doc.pages.len(),
            |ui, range| {
                for index in range.clone() {
                    let selected = doc.current_page == index;
                    let page = &mut doc.pages[index];
                    ui.vertical_centered(|ui| {
                        ui.set_height(THUMB_ROW);
                        ui.add_space(14.0);
                        let natural = page.thumbnail.as_ref().map_or(
                            vec2(page.info.width, page.info.height),
                            TextureHandle::size_vec2,
                        );
                        let fit = (THUMB_BOX.x / natural.x.max(1.0))
                            .min(THUMB_BOX.y / natural.y.max(1.0));
                        let size = (natural * fit).round().max(vec2(8.0, 8.0));
                        let backdrop = ui.painter().add(Shape::Noop);
                        let (rect, response) =
                            ui.allocate_exact_size(size, Sense::click_and_drag());
                        response.widget_info(|| {
                            WidgetInfo::selected(
                                WidgetType::Button,
                                true,
                                selected,
                                format!("Page {}", index + 1),
                            )
                        });
                        ui.add_space(2.0);
                        let number = ui.label(
                            RichText::new((index + 1).to_string())
                                .size(12.5)
                                .color(if selected { p.text } else { p.muted }),
                        );
                        if selected || response.has_focus() {
                            let frame = rect.union(number.rect).expand2(vec2(8.0, 6.0));
                            ui.painter().set(
                                backdrop,
                                egui::epaint::RectShape::new(
                                    frame,
                                    CornerRadius::same(theme::RADIUS),
                                    if selected {
                                        p.accent_soft
                                    } else {
                                        Color32::TRANSPARENT
                                    },
                                    Stroke::new(2.0, p.accent),
                                    StrokeKind::Inside,
                                ),
                            );
                        } else if response.hovered() {
                            let frame = rect.union(number.rect).expand2(vec2(8.0, 6.0));
                            ui.painter().set(
                                backdrop,
                                egui::epaint::RectShape::filled(
                                    frame,
                                    CornerRadius::same(theme::RADIUS),
                                    p.hover,
                                ),
                            );
                        }
                        paint_thumbnail(ui, rect, page, p);
                        if page.thumbnail.is_none()
                            && !page.pending_thumbnail
                            && page.error.is_none()
                        {
                            page.pending_thumbnail = true;
                            requests.push(Request::Thumbnail(index as u16));
                        }
                        if response.clicked() {
                            jump = Some(index);
                        }
                        Popup::context_menu(&response).show(|ui| {
                            if let Ok(page) = u16::try_from(index)
                                && let Some(edit) = page_menu(ui, p, page, count, idle)
                            {
                                action = Some(edit);
                            }
                        });
                        if idle && response.drag_started() {
                            self.dragging_page = Some(index);
                        }
                        if let Some(from) = self.dragging_page
                            && response.contains_pointer()
                        {
                            if from != index {
                                ui.painter().rect_stroke(
                                    rect.expand(4.0),
                                    CornerRadius::same(4),
                                    Stroke::new(2.0, p.accent),
                                    StrokeKind::Outside,
                                );
                            }
                            if ui.input(|i| i.pointer.any_released()) && from != index {
                                action = Some(pdf_editor_core::PageEdit::Move {
                                    from: from as u16,
                                    to: index as u16,
                                });
                            }
                        }
                    });
                }
                for (i, page) in doc.pages.iter_mut().enumerate() {
                    if i + 2 < range.start || i > range.end + 2 {
                        page.thumbnail = None;
                    }
                }
            },
        );
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

    /// Floating toolbar for the selected text object: run, size, color,
    /// apply, delete and deselect.
    fn object_tools(&mut self, ui: &mut Ui, p: &Palette) {
        let ready = self.busy == 0
            && !self
                .doc
                .as_ref()
                .and_then(|d| d.edit.as_ref())
                .is_some_and(|e| e.text != e.line.text);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let previous = self.text_tool.selected;
            let runs = self.text_tool.selection.len();
            if runs > 1 {
                egui::ComboBox::from_id_salt("selected-run")
                    .selected_text(format!("Run {} of {runs}", self.text_tool.selected + 1))
                    .show_ui(ui, |ui| {
                        for (index, object) in self.text_tool.selection.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.text_tool.selected,
                                index,
                                format!("{}: {}", index + 1, object.text),
                            );
                        }
                    });
            }
            if previous != self.text_tool.selected
                && let Some(o) = self.text_tool.selection.get(self.text_tool.selected)
            {
                self.text_tool.size = o.size;
                self.text_tool.color = [o.color[0], o.color[1], o.color[2]];
            }
            if let Some(o) = self.text_tool.selection.get(self.text_tool.selected) {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(RichText::new(shorten(&o.font, 24)).color(p.muted))
                        .selectable(false),
                )
                .on_hover_text(format!("Font: {}", o.font));
            }
            widgets::divider(ui, p, 22.0);
            ui.add(
                egui::DragValue::new(&mut self.text_tool.size)
                    .range(1.0..=1000.0)
                    .suffix(" pt"),
            )
            .on_hover_text("Font size");
            ui.color_edit_button_srgb(&mut self.text_tool.color)
                .on_hover_text("Text color");
            widgets::divider(ui, p, 22.0);
            let finish = "Finish the text edit first";
            if Btn::new("Apply style")
                .id("apply-style")
                .look(Look::OutlineAccent)
                .height(30.0)
                .enabled(ready)
                .tip(if ready {
                    "Apply the size and color"
                } else {
                    finish
                })
                .show(ui, p)
                .clicked()
                && let Some(object) = self
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
            if Btn::icon(Icon::Trash)
                .id("delete-object")
                .height(30.0)
                .enabled(ready)
                .tip(if ready { "Delete text object" } else { finish })
                .show(ui, p)
                .clicked()
                && let Some(object) = self
                    .text_tool
                    .selection
                    .get(self.text_tool.selected)
                    .cloned()
            {
                self.cancel_edit();
                self.busy += 1;
                self.send(Request::DeleteText(object));
            }
            if Btn::icon(Icon::Close)
                .id("deselect")
                .height(30.0)
                .tip("Deselect")
                .show(ui, p)
                .clicked()
            {
                self.commit_edit();
                self.text_tool.selection.clear();
            }
        });
    }

    /// Floating toolbar for placing new text.
    fn text_tools(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if self.text_tool.at.is_none() {
                ui.label(RichText::new("Click the page where the text starts").color(p.muted));
                widgets::divider(ui, p, 22.0);
            }
            ui.add(
                TextEdit::singleline(&mut self.text_tool.text)
                    .desired_width(160.0)
                    .hint_text("Text"),
            );
            egui::ComboBox::from_id_salt("system-font")
                .width(150.0)
                .selected_text(shorten(&self.text_tool.font, 20))
                .show_ui(ui, |ui| {
                    for name in &self.text_tool.fonts {
                        ui.selectable_value(&mut self.text_tool.font, name.clone(), name);
                    }
                })
                .response
                .on_hover_text("Font");
            ui.add(
                egui::DragValue::new(&mut self.text_tool.size)
                    .range(1.0..=1000.0)
                    .suffix(" pt"),
            )
            .on_hover_text("Font size");
            ui.color_edit_button_srgb(&mut self.text_tool.color)
                .on_hover_text("Text color");
            widgets::divider(ui, p, 22.0);
            if Btn::new("Place text")
                .id("place-text")
                .look(Look::Primary)
                .height(30.0)
                .enabled(self.busy == 0 && self.text_tool.at.is_some())
                .tip("Add the text at the chosen point")
                .show(ui, p)
                .clicked()
                && let Some((page, at)) = self.text_tool.at
            {
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
            if Btn::icon(Icon::Close)
                .id("cancel-text")
                .height(30.0)
                .tip("Cancel")
                .show(ui, p)
                .clicked()
            {
                self.text_tool.adding = false;
            }
        });
    }

    /// Find bar and text toolbars floating over the page canvas.
    fn floating_tools(&mut self, ctx: &Context, canvas: Rect, p: &Palette) {
        let mut top = canvas.top() + 12.0;
        if self.find.open {
            let at = pos2(canvas.right() - 16.0, top);
            floating(ctx, "find-bar", canvas, Align2::RIGHT_TOP, at, p, |ui| {
                self.find_bar(ui, p);
            });
            top += 60.0;
        }
        let adding = self.text_tool.adding;
        if !adding && self.text_tool.selection.is_empty() {
            return;
        }
        let (pivot, at) = match self.tool_anchor.filter(|r| canvas.intersects(*r)) {
            Some(r) if r.top() - 64.0 > top => {
                (Align2::CENTER_BOTTOM, pos2(r.center().x, r.top() - 10.0))
            }
            Some(r) => (Align2::CENTER_TOP, pos2(r.center().x, r.bottom() + 10.0)),
            None => (Align2::CENTER_TOP, pos2(canvas.center().x, top)),
        };
        floating(ctx, "text-tools", canvas, pivot, at, p, |ui| {
            if adding {
                self.text_tools(ui, p);
            } else {
                self.object_tools(ui, p);
            }
        });
    }

    fn go_match(&mut self) {
        if let Some((page, boxes)) = self.find.matches.get(self.find.active)
            && let Some(doc) = &mut self.doc
        {
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
    fn find_bar(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add_space(4.0);
            widgets::icon(ui, Icon::Search, 18.0, p.muted);
            let field_id = Id::new("find-field");
            let focused = ui.memory(|m| m.has_focus(field_id));
            Frame::new()
                .fill(p.field)
                .stroke(Stroke::new(
                    1.0,
                    if focused { p.accent } else { p.border_strong },
                ))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(Margin {
                    left: 10,
                    right: 3,
                    top: 3,
                    bottom: 3,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let field = ui.add(
                            TextEdit::singleline(&mut self.find.query)
                                .id(field_id)
                                .frame(Frame::NONE)
                                .hint_text("Find in document")
                                .desired_width(170.0),
                        );
                        if self.find.focus {
                            field.request_focus();
                            self.find.focus = false;
                        }
                        if field.changed() {
                            self.find.matches.clear();
                            self.send(Request::Find(self.find.query.clone()));
                        }
                        let clear = Btn::icon(Icon::Close)
                            .id("find-clear")
                            .height(24.0)
                            .enabled(!self.find.query.is_empty())
                            .tip("Clear")
                            .show(ui, p);
                        if clear.clicked() {
                            self.find.query.clear();
                            self.find.matches.clear();
                            self.find.focus = true;
                        }
                    });
                });
            if Btn::new("Replace…").show(ui, p).clicked() {
                self.replace.open = true;
                self.replace.query = self.find.query.clone();
                self.replace.count = None;
            }
            let count = self.find.matches.len();
            let position = if count == 0 { 0 } else { self.find.active + 1 };
            ui.add_sized(
                vec2(58.0, 30.0),
                egui::Label::new(RichText::new(format!("{position} of {count}")).color(p.muted)),
            );
            if Btn::icon(Icon::ArrowUp)
                .id("find-previous")
                .look(Look::Outline)
                .height(32.0)
                .enabled(count > 0)
                .tip("Previous match")
                .show(ui, p)
                .clicked()
            {
                self.find.active = (self.find.active + count - 1) % count;
                self.go_match();
            }
            if Btn::icon(Icon::ArrowDown)
                .id("find-next")
                .look(Look::OutlineAccent)
                .height(32.0)
                .enabled(count > 0)
                .tip("Next match")
                .show(ui, p)
                .clicked()
            {
                self.find.active = (self.find.active + 1) % count;
                self.go_match();
            }
            widgets::divider(ui, p, 24.0);
            if Btn::icon(Icon::Close)
                .id("find-close")
                .height(32.0)
                .tip("Close find")
                .show(ui, p)
                .clicked()
            {
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
        if committed || self.busy > 0 || self.doc.as_ref().is_some_and(DocView::dirty) {
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

    /// Top bar: the document toolbar, or the app bar on the home and
    /// settings pages.
    fn toolbar(&mut self, ui: &mut Ui, p: &Palette) {
        if self.settings_open {
            self.settings_bar(ui, p);
        } else if self.doc.is_some() {
            self.document_bar(ui, p);
        } else {
            self.home_bar(ui, p);
        }
    }

    fn document_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let idle = self.busy == 0 && self.engine_alive;
        let ready = self.setup_error.is_none() && self.engine_alive;
        // Narrow windows show icons only; every button keeps its tooltip.
        let compact = ui.available_width() < 1120.0;
        let text = |label: &'static str| if compact { "" } else { label };
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            widgets::icon(ui, Icon::File, 24.0, p.accent_strong);
            ui.add_space(6.0);
            if let Some(doc) = &self.doc {
                let name = RichText::new(&doc.name)
                    .font(theme::semibold(15.0))
                    .color(p.text);
                let width = if compact { 120.0 } else { 240.0 };
                ui.allocate_ui(vec2(width, 30.0), |ui| {
                    ui.add(egui::Label::new(name).truncate())
                        .on_hover_text(shown_path(&doc.source));
                });
                if doc.dirty() {
                    ui.add_space(6.0);
                    widgets::dot(ui, p.accent_strong);
                    ui.label(RichText::new("Edited").size(13.5).color(p.muted));
                }
            }
            widgets::divider(ui, p, 26.0);
            if Btn::new(text("Open"))
                .with_icon(Icon::Folder)
                .id("open")
                .enabled(ready && idle)
                .tip("Open a PDF (Ctrl+O)")
                .show(ui, p)
                .clicked()
            {
                self.request_open(None);
            }
            let recent = Btn::icon(Icon::ChevronDown)
                .id("recent")
                .min_width(24.0)
                .height(34.0)
                .tip("Recent files")
                .show(ui, p);
            let mut open = None;
            Popup::menu(&recent).show(|ui| {
                ui.set_min_width(260.0);
                if Btn::new("Markup and notes…")
                    .enabled(idle)
                    .min_width(260.0)
                    .align_left()
                    .show(ui, p)
                    .clicked()
                {
                    self.markup.open = true;
                    if let Some(o) = self.text_tool.selection.get(self.text_tool.selected) {
                        self.markup.draft.page = o.page;
                        self.markup.draft.bounds = o.bounds;
                    }
                    self.commit_edit();
                    self.busy += 1;
                    self.send(Request::Annotations);
                    ui.close();
                }
                if Btn::new("Merge, split and extract…")
                    .enabled(idle)
                    .min_width(260.0)
                    .align_left()
                    .show(ui, p)
                    .clicked()
                {
                    self.organize.open = true;
                    ui.close();
                }
                if Btn::new("Find and replace…")
                    .enabled(idle)
                    .min_width(260.0)
                    .align_left()
                    .show(ui, p)
                    .clicked()
                {
                    self.replace.open = true;
                    ui.close();
                }
                if Btn::new("Fill form…")
                    .enabled(idle)
                    .min_width(260.0)
                    .align_left()
                    .show(ui, p)
                    .clicked()
                {
                    self.commit_edit();
                    self.forms.open = true;
                    self.busy += 1;
                    self.send(Request::FormFields);
                    ui.close();
                }
                if Btn::new("Signatures…")
                    .enabled(idle)
                    .min_width(260.0)
                    .align_left()
                    .show(ui, p)
                    .clicked()
                {
                    self.signatures.open = true;
                    ui.close();
                }
                if Btn::new("Print…")
                    .enabled(idle)
                    .min_width(260.0)
                    .align_left()
                    .show(ui, p)
                    .clicked()
                {
                    self.commit_edit();
                    self.busy += 1;
                    self.send(Request::Print);
                    ui.close();
                }
                ui.separator();
                if self.recent_files.is_empty() {
                    ui.label(RichText::new("No recent files").color(p.muted));
                }
                for path in &self.recent_files {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let tip = shown_path(path);
                    if Btn::new(&name)
                        .with_icon(Icon::File)
                        .align_left()
                        .min_width(260.0)
                        .height(32.0)
                        .enabled(idle)
                        .tip(&tip)
                        .show(ui, p)
                        .clicked()
                    {
                        open = Some(path.clone());
                        ui.close();
                    }
                }
            });
            if let Some(path) = open {
                self.request_open(Some(path));
            }
            if Btn::new(text("Save As"))
                .with_icon(Icon::Save)
                .id("save-as")
                .enabled(idle)
                .tip("Save a copy; the original is never changed (Ctrl+S)")
                .show(ui, p)
                .clicked()
            {
                self.save_as();
            }
            widgets::divider(ui, p, 26.0);
            let (can_undo, can_redo) = self
                .doc
                .as_ref()
                .map_or((false, false), |d| (d.can_undo, d.can_redo));
            if Btn::new(text("Undo"))
                .with_icon(Icon::Undo)
                .id("undo")
                .enabled(can_undo && idle)
                .tip("Undo (Ctrl+Z)")
                .show(ui, p)
                .clicked()
            {
                self.undo();
            }
            if Btn::new(text("Redo"))
                .with_icon(Icon::Redo)
                .id("redo")
                .enabled(can_redo && idle)
                .tip("Redo (Ctrl+Y)")
                .show(ui, p)
                .clicked()
            {
                self.redo();
            }
            widgets::divider(ui, p, 26.0);
            let adding = self.text_tool.adding;
            if Btn::new(text("Add text"))
                .with_icon(Icon::TextPlus)
                .id("add-text")
                .look(if adding { Look::Active } else { Look::Ghost })
                .enabled(idle)
                .tip("Place a new text box")
                .show(ui, p)
                .clicked()
            {
                self.commit_edit();
                self.text_tool.adding = !adding;
                self.text_tool.at = None;
                if self.text_tool.fonts.is_empty() {
                    self.send(Request::Fonts);
                }
            }
            if Btn::new(text("Find"))
                .with_icon(Icon::Search)
                .id("find")
                .look(if self.find.open {
                    Look::Active
                } else {
                    Look::Ghost
                })
                .tip("Find text (Ctrl+F)")
                .show(ui, p)
                .clicked()
            {
                self.find.open = !self.find.open;
                self.find.focus = self.find.open;
            }
            ui.add_space(6.0);
            self.zoom_controls(ui, p);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                self.settings_button(ui, p);
                if self.busy > 0 {
                    ui.add(egui::Spinner::new().size(16.0).color(p.muted));
                }
            });
        });
    }

    fn home_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let ready = self.setup_error.is_none() && self.engine_alive && self.busy == 0;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            brand(ui, p);
            for (recent, icon, label, key) in [
                (false, Icon::Home, "Home", "nav-home"),
                (true, Icon::Clock, "Recent", "nav-recent"),
            ] {
                let look = if self.home_recent == recent {
                    Look::Active
                } else {
                    Look::Ghost
                };
                if Btn::new(label)
                    .with_icon(icon)
                    .id(key)
                    .look(look)
                    .show(ui, p)
                    .clicked()
                {
                    self.home_recent = recent;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                self.settings_button(ui, p);
                if Btn::new("Open PDF")
                    .with_icon(Icon::Folder)
                    .id("open")
                    .look(Look::Outline)
                    .enabled(ready)
                    .tip("Open a PDF (Ctrl+O)")
                    .show(ui, p)
                    .clicked()
                {
                    self.request_open(None);
                }
                if self.busy > 0 {
                    ui.add(egui::Spinner::new().size(16.0).color(p.muted));
                }
            });
        });
    }

    fn settings_bar(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            brand(ui, p);
            let back = match &self.doc {
                Some(doc) => format!("Back to {}", shorten(&doc.name, 48)),
                None => "Back".to_string(),
            };
            if Btn::new(&back)
                .with_icon(Icon::ArrowLeft)
                .id("back")
                .tip("Close settings (Esc)")
                .show(ui, p)
                .clicked()
            {
                self.settings_open = false;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.settings_button(ui, p);
            });
        });
    }

    fn settings_button(&mut self, ui: &mut Ui, p: &Palette) {
        let look = if self.settings_open {
            Look::Active
        } else {
            Look::Ghost
        };
        if Btn::icon(Icon::Gear)
            .id("settings")
            .look(look)
            .tip("Settings")
            .show(ui, p)
            .clicked()
        {
            self.settings_open = !self.settings_open;
            self.commit_edit();
        }
    }

    /// Bordered zoom group: out, presets, in.
    fn zoom_controls(&mut self, ui: &mut Ui, p: &Palette) {
        let has_doc = self.doc.is_some();
        let zoom = self.doc.as_ref().map_or(1.0, |d| d.zoom);
        Frame::new()
            .stroke(Stroke::new(1.0, p.border_strong))
            .corner_radius(CornerRadius::same(theme::RADIUS))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if Btn::icon(Icon::Minus)
                    .id("zoom-out")
                    .height(32.0)
                    .min_width(36.0)
                    .enabled(has_doc && zoom > MIN_ZOOM)
                    .tip("Zoom out (Ctrl+\u{2212})")
                    .show(ui, p)
                    .clicked()
                {
                    self.step_zoom(false);
                }
                widgets::divider(ui, p, 32.0);
                let label = format!("{:.0}%", zoom * 100.0);
                let presets = Btn::new(&label)
                    .id("zoom-presets")
                    .height(32.0)
                    .min_width(64.0)
                    .enabled(has_doc)
                    .tip("Zoom presets (Ctrl+0 resets to 100%)")
                    .show(ui, p);
                Popup::menu(&presets).show(|ui| self.zoom_menu(ui, p));
                widgets::divider(ui, p, 32.0);
                if Btn::icon(Icon::Plus)
                    .id("zoom-in")
                    .height(32.0)
                    .min_width(36.0)
                    .enabled(has_doc && zoom < MAX_ZOOM)
                    .tip("Zoom in (Ctrl++)")
                    .show(ui, p)
                    .clicked()
                {
                    self.step_zoom(true);
                }
            });
    }

    fn zoom_menu(&mut self, ui: &mut Ui, p: &Palette) {
        let row = |ui: &mut Ui, label: &str| {
            Btn::new(label)
                .align_left()
                .min_width(150.0)
                .height(30.0)
                .show(ui, p)
                .clicked()
        };
        for step in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0] {
            if row(ui, &format!("{:.0}%", step * 100.0)) {
                self.set_zoom(step, None);
                ui.close();
            }
        }
        ui.separator();
        if row(ui, "Fit width") {
            self.fit_zoom(false);
            ui.close();
        }
        if row(ui, "Fit page") {
            self.fit_zoom(true);
            ui.close();
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(doc) = &self.doc else { return };
        let (current, total) = (doc.current_page + 1, doc.pages.len());
        let hint = if doc.edit.is_some() {
            "Enter or click away applies \u{b7} Esc cancels"
        } else {
            "Click text to edit it in place"
        };
        let zoom = format!("{:.0}%", doc.zoom * 100.0);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("Page").color(p.muted));
            ui.label(RichText::new(current.to_string()).color(p.text));
            ui.label(RichText::new(format!("of {total}")).color(p.muted));
            ui.add_space(16.0);
            ui.add(egui::Label::new(RichText::new(hint).small().color(p.muted)).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let presets = Btn::new(&zoom)
                    .trailing(Icon::ChevronDown)
                    .id("status-zoom")
                    .height(28.0)
                    .tip("Zoom presets")
                    .show(ui, p);
                Popup::menu(&presets)
                    .align(RectAlign::TOP_END)
                    .show(|ui| self.zoom_menu(ui, p));
                widgets::divider(ui, p, 18.0);
                let pages = Btn::icon(Icon::Sidebar)
                    .id("toggle-pages")
                    .height(28.0)
                    .look(if self.pages_open {
                        Look::Ghost
                    } else {
                        Look::Active
                    })
                    .tip(if self.pages_open {
                        "Hide pages"
                    } else {
                        "Show pages"
                    })
                    .show(ui, p);
                if pages.clicked() {
                    self.pages_open = !self.pages_open;
                }
                widgets::divider(ui, p, 18.0);
                ui.label(RichText::new(version_label()).small().color(p.muted))
                    .on_hover_text("PDF Editor version");
            });
        });
    }

    /// Home page: open card and recent files, or the recent list alone.
    fn empty_state(&mut self, ui: &mut Ui, p: &Palette) {
        let mut open: Option<Option<PathBuf>> = None;
        let recent_files = self.recent_files.clone();
        let idle = self.busy == 0 && self.engine_alive && self.setup_error.is_none();
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let width = (ui.available_width() - 48.0).clamp(280.0, 620.0);
            let left = ((ui.available_width() - width) / 2.0).max(0.0);
            ui.add_space(44.0);
            ui.horizontal(|ui| {
                ui.add_space(left);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    if !self.home_recent {
                        ui.label(
                            RichText::new("WORKSPACE")
                                .size(11.5)
                                .extra_letter_spacing(2.4)
                                .color(p.muted),
                        );
                        ui.label(RichText::new("Open a PDF").font(theme::semibold(34.0)));
                        ui.label(
                            RichText::new("Drop a file here or browse your computer.")
                                .size(16.0)
                                .color(p.muted),
                        );
                        ui.add_space(14.0);
                        if self.drop_zone(ui, p, width) {
                            open = Some(None);
                        }
                        ui.add_space(32.0);
                    }
                    ui.label(RichText::new("Recent files").font(theme::semibold(24.0)));
                    let summary = if recent_files.is_empty() {
                        "Files you open appear here.".to_string()
                    } else {
                        format!("{} most recent", recent_files.len())
                    };
                    ui.label(RichText::new(summary).color(p.muted));
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for (index, path) in recent_files.iter().enumerate() {
                        if recent_row(ui, p, index, path, idle).clicked() {
                            open = Some(Some(path.clone()));
                        }
                    }
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(format!(
                            "PDF Editor {} \u{b7} Works offline",
                            version_label()
                        ))
                        .small()
                        .color(p.muted),
                    );
                    ui.add_space(28.0);
                });
            });
        });
        if let Some(path) = open {
            self.request_open(path);
        }
    }

    /// Dashed drop card with the open button. Returns true when clicked.
    fn drop_zone(&self, ui: &mut Ui, p: &Palette, width: f32) -> bool {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 248.0), Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(10),
            p.surface,
            Stroke::new(1.0, p.border),
            StrokeKind::Inside,
        );
        widgets::dashed_rect(ui, rect.shrink(8.0), Stroke::new(1.0, p.border_strong));
        let mut clicked = false;
        let inner = egui::UiBuilder::new()
            .max_rect(rect.shrink(20.0))
            .layout(Layout::top_down(Align::Center));
        // A child Ui leaves the parent's cursor below the card.
        ui.new_child(inner).scope(|ui| {
            ui.add_space(26.0);
            widgets::icon(ui, Icon::File, 44.0, p.muted);
            ui.add_space(10.0);
            if let Some(error) = &self.setup_error {
                ui.label(RichText::new("PDF engine missing").font(theme::semibold(17.0)));
                ui.label(RichText::new(error).color(p.muted));
                return;
            }
            if self.opening {
                ui.add_space(8.0);
                ui.add(egui::Spinner::new().size(22.0).color(p.accent));
                ui.label(RichText::new("Opening\u{2026}").color(p.muted));
                return;
            }
            clicked = Btn::new("Choose file")
                .id("choose-file")
                .look(Look::OutlineAccent)
                .height(38.0)
                .min_width(150.0)
                .enabled(self.engine_alive && self.busy == 0)
                .tip("Open a PDF (Ctrl+O)")
                .show(ui, p)
                .clicked();
            ui.add_space(2.0);
            ui.label(RichText::new("or drop a PDF here").color(p.muted));
            let keys = ["Ctrl", "O"];
            let width = widgets::keys_width(ui, &keys);
            ui.allocate_ui_with_layout(
                vec2(width, 26.0),
                Layout::left_to_right(Align::Center),
                |ui| widgets::keys(ui, p, &keys),
            );
        });
        clicked
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
        self.tool_anchor = None;
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
                if let Some(selected) = self.text_tool.selection.get(self.text_tool.selected)
                    && selected.page == page_no
                    && let Some(objects) = &page.objects
                {
                    let screen = to_screen(raster_rect(&objects.transform, selected), rect, scale);
                    painter.rect_stroke(
                        screen.expand(2.0),
                        2.0,
                        Stroke::new(1.5, p.accent),
                        StrokeKind::Outside,
                    );
                    self.tool_anchor = Some(screen.expand(2.0));
                }
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
                            let fill = if n == self.find.active {
                                p.highlight_active
                            } else {
                                p.highlight
                            };
                            painter.rect_filled(r.expand(1.0), CornerRadius::same(2), fill);
                        }
                    }
                }

                if let Some(objects) = &page.objects {
                    self.signatures.paint_placement(
                        &painter,
                        page_no,
                        rect.min,
                        scale,
                        objects.transform,
                    );
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
                if self.text_tool.adding
                    && response.clicked()
                    && !busy
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    self.text_tool.at = Some((page_no, ((pos - rect.min) / scale).to_pos2()));
                }
                if let Some((pg, at)) = self.text_tool.at
                    && pg == page_no
                    && self.text_tool.adding
                {
                    let at = rect.min + at.to_vec2() * scale;
                    painter.circle_stroke(at, 5.0, Stroke::new(1.5, p.accent));
                    self.tool_anchor = Some(Rect::from_center_size(at, vec2(10.0, 10.0)));
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
                let discard = format!("Discard and {}", action.to_lowercase());
                if Btn::new(&discard)
                    .look(Look::Danger)
                    .enabled(self.busy == 0)
                    .show(ui, p)
                    .clicked()
                {
                    choice = Some(true);
                }
                if Btn::new("Cancel")
                    .look(Look::Outline)
                    .min_width(84.0)
                    .show(ui, p)
                    .clicked()
                {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match (choice, self.confirm.take()) {
            (Some(true), Some(Pending::Open(path))) => {
                // Keep dirty state and recovery until another document actually opens.
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
        if doc.protected
            || !doc.dirty()
            || doc.copying
            || doc.copied_generation == Some(doc.generation)
        {
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
                    let button = |label, look| Btn::new(label).look(look).min_width(84.0);
                    if button("Restore", Look::Primary).show(ui, p).clicked() {
                        action = Some(0);
                    }
                    if button("Discard", Look::Outline).show(ui, p).clicked() {
                        action = Some(1);
                    }
                    if button("Later", Look::Outline)
                        .tip("Keep the copy and ask again next time")
                        .show(ui, p)
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

    fn settings_nav(&mut self, ui: &mut Ui, p: &Palette) {
        Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 16,
                top: 28,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Settings").font(theme::semibold(22.0)));
                });
                ui.add_space(14.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                let width = ui.available_width();
                for tab in SettingsTab::ALL {
                    let selected = self.settings_tab == tab;
                    let response = Btn::new(tab.title())
                        .with_icon(tab.icon())
                        .id(tab.title())
                        .align_left()
                        .min_width(width)
                        .height(44.0)
                        .look(if selected { Look::Active } else { Look::Ghost })
                        .show(ui, p);
                    if selected {
                        let bar = Rect::from_min_size(
                            response.rect.left_top(),
                            vec2(3.0, response.rect.height()),
                        );
                        ui.painter().rect_filled(
                            bar,
                            CornerRadius {
                                nw: 2,
                                sw: 2,
                                ne: 0,
                                se: 0,
                            },
                            p.accent,
                        );
                    }
                    if response.clicked() {
                        self.settings_tab = tab;
                    }
                }
            });
    }

    fn settings_page(&mut self, ui: &mut Ui, p: &Palette) {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.settings_open = false;
        }
        let mut policy = self.policy;
        let mut autosave = self.autosave;
        let tab = self.settings_tab;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            Frame::new()
                .inner_margin(Margin {
                    left: 40,
                    right: 40,
                    top: 28,
                    bottom: 32,
                })
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(880.0));
                    ui.label(RichText::new(tab.title()).font(theme::semibold(28.0)));
                    ui.label(RichText::new(tab.summary()).size(15.0).color(p.muted));
                    ui.add_space(16.0);
                    widgets::rule(ui, p);
                    ui.add_space(18.0);
                    match tab {
                        SettingsTab::General => self.general_settings(ui, p, &mut autosave),
                        SettingsTab::Editing => editing_settings(ui, p, &mut policy),
                        SettingsTab::Shortcuts => {
                            section_title(ui, "Keyboard shortcuts");
                            for (action, keys, note) in SHORTCUTS {
                                widgets::setting_row(ui, p, action, note, |ui| {
                                    widgets::keys(ui, p, keys);
                                });
                            }
                        }
                        SettingsTab::About => {
                            section_title(ui, "PDF Editor");
                            widgets::setting_row(ui, p, "Version", "", |ui| {
                                ui.label(RichText::new(version_label()).color(p.text));
                            });
                            widgets::setting_row(
                                ui,
                                p,
                                "Privacy",
                                "Works offline. Documents stay on this computer and the editor \
                                 opens no network connections.",
                                |_| {},
                            );
                            widgets::setting_row(ui, p, "License", "", |ui| {
                                ui.label(RichText::new(env!("CARGO_PKG_LICENSE")).color(p.text));
                            });
                            widgets::setting_row(ui, p, "Interface font", "", |ui| {
                                ui.label(
                                    RichText::new("Inter, SIL Open Font License 1.1").color(p.text),
                                );
                            });
                        }
                    }
                });
        });
        if policy != self.policy {
            self.set_policy(policy);
        }
        if autosave != self.autosave {
            self.set_autosave(autosave);
        }
    }

    fn general_settings(&self, ui: &mut Ui, p: &Palette, autosave: &mut AutoSave) {
        section_title(ui, "Auto-save");
        let label = |value: AutoSave| match value.0 {
            None => "Off".to_string(),
            Some(minutes) => format!("Every {minutes} minutes"),
        };
        widgets::setting_row(
            ui,
            p,
            "Recovery copies",
            "Copy unsaved edits to a private recovery folder on this computer. If the \
             editor closes unexpectedly, it offers to restore them on the next start.",
            |ui| {
                egui::ComboBox::from_id_salt("autosave")
                    .width(240.0)
                    .selected_text(label(*autosave))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(autosave, AutoSave(None), label(AutoSave(None)));
                        for minutes in recovery::INTERVALS {
                            let value = AutoSave(Some(minutes));
                            ui.selectable_value(autosave, value, label(value));
                        }
                    });
            },
        );
        let status = match (&self.session_error, autosave.0) {
            (Some(e), Some(_)) => format!("Auto-save is unavailable: {e}"),
            (_, None) => {
                "Auto-save is off. Unsaved edits are lost if the editor closes unexpectedly."
                    .to_string()
            }
            (None, Some(_)) => "Copies are deleted when you save, discard your edits or close \
                                normally. Your original file and saved copies are never \
                                overwritten."
                .to_string(),
        };
        ui.label(RichText::new(status).small().color(p.muted));
        if autosave.0.is_some()
            && self.session_error.is_none()
            && let Some(folder) = recovery::root()
        {
            let folder = shown_path(&folder);
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Folder: {folder}"))
                        .small()
                        .color(p.muted),
                )
                .truncate(),
            )
            .on_hover_text(folder);
        }
        ui.add_space(20.0);
        widgets::rule(ui, p);
        ui.add_space(18.0);
        section_title(ui, "Safety");
        widgets::setting_row(
            ui,
            p,
            "Unsaved edits",
            "Closing the window or opening another PDF asks before discarding unsaved edits.",
            |ui| {
                ui.label(RichText::new("Always on").color(p.muted));
            },
        );
        widgets::setting_row(
            ui,
            p,
            "Original files",
            "Save As writes a new file. The original PDF is never changed.",
            |ui| {
                ui.label(RichText::new("Always on").color(p.muted));
            },
        );
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
                    .inner_margin(Margin::symmetric(14, 10))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show_separator_line(false)
            .min_size(56.0)
            .show(ui, |ui| self.toolbar(ui, &p));
        let toolbar_bottom = toolbar.response.rect.bottom();

        let document = self.doc.is_some() && !self.settings_open;
        if document {
            egui::Panel::bottom("status")
                .frame(
                    Frame::new()
                        .fill(p.chrome)
                        .inner_margin(Margin::symmetric(16, 0))
                        .stroke(Stroke::new(1.0, p.border)),
                )
                .show_separator_line(false)
                .exact_size(36.0)
                .show(ui, |ui| self.status_bar(ui, &p));
            if self.pages_open {
                egui::Panel::left("thumbnails")
                    .frame(Frame::new().fill(p.chrome))
                    .default_size(200.0)
                    .min_size(150.0)
                    .max_size(320.0)
                    .resizable(true)
                    .show(ui, |ui| self.thumbnails(ui, &p));
            }
        }
        if self.settings_open {
            egui::Panel::left("settings-nav")
                .frame(Frame::new().fill(p.chrome))
                .exact_size(280.0)
                .resizable(false)
                .show(ui, |ui| self.settings_nav(ui, &p));
        }
        let fill = if document { p.canvas } else { p.chrome };
        let central = egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(fill))
            .show(ui, |ui| {
                if self.settings_open {
                    self.settings_page(ui, &p);
                } else if self.doc.is_some() {
                    self.canvas(ui, &p);
                } else {
                    self.empty_state(ui, &p);
                }
            });
        if document {
            self.floating_tools(&ctx, central.response.rect, &p);
        }

        if let Some(request) = self.markup.show(
            &ctx,
            &p,
            self.busy == 0,
            self.doc.as_ref().map_or(0, |d| d.pages.len()),
        ) {
            self.commit_edit();
            self.busy += 1;
            self.send(request);
        }
        if let Some(request) = self.organize.show(&ctx, &p, self.busy == 0) {
            self.commit_edit();
            self.busy += 1;
            self.send(request);
        }
        if let Some(request) = self.password.show(&ctx, &p, self.busy == 0) {
            self.opening = matches!(request, Request::OpenPassword(..));
            self.busy += 1;
            self.send(request);
        }
        if let Some(request) = self.replace.show(&ctx, &p, self.busy == 0) {
            self.commit_edit();
            self.busy += 1;
            self.send(request);
        }
        if let Some(request) = self.forms.show(&ctx, &p, self.busy == 0) {
            self.commit_edit();
            self.busy += 1;
            self.send(request);
        }
        let signature_folder = self
            .config_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("signatures"));
        let pages = self.doc.as_ref().map_or(0, |d| d.pages.len());
        if let Some(request) =
            self.signatures
                .show(&ctx, &p, signature_folder.as_deref(), pages, self.busy == 0)
        {
            self.commit_edit();
            self.busy += 1;
            self.send(request);
        }
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

/// Logo and name at the left of the home and settings bars.
fn brand(ui: &mut Ui, p: &Palette) {
    widgets::icon(ui, Icon::File, 26.0, p.accent_strong);
    ui.add_space(4.0);
    ui.label(RichText::new("PDF Editor").font(theme::semibold(16.0)));
    widgets::divider(ui, p, 28.0);
}

fn section_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(theme::semibold(18.0)));
    ui.add_space(10.0);
}

fn editing_settings(ui: &mut Ui, p: &Palette, policy: &mut Substitution) {
    section_title(ui, "Font substitution");
    widgets::setting_row(
        ui,
        p,
        "When a typed character is missing",
        "The original font lacks the character and is not installed. The closest \
         installed font is used and a warning is shown.",
        |ui| {
            widgets::segmented(
                ui,
                p,
                "substitution",
                policy,
                &[
                    (Substitution::Line, "This line"),
                    (Substitution::Document, "Whole document"),
                ],
            );
        },
    );
    let detail = match policy {
        Substitution::Line => "Only the edited line uses the replacement font.",
        Substitution::Document => {
            "Every line using the missing font switches, for a consistent look."
        }
    };
    ui.label(RichText::new(detail).small().color(p.muted));
    ui.add_space(20.0);
    widgets::rule(ui, p);
    ui.add_space(18.0);
    section_title(ui, "Text editing");
    widgets::setting_row(
        ui,
        p,
        "Apply an edit",
        "Clicking elsewhere also applies the typed text.",
        |ui| widgets::keys(ui, p, &["Enter"]),
    );
    widgets::setting_row(ui, p, "Cancel an edit", "", |ui| {
        widgets::keys(ui, p, &["Esc"]);
    });
}

/// Page actions shared by the sidebar menu and a thumbnail's context menu.
fn page_menu(
    ui: &mut Ui,
    p: &Palette,
    page: u16,
    count: usize,
    idle: bool,
) -> Option<pdf_editor_core::PageEdit> {
    use pdf_editor_core::PageEdit;
    let item = |ui: &mut Ui, icon: Icon, label: &str, enabled: bool| {
        let clicked = Btn::new(label)
            .with_icon(icon)
            .align_left()
            .min_width(230.0)
            .height(34.0)
            .enabled(enabled)
            .show(ui, p)
            .clicked();
        if clicked {
            ui.close();
        }
        clicked
    };
    let mut action = None;
    if item(ui, Icon::RotateRight, "Rotate right", idle) {
        action = Some(PageEdit::Rotate { page });
    }
    if item(ui, Icon::Trash, "Delete page", idle && count > 1) {
        action = Some(PageEdit::Delete { page });
    }
    if item(ui, Icon::FilePlus, "Insert pages from PDF\u{2026}", idle) {
        action = pick_insert(page);
    }
    action
}

fn pick_insert(before: u16) -> Option<pdf_editor_core::PageEdit> {
    let path = rfd::FileDialog::new()
        .set_title("Insert pages from PDF")
        .add_filter("PDF document", &["pdf"])
        .pick_file()?;
    Some(pdf_editor_core::PageEdit::Insert { path, before })
}

fn paint_thumbnail(ui: &Ui, rect: Rect, page: &PageView, p: &Palette) {
    let painter = ui.painter();
    let radius = CornerRadius::same(2);
    painter.add(
        Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 0,
            color: p.page_shadow,
        }
        .as_shape(rect, radius),
    );
    painter.rect_filled(rect, radius, Color32::WHITE);
    match &page.thumbnail {
        Some(texture) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), rect, uv, Color32::WHITE);
        }
        None => {
            let text = if page.error.is_some() {
                "Unavailable"
            } else {
                "Loading\u{2026}"
            };
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(11.5),
                Color32::from_gray(140),
            );
        }
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_black_alpha(24)),
        StrokeKind::Outside,
    );
}

/// One recent file: icon, name and folder; an arrow appears on hover.
fn recent_row(
    ui: &mut Ui,
    p: &Palette,
    index: usize,
    path: &std::path::Path,
    enabled: bool,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::hover());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, Id::new(("recent-file", index)), sense);
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, &name));
    let hot = enabled && (response.hovered() || response.has_focus());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(theme::RADIUS),
        if hot { p.hover } else { p.surface },
        Stroke::new(
            1.0,
            if response.has_focus() {
                p.accent
            } else {
                p.border
            },
        ),
        StrokeKind::Inside,
    );
    let icon = Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), vec2(22.0, 22.0));
    crate::icons::paint(
        painter,
        icon,
        Icon::File,
        if hot { p.text } else { p.muted },
    );
    let text_width = rect.width() - 56.0 - 52.0;
    let line = |text: &str, font: FontId, color: Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_width.max(40.0));
        painter.layout_job(job)
    };
    let title = line(&name, theme::semibold(14.5), p.text);
    let folder = path.parent().map(shown_path).unwrap_or_default();
    let folder = line(&folder, FontId::proportional(12.5), p.muted);
    painter.galley(pos2(rect.left() + 56.0, rect.top() + 10.0), title, p.text);
    painter.galley(pos2(rect.left() + 56.0, rect.top() + 31.0), folder, p.muted);
    if hot {
        let arrow =
            Rect::from_center_size(pos2(rect.right() - 28.0, rect.center().y), vec2(18.0, 18.0));
        crate::icons::paint(painter, arrow, Icon::ArrowRight, p.accent_strong);
    }
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Path for display, without the `\\?\` prefix of canonicalized Windows paths.
fn shown_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.to_string(),
        _ => text,
    }
}

/// Shortens `text` to `max` characters with an ellipsis.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let head: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{head}\u{2026}")
    }
}

/// Floating bar over the canvas, kept inside `bounds`.
fn floating(
    ctx: &Context,
    id: &str,
    bounds: Rect,
    pivot: Align2,
    at: Pos2,
    p: &Palette,
    add: impl FnOnce(&mut Ui),
) {
    egui::Area::new(Id::new(id))
        .pivot(pivot)
        .fixed_pos(at)
        .constrain_to(bounds.shrink(8.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0, p.border_strong))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(8, 6))
                .shadow(Shadow {
                    offset: [0, 6],
                    blur: 20,
                    spread: 0,
                    color: p.page_shadow,
                })
                .show(ui, add);
        });
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
    for corner in [rect.left_top(), rect.right_bottom()] {
        painter.circle(corner, 3.5, Color32::WHITE, Stroke::new(1.5, p.accent));
    }

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
    fn shown_paths_drop_the_verbatim_prefix() {
        let path = PathBuf::from(r"\\?\D:\docs\a.pdf");
        assert_eq!(shown_path(&path), r"D:\docs\a.pdf");
        let unc = PathBuf::from(r"\\?\UNC\server\share\a.pdf");
        assert_eq!(shown_path(&unc), unc.display().to_string());
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
    fn mock_app() -> (
        EditorApp,
        std::sync::mpsc::Receiver<Request>,
        std::sync::mpsc::Sender<Reply>,
    ) {
        let (worker, inbox, outbox) = crate::worker::test_channel();
        (
            EditorApp {
                recent_files: vec![],
                dragging_page: None,
                text_tool: TextTool::default(),
                signatures: Default::default(),
                forms: Default::default(),
                replace: Default::default(),
                password: Default::default(),
                organize: Default::default(),
                markup: Default::default(),
                find: FindState::default(),
                worker,
                engine_alive: true,
                setup_error: None,
                policy: Substitution::Line,
                config_path: None,
                doc: None,
                opening: false,
                busy: 0,
                banners: vec![],
                next_banner: 0,
                settings_open: false,
                settings_tab: SettingsTab::default(),
                pages_open: true,
                home_recent: false,
                tool_anchor: None,
                confirm: None,
                allow_close: false,
                title: String::new(),
                autosave: AutoSave(None),
                autosave_path: None,
                session: None,
                session_error: None,
                orphans: vec![],
                restoring: None,
            },
            inbox,
            outbox,
        )
    }
    fn opened(app: &mut EditorApp, ctx: &Context) {
        app.handle(
            ctx,
            Reply::Opened {
                protected: false,
                generation: 1,
                path: PathBuf::from("sample.pdf"),
                name: "sample.pdf".into(),
                pages: vec![
                    PageInfo {
                        width: 300.0,
                        height: 400.0,
                    },
                    PageInfo {
                        width: 400.0,
                        height: 300.0,
                    },
                ],
                suggested: PathBuf::from("sample_edited.pdf"),
            },
        );
    }
    #[test]
    fn pending_edits_and_failed_opens_keep_unsaved_guards() {
        let ctx = Context::default();
        let (mut app, _inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        app.doc.as_mut().unwrap().changes.edited();
        app.request_open(Some(PathBuf::from("missing.pdf")));
        assert!(matches!(app.confirm, Some(Pending::Open(_))));
        assert!(app.doc.as_ref().unwrap().dirty());
        app.confirm = None;
        app.open_path(PathBuf::from("missing.pdf"));
        app.handle(
            &ctx,
            Reply::Failed {
                message: "missing".into(),
                blocking: true,
                page: None,
            },
        );
        assert!(app.doc.as_ref().unwrap().dirty());
        app.busy = 1;
        app.opening = true;
        app.handle(
            &ctx,
            Reply::Failed {
                message: "search failed".into(),
                blocking: false,
                page: None,
            },
        );
        assert_eq!(app.busy, 1);
        assert!(app.opening);
        app.doc.as_mut().unwrap().changes.mark_saved();
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let mut output = ctx.run_ui(input, |ui| app.close_guard(ui.ctx()));
        output.textures_delta.clear();
        assert!(matches!(app.confirm, Some(Pending::Close)));
        assert!(!app.allow_close);
    }
    #[test]
    fn feature_panels_render_and_page_changes_refresh_dimensions() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        app.find.open = true;
        app.find.query = "sample".into();
        app.find.matches = vec![(1, vec![[10.0, 20.0, 30.0, 40.0]])];
        app.go_match();
        assert!(app.doc.as_ref().unwrap().scroll_to.unwrap().y > 400.0);
        app.text_tool.selection = vec![glyph("sample", 10.0, 50.0).0];
        for dark in [false, true] {
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1180.0, 820.0))),
                    ..Default::default()
                },
                |ui| {
                    let p = theme::palette(ui);
                    egui::Panel::top("test-toolbar").show(ui, |ui| app.toolbar(ui, &p));
                    egui::Panel::bottom("test-status").show(ui, |ui| app.status_bar(ui, &p));
                    egui::Panel::left("test-thumbnails")
                        .exact_size(200.0)
                        .show(ui, |ui| app.thumbnails(ui, &p));
                    let canvas = egui::CentralPanel::default()
                        .show(ui, |ui| app.canvas(ui, &p))
                        .response
                        .rect;
                    app.floating_tools(ui.ctx(), canvas, &p);
                },
            );
            assert!(!output.shapes.is_empty());
            output.textures_delta.clear();
        }
        app.handle(
            &ctx,
            Reply::Changed {
                pages: vec![PageInfo {
                    width: 400.0,
                    height: 300.0,
                }],
                generation: 2,
                kind: ChangeKind::Edit,
                can_undo: true,
                can_redo: false,
                warning: None,
                objects_changed: 1,
            },
        );
        let doc = app.doc.as_ref().unwrap();
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.pages[0].info.width, 400.0);
        assert!(doc.dirty());
        assert!(app.text_tool.selection.is_empty());
    }
    fn tool_frame(
        app: &mut EditorApp,
        ctx: &Context,
        events: Vec<egui::Event>,
        tool: u8,
    ) -> Vec<egui::epaint::ClippedShape> {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1180.0, 820.0))),
                ..Default::default()
            },
            |ui| {
                let p = theme::palette(ui);
                match tool {
                    0 => app.text_tools(ui, &p),
                    1 => app.object_tools(ui, &p),
                    2 => app.find_bar(ui, &p),
                    _ => app.thumbnails(ui, &p),
                }
            },
        );
        output.textures_delta.clear();
        output.shapes
    }
    fn button_center(shapes: &[egui::epaint::ClippedShape], label: &str) -> Pos2 {
        for clipped in shapes {
            if let egui::epaint::Shape::Text(text) = &clipped.shape
                && text.galley.text() == label
            {
                return text.pos + text.galley.size() * 0.5;
            }
        }
        panic!("Missing button: {label}");
    }
    /// Center of a button drawn with a stable key in the last frame.
    fn tool_center(ctx: &Context, key: &str) -> Pos2 {
        ctx.read_response(widgets::tool_id(key))
            .unwrap_or_else(|| panic!("Missing button: {key}"))
            .rect
            .center()
    }
    fn click_at(pos: Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn text_and_find_buttons_dispatch_real_worker_requests() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        app.text_tool.adding = true;
        app.text_tool.at = Some((1, pos2(40.0, 60.0)));
        app.text_tool.text = "Placed".into();
        app.text_tool.size = 24.0;
        app.text_tool.color = [20, 40, 80];
        let shapes = tool_frame(&mut app, &ctx, vec![], 0);
        let at = button_center(&shapes, "Place text");
        tool_frame(&mut app, &ctx, click_at(at), 0);
        let Request::AddText { text, raster } = inbox.try_recv().unwrap() else {
            panic!("Expected AddText")
        };
        assert_eq!(text.text, "Placed");
        assert_eq!(text.page, 1);
        assert_eq!(text.size, 24.0);
        assert_eq!(text.color, [20, 40, 80]);
        assert_eq!(raster, (40.0, 60.0));
        app.busy = 0;
        app.text_tool.selection = vec![glyph("chosen", 10.0, 50.0).0];
        let shapes = tool_frame(&mut app, &ctx, vec![], 1);
        let at = button_center(&shapes, "Apply style");
        tool_frame(&mut app, &ctx, click_at(at), 1);
        assert!(matches!(
            inbox.try_recv().unwrap(),
            Request::StyleText { size: 24.0, .. }
        ));
        app.busy = 0;
        tool_frame(&mut app, &ctx, vec![], 1);
        let at = tool_center(&ctx, "delete-object");
        tool_frame(&mut app, &ctx, click_at(at), 1);
        assert!(matches!(inbox.try_recv().unwrap(), Request::DeleteText(_)));
        app.find.matches = vec![
            (0, vec![[0.0, 0.0, 10.0, 10.0]]),
            (1, vec![[0.0, 0.0, 10.0, 10.0]]),
        ];
        tool_frame(&mut app, &ctx, vec![], 2);
        let at = tool_center(&ctx, "find-next");
        tool_frame(&mut app, &ctx, click_at(at), 2);
        assert_eq!(app.find.active, 1);
        tool_frame(&mut app, &ctx, vec![], 2);
        let at = tool_center(&ctx, "find-next");
        tool_frame(&mut app, &ctx, click_at(at), 2);
        assert_eq!(app.find.active, 0);
        tool_frame(&mut app, &ctx, vec![], 2);
        let at = tool_center(&ctx, "find-previous");
        tool_frame(&mut app, &ctx, click_at(at), 2);
        assert_eq!(app.find.active, 1);
    }
    #[test]
    fn thumbnail_drag_dispatches_page_move() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        for (index, page) in app.doc.as_mut().unwrap().pages.iter_mut().enumerate() {
            page.thumbnail = Some(ctx.load_texture(
                format!("test-{index}"),
                egui::ColorImage::filled([80, 120], Color32::WHITE),
                TextureOptions::LINEAR,
            ));
        }
        let shapes = tool_frame(&mut app, &ctx, vec![], 3);
        let from = button_center(&shapes, "1") - vec2(0.0, 50.0);
        let to = button_center(&shapes, "2") - vec2(0.0, 50.0);
        tool_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(from),
                egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            3,
        );
        tool_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(to)], 3);
        tool_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            3,
        );
        assert!(matches!(
            inbox.try_recv().unwrap(),
            Request::Pages(pdf_editor_core::PageEdit::Move { from: 0, to: 1 })
        ));
    }
    #[test]
    fn toolbar_actions_fit_minimum_window_width() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        for width in [640.0, 1180.0] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 420.0))),
                    ..Default::default()
                },
                |ui| {
                    let p = theme::palette(ui);
                    egui::Panel::top("toolbar")
                        .min_size(56.0)
                        .show(ui, |ui| app.toolbar(ui, &p));
                },
            );
            for key in [
                "open",
                "recent",
                "save-as",
                "undo",
                "redo",
                "add-text",
                "find",
                "zoom-out",
                "zoom-presets",
                "zoom-in",
                "settings",
            ] {
                let rect = ctx.read_response(widgets::tool_id(key)).unwrap().rect;
                assert!(rect.min.x >= 0.0 && rect.max.x <= width, "{key}: {rect:?}");
                assert!(rect.max.y <= 56.0, "{key} wrapped: {rect:?}");
            }
            if width > 1120.0 {
                // Wide windows label the main actions.
                for label in ["Open", "Save As", "Undo", "Redo", "Add text", "Find"] {
                    button_center(&output.shapes, label);
                }
            }
            output.textures_delta.clear();
        }
    }
    #[test]
    fn home_and_settings_pages_render_in_both_themes() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _inbox, _outbox) = mock_app();
        app.recent_files = vec![PathBuf::from(r"C:\Docs\invoice.pdf")];
        for dark in [false, true] {
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            for page in 0..6 {
                app.settings_open = page >= 2;
                app.home_recent = page == 1;
                app.settings_tab = SettingsTab::ALL[page.max(2) - 2];
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1180.0, 820.0))),
                        ..Default::default()
                    },
                    |ui| {
                        let p = theme::palette(ui);
                        egui::Panel::top("toolbar").show(ui, |ui| app.toolbar(ui, &p));
                        if app.settings_open {
                            egui::Panel::left("nav").show(ui, |ui| app.settings_nav(ui, &p));
                        }
                        egui::CentralPanel::default().show(ui, |ui| {
                            if app.settings_open {
                                app.settings_page(ui, &p);
                            } else {
                                app.empty_state(ui, &p);
                            }
                        });
                    },
                );
                let texts: Vec<String> = output
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect();
                let expected = match page {
                    0 => "Open a PDF",
                    1 => "invoice.pdf",
                    _ => app.settings_tab.title(),
                };
                assert!(texts.iter().any(|t| t == expected), "{expected}: {texts:?}");
                output.textures_delta.clear();
            }
        }
        // The settings nav switches pages.
        app.settings_open = true;
        app.settings_tab = SettingsTab::General;
        let frame = |app: &mut EditorApp, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1180.0, 820.0))),
                    ..Default::default()
                },
                |ui| {
                    let p = theme::palette(ui);
                    egui::Panel::left("nav").show(ui, |ui| app.settings_nav(ui, &p));
                },
            );
            output.textures_delta.clear();
        };
        // The first frame after the page layout changes settles the panel.
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(&mut app, click_at(tool_center(&ctx, "Shortcuts")));
        assert!(app.settings_tab == SettingsTab::Shortcuts);
    }
}
