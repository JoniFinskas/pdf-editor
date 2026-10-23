//! Editor window: toolbar, page canvas, in-place edit box, banners and modals.

use crate::theme::{self, Palette};
use crate::worker::{ChangeKind, PageTransform, Reply, Request, Worker};
use eframe::egui::{
    self, Align, Align2, Button, Color32, Context, CornerRadius, CursorIcon, FontId, Frame, Id,
    Key, KeyboardShortcut, Layout, Margin, Modifiers, Pos2, Rect, RichText, ScrollArea, Sense,
    Shadow, Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand,
    pos2, vec2,
};
use pdf_editor_core::{PageInfo, Substitution, TextObject};
use std::path::PathBuf;

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
    settings_open: bool,
    confirm: Option<Pending>,
    allow_close: bool,
    title: String,
}

struct DocView {
    name: String,
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
    /// Page and raster rect of an edit being applied.
    applying: Option<(usize, Rect)>,
    scroll_to: Option<Vec2>,
    offset: Vec2,
    viewport: Rect,
    current_page: usize,
}

struct PageView {
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
    /// Text objects with their extents in unscaled raster points.
    items: Vec<(TextObject, Rect)>,
}

struct EditState {
    page: usize,
    object: TextObject,
    rect: Rect,
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
        let worker = Worker::spawn(cc.egui_ctx.clone(), library.unwrap_or_default(), policy);
        let mut app = Self {
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
        if self.doc.as_ref().is_some_and(DocView::dirty) {
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
        let Some(doc) = &self.doc else { return };
        if self.busy > 0 {
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

    fn apply_edit(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        let Some(edit) = doc.edit.take() else { return };
        if edit.text == edit.object.text || self.busy > 0 {
            return;
        }
        doc.applying = Some((edit.page, edit.rect));
        self.busy += 1;
        self.send(Request::Edit {
            object: edit.object,
            text: edit.text,
        });
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
            Reply::Opened {
                generation,
                name,
                pages,
                suggested,
            } => {
                self.busy = self.busy.saturating_sub(1);
                self.opening = false;
                self.banners.retain(|b| b.kind != BannerKind::Error);
                let zoom = self.doc.as_ref().map_or(1.0, |d| d.zoom);
                self.doc = Some(DocView {
                    name,
                    generation,
                    pages: pages
                        .into_iter()
                        .map(|info| PageView {
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
                    saved: Some(0),
                    edit: None,
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
                    view.objects = Some(PageObjects { generation, items });
                }
            }
            Reply::Changed {
                generation,
                kind,
                can_undo,
                can_redo,
                warning,
                objects_changed,
            } => {
                self.busy = self.busy.saturating_sub(1);
                let Some(doc) = &mut self.doc else { return };
                doc.applying = None;
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
                if doc.generation != generation {
                    doc.generation = generation;
                    doc.edit = None;
                    for page in &mut doc.pages {
                        page.pending_render = None;
                        page.pending_objects = None;
                        page.objects = None;
                    }
                }
                if let Some(warning) = warning {
                    self.banner(BannerKind::Warning, warning);
                } else if kind == ChangeKind::Edit && objects_changed > 1 {
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

    fn page_mut(&mut self, page: u16) -> Option<&mut PageView> {
        self.doc.as_mut()?.pages.get_mut(usize::from(page))
    }

    // ----- Input -----

    fn shortcuts(&mut self, ctx: &Context) {
        if self.confirm.is_some() || self.settings_open {
            return;
        }
        let pressed = |m: Modifiers, k: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
        };
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        if pressed(Modifiers::COMMAND, Key::O) {
            self.request_open(None);
        }
        if pressed(shift, Key::S) || pressed(Modifiers::COMMAND, Key::S) {
            self.save_as();
        }
        if pressed(Modifiers::COMMAND, Key::Plus) || pressed(Modifiers::COMMAND, Key::Equals) {
            self.step_zoom(true);
        }
        if pressed(Modifiers::COMMAND, Key::Minus) {
            self.step_zoom(false);
        }
        if pressed(Modifiers::COMMAND, Key::Num0) {
            self.set_zoom(1.0, None);
        }
        // Text fields keep their own undo while editing.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if pressed(shift, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if pressed(Modifiers::COMMAND, Key::Z) {
            self.undo();
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
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.doc.as_ref().is_some_and(DocView::dirty)
        {
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
            self.zoom_controls(ui, has_doc);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if tool_button(ui, "Settings", true, "Font substitution settings").clicked() {
                    self.settings_open = true;
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
                    "Enter applies · Esc cancels"
                } else if self.doc.is_some() {
                    "Click text to edit it in place"
                } else {
                    "Ctrl+O to open · Drop a PDF anywhere"
                };
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
        let mut clicked_empty = false;

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

                // Hover and click hit testing against cached object extents.
                let response = ui.interact(rect, Id::new(("pdf-page", index)), Sense::click());
                let hit = response.hover_pos().and_then(|pos| {
                    let at = ((pos - rect.min) / scale).to_pos2();
                    page.objects.as_ref().and_then(|o| {
                        o.items
                            .iter()
                            .rev()
                            .find(|(_, r)| r.expand(1.0).contains(at))
                    })
                });
                let editing_this = |obj: &TextObject| {
                    doc.edit
                        .as_ref()
                        .is_some_and(|e| e.page == index && e.object.index == obj.index)
                };
                if let Some((object, raster)) = hit {
                    if !editing_this(object) && !busy {
                        let screen = to_screen(*raster, rect, scale);
                        painter.rect_stroke(
                            screen.expand(2.0),
                            CornerRadius::same(3),
                            Stroke::new(1.0, p.accent),
                            StrokeKind::Outside,
                        );
                        ctx.set_cursor_icon(CursorIcon::Text);
                    }
                    if response.clicked() && !busy && !editing_this(object) {
                        start_edit = Some(EditState {
                            page: index,
                            object: object.clone(),
                            rect: *raster,
                            text: object.text.clone(),
                            focus: true,
                        });
                    }
                } else if response.clicked() {
                    clicked_empty = true;
                }
                if let Some((applying_page, raster)) = doc.applying
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

            if let Some(edit) = start_edit.take() {
                doc.edit = Some(edit);
            } else if clicked_empty {
                doc.edit = None;
            }
            edit_box(ui, doc, &page_rects, scale, p)
        });

        doc.offset = output.state.offset;
        doc.viewport = output.inner_rect;
        let apply = output.inner;
        for request in requests {
            self.send(request);
        }
        match apply {
            Some(true) => self.apply_edit(),
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

    fn settings_modal(&mut self, ctx: &Context, p: &Palette) {
        if !self.settings_open {
            return;
        }
        let mut policy = self.policy;
        let mut close = false;
        let modal = egui::Modal::new(Id::new("settings")).frame(modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new("Settings").font(theme::semibold(17.0)));
            ui.add_space(2.0);
            ui.label(
                RichText::new("When the original font is not embedded with the needed characters and is not installed:")
                    .color(p.muted),
            );
            ui.add_space(6.0);
            choice(ui, p, &mut policy, Substitution::Line, "Substitute this line only",
                "Only the edited text object uses the closest system font.");
            choice(ui, p, &mut policy, Substitution::Document, "Substitute whole document",
                "Every text object using the missing font switches, for a consistent look.");
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(secondary_button("Done", p)).clicked() {
                    close = true;
                }
            });
        });
        if policy != self.policy {
            self.set_policy(policy);
        }
        if close || modal.should_close() {
            self.settings_open = false;
        }
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
                if let Some(doc) = &mut self.doc {
                    doc.saved = Some(doc.position);
                }
                self.open_dialog_or_path(path);
            }
            (Some(true), Some(Pending::Close)) => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            (Some(_), _) => {}
            (None, pending) => self.confirm = pending,
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

        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(p.canvas))
            .show(ui, |ui| {
                if self.doc.is_some() {
                    self.canvas(ui, &p);
                } else {
                    self.empty_state(ui, &p);
                }
            });

        self.banners(&ctx, toolbar_bottom, &p);
        self.settings_modal(&ctx, &p);
        self.confirm_modal(&ctx, &p);
        self.drop_overlay(&ctx, &p);
        self.update_title(&ctx);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}

// ----- Drawing helpers -----

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

/// Draws the in-place edit box. Returns Some(true) to apply, Some(false) to cancel.
fn edit_box(
    ui: &mut Ui,
    doc: &mut DocView,
    page_rects: &[Rect],
    scale: f32,
    p: &Palette,
) -> Option<bool> {
    let edit = doc.edit.as_mut()?;
    let page_rect = *page_rects.get(edit.page)?;
    let screen = to_screen(edit.rect, page_rect, scale);

    // Match the visible glyph size: the object's font size scaled by its
    // matrix. Bounds are glyph-tight (cap height without descenders is about
    // 0.7 em), so they only serve as a fallback when the content stream adds
    // its own scaling.
    let [a, b, c, d, _, _] = edit.object.matrix;
    let from_matrix = edit.object.size * (a * d - b * c).abs().sqrt() * scale;
    let height = screen.height().max(4.0);
    let font_px = if from_matrix > height * 0.6 && from_matrix < height * 2.4 {
        from_matrix
    } else {
        height * 1.2
    }
    .clamp(6.0, 240.0);
    let font = FontId::proportional(font_px);
    let [r, g, bl, alpha] = edit.object.color;
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

    let id = Id::new((
        "edit-box",
        edit.page,
        edit.object.index,
        edit.object.revision,
    ));
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
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        return Some(enter);
    }
    None
}

fn to_screen(raster: Rect, page: Rect, scale: f32) -> Rect {
    Rect::from_min_max(
        page.min + raster.min.to_vec2() * scale,
        page.min + raster.max.to_vec2() * scale,
    )
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
