//! Editor window: toolbar, page canvas, in-place edit box, banners, modals
//! and the settings page.

#[path = "catalog.rs"]
mod catalog;
#[path = "qol.rs"]
mod qol;
#[path = "qol_sessions.rs"]
mod qol_sessions;
#[path = "qol_gestures.rs"]
mod qol_gestures;
#[path = "tiles.rs"]
mod tiles;
#[path = "affine_edit.rs"]
mod affine_edit;
#[path = "drag_preview.rs"]
mod drag_preview;
#[path = "font_readout.rs"]
mod font_readout;
use crate::icons::Icon;
use crate::recovery::{self, AutoSave, Orphan, Session};
use crate::theme::{self, Palette};
use crate::widgets::{self, Btn, Look};
use crate::worker::{ChangeKind, PageTransform, Reply, Request, Worker};
use eframe::egui::{
    self, Align, Align2, Color32, Context, CornerRadius, CursorIcon, FontId, Frame, Id,
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
const SHORTCUTS: [(&str, &[&str], &str); 11] = [
    ("Search all tools", &["Ctrl", "K"], "Arrow keys choose; Enter opens."),
    ("Go to page", &["Ctrl", "G"], ""),
    ("Open a PDF", &["Ctrl", "O"], ""),
    ("Save working copy", &["Ctrl", "S"], "Ctrl + Shift + S creates another copy."),
    ("Undo", &["Ctrl", "Z"], ""),
    ("Redo", &["Ctrl", "Y"], "Ctrl + Shift + Z also works."),
    ("Find text", &["Ctrl", "F"], ""),
    ("Zoom in", &["Ctrl", "+"], ""),
    ("Zoom out", &["Ctrl", "\u{2212}"], ""),
    ("Actual size", &["Ctrl", "0"], "Resets the zoom to 100%."),
    ("Zoom at the pointer", &["Ctrl", "Wheel"], ""),
];

/// Width of the document menu under the Open button.
const MENU_WIDTH: f32 = 280.0;

/// Tool windows and actions in the document menu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    ImageCreate,
    ScanCleanup,
    Replace,
    Forms,
    Signatures,
    Pictures,
    Markup,
    Drawing,
    Privacy,
    Redact,
    Ocr,
    Bookmarks,
    Organize,
    Crop,
    Stamps,
    Export,
    Tables,
    Word,
    Compress,
    Sanitize,
    Compare,
    Print,
}

type MenuSection = (&'static str, &'static [(Tool, &'static str, Icon)]);

impl Tool {
    const MENU: [MenuSection; 2] = [
        (
            "Edit",
            &[
                (Tool::Replace, "Find and replace…", Icon::Replace),
                (Tool::Forms, "Forms: fill, design, flatten...", Icon::Form),
                (Tool::Signatures, "Signatures…", Icon::Signature),
                (Tool::Pictures, "Images…", Icon::Image),
                (Tool::ImageCreate, "Images to PDF…", Icon::Image),
                (Tool::ScanCleanup, "Scan cleanup…", Icon::Scan),
                (Tool::Markup, "Markup and notes…", Icon::Highlighter),
                (Tool::Drawing, "Draw and shapes…", Icon::Pen),
                (Tool::Redact, "Redact…", Icon::Redact),
                (Tool::Privacy, "Search and redact…", Icon::Redact),
                (Tool::Ocr, "OCR current page", Icon::Scan),
            ],
        ),
        (
            "Document",
            &[
                (Tool::Bookmarks, "Bookmarks sidebar", Icon::Bookmark),
                (Tool::Organize, "Merge, split and extract…", Icon::Layers),
                (Tool::Crop, "Crop page…", Icon::Crop),
                (Tool::Stamps, "Headers, footers and Bates…", Icon::Hash),
                (Tool::Export, "Export pages as images…", Icon::Export),
                (Tool::Tables, "Tables to Excel or CSV...", Icon::Export),
                (Tool::Word, "Export to Word...", Icon::Export),
                (Tool::Compress, "Compress images…", Icon::Compress),
                (Tool::Sanitize, "Sanitize document…", Icon::Redact),
                (Tool::Compare, "Compare PDFs…", Icon::Layers),
                (Tool::Print, "Print…", Icon::Print),
            ],
        ),
    ];

    fn description(self) -> &'static str {
        match self {
            Self::ImageCreate => "Create a PDF from JPG, PNG or HEIC photos; reorder images and choose page size.",
            Self::ScanCleanup => "Straighten scanned photos, correct perspective, choose grayscale or black and white.",
            Self::Replace => "Find text and replace one match or all matches while preserving fonts.",
            Self::Forms => "Fill, design, move and resize fields; checkboxes, radio groups and dropdowns; flatten forms.",
            Self::Signatures => "Draw or import a reusable signature, then place and resize it.",
            Self::Pictures => "Insert PNG or JPG images; move, resize and remove pictures.",
            Self::Markup => "Highlight, underline, strike through and add editable sticky notes.",
            Self::Drawing => "Freehand ink, rectangles, ellipses, lines and arrows.",
            Self::Privacy => "Find emails, phone numbers, IBANs, identity codes or card patterns; preview and redact.",
            Self::Redact => "Permanently remove text, images and vector content from a selected rectangle.",
            Self::Ocr => "Recognize scanned text using installed Windows OCR languages.",
            Self::Bookmarks => "Navigate, add, rename, delete and reorder document bookmarks.",
            Self::Organize => "Merge PDFs, split by page ranges or extract selected pages to a new file.",
            Self::Crop => "Change the visible page area using crop bounds.",
            Self::Stamps => "Page numbers, watermarks, headers, footers, date tokens and Bates numbering.",
            Self::Export => "Export pages as PNG or JPG with a chosen DPI.",
            Self::Tables => "Select a table, preview rows and columns, export Excel XLSX or CSV.",
            Self::Word => "Export paragraphs, fonts, images and simple tables to Word DOCX.",
            Self::Compress => "Downscale and recompress images; compare file sizes.",
            Self::Sanitize => "Remove metadata, hidden text, scripts, attachments and optional comments or links.",
            Self::Compare => "Compare current edits with the original, or compare two saved PDFs.",
            Self::Print => "Print through the Windows print dialog.",
        }
    }

    fn matches(self, label: &str, query: &str) -> bool {
        let haystack = format!("{label} {} {}", self.description(), self.aliases()).to_lowercase();
        query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
    }

    /// Stable button key, so tests can find the menu entries.
    fn key(self) -> &'static str {
        match self {
            Tool::ImageCreate => "tool-image-create",
            Tool::ScanCleanup => "tool-scan-cleanup",
            Tool::Replace => "tool-replace",
            Tool::Forms => "tool-forms",
            Tool::Signatures => "tool-signatures",
            Tool::Pictures => "tool-pictures",
            Tool::Markup => "tool-markup",
            Tool::Drawing => "tool-drawing",
            Tool::Privacy => "tool-privacy",
            Tool::Redact => "tool-redact",
            Tool::Ocr => "tool-ocr",
            Tool::Bookmarks => "tool-bookmarks",
            Tool::Organize => "tool-organize",
            Tool::Crop => "tool-crop",
            Tool::Stamps => "tool-stamps",
            Tool::Export => "tool-export",
            Tool::Tables => "tool-tables",
            Tool::Word => "tool-word",
            Tool::Compress => "tool-compress",
            Tool::Sanitize => "tool-sanitize",
            Tool::Compare => "tool-compare",
            Tool::Print => "print-document",
        }
    }
}

/// Each tab owns a complete editing session, including its worker and recovery lock.
pub struct TabbedApp {
    sessions: qol_sessions::Sessions,
    native_guard: Option<Box<dyn FnMut()>>,
    clipboard: crate::worker::SharedClipboard,
    tabs: Vec<(u64, EditorApp)>,
    active: usize,
    next_id: u64,
    closing: Option<Option<u64>>,
    allow_exit: bool,
    instance: Option<crate::instance::Instance>,
    merge_tab: Option<u64>,
}
impl TabbedApp {
    pub fn with_native_guard(mut self, guard: Option<Box<dyn FnMut()>>) -> Self {
        self.native_guard = guard;
        self
    }
    pub fn new(ctx: &Context, initial: pdf_editor_core::Result<Option<PathBuf>>) -> Self {
        let clipboard = crate::worker::SharedClipboard::default();
        let restore=matches!(&initial,Ok(None));
        let first=EditorApp::new(ctx,initial,clipboard.clone());
        let sessions=qol_sessions::Sessions::new(first.config_path.as_deref());
        let mut app=Self {
            sessions,
            native_guard: None,
            tabs: vec![(0, first)],
            clipboard,
            active: 0,
            next_id: 1,
            closing: None,
            allow_exit: false,
            instance: None,
            merge_tab: None,
        };
        app.sessions_start(ctx,restore);
        app
    }
    pub fn with_instance(
        mut self,
        instance: Option<crate::instance::Instance>,
        action: String,
    ) -> Self {
        self.instance = instance;
        match action.as_str() {
            "merge" => {
                self.merge_tab = Some(0);
                self.tabs[0].1.organize.collect_shell_selection();
            }
            "compress" => self.tabs[0].1.compression.open = true,
            "export" => self.tabs[0].1.export.open = true,
            _ => {}
        }
        self
    }
    fn receive(&mut self, ctx: &Context, request: crate::instance::Incoming) {
        if request.action == "merge"
            && let Some(index) = self.merge_tab.and_then(|id| {
                self.tabs
                    .iter()
                    .position(|(key, tab)| *key == id && tab.organize.collecting_shell_selection())
            })
        {
            self.active = index;
            if !self.tabs[index].1.organize.add_path(request.path) {
                self.tabs[index].1.banner(
                    BannerKind::Error,
                    "Merge selection exceeds 100 additional PDFs. Review the retained list.",
                );
            }
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            return;
        }
        if self.tabs.len() >= 32 {
            self.tabs[self.active].1.banner(
                BannerKind::Error,
                "Close a tab before opening more documents (32-tab limit).",
            );
            return;
        }
        self.add(ctx, Some(request.path));
        let (id, tab) = &mut self.tabs[self.active];
        match request.action.as_str() {
            "merge" => {
                self.merge_tab = Some(*id);
                tab.organize.collect_shell_selection();
            }
            "compress" => tab.compression.open = true,
            "export" => tab.export.open = true,
            _ => {}
        }
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }
    fn add(&mut self, ctx: &Context, path: Option<PathBuf>) {
        if self.tabs.len() >= 32 {
            self.tabs[self.active].1.banner(
                BannerKind::Error,
                "Close a tab before opening more documents (32-tab limit).",
            );
            return;
        }
        let mut tab = EditorApp::new(ctx, Ok(path), self.clipboard.clone());
        tab.orphans.clear();
        self.tabs.push((self.next_id, tab));
        self.next_id += 1;
        self.active = self.tabs.len() - 1;
    }
    fn needs_warning(tab: &EditorApp) -> bool {
        tab.busy > 0
            || tab.doc.as_ref().is_some_and(|d| {
                d.dirty() || d.edit.as_ref().is_some_and(|e| e.text != e.line.text)
            })
    }
    fn remove(&mut self, id: u64, ctx: &Context) {
        self.sessions_remember_closed(id);
        if let Some(index) = self.tabs.iter().position(|(key, _)| *key == id) {
            let (_, mut tab) = self.tabs.remove(index);
            tab.allow_close = true;
            eframe::App::on_exit(&mut tab, None);
            if index < self.active {
                self.active -= 1;
            }
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
        if self.tabs.is_empty() {
            self.add(ctx, None);
        }
    }
}
impl eframe::App for TabbedApp {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if let Some(guard) = &mut self.native_guard { guard(); }
        let ctx = ui.ctx().clone();
        self.sessions_shortcuts(&ctx);
        if let Some(instance) = &self.instance {
            let requests = instance.poll();
            for request in requests {
                self.receive(&ctx, request);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let p = theme::palette(ui);
        if self.closing.is_some() && ctx.input_mut(|i|i.consume_key(egui::Modifiers::NONE,Key::Escape)) {
            self.closing = None;
        }
        for (index, (_, tab)) in self.tabs.iter_mut().enumerate() {
            if index != self.active {
                tab.poll(&ctx);
                tab.autosave_tick(&ctx);
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_exit
            && self.tabs.iter().any(|(_, t)| Self::needs_warning(t))
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.closing = Some(None);
        }
        let mut select = None;
        let mut close = None;
        let mut new = false;
        egui::Panel::top("document-tabs")
            .frame(Frame::new().fill(p.canvas).inner_margin(Margin {
                left: 8,
                right: 8,
                top: 6,
                bottom: 0,
            }))
            .show_separator_line(false)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for (index, (id, tab)) in self.tabs.iter().enumerate() {
                                ui.push_id(id, |ui| {
                                    let name = tab
                                        .doc
                                        .as_ref()
                                        .map_or("New document", |d| d.name.as_str());
                                    let (picked, closed) = document_tab(
                                        ui,
                                        &p,
                                        name,
                                        self.active == index,
                                        Self::needs_warning(tab),
                                    );
                                    if picked {
                                        select = Some(index);
                                    }
                                    if closed {
                                        close = Some(*id);
                                    }
                                });
                            }
                            if Btn::icon(Icon::Plus)
                                .id("new-tab")
                                .height(30.0)
                                .tip("New tab")
                                .show(ui, &p)
                                .clicked()
                            {
                                new = true;
                            }
                        });
                    });
            });
        if let Some(index) = select {
            self.tabs[self.active].1.commit_edit();
            self.active = index;
            self.tabs[index].1.title.clear();
        }
        if new {
            self.add(&ctx, None);
        }
        if let Some(id) = close {
            if self
                .tabs
                .iter()
                .find(|(key, _)| *key == id)
                .is_some_and(|(_, t)| Self::needs_warning(t))
            {
                self.closing = Some(Some(id));
            } else {
                self.remove(id, &ctx);
            }
        }
        let (id, tab) = &mut self.tabs[self.active];
        ui.push_id(*id, |ui| eframe::App::ui(tab, ui, frame));
        if let Some(path) = self.tabs.iter_mut().find_map(|(_, tab)| tab.new_tab_path.take()) {
            self.add(&ctx, Some(path));
        }
        if let Some(target) = self.closing {
            let busy = self
                .tabs
                .iter()
                .any(|(id, t)| target.is_none_or(|key| key == *id) && t.busy > 0);
            let choice = close_tabs_prompt(&ctx,&p,target.is_some(),busy);
            if let Some(discard) = choice {
                self.closing = None;
                if discard {
                    if let Some(id) = target {
                        self.remove(id, &ctx);
                    } else {
                        self.allow_exit = true;
                        for (_, t) in &mut self.tabs {
                            t.allow_close = true;
                        }
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                }
            }
        }
            self.sessions_tick(&ctx);
    }
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.sessions_flush();
        for (_, tab) in &mut self.tabs {
            eframe::App::on_exit(tab, gl);
        }
    }
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.tabs[self.active].1.clear_color(visuals)
    }
}

pub struct EditorApp {
    qol: qol::Qol,
    surface: crate::window_state::Surface,
    tabbed: bool,
    new_tab_path: Option<PathBuf>,
    tools_open: bool,
    tools_query: String,
    tools_focus: bool,
    tools_category: String,
    tools_selected: usize,
    snap_enabled: bool,
    drag_preview: drag_preview::Preview,
    page_jump: crate::navigation::PageJump,
    picker: crate::dialogs::Picker,
    insert_dialog: crate::dialogs::ToolDialog<Request>,
    picker_save: bool,
    pending_action: Option<Request>,
    pending_comparison: Option<u64>,
    compare_origin: Option<PathBuf>,
    recent_files: Vec<PathBuf>,
    dragging_page: Option<usize>,
    text_tool: TextTool,
    image_create: crate::image_create::ImageCreate,
    scan_cleanup: crate::scan_cleanup::ScanCleanup,
    signatures: crate::signatures::Signatures,
    forms: crate::forms::Forms,
    replace: crate::replace::Replace,
    password: crate::passwords::PasswordDialog,
    organize: crate::organize::Organize,
    markup: crate::markup::MarkupTool,
    pictures: crate::pictures::Pictures,
    privacy: crate::privacy::Privacy,
    redaction: crate::redaction::Redaction,
    compression: crate::compression::Compression,
    sanitize: crate::sanitize::Sanitize,
    compare: crate::compare::Compare,
    stamps: crate::stamps::Stamps,
    export: crate::export::Export,
    tables: crate::table_export::Tables,
    word: crate::word_export::WordExport,
    crop: crate::crop::Crop,
    bookmarks: crate::bookmarks::Bookmarks,
    drawing: crate::drawing::DrawingTool,
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
    integration: crate::windows_integration::Integration,
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
    refresh_only: bool,
    open: bool,
    focus: bool,
    query: String,
    matches: Vec<(u16, Vec<[f32; 4]>)>,
    active: usize,
}

struct DocView {
    panning: bool,
    object_mode: bool,
    selected_item: Option<pdf_editor_core::PageItem>,
    dragging_item: Option<ItemDrag>,
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
    pending_edit: Option<EditState>,
    scroll_to: Option<Vec2>,
    offset: Vec2,
    viewport: Rect,
    current_page: usize,
    explicit_page: Option<usize>,
}

struct PageView {
    tiles: tiles::PageTiles,
    thumbnail_generation: Option<u64>,
    thumbnail: Option<TextureHandle>,
    pending_thumbnail: bool,
    info: PageInfo,
    texture: Option<PageTexture>,
    pending_render: Option<(f32, u64)>,
    objects: Option<PageObjects>,
    pending_objects: Option<u64>,
    error: Option<String>,
}

/// Keep cached GPU page rasters below 128 MiB per document. The incoming page
/// is included in the budget; evict distant pages before allocating its slot.
fn trim_page_cache(pages: &mut [PageView], incoming: usize, bytes: usize) {
    trim_page_cache_to(pages, incoming, bytes, 128 * 1024 * 1024);
}

fn trim_page_cache_to(pages: &mut [PageView], incoming: usize, bytes: usize, limit: usize) {
    let mut total = bytes;
    let mut cached = Vec::new();
    for (index, page) in pages.iter_mut().enumerate() {
        if index == incoming {
            page.texture = None;
            total=total.saturating_add(page.tiles.bytes());
        } else if let Some(texture) = &page.texture {
            let size = texture.handle.size();
            let cost = size[0].saturating_mul(size[1]).saturating_mul(4).saturating_add(page.tiles.bytes());
            total = total.saturating_add(cost);
            cached.push((index, cost));
        } else if page.tiles.bytes()>0 {
            let cost=page.tiles.bytes();total=total.saturating_add(cost);cached.push((index,cost));
        }
    }
    cached.sort_unstable_by_key(|(index, _)| std::cmp::Reverse(index.abs_diff(incoming)));
    for (index, cost) in cached {
        if total <= limit {
            break;
        }
        pages[index].texture = None;
        pages[index].tiles.discard_cache();
        total = total.saturating_sub(cost);
    }
}

fn reserve_tiles(pages:&mut [PageView],incoming:usize,extra:usize) {
    let cost=|page:&PageView|page.texture.as_ref().map_or(0,|texture|texture.handle.byte_size())+page.tiles.bytes();
    let mut total=pages.iter().map(cost).sum::<usize>().saturating_add(extra);
    let mut candidates=(0..pages.len()).filter(|&index|index!=incoming).collect::<Vec<_>>();
    candidates.sort_by_key(|&index|std::cmp::Reverse(index.abs_diff(incoming)));
    for index in candidates {
        if total<=128*1024*1024 {break;}
        total=total.saturating_sub(cost(&pages[index]));pages[index].texture=None;pages[index].tiles.discard_cache();
    }
    if total>128*1024*1024 {pages[incoming].tiles.discard_cache();}
}

struct PageTexture {
    handle: TextureHandle,
    scale: f32,
    generation: u64,
}

struct PageObjects {
    items: Vec<pdf_editor_core::PageItem>,
    transform: PageTransform,
    generation: u64,
    /// Editable lines in paint order.
    lines: Vec<Line>,
}

fn selection_items(objects: &PageObjects, item: &pdf_editor_core::PageItem) -> Vec<pdf_editor_core::PageItem> {
    if item.kind == "Text"
        && let Some(line)=objects.lines.iter().find(|line|line.objects.iter().any(|o|o.index==item.index)) {
        return objects.items.iter().filter(|candidate|line.objects.iter().any(|o|o.index==candidate.index)).cloned().collect();
    }
    vec![item.clone()]
}
fn selection_item(objects: &PageObjects, item: &pdf_editor_core::PageItem) -> pdf_editor_core::PageItem {
    let mut selected=item.clone();
    for member in selection_items(objects,item) {
        selected.bounds.left=selected.bounds.left.min(member.bounds.left);
        selected.bounds.right=selected.bounds.right.max(member.bounds.right);
        selected.bounds.bottom=selected.bounds.bottom.min(member.bounds.bottom);
        selected.bounds.top=selected.bounds.top.max(member.bounds.top);
    }
    selected
}

struct ItemDrag {
    page: usize,
    items: Vec<pdf_editor_core::PageItem>,
    bounds: Rect,
    delta: Vec2,
    anchor: Pos2,
    resize_handle: Option<usize>,
    preview: Option<Rect>,
    snap_state: crate::snapping::State,
    snap_targets: Vec<Rect>,
}

/// Adjacent text objects that read as one line, edited together. Browser
/// print output often places every glyph in its own object.
#[derive(Clone)]
struct Line {
    paragraph: Option<pdf_editor_core::Paragraph>,
    fallback: Option<String>,
    objects: Vec<TextObject>,
    /// Union of the objects' extents in unscaled raster points.
    rect: Rect,
    text: String,
}

#[derive(Clone)]
struct EditState {
    page: usize,
    line: Line,
    text: String,
    focus: bool,
    rejected_text: Option<String>,
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
    fn restore_failed_edit(&mut self) {
        self.applying = None;
        self.reopen = None;
        if let Some(mut edit) = self.pending_edit.take() {
            edit.rejected_text = Some(edit.text.clone());
            edit.focus = true;
            self.edit = Some(edit);
        }
    }
    fn dirty(&self) -> bool {
        self.changes.is_dirty()
    }
    fn scale(&self) -> f32 {
        self.zoom * PT_TO_UI
    }
}

impl EditorApp {
    pub fn new(
        ctx: &Context,
        initial: pdf_editor_core::Result<Option<PathBuf>>,
        clipboard: crate::worker::SharedClipboard,
    ) -> Self {
        theme::install(ctx);
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
        let worker = Worker::spawn(ctx.clone(), library.unwrap_or_default(), policy, clipboard);
        let mut app = Self {
            qol: Default::default(),
            surface: Default::default(),
            tabbed: true,
            new_tab_path: None,
            tools_open: true,
            tools_query: String::new(),
            tools_focus: false,
            tools_category: String::new(),
            tools_selected: 0,
            snap_enabled: true,
            drag_preview: Default::default(),
            page_jump: crate::navigation::PageJump::default(),
            picker: crate::dialogs::Picker::default(),
            insert_dialog: Default::default(),
            picker_save: false,
            pending_action: None,
            pending_comparison: None,
            compare_origin: None,
            recent_files: config_path
                .as_deref()
                .and_then(|p| pdf_editor_core::Settings::load(p).ok())
                .map_or_else(Vec::new, |s| s.recent_files),
            dragging_page: None,
            text_tool: TextTool::default(),
            image_create: Default::default(),
            scan_cleanup: Default::default(),
            signatures: Default::default(),
            forms: Default::default(),
            replace: Default::default(),
            password: Default::default(),
            organize: Default::default(),
            markup: Default::default(),
            pictures: Default::default(),
            privacy: Default::default(),
            redaction: Default::default(),
            compression: Default::default(),
            sanitize: Default::default(),
            compare: Default::default(),
            stamps: Default::default(),
            export: Default::default(),
            tables: Default::default(),
            word: Default::default(),
            crop: Default::default(),
            drawing: Default::default(),
            bookmarks: Default::default(),
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
            integration: Default::default(),
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
        if let Request::EditItems(items,pdf_editor_core::ObjectAction::Move {..}|pdf_editor_core::ObjectAction::Resize {..})|Request::LayoutItems(items,_) = &request {
            self.qol.restore_objects=items.first().map(|first|(first.page,items.iter().map(|i|i.index).collect()));
        }

        if matches!(&request,Request::ReplacementCount(_)|Request::ReplacementCountOptions(..)) {self.qol.replace_context=self.doc.as_ref().map(|d|(d.generation,self.current_search_options()));}
        let request = if self.qol.advanced() {
            let options = self.current_search_options();
            match request {
                Request::Find(query) => Request::FindOptions(query, options),
                Request::ReplacementCount(query) => Request::ReplacementCountOptions(query, options),
                Request::Replace {query,text,occurrence} => Request::ReplaceOptions {query,text,occurrence,options},
                other => other,
            }
        } else {request};
        if !self.worker.send(request) {
            self.engine_stopped();
        }
    }

    fn engine_stopped(&mut self) {
        self.drag_preview = Default::default();
        self.pending_action = None;
        self.pending_comparison=None;
        self.busy = 0;
        self.opening = false;
        if let Some(doc) = &mut self.doc { doc.restore_failed_edit(); }
        if self.engine_alive {
            self.engine_alive = false;
            let draft = self.doc.as_ref().is_some_and(|doc|doc.edit.as_ref().is_some_and(|edit|edit.text != edit.line.text));
            self.banner(
                BannerKind::Error,
                if draft {"The PDF engine stopped. Copy the draft before restarting."}
                else {"The PDF engine stopped. Restart the editor to continue."},
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
        if self.tabbed {
            self.commit_edit();
            self.open_dialog_or_path(path);
            return;
        }
        let committed = self.commit_edit();
        if committed || self.busy > 0 || self.doc.as_ref().is_some_and(|d| d.dirty() || d.edit.as_ref().is_some_and(|e|e.text != e.line.text)) {
            self.confirm = Some(Pending::Open(path));
        } else {
            self.open_dialog_or_path(path);
        }
    }

    fn open_dialog_or_path(&mut self, path: Option<PathBuf>) {
        if self.picker.pending() { return; }
        if let Some(path) = path { self.open_path(path); return; }
        self.picker_save = false;
        let directory=self.doc.as_ref().map(|doc|&doc.source).or_else(||self.recent_files.first()).and_then(|path|path.parent()).map(std::path::Path::to_path_buf);
        if let Err(error) = self.picker.start(move || {
            let mut dialog=rfd::FileDialog::new().set_title("Open PDF").add_filter("PDF document", &["pdf"]);
            if let Some(directory)=directory.and_then(|path|path.canonicalize().ok()) {dialog=dialog.set_directory(directory);}
            dialog.pick_file()
        }) {
            self.banner(BannerKind::Error, format!("Cannot open file picker: {error}"));
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        if self.picker.pending() { return; }
        if self.tabbed && self.doc.is_some() {
            self.new_tab_path = Some(path);
            return;
        }
        if self.setup_error.is_some() || self.busy > 0 {
            return;
        }
        self.opening = true;
        self.busy += 1;
        let request=self.restoring.as_ref().filter(|orphan|orphan.copy==path).map(|orphan|orphan.source.clone().unwrap_or_else(||path.clone()))
            .map_or_else(||Request::Open(path.clone()),|source|Request::OpenRecovery {path:path.clone(),source});
        self.send(request);
    }

    fn save_as(&mut self) {
        // The worker handles requests in order, so a committed edit is
        // applied before the save.
        let committed = self.commit_edit();
        let Some(doc) = &self.doc else { return };
        if doc.edit.as_ref().is_some_and(|e|e.text != e.line.text) {
            self.banner(BannerKind::Warning,"Correct the rejected text edit or press Escape to discard it before saving.");
            return;
        }
        if self.busy > u32::from(committed) {
            return;
        }
        if self.picker.pending() { return; }
        let suggested = if self.recovery_destination(&doc.suggested) {
            PathBuf::from("Recovered_edited.pdf")
        } else {doc.suggested.clone()};
        self.picker_save = true;
        if let Err(error) = self.picker.start(move || {
            let mut dialog = rfd::FileDialog::new().set_title("Save As")
                .add_filter("PDF document", &["pdf"]);
            if let Some(dir) = suggested.parent().and_then(|dir|dir.canonicalize().ok()) { dialog = dialog.set_directory(dir); }
            if let Some(name) = suggested.file_name() { dialog = dialog.set_file_name(name.to_string_lossy()); }
            dialog.save_file()
        }) {
            self.banner(BannerKind::Error, format!("Cannot open file picker: {error}"));
        }
    }

    /// Run document actions only after preceding edits have succeeded.
    /// A rejected edit cancels the intent instead of using an older version.
    fn document_action(&mut self, request: Request) {
        if !self.engine_alive { return; }
        let reserved = match &request {
            Request::SaveAs(path)|Request::SavePassword(path,_)|Request::ExportSanitized(_,path)
                |Request::Extract(_,path)|Request::ExportWord(path)|Request::ExportTable(_,_,path)
                |Request::ImagesToPdf(_,_,path) => self.recovery_destination(path),
            Request::SaveWorkingCopy => self.qol.working_copy.as_ref().is_some_and(|path|self.recovery_destination(path)),
            Request::Split(_,folder)|Request::ExportImages(_,_,_,folder) => self.recovery_destination(&folder.join("output")),
            _ => false,
        };
        if reserved {
            self.banner(BannerKind::Warning,"Choose a folder outside recovery storage.");
            return;
        }
        if !matches!(request, Request::OpenPassword(..)|Request::ImagesToPdf(..)) {
            if self.doc.is_none() || self.opening || self.pending_action.is_some() {
                return;
            }
            self.commit_edit();
            if self.doc.as_ref().and_then(|d|d.edit.as_ref()).is_some_and(|e|e.text != e.line.text) {
                self.banner(BannerKind::Warning,"Apply the text edit or press Escape to discard it first.");
                return;
            }
            if self.busy > 0 {
                self.pending_action = Some(request);
                return;
            }
        }
        self.opening = matches!(request, Request::OpenPassword(..));
        self.busy += 1;
        self.send(request);
    }

    fn open_comparison(&mut self) {
        if self.compare.is_current() {self.refresh_comparison();return;}
        self.commit_edit();
        if self.doc.as_ref().and_then(|d|d.edit.as_ref()).is_some_and(|e|e.text!=e.line.text) {
            self.banner(BannerKind::Warning,"Apply or discard the text edit before comparing.");return;
        }
        let Some(doc)=&self.doc else {self.compare.open_for(None);return;};
        let suggestion=if self.compare_origin.is_none() {crate::comparison_origin::suggest(&doc.source)}else{None};
        let id=self.compare.open_current(doc.source.clone(),self.compare_origin.clone(),suggestion);
        self.pending_comparison=Some(id);
    }

    fn refresh_comparison(&mut self) {
        if !self.compare.is_current() {self.open_comparison();return;}
        self.commit_edit();
        if self.doc.as_ref().and_then(|d|d.edit.as_ref()).is_some_and(|e|e.text!=e.line.text) {
            self.banner(BannerKind::Warning,"Apply or discard the text edit before comparing.");return;
        }
        self.pending_comparison=Some(self.compare.prepare_current());
    }

    fn recovery_destination(&self, path: &std::path::Path) -> bool {
        self.session.as_ref().is_some_and(|s|s.contains_output(path))
            || self.restoring.as_ref().is_some_and(|o|o.contains_output(path))
            || self.doc.as_ref().and_then(|d|d.restored_from.as_ref()).is_some_and(|o|o.contains_output(path))
            || self.orphans.iter().any(|o|o.contains_output(path))
            || recovery::root().is_some_and(|root|recovery::contains_output(&root,path))
    }

    fn refresh_tool_lists(&mut self) {
        for (open, request) in [
            (self.markup.open, Request::Annotations),
            (self.pictures.open, Request::Pictures),
        ] {
            if open {
                self.busy += 1;
                self.send(request);
            }
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
        self.text_tool.selection.clear();
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
            self.text_tool.selection.clear();
            return false;
        }
        // A focus change must not repeatedly submit a draft already rejected.
        if edit.rejected_text.as_ref() == Some(&edit.text) {
            doc.edit = Some(edit);
            return false;
        }
        doc.applying = Some((edit.page, edit.line.rect, edit.line.objects.len()));
        doc.pending_edit = Some(edit.clone());
        self.busy += 1;
        self.send(match edit.line.paragraph {
            Some(paragraph) => Request::EditParagraph(paragraph, edit.text),
            None => Request::Edit {
                objects: edit.line.objects,
                text: edit.text,
            },
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
        let content = doc.scroll_to.unwrap_or(doc.offset) + in_view;
        let ratio = zoom / doc.zoom;
        let scale = doc.scale();
        let widest = doc.pages.iter().map(|page| page.info.width).fold(0.0, f32::max);
        let old_width = (widest * scale + 2.0 * CANVAS_PAD).max(doc.viewport.width());
        let new_width = (widest * scale * ratio + 2.0 * CANVAS_PAD).max(doc.viewport.width());
        // Padding and page gaps stay in UI points; only page content scales.
        let mut fixed_y = content.y.min(CANVAS_PAD);
        let mut top = CANVAS_PAD;
        let mut page_width = 0.0;
        for (index, page) in doc.pages.iter().enumerate() {
            page_width = page.info.width * scale;
            let bottom = top + page.info.height * scale;
            if content.y <= bottom {
                break;
            }
            let last = index + 1 == doc.pages.len();
            fixed_y += if last {
                content.y - bottom
            } else {
                (content.y - bottom).min(PAGE_GAP)
            };
            if last || content.y <= bottom + PAGE_GAP {
                break;
            }
            top = bottom + PAGE_GAP;
        }
        let old_left = (old_width - page_width) / 2.0;
        let new_left = (new_width - page_width * ratio) / 2.0;
        let scaled = vec2(
            new_left + (content.x - old_left) * ratio,
            fixed_y + (content.y - fixed_y) * ratio,
        );
        doc.scroll_to = Some((scaled - in_view).max(Vec2::ZERO));
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
        if let Some(result) = self.picker.poll() {
            match result {
                Ok(Some(mut path)) if self.picker_save => {
                    if path.extension().is_none() { path.set_extension("pdf"); }
                    self.password.save(path);
                }
                Ok(Some(path)) => self.open_path(path),
                Ok(None) => {},
                Err(error) => self.banner(BannerKind::Error, error),
            }
        }
        if self.picker.pending() { ctx.request_repaint_after(std::time::Duration::from_millis(50)); }

        let Some(replies) = self.worker.poll() else {
            self.engine_stopped();
            return;
        };
        for reply in replies {
            self.handle(ctx, reply);
        }
        if self.busy == 0 && let Some(request) = self.pending_action.take() {
            self.document_action(request);
        }
        if self.busy==0 && let Some(id)=self.pending_comparison.take() && self.compare.accepts_current(id) {
            self.busy+=1;self.send(Request::ComparisonSnapshot(id));
        }

    }

    fn handle(&mut self, ctx: &Context, reply: Reply) {
        match reply {
            Reply::ObjectPreview {id,generation,result} => self.drag_preview.receive(ctx,id,generation,result),
            Reply::Tiles {generation,page,scale,images}=>{
                if self.doc.as_ref().is_none_or(|doc|doc.generation!=generation) {return;}
                if self.doc.as_ref().and_then(|doc|doc.pages.get(usize::from(page))).is_none_or(|view|!view.tiles.accepts(scale,generation)) {return;}
                if let Ok(images)=&images && let Some(doc)=self.doc.as_mut() && usize::from(page)<doc.pages.len() {
                    reserve_tiles(&mut doc.pages,usize::from(page),images.iter().map(|(_,image)|image.rgba.len()).sum());
                }
                if let Some(view)=self.page_mut(page) && let Some(error)=view.tiles.receive(ctx,scale,generation,images) {
                    self.banner(BannerKind::Warning,format!("Sharp preview unavailable: {error}"));
                }
            }
            Reply::SanitizedExported(path)=>{
                self.busy=self.busy.saturating_sub(1);
                self.banner(BannerKind::Success,format!("Saved and verified: {}",path.file_name().unwrap_or_default().to_string_lossy()));
            }
            Reply::ComparisonSnapshot {id,generation,original,current}=>{
                self.busy=self.busy.saturating_sub(1);
                if !self.compare.accepts_current(id) {return;}
                if self.doc.as_ref().is_some_and(|d|d.generation==generation) {
                    self.compare.receive_current(ctx,id,[original,current]);
                } else {self.pending_comparison=Some(self.compare.prepare_current());}
            }

            Reply::TableDetected(grid) => {
                self.busy = self.busy.saturating_sub(1);
                self.tables.grid = Some(grid);
                self.tables.open = true;
            }
            Reply::ScanPreview {
                generation,
                page,
                picture,
                corrected,
            } => {
                self.busy = self.busy.saturating_sub(1);
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.generation == generation)
                {
                    self.scan_cleanup.receive(ctx, page, picture, corrected);
                }
            }
            Reply::ItemsCopied => {
                self.busy = self.busy.saturating_sub(1);
            }
            Reply::Bookmarks(items) => {
                self.busy = self.busy.saturating_sub(1);
                self.bookmarks.receive(items);
            }
            Reply::Pictures(items) => {
                self.busy = self.busy.saturating_sub(1);
                self.pictures.selected = self.pictures.selected
                    .and_then(|i| self.pictures.items.get(i))
                    .and_then(|old| items.iter().position(|new| {
                        new.name == old.name && new.page == old.page
                            && new.placement.left == old.placement.left
                            && new.placement.bottom == old.placement.bottom
                            && new.placement.width == old.placement.width
                            && new.placement.height == old.placement.height
                    }));
                self.pictures.items = items;
            }
            Reply::Annotations(items) => {
                self.busy = self.busy.saturating_sub(1);
                self.markup.selected = self.markup.selected
                    .and_then(|i| self.markup.items.get(i))
                    .and_then(|old| items.iter().position(|new| {
                        new.index == old.index && new.markup.page == old.markup.page
                            && new.markup.kind == old.markup.kind
                            && new.markup.bounds == old.markup.bounds
                            && new.markup.color == old.markup.color
                            && new.markup.opacity == old.markup.opacity
                            && new.markup.text == old.markup.text
                    }));
                self.markup.items = items;
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
            Reply::PrivacyPreview(preview, boxes) => {
                self.busy = self.busy.saturating_sub(1);
                self.find.matches = boxes;
                self.find.active = 0;
                self.privacy.preview = Some(preview);
                self.go_match();
            }
            Reply::ReplacementCount(query, count) => {
                self.busy = self.busy.saturating_sub(1);
                if self.replace.query == query && self.qol.replace_context.as_ref().is_none_or(|context|self.doc.as_ref().is_some_and(|d|*context==(d.generation,self.current_search_options()))) {
                    self.replace.count = Some(count);
                }
            }
            Reply::FormFields(fields, designed) => {
                self.busy = self.busy.saturating_sub(1);
                self.forms.fields = fields;
                self.forms.designer.refresh(designed);
                self.forms.designer.page_sizes = self
                    .doc
                    .as_ref()
                    .map(|doc| {
                        doc.pages
                            .iter()
                            .map(|page| (page.info.width, page.info.height))
                            .collect()
                    })
                    .unwrap_or_default();
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
                    p.thumbnail_generation = Some(generation);
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
                    && self.current_search_options() == Default::default()
                {
                    self.find.matches = matches;
                    if self.find.refresh_only {
                        self.find.active = self
                            .find
                            .active
                            .min(self.find.matches.len().saturating_sub(1));
                        self.find.refresh_only = false;
                    } else {
                        self.find.active = 0;
                        self.go_match();
                    }
                }
            }
            Reply::Opened {
                generation,
                path,
                name,
                pages,
                suggested,
                protected,
                can_undo,
                can_redo,
            } => {
                self.pending_action = None;
                self.drag_preview = Default::default();
                self.sanitize.close();
                self.compare.reset_document();self.pending_comparison=None;
                self.compare_origin=if protected {None}else{self.config_path.as_deref().and_then(|config|crate::comparison_origin::resolve(config,&path).ok().flatten())};
                self.tools_open=false;
                self.qol.document_changed();self.qol.restore_objects=None;
                self.qol.working_copy=None; self.qol.copy_encrypted=false; self.qol.original_protected=protected;
                self.scan_cleanup.invalidate();
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
                self.tables = Default::default();
                self.text_tool.selection.clear();
                self.text_tool.adding = false;
                self.text_tool.at = None;
                self.find.matches.clear();
                self.privacy.preview = None;
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
                        let suggested = orphan.source.as_deref().and_then(edited_name)
                            .unwrap_or_else(||PathBuf::from("Recovered_edited.pdf"));
                        (format!("{} (recovered)", orphan.name()), source, suggested)
                    }
                    None => (name, path, suggested),
                };
                self.doc = Some(DocView {
                    panning: false,
                    object_mode: true,
                    selected_item: None,
                    dragging_item: None,
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
                            tiles: Default::default(),
                            thumbnail_generation: None,
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
                    can_undo,
                    can_redo,
                    changes: if restored.is_some() {
                        pdf_editor_core::ChangeState::recovered()
                    } else {
                        pdf_editor_core::ChangeState::default()
                    },
                    restored_from: restored,
                    edit: None,
                    reopen: None,
                    applying: None,
                    pending_edit: None,
                    scroll_to: Some(Vec2::ZERO),
                    offset: Vec2::ZERO,
                    viewport: Rect::NOTHING,
                    current_page: 0,
                    explicit_page: None,
                });
                self.sessions_apply_view();
                self.markup.items.clear();
                self.markup.selected = None;
                self.pictures.items.clear();
                self.pictures.selected = None;
                self.refresh_tool_lists();
                self.forms.fields.clear();
                self.forms.designer = Default::default();
                if self.forms.open {
                    self.busy += 1;
                    self.send(Request::FormFields);
                }
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
                // A late render from the previous display must not evict a newer
                // request's preview or cause another expensive redraw cycle.
                if view.pending_render.is_some_and(|pending| pending != (scale, generation)) {
                    return;
                }
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
                if let Some(doc) = self.doc.as_mut() {
                    trim_page_cache(&mut doc.pages, usize::from(page), size[0] * size[1] * 4);
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
                self.drag_preview.rendered(usize::from(page), generation);
            }
            Reply::Objects {
                generation,
                page,
                transform,
                objects,
                items,
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
                    let text_items = objects
                        .into_iter()
                        .map(|o| {
                            let rect = raster_rect(&transform, &o);
                            (o, rect)
                        })
                        .collect();
                    view.objects = Some(PageObjects {
                        items,
                        transform,
                        generation,
                        lines: group_lines(text_items),
                    });
                    if self.qol.restore_objects.as_ref().is_some_and(|(p,_)|*p==page) {
                        let (_,indices)=self.qol.restore_objects.take().unwrap();
                        self.qol.objects=self.doc.as_ref().unwrap().pages[usize::from(page)].objects.as_ref().unwrap().items.iter().filter(|item|indices.contains(&item.index)).cloned().collect();
                        let primary=self.qol.primary();
                        self.doc.as_mut().unwrap().selected_item=primary;
                    }
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
                let comparison_changed=doc.generation!=generation;
                let selected = doc.applying.take().map_or(1, |(_, _, n)| n);
                doc.pending_edit = None;
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
                    self.drag_preview.changed(generation);
                    self.qol.document_changed();
                    self.scan_cleanup.invalidate();
                    doc.selected_item = None;
                    doc.dragging_item = None;
                    self.text_tool.selection.clear();
                    doc.generation = generation;
                    self.find.matches.clear();
                    self.privacy.preview = None;
                    self.replace.count = None;
                    doc.edit = None;
                    if !pages.is_empty() {
                        doc.pages = pages
                            .into_iter()
                            .map(|info| PageView {
                            tiles: Default::default(),
                                thumbnail_generation: None,
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
                        // Keep the previous preview until its replacement arrives.
                        page.thumbnail_generation = None;
                        page.pending_thumbnail = false;
                        page.tiles.clear();
                        page.pending_render = None;
                        page.pending_objects = None;
                        page.objects = None;
                        page.error = None;
                    }
                }
                if comparison_changed && !self.find.query.is_empty() {
                    self.find.refresh_only = true;
                    self.send(Request::Find(self.find.query.clone()));
                }
                if comparison_changed && self.compare.is_current() {self.pending_comparison=Some(self.compare.prepare_current());}
                if self.bookmarks.open {
                    self.busy += 1;
                    self.worker.send(Request::Bookmarks);
                }
                if comparison_changed {
                    self.refresh_tool_lists();
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
            Reply::Characters {generation,page,characters} => {
                if self.doc.as_ref().is_some_and(|d|d.generation==generation) {
                    self.qol.pending_characters.remove(&page);
                    match characters { Ok(chars)=>{self.qol.characters.insert(page,chars);},Err(error)=>self.banner(BannerKind::Warning,error) }
                }
            }
            Reply::SearchResults {generation,query,options,hits,boxes} => {
                if self.doc.as_ref().is_some_and(|d|d.generation==generation) && self.find.query==query && self.current_search_options()==options {
                    self.find.matches=boxes; self.qol.hits=hits;
                    if self.find.refresh_only {self.find.active=self.find.active.min(self.find.matches.len().saturating_sub(1)); self.find.refresh_only=false;}
                    else {self.find.active=0;self.go_match();}
                }
            }
            Reply::WorkingCopySaved {path,encrypted} => {
                if !encrypted && !self.qol.original_protected && let (Some(config),Some(doc))=(&self.config_path,&self.doc) {
                    let source=self.compare_origin.as_ref().unwrap_or(&doc.source);
                    if crate::comparison_origin::remember(config,source,&path).is_err() {
                        self.banner(BannerKind::Warning,"Saved. The original-file link could not be remembered.");
                    }
                }

                self.qol.working_copy=Some(path.clone());self.qol.copy_encrypted=encrypted;
                if let Some(doc)=&mut self.doc {
                    doc.protected=self.qol.original_protected||encrypted;
                    doc.name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                }
                self.handle(ctx,Reply::Saved(path));
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
                    let _ = std::fs::remove_file(path.with_extension("history"));
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
                    let _ = std::fs::remove_file(path.with_extension("history"));
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
                    let _ = std::fs::remove_file(path.with_extension("history"));
                        doc.next_copy=Some(ctx.input(|i|i.time)+60.0);
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
                self.drag_preview.cancel();
                self.busy = self.busy.saturating_sub(1);
                self.pending_comparison=None;self.compare.preparation_failed(&message);
                if self.pending_action.take().is_some() {
                    self.banner(BannerKind::Warning,
                        "Cancelled because a pending edit failed. Apply or discard the edit first.");
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
                    doc.restore_failed_edit();
                }
                let kind=if message=="Operation cancelled" {BannerKind::Warning} else {BannerKind::Error};
                self.banner(kind, message);
            }
        }
    }

    fn thumbnails(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(doc) = &self.doc else { return };
        let idle = self.busy == 0 && !self.insert_dialog.busy();
        let count = doc.pages.len();
        let current = u16::try_from(doc.current_page).unwrap_or(0);
        let mut action = None;
        let mut batch = None;
        let mut insert = None;
        self.insert_dialog.feedback(ui,p);
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
                            if let Some(edit) = page_menu(ui, p, current, count, idle, &mut insert) {
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
                            insert = Some(current);
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
        self.page_actions(ui,p,idle);

        let Some(doc) = &mut self.doc else { return };
        let mut requests = Vec::new();
        let mut jump = None;
        ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            THUMB_ROW,
            doc.pages.len(),
            |ui, range| {
                for index in range.clone() {
                    let selected = if self.qol.pages.is_empty() {doc.current_page == index} else {self.qol.pages.contains(&index)};
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
                        if idle && response.hovered() && self.dragging_page.is_none() {
                            ui.ctx().set_cursor_icon(CursorIcon::Grab);
                            // A visible grip makes the thumbnail's drag action discoverable.
                            for x in [rect.right()-9.0,rect.right()-4.0] {
                                for y in [rect.top()+5.0,rect.top()+10.0,rect.top()+15.0] {
                                    ui.painter().circle_filled(pos2(x,y),1.2,p.accent);
                                }
                            }
                        }
                        if (page.thumbnail.is_none()
                            || page.thumbnail_generation != Some(doc.generation))
                            && !page.pending_thumbnail
                            && page.error.is_none()
                        {
                            page.pending_thumbnail = true;
                            requests.push(Request::Thumbnail(index as u16));
                        }
                        if response.clicked() {
                            let mods=ui.input(|i|i.modifiers);
                            self.qol.page_click(index,mods.shift,mods.command);
                            jump = Some(index);
                        }
                        Popup::context_menu(&response).show(|ui| {
                            if let Ok(page) = u16::try_from(index)
                                && let Some(edit) = page_menu(ui, p, page, count, idle, &mut insert)
                            {
                                action = Some(edit);
                            }
                        });
                        if idle && response.drag_started() {
                            if !self.qol.pages.contains(&index) {self.qol.page_click(index,false,false);}
                            self.dragging_page = Some(index);
                        }
                        if let Some(from) = self.dragging_page {
                            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                            if from == index && let Some(pointer)=ui.input(|i|i.pointer.hover_pos()) {
                                let painter=ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip,Id::new("page-drag-preview")));
                                let ghost=Rect::from_min_size(pointer+vec2(18.0,18.0),size*0.55);
                                painter.rect_filled(ghost.expand(3.0),3.0,p.surface);
                                if let Some(texture)=&page.thumbnail {
                                    painter.image(texture.id(),ghost,Rect::from_min_max(Pos2::ZERO,pos2(1.0,1.0)),Color32::WHITE.gamma_multiply(0.8));
                                }
                                painter.rect_stroke(ghost.expand(3.0),3.0,Stroke::new(1.5,p.accent),StrokeKind::Outside);
                                painter.text(ghost.center_bottom()+vec2(0.0,5.0),Align2::CENTER_TOP,
                                    format!("Page {}",from+1),FontId::proportional(12.0),p.text);
                            }
                            if let Some(pointer)=ui.input(|i|i.pointer.hover_pos())
                                && rect.expand2(vec2(18.0,10.0)).contains(pointer) {
                                let after=pointer.y>=rect.center().y;
                                let to=reorder_destination(from,index,after,count);
                                if from!=to || self.qol.pages.len()>1 {
                                    let y=if after {number.rect.bottom()+8.0} else {rect.top()-8.0};
                                    let y=ui.ctx().animate_value_with_time(Id::new("page-insertion-y"),y,0.10);
                                    ui.painter().line_segment([pos2(rect.left()-8.0,y),pos2(rect.right()+8.0,y)],Stroke::new(3.0,p.accent));
                                    ui.painter().circle_filled(pos2(rect.left()-8.0,y),3.0,p.accent);
                                    if ui.input(|i| i.pointer.any_released()) {
                                        if self.qol.pages.len()>1 { batch=Some(pdf_editor_core::PageBatch::Move {pages:self.qol.page_indices(from),before:(index+usize::from(after)) as u16}); }
                                        else {action=Some(pdf_editor_core::PageEdit::Move {from:from as u16,to:to as u16});}
                                    }
                                }
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
            doc.explicit_page = Some(index);
            doc.current_page = index;
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
        if let Some(before) = insert {
            self.insert_dialog.launch(move || rfd::FileDialog::new().set_title("Insert pages from PDF")
                .add_filter("PDF document", &["pdf"]).pick_file()
                .map(|path| Request::Pages(pdf_editor_core::PageEdit::Insert { path,before })));
        }
        if let Some(batch) = batch {self.document_action(Request::PageBatch(batch));}
        if let Some(action) = action {
            self.document_action(Request::Pages(action));
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
        let font = font_readout::Readout::new(self.text_tool.selection.get(self.text_tool.selected).into_iter().map(|o|o.font.as_str()));
        let label = font.as_ref().map_or_else(||"Font…".to_owned(),|font|format!("Font: {}",shorten(&font.name,28)));
        let tip = font.as_ref().map_or_else(||"Change font and style".to_owned(),|font|format!("{}\nChange font and style",font.detail));
        if Btn::new(&label).id("text-font-format").tip(&tip).enabled(ready).show(ui,p).clicked() {
            self.qol.format_targets=self.formatting_targets();
            self.qol.format_open=true;
            if self.text_tool.fonts.is_empty() {self.send(Request::Fonts);}
        }
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
            let spelling =
                crate::spelling::prepare(ui, Id::new("new-text-spelling"), &self.text_tool.text);
            let spell_font = egui::TextStyle::Body.resolve(ui.style());
            let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, width: f32| {
                spelling.layout(ui, text, width, &spell_font, p.text)
            };
            let response = ui.add(
                TextEdit::singleline(&mut self.text_tool.text)
                    .layouter(&mut layouter)
                    .desired_width(160.0)
                    .hint_text("Text"),
            );
            spelling.menu(&response, &mut self.text_tool.text);
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
                self.document_action(Request::AddText {
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
            doc.current_page = usize::from(*page);
            doc.explicit_page = Some(usize::from(*page));
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
            if focused {
                let previous = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, Key::Enter));
                let next = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
                let count = self.find.matches.len();
                if count > 0 && (previous || next) {
                    self.find.active = (self.find.active + if previous { count - 1 } else { 1 }) % count;
                    self.go_match();
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                    self.find.open = false;
                    ui.memory_mut(|m| m.surrender_focus(field_id));
                }
            }
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
                            self.find.refresh_only = false;
                            self.find.matches.clear();
                            self.privacy.preview = None;
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
                            self.find.matches.clear();self.qol.hits.clear();
                            self.privacy.preview = None;
                            self.find.focus = true;
                        }
                    });
                });
            if Btn::new("Options & results").id("search-options").show(ui,p).clicked() {
                self.qol.search_open=true;
                self.send(Request::FindOptions(self.find.query.clone(),self.current_search_options()));
            }
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
                .tip("Previous match (Shift+Enter)")
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
                .tip("Next match (Enter)")
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
        for command in crate::shortcuts::consume(ctx, ctx.text_edit_focused()) {
            use crate::shortcuts::Command;
            match command {
                Command::GoToPage => {
                    self.commit_edit();
                    if let Some(doc) = &self.doc { self.page_jump.open_at(doc.current_page); }
                },
                Command::Tools => { self.tools_open = true; self.tools_focus = true; },
                Command::Open => self.request_open(None),
                Command::Save => self.save(),
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
        if dropped
            .iter()
            .any(|p| crate::image_create::ImageCreate::accepts(p))
        {
            self.image_create.add(dropped);
            return;
        }
        if let Some(path) = dropped.into_iter().next() {
            self.request_open(Some(path));
        }
    }

    fn close_guard(&mut self, ctx: &Context) {
        if self.tabbed || !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        let committed = self.commit_edit();
        if committed || self.busy > 0 || self.doc.as_ref().is_some_and(|d| d.dirty() || d.edit.as_ref().is_some_and(|e|e.text != e.line.text)) {
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
    /// Opens a tool picked from the document menu.
    fn open_tool(&mut self, tool: Tool) {
        match tool {
            Tool::ImageCreate => self.image_create.open = true,
            Tool::ScanCleanup => {
                self.commit_edit();
                self.scan_cleanup.open = true;
            }
            Tool::Bookmarks => {
                self.bookmarks.open = true;
                self.pages_open = false;
                self.document_action(Request::Bookmarks);
            }
            Tool::Drawing => self.drawing.open = true,
            Tool::Crop => self.crop.open = true,
            Tool::Export => self.export.open = true,
            Tool::Tables => self.tables.open = true,
            Tool::Word => self.word.open = true,
            Tool::Stamps => self.stamps.open = true,
            Tool::Compress => self.compression.open = true,
            Tool::Sanitize => self.sanitize.open = true,
            Tool::Compare => self.open_comparison(),
            Tool::Privacy => {
                self.privacy.open = true;
            }
            Tool::Redact => {
                self.redaction.open = true;
                if let Some(o) = self.text_tool.selection.get(self.text_tool.selected) {
                    self.redaction.page = o.page;
                    self.redaction.bounds = o.bounds;
                }
            }
            Tool::Ocr => {
                let page = self.doc.as_ref().map_or(0, |d| d.current_page as u16);
                self.document_action(Request::Ocr(page));
            }
            Tool::Pictures => {
                self.pictures.open = true;
                self.document_action(Request::Pictures);
            }
            Tool::Markup => {
                self.markup.open = true;
                if let Some(o) = self.text_tool.selection.get(self.text_tool.selected) {
                    self.markup.draft.page = o.page;
                    self.markup.draft.bounds = o.bounds;
                }
                self.document_action(Request::Annotations);
            }
            Tool::Organize => self.organize.open = true,
            Tool::Replace => self.replace.open = true,
            Tool::Forms => {
                self.forms.open = true;
                self.document_action(Request::FormFields);
            }
            Tool::Signatures => self.signatures.open = true,
            Tool::Print => {
                self.document_action(Request::Print);
            }
        }
    }

    fn go_to_page(&mut self, page: usize) {
        if let Some(doc) = &mut self.doc {
            if page >= doc.pages.len() { return; }
            doc.explicit_page = Some(page);
            doc.current_page = page;
            let y = CANVAS_PAD + doc.pages.iter().take(page)
                .map(|p| p.info.height * doc.scale() + PAGE_GAP).sum::<f32>();
            doc.scroll_to = Some(vec2(doc.offset.x, y));
        }
    }

    fn set_object_mode(&mut self, enabled: bool) {
        if self.busy > 0 { return; }
        let selection = self.doc.as_ref().and_then(|doc| doc.edit.as_ref().map(|edit| (edit.page, edit.line.rect.center())));
        self.commit_edit();
        self.text_tool.adding = false;
        self.drawing.active = false;
        self.tables.selecting = false;
        self.text_tool.selection.clear();
        if let Some(doc) = &mut self.doc {
            doc.object_mode = true;
            doc.dragging_item = None;
            if enabled { doc.reopen = selection; }
            else { doc.selected_item = None; }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &Palette) {
        if self.settings_open {
            self.settings_bar(ui, p);
        } else if self.doc.is_some() {
            self.document_bar(ui, p);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if Btn::new("All tools").id("all-tools").height(30.0)
                    .look(if self.tools_open { Look::Active } else { Look::Ghost })
                    .tip("Browse and search every tool (Ctrl+K)").show(ui, p).clicked() {
                    self.tools_open = !self.tools_open;
                }
                self.document_tools(ui, p);
                self.qol_toolbar(ui, p);
                ui.checkbox(&mut self.snap_enabled, "Snap").on_hover_text("Align edges, centers and gaps. Hold Alt to bypass.");
                widgets::divider(ui, p, 22.0);
                if Btn::new("Compare PDFs").id("compare-visible").height(30.0).show(ui, p).clicked() { self.open_tool(Tool::Compare); }

            });
        } else {
            self.home_bar(ui, p);
        }
    }


    fn document_tools(&mut self, ui: &mut Ui, p: &Palette) {
        let idle = self.busy == 0 && self.engine_alive;
            let recent = Btn::icon(Icon::ChevronDown)
                .id("recent")
                .min_width(24.0)
                .height(34.0)
                .tip("Document tools and recent files")
                .show(ui, p);
            let mut open = None;
            let mut tool = None;
            Popup::menu(&recent).show(|ui| {
                egui::ScrollArea::vertical()
                    .max_height((ui.ctx().content_rect().height() - 120.0).max(160.0))
                    .show(ui, |ui| {
                        ui.set_width(MENU_WIDTH);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if Btn::new("Save As...").with_icon(Icon::Save).id("save-copy-as").align_left().min_width(MENU_WIDTH).enabled(idle).show(ui,p).clicked() {self.save_as();ui.close();}
                        for (section, tools) in Tool::MENU {
                            menu_caption(ui, p, section);
                            for &(kind, label, icon) in tools {
                                let enabled = if kind == Tool::Bookmarks {
                                    self.doc.is_some() && self.busy == 0
                                } else {
                                    idle
                                };
                                if Btn::new(label)
                                    .with_icon(icon)
                                    .id(kind.key())
                                    .align_left()
                                    .min_width(MENU_WIDTH)
                                    .height(32.0)
                                    .enabled(enabled)
                                    .show(ui, p)
                                    .clicked()
                                {
                                    tool = Some(kind);
                                    ui.close();
                                }
                            }
                            ui.add_space(6.0);
                            widgets::rule(ui, p);
                        }
                        menu_caption(ui, p, "Recent files");
                        if self.recent_files.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(10.0);
                                ui.label(RichText::new("No recent files").color(p.muted));
                            });
                            ui.add_space(6.0);
                        }
                        for path in &self.recent_files {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            let tip = shown_path(path);
                            if Btn::new(&name)
                                .with_icon(Icon::File)
                                .align_left()
                                .min_width(MENU_WIDTH)
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
            });
            if let Some(tool) = tool {
                self.open_tool(tool);
            }
            if let Some(path) = open {
                self.request_open(Some(path));
            }
    }

    fn document_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let idle = self.busy == 0 && self.engine_alive;
        let ready = self.setup_error.is_none() && self.engine_alive;
        // Narrow windows show icons only; every button keeps its tooltip.
        let compact = ui.available_width() < 1120.0;
        let text = |label: &'static str| if compact { "" } else { label };
        ui.horizontal(|ui| {
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
            if Btn::new(text("Save"))
                .with_icon(Icon::Save)
                .id("save-as")
                .enabled(idle)
                .tip("Save working copy (Ctrl+S); first save creates a new file")
                .show(ui, p)
                .clicked()
            {
                self.save();
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
            if Btn::new("All tools").id("all-tools").tip("Browse all tools (Ctrl+K)").show(ui, p).clicked() {
                self.tools_open = !self.tools_open;
            }
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
                if Btn::new("Images to PDF…")
                    .enabled(ready && self.busy == 0)
                    .show(ui, p)
                    .clicked()
                {
                    self.image_create.open = true;
                }
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
        } else if doc.object_mode {
            "Drag objects to move  /  use handles to resize"
        } else {
            "Click text to edit it in place"
        };
        let zoom = format!("{:.0}%", doc.zoom * 100.0);
        let font = font_readout::Readout::new(self.formatting_objects().map(|object|object.font.as_str()));
        let selection_info = font.as_ref().map_or_else(||hint.to_owned(),|font|format!("Font: {}",font.name));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("Page").color(p.muted));
            if Btn::new(&current.to_string()).id("go-to-page").height(28.0)
                .tip("Go to page (Ctrl+G)").show(ui,p).clicked() { self.page_jump.open_at(current - 1); }
            ui.label(RichText::new(format!("of {total}")).color(p.muted));
            ui.add_space(16.0);
            let width = (ui.available_width()-240.).clamp(0.,300.);
            let info = ui.add_sized(vec2(width,0.),egui::Label::new(RichText::new(selection_info).small().color(p.muted)).truncate());
            if let Some(font) = font {info.on_hover_text(font.detail);}
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
                    if self.pages_open {
                        self.bookmarks.open = false;
                    }
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
        let surface_ready = self.surface.ready(&ctx);
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

        if self.signatures.placing && ctx.input_mut(|i|i.consume_key(egui::Modifiers::NONE,Key::Escape)) {
            self.signatures.placing = false;
        }
        let busy = self.busy > 0;
        self.tool_anchor = None;
        let Some(doc) = &mut self.doc else { return };
        if doc.dragging_item.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE,Key::Escape)) {
            doc.dragging_item = None;
            doc.panning = false;
            self.drag_preview.cancel();
        }
        if pointer.is_some_and(|pos| canvas_rect.contains(pos))
            && ctx.input(|i| i.events.iter().any(|event| matches!(event, egui::Event::MouseWheel { .. })) || i.pointer.is_decidedly_dragging()) {
            doc.explicit_page = None;
        }
        if let Some(page) = doc.explicit_page {
            let page = page.min(doc.pages.len().saturating_sub(1));
            doc.explicit_page = Some(page);
            doc.current_page = page;
        }
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

        let mut area = ScrollArea::both()
            .auto_shrink(false)
            .scroll_source(egui::scroll_area::ScrollSource::ALL);
        if let Some(offset) = doc.scroll_to.take() {
            area = area.scroll_offset(offset);
        }
        let mut requests = Vec::new();
        let mut start_edit: Option<EditState> = None;
        let mut clicked_at: Option<(usize, Pos2)> = None;
        if !self.qol.text_mode && doc.object_mode && !busy && doc.edit.is_none() && !ctx.text_edit_focused() {
            let clipboard_event = |paste| {
                ctx.input_mut(|i| {
                    let found = i.events.iter().position(|event| {
                        if paste {
                            matches!(event, egui::Event::Paste(_))
                        } else {
                            matches!(event, egui::Event::Copy)
                        }
                    });
                    if let Some(index) = found {
                        i.events.remove(index);
                        true
                    } else {
                        false
                    }
                })
            };
            if clipboard_event(true)
                || ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::V))
            {
                requests.push(Request::PasteItems(doc.current_page as u16));
                self.busy += 1;
            } else if let Some(item) = &doc.selected_item {
                let selected=doc.pages.get(usize::from(item.page)).and_then(|page|page.objects.as_ref())
                    .map_or_else(||vec![item.clone()],|objects|self.qol.items(objects,item));
                let request = if clipboard_event(false)
                    || ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::C))
                {
                    Some(Request::CopyItems(selected.clone()))
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Delete)) {
                    Some(Request::EditItems(
                        selected.clone(),
                        pdf_editor_core::ObjectAction::Delete,
                    ))
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::D)) {
                    Some(Request::EditItems(
                        selected.clone(),
                        pdf_editor_core::ObjectAction::Duplicate {
                            dx: 12.0,
                            dy: -12.0,
                        },
                    ))
                } else {
                    None
                };
                if let Some(request) = request {
                    requests.push(request);
                    self.busy += 1;
                }
            }
        }

        if !self.qol.text_mode && !busy && doc.edit.is_none() && !ctx.text_edit_focused()
            && let Some(item)=&doc.selected_item
            && let Some(objects)=doc.pages.get(usize::from(item.page)).and_then(|p|p.objects.as_ref()) {
            let step=if ctx.input(|i|i.modifiers.shift) {10.0} else {1.0};
            let mut delta=Vec2::ZERO;
            for (key,vector) in [(Key::ArrowLeft,vec2(-step,0.0)),(Key::ArrowRight,vec2(step,0.0)),(Key::ArrowUp,vec2(0.0,-step)),(Key::ArrowDown,vec2(0.0,step))] {
                let mods=if step>1.0 {egui::Modifiers::SHIFT} else {egui::Modifiers::NONE};
                if ctx.input_mut(|i|i.consume_key(mods,key)) {delta+=vector;}
            }
            if delta!=Vec2::ZERO && let Some((dx,dy))=raster_delta(&objects.transform,delta) {
                requests.push(Request::EditItems(self.qol.items(objects,item),pdf_editor_core::ObjectAction::Move {dx,dy})); self.busy+=1;
            }
        }
        if self.qol.text_mode && !ctx.text_edit_focused() {
            let copied=ctx.input_mut(|i| {
                if let Some(index)=i.events.iter().position(|event|matches!(event,egui::Event::Copy)) {i.events.remove(index);true} else {i.consume_key(egui::Modifiers::COMMAND,Key::C)}
            });
            if copied && let Some((text,_))=self.qol.selection() {ctx.copy_text(text);}
        }
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

            if self.qol.text_dragging && let Some(pointer)=ctx.input(|i|i.pointer.hover_pos()) {
                let clip=ui.clip_rect();
                let delta=if pointer.y<clip.top()+28.0 {18.0} else if pointer.y>clip.bottom()-28.0 {-18.0} else {0.0};
                if delta!=0.0 {ui.scroll_with_delta_animation(vec2(0.0,delta),egui::style::ScrollAnimation::none());ctx.request_repaint();}
            }
            for (index, page) in doc.pages.iter_mut().enumerate() {
                let size = vec2(page.info.width, page.info.height) * scale;
                let local = Rect::from_min_size(pos2((content_w - size.x) / 2.0, y), size);
                y += size.y + PAGE_GAP;
                let rect = round_rect(local.translate(origin.to_vec2()), ppp);
                page_rects.push(rect);
                if doc.explicit_page.is_none() && local.min.y <= center_y {
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
                let requested=scale*ppp;
                let capped=render_scale(&page.info,requested);
                let wanted=if capped<requested/1.02 {
                    render_scale(&page.info,capped.min((2_000_000.0/(page.info.width*page.info.height)).sqrt()))
                }else{capped};
                let stale = page.texture.as_ref().is_none_or(|t| {
                    t.generation != doc.generation || (t.scale - wanted).abs() > wanted * 0.01
                });
                if stale
                    && surface_ready
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
                let sharp_scale=(scale*ppp*64.).floor()/64.;
                if sharp_scale>wanted*1.02 && surface_ready && page.error.is_none() {
                    if let Some(regions)=page.tiles.plan(&page.info,rect,ui.clip_rect(),sharp_scale,doc.generation) {
                        requests.push(Request::RenderTiles {page:page_no,scale:sharp_scale,regions});
                    }
                } else {page.tiles.clear();}
                paint_page(&painter, ui, rect, page, p);
                page.tiles.paint(&painter,rect,&page.info,doc.generation);
                if self.drag_preview.committing() {
                    self.drag_preview.paint(&painter,rect,scale,index,doc.generation,page.texture.as_ref(),p.accent);
                }
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
                if self.find.open || (self.privacy.open && self.privacy.preview.is_some()) {
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

                if !self.signatures.placing && let Some(objects) = &page.objects {
                    self.signatures.paint_placement(
                        &painter,
                        page_no,
                        rect.min,
                        scale,
                        objects.transform,
                    );
                }
                // Hover and click hit testing against cached object extents.
                let mut response =
                    ui.interact(rect, Id::new(("pdf-page", index)), Sense::click_and_drag());
                if self.signatures.placing && !busy {
                    if let Some(objects) = &page.objects {
                        if let Some(request) = self.signatures.place_at_pointer(&response,page_no,rect.min,scale,objects.transform) {
                            requests.push(request);
                            self.busy += 1;
                        }
                        self.signatures.paint_placement(&painter,page_no,rect.min,scale,objects.transform);
                    }
                    continue;
                }
                if response.clicked() || response.dragged() {
                    doc.explicit_page = Some(index);
                    doc.current_page = index;
                }
                if !busy && doc.edit.is_none() && !self.drawing.active && !self.tables.selecting && !self.text_tool.adding
                    && self.qol.on_canvas(&ctx,&response,&painter,page_no,rect,scale,page.objects.as_ref(),&mut doc.selected_item,&mut requests,p.accent) {
                    doc.panning=false;doc.dragging_item=None;
                    continue;
                }
                // Separate hit targets keep handles usable just outside page edges.
                if !busy
                    && doc.object_mode
                    && doc.edit.is_none()
                    && let Some(item) = doc.selected_item.as_ref().filter(|i| i.page == page_no)
                    && let Some(objects) = &page.objects
                {
                    let selection =
                        to_screen(bounds_rect(&objects.transform, item.bounds), rect, scale);
                    for (handle, center) in resize_handles(selection).into_iter().enumerate() {
                        let handle_response = ui.interact(
                            Rect::from_center_size(center, vec2(14.0, 14.0)),
                            Id::new(("object-resize", index, handle)),
                            Sense::click_and_drag(),
                        );
                        if handle_response.hovered()
                            || handle_response.dragged()
                            || handle_response.drag_stopped()
                            || handle_response.is_pointer_button_down_on()
                            || handle_response.clicked()
                        {
                            response = handle_response;
                        }
                    }
                }

                let table_selecting = self.tables.selecting;
                if !busy
                    && let Some(objects) = &page.objects
                    && let Some(request) = self.tables.on_page(
                        &response,
                        &painter,
                        page_no,
                        rect,
                        scale,
                        objects.transform,
                    )
                {
                    self.busy += 1;
                    requests.push(request);
                }
                if !busy
                    && !table_selecting
                    && let Some(objects) = &page.objects
                    && let Some(request) = self.drawing.on_page(
                        &response,
                        &painter,
                        page_no,
                        rect,
                        scale,
                        objects.transform,
                    )
                {
                    self.busy += 1;
                    requests.push(request);
                }
                // A line clicked while the previous edit was applied opens
                // once this page's objects reflect that edit.
                if let Some((reopen_page, at)) = doc.reopen
                    && reopen_page == index
                    && !busy
                    && let Some(objects) = &page.objects
                    && objects.generation == doc.generation
                {
                    doc.reopen = None;
                    if doc.object_mode {
                        doc.selected_item = objects.items.iter().rev().find(|item| bounds_rect(&objects.transform, item.bounds).contains(at)).map(|item|selection_item(objects,item));
                    } else if let Some(line) = objects.lines.iter().rev().find(|l| l.rect.contains(at)) {
                        start_edit = Some(EditState {
                            rejected_text: None,
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
                // Imported designs often keep their text inside Form XObjects.
                // Expose the existing lossless ungroup operation at the point
                // of editing, then reopen the clicked text after the refresh.
                if hit.is_none()
                    && doc.edit.is_none()
                    && !self.drawing.active
                    && !self.text_tool.adding
                    && !table_selecting
                    && !busy
                    && let Some(pos) = response.hover_pos()
                    && let Some(objects) = &page.objects
                {
                    let at = ((pos - rect.min) / scale).to_pos2();
                    if let Some(item) = objects.items.iter().rev().find(|item| {
                        item.kind == "XObjectForm"
                            && bounds_rect(&objects.transform, item.bounds).contains(at)
                    }) {
                        let screen =
                            to_screen(bounds_rect(&objects.transform, item.bounds), rect, scale);
                        painter.rect_stroke(
                            screen,
                            3.0,
                            Stroke::new(1.0, p.accent),
                            StrokeKind::Outside,
                        );
                        ctx.set_cursor_icon(CursorIcon::Grab);
                        if response.double_clicked() || response.triple_clicked() {
                            doc.reopen = Some((index, at));
                            self.busy += 1;
                            requests.push(Request::PrepareGroupedText(item.clone()));
                        }
                    }
                }
                if !self.drawing.active && !self.text_tool.adding && !busy && !table_selecting {
                    let at = (if response.drag_started() {
                        ctx.input(|i| i.pointer.press_origin())
                    } else {
                        response.interact_pointer_pos().or_else(|| response.hover_pos())
                    })
                    .map(|pos| ((pos - rect.min) / scale).to_pos2());
                    let item_hit = page.objects.as_ref().and_then(|objects| {
                        at.and_then(|at| {
                            // Whitespace inside a text block must select that block,
                            // not an empty gap or a page-sized background underneath.
                            objects.lines.iter().rev().find(|line|line.rect.expand(1.0).contains(at))
                                .and_then(|line|objects.items.iter().find(|item|line.objects.iter().any(|o|o.index==item.index)))
                                .or_else(||objects.items.iter().rev().find(|item| {
                                    bounds_rect(&objects.transform, item.bounds).expand(2.0).contains(at)
                                }))
                        })
                    });
                    if item_hit.is_some() && doc.edit.is_none() && response.hovered() {
                        ctx.set_cursor_icon(CursorIcon::Grab);
                    }
                    let resize_handle = doc
                        .selected_item
                        .as_ref()
                        .filter(|item| doc.object_mode && item.page == page_no)
                        .zip(page.objects.as_ref())
                        .and_then(|(item, objects)| {
                            let bounds = bounds_rect(&objects.transform, item.bounds);
                            at.and_then(|at| {
                                resize_handles(bounds)
                                    .iter()
                                    .position(|pos| pos.distance(at) <= 7.0 / scale)
                            })
                        });
                    if doc.object_mode
                        && resize_handle.is_none()
                        && (response.clicked() || response.secondary_clicked())
                    {
                        if let Some(objects)=&page.objects {
                            let extend=ctx.input(|i|i.modifiers.shift || i.modifiers.command);
                            let keep=response.secondary_clicked()&&item_hit.is_some_and(|item|self.qol.objects.iter().any(|picked|picked.page==item.page&&picked.revision==item.revision&&picked.index==item.index));
                            if !keep {doc.selected_item=self.qol.select(objects,item_hit,extend);}
                        }
                        if doc.edit.as_ref().is_none_or(|e| e.text == e.line.text) {
                            doc.edit = None;
                            self.text_tool.selection.clear();
                        }
                    }
                    if response.drag_started() && resize_handle.is_none() && doc.object_mode
                        && let Some(objects)=&page.objects && let Some(item)=item_hit
                        && !self.qol.objects.iter().any(|picked|picked.page==item.page&&picked.index==item.index) {
                        doc.selected_item=self.qol.select(objects,Some(item),false);
                    }
                    let can_drag = doc.edit.is_none();
                    if can_drag && ctx.input(|i| i.pointer.primary_pressed())
                        && response.is_pointer_button_down_on()
                        && let Some(objects) = &page.objects
                        && let Some(item) = resize_handle.and(doc.selected_item.as_ref()).or(item_hit) {
                        let items = self.qol.items(objects,item);
                        let preview_scale = wanted.min((1_900_000. / (page.info.width * page.info.height)).sqrt());
                        if let Some(id) = self.drag_preview.plan(doc.generation,index,&items,preview_scale) {
                            requests.push(Request::ObjectPreview {id,items,scale:preview_scale});
                        }
                    }
                    if response.drag_started() && can_drag {
                        let items = if resize_handle.is_some() {
                            doc.selected_item.as_ref().zip(page.objects.as_ref())
                                .map_or_else(Vec::new,|(item,objects)|self.qol.items(objects,item))
                        } else if doc.object_mode {
                            item_hit.zip(page.objects.as_ref())
                                .map_or_else(Vec::new,|(item,objects)|self.qol.items(objects,item))
                        } else if let Some(objects) = &page.objects {
                            objects
                                .lines
                                .iter()
                                .rev()
                                .find(|line| {
                                    at.is_some_and(|at| line.rect.expand(1.0).contains(at))
                                })
                                .map(|line| {
                                    objects
                                        .items
                                        .iter()
                                        .filter(|item| {
                                            line.objects.iter().any(|o| o.index == item.index)
                                        })
                                        .cloned()
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        doc.panning = items.is_empty();
                        if !items.is_empty()
                            && let Some(objects) = &page.objects
                        {
                            let bounds = items.iter().fold(Rect::NOTHING, |bounds, item| {
                                bounds.union(bounds_rect(&objects.transform, item.bounds))
                            });
                            let moving = items.iter().map(|item|item.index).collect::<std::collections::HashSet<_>>();
                            let snap_targets = objects.items.iter().filter(|item|!moving.contains(&item.index))
                                .map(|item|bounds_rect(&objects.transform,item.bounds)).collect();
                            let preview_scale = wanted.min((1_900_000. / (page.info.width * page.info.height)).sqrt());
                            if let Some(id) = self.drag_preview.plan(doc.generation,index,&items,preview_scale) {
                                requests.push(Request::ObjectPreview {id,items:items.clone(),scale:preview_scale});
                            }
                            doc.dragging_item = Some(ItemDrag {
                                page: index,
                                items,
                                bounds,
                                delta: Vec2::ZERO,
                                anchor: ((ctx.input(|i| i.pointer.press_origin()).unwrap_or(rect.min)-rect.min)/scale).to_pos2(),
                                resize_handle,
                                preview: None,
                                snap_state: Default::default(),
                                snap_targets,
                            });
                        }
                    }
                    if let Some(drag) = &mut doc.dragging_item
                        && drag.page == index
                    {
                        if let Some(pointer) = ctx.input(|i| i.pointer.interact_pos()) {
                            drag.delta = ((pointer - rect.min) / scale).to_pos2() - drag.anchor;
                        }
                        let mut preview_bounds = drag.resize_handle.map_or_else(
                            || drag.bounds.translate(drag.delta),
                            |handle| resized_rect(drag.bounds, handle, drag.delta),
                        );
                        let mut guides = None;
                        if self.snap_enabled && !ctx.input(|i| i.modifiers.alt)
                            && page.objects.is_some()
                        {
                            let page_bounds = Rect::from_min_size(Pos2::ZERO,vec2(page.info.width,page.info.height));
                            let targets = drag.snap_targets.iter().copied();
                            let snapped = crate::snapping::align(preview_bounds,drag.resize_handle,page_bounds,targets,6.0/scale,&mut drag.snap_state);
                            preview_bounds = snapped.bounds;
                            guides = Some(snapped);
                        } else {
                            drag.snap_state = Default::default();
                        }
                        drag.preview = Some(preview_bounds);
                        self.drag_preview.place(drag.bounds,preview_bounds);
                        self.drag_preview.paint(&painter,rect,scale,index,doc.generation,page.texture.as_ref(),p.accent);
                        if let Some(guides) = &guides { crate::snapping::paint(&painter,guides,rect,scale,p.accent); }
                        let preview = to_screen(preview_bounds, rect, scale);
                        painter.rect_stroke(
                            preview,
                            2.0,
                            Stroke::new(2.0, p.accent),
                            StrokeKind::Outside,
                        );
                        ctx.set_cursor_icon(
                            drag.resize_handle
                                .map_or(CursorIcon::Grabbing, resize_cursor),
                        );
                        if response.drag_stopped() {
                            let drag = doc.dragging_item.take().unwrap();
                            if let Some(objects) = &page.objects
                                && let Some((dx, dy)) = raster_delta(&objects.transform, if drag.resize_handle.is_some() { drag.delta } else { preview_bounds.min-drag.bounds.min })
                                && ((preview_bounds.min-drag.bounds.min).abs().max_elem() > 0.01
                                    || (preview_bounds.max-drag.bounds.max).abs().max_elem() > 0.01)
                            {
                                self.busy += 1;
                                let action = if drag.resize_handle.is_some() {
                                    let bounds = preview_bounds;
                                    raster_bounds(&objects.transform, bounds).map(|bounds| {
                                        pdf_editor_core::ObjectAction::Resize { bounds }
                                    })
                                } else {
                                    Some(pdf_editor_core::ObjectAction::Move { dx, dy })
                                };
                                if let Some(action) = action {
                                    self.drag_preview.commit();
                                    requests.push(Request::EditItems(drag.items, action));
                                } else {
                                    self.busy = self.busy.saturating_sub(1);
                                }
                            } else {
                                self.drag_preview.cancel();
                            }
                        }
                    } else if response.dragged() && can_drag && doc.panning {
                        ui.scroll_with_delta_animation(
                            response.drag_delta(),
                            egui::style::ScrollAnimation::none(),
                        );
                        ctx.set_cursor_icon(CursorIcon::Grabbing);
                    }
                    if response.drag_stopped() {
                        doc.panning = false;
                    }
                    if doc.object_mode && doc.edit.is_none()
                        && let Some(item) = &doc.selected_item
                        && item.page == page_no
                        && let Some(objects) = &page.objects
                    {
                        let preview_bounds = doc
                            .dragging_item
                            .as_ref()
                            .filter(|drag| drag.page == index)
                            .map(|drag| {
                                drag.preview.unwrap_or_else(|| drag.bounds.translate(drag.delta))
                            })
                            .or_else(|| self.drag_preview.committing().then(||self.drag_preview.placement()).flatten())
                            .unwrap_or_else(|| bounds_rect(&objects.transform, item.bounds));
                        for picked in self.qol.objects.iter().filter(|_| doc.dragging_item.is_none() && !self.drag_preview.committing()) {
                            if picked.page==page_no {painter.rect_stroke(to_screen(bounds_rect(&objects.transform,picked.bounds),rect,scale),1.0,Stroke::new(1.0,p.accent),StrokeKind::Outside);}
                        }
                        let selection = to_screen(preview_bounds, rect, scale);
                        for handle in resize_handles(selection) {
                            let square = Rect::from_center_size(handle, vec2(7.0, 7.0));
                            painter.rect_filled(square, 1.0, Color32::WHITE);
                            painter.rect_stroke(
                                square,
                                1.0,
                                Stroke::new(1.0, p.accent),
                                StrokeKind::Outside,
                            );
                        }
                        if let Some(handle) = resize_handle {
                            ctx.set_cursor_icon(resize_cursor(handle));
                        }
                        painter.rect_stroke(
                            selection,
                            2.0,
                            Stroke::new(1.5, p.accent),
                            StrokeKind::Outside,
                        );
                    }
                    if doc.object_mode && let Some(objects)=&page.objects {
                        response.context_menu(|ui| {
                            if let Some(item) = doc
                                .selected_item
                                .as_ref()
                                .filter(|item| item.page == page_no)
                            {
                                ui.label(format!("{} object", item.kind));
                                if item.kind == "XObjectForm" && ui.button("Ungroup").clicked() {
                                    self.busy += 1;
                                    requests.push(Request::UngroupItem(item.clone()));
                                    ui.close();
                                }
                                if ui.button("Copy").clicked() {
                                    self.busy += 1;
                                    requests.push(Request::CopyItems(self.qol.items(objects,item)));
                                    ui.close();
                                }
                                for (label, action) in [
                                    ("Bring to front", pdf_editor_core::ObjectAction::BringToFront),
                                    ("Send to back", pdf_editor_core::ObjectAction::SendToBack),
                                    (
                                        "Duplicate",
                                        pdf_editor_core::ObjectAction::Duplicate {
                                            dx: 12.0,
                                            dy: -12.0,
                                        },
                                    ),
                                    ("Delete", pdf_editor_core::ObjectAction::Delete),
                                ] {
                                    if ui.button(label).clicked() {
                                        self.busy += 1;
                                        requests
                                            .push(Request::EditItems(self.qol.items(objects,item), action));
                                        ui.close();
                                    }
                                }
                            }
                            if ui.button("Paste on this page").clicked() {
                                self.busy += 1;
                                requests.push(Request::PasteItems(page_no));
                                ui.close();
                            }
                        });
                    }
                }
                let editing_this = |line: &Line| {
                    doc.edit.as_ref().is_some_and(|e| {
                        e.page == index && e.line.objects[0].index == line.objects[0].index
                    })
                };
                if self.text_tool.adding
                    && !table_selecting
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
                if let Some(line) = hit.filter(|_| {
                    !self.text_tool.adding
                        && !self.drawing.active
                        && !table_selecting
                }) {
                    if !editing_this(line) && !busy {
                        let screen = to_screen(line.rect, rect, scale);
                        painter.rect_stroke(
                            screen.expand(2.0),
                            CornerRadius::same(3),
                            Stroke::new(1.0, p.accent),
                            StrokeKind::Outside,
                        );
                        ctx.set_cursor_icon(if doc.edit.is_some() { CursorIcon::Text } else { CursorIcon::Grab });
                    }
                    if (response.double_clicked() || response.triple_clicked() || (!doc.object_mode && response.clicked())) && !busy && !editing_this(line) {
                        let chosen = if ctx.input(|i| i.modifiers.alt) && line.paragraph.is_some() {
                            let items = page
                                .objects
                                .as_ref()
                                .map(|objects| {
                                    line.objects
                                        .iter()
                                        .map(|o| (o.clone(), raster_rect(&objects.transform, o)))
                                        .collect()
                                })
                                .unwrap_or_default();
                            let at = response
                                .interact_pointer_pos()
                                .map(|pos| ((pos - rect.min) / scale).to_pos2());
                            group_lines_fallback(items)
                                .into_iter()
                                .find(|l| at.is_some_and(|at| l.rect.expand(2.0).contains(at)))
                                .unwrap_or_else(|| line.clone())
                        } else {
                            line.clone()
                        };
                        start_edit = Some(EditState {
                            rejected_text: None,
                            page: index,
                            text: chosen.text.clone(),
                            line: chosen,
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
                        page.tiles.clear();
                        page.objects = None;
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
        for banner in &mut self.banners {
            let lifetime = match banner.kind {
                BannerKind::Error => 12.0,
                BannerKind::Warning => 10.0,
                BannerKind::Success => 5.0,
            };
            banner.expires.get_or_insert(now + lifetime);
        }
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
            .movable(false)
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
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                if Btn::new("×")
                                    .id(&format!("dismiss-banner-{}", banner.id))
                                    .height(28.0)
                                    .min_width(28.0)
                                    .tip("Dismiss")
                                    .show(ui, p)
                                    .clicked()
                                {
                                    dismissed.push(banner.id);
                                }
                                let text_width = (ui.available_width() - 20.0).max(40.0);
                                ui.add_sized(
                                    vec2(text_width, 0.0),
                                    egui::Label::new(RichText::new(&banner.text).color(fg)).wrap(),
                                );
                                let (dot, _) =
                                    ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                                ui.painter().circle_filled(dot.center(), 3.5, fg);
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
                RichText::new("Your edits have not been saved. Save the working copy first to keep them.")
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
                if Btn::new("Cancel").id("unsaved-cancel")
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

    /// Distinguishes applied snapshots from drafts still being typed.
    fn recovery_status(&self) -> &'static str {
        if self.doc.as_ref().is_some_and(|d| d.protected) { return "Recovery disabled"; }
        if self.autosave.0.is_none() { return "Recovery off"; }
        if self.session.is_none() || self.session_error.is_some() { return "Recovery unavailable"; }
        if let Some(doc) = &self.doc {
            if doc.edit.as_ref().is_some_and(|e| e.text != e.line.text) { return "Draft not applied"; }
            if doc.copying { return "Writing recovery…"; }
            if doc.dirty() && doc.copied_generation == Some(doc.generation) { return "Recovery saved"; }
            if doc.dirty() { return "Recovery pending"; }
        }
        "Recovery on"
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

    fn general_settings(&mut self, ui: &mut Ui, p: &Palette, autosave: &mut AutoSave) {
        self.integration.show(ui);
        if ui.checkbox(&mut self.qol.remember_session,"Restore tabs on startup").changed() {self.qol.remember_changed=true;}
        ui.add_space(20.0);
        section_title(ui, "Auto-save");
        let label = |value: AutoSave| match value.0 {
            None => "Off".to_string(),
            Some(minutes) => format!("Every {minutes} minutes"),
        };
        widgets::setting_row(
            ui,
            p,
            "Recovery copies",
            "Recover applied edits after a crash.",
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
        if let Some(error)=&self.session_error {ui.colored_label(p.warn_fg,format!("Recovery unavailable: {error}"));}
        else {ui.label(RichText::new(self.recovery_status()).small().color(p.muted)).on_hover_text("Recovery copies stay on this computer. Password-protected documents are excluded.");}

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
        if let Some(request) = self.insert_dialog.poll(&ctx,self.doc.is_some(),self.busy == 0) {
            self.document_action(request);
        }

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
        if self.tools_open && !self.settings_open {
            egui::Panel::right("all-tools").frame(Frame::new().fill(p.chrome).inner_margin(12))
                .default_size(290.0).min_size(230.0).max_size(360.0).resizable(true)
                .show(ui, |ui| self.tool_catalog(ui, &p));
        }
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
            if self.bookmarks.open && !self.tools_open {
                let mut action = None;
                let current = self.doc.as_ref().map_or(0, |d| d.current_page as u16);
                egui::Panel::left("bookmarks")
                    .frame(Frame::new().fill(p.chrome))
                    .default_size(260.0)
                    .min_size(220.0)
                    .max_size(420.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        action = self.bookmarks.show(ui, &p, current, self.busy == 0);
                    });
                match action {
                    Some(crate::bookmarks::Action::Request(request)) => {
                        self.document_action(request);
                    }
                    Some(crate::bookmarks::Action::Navigate(page)) => {
                        self.commit_edit();
                        if let Some(doc) = &mut self.doc {
                            let y = CANVAS_PAD
                                + doc
                                    .pages
                                    .iter()
                                    .take(usize::from(page))
                                    .map(|p| p.info.height * doc.scale() + PAGE_GAP)
                                    .sum::<f32>();
                            doc.scroll_to = Some(vec2(doc.offset.x, y));
                        }
                    }
                    None => {}
                }
            }
            if self.pages_open && !self.tools_open {
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

        if let Some(page) = self.page_jump.show(&ctx, &p, self.doc.as_ref().map_or(0, |d| d.pages.len())) {
            self.commit_edit();
            self.go_to_page(page);
        }
        if let Some(request) = self.image_create.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        if let Some(request) = self.scan_cleanup.show(
            &ctx,
            &p,
            self.busy == 0,
            self.doc.as_ref().map_or(0, |d| d.pages.len()),
        ) {
            self.document_action(request);
        }
        let drawing_was_active = self.drawing.active;
        self.drawing.show(&ctx, &p);
        if !drawing_was_active && self.drawing.active {
            self.commit_edit();
        }
        if let Some(request) = self.crop.show(
            &ctx,
            &p,
            self.busy == 0,
            self.doc.as_ref().map_or(0, |d| d.pages.len()),
        ) {
            self.document_action(request);
        }
        if let Some(request) = self.word.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        let selecting_table = self.tables.selecting;
        if let Some(request) = self.tables.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        if !selecting_table && self.tables.selecting {
            self.commit_edit();
            self.text_tool.adding = false;
            self.drawing.active = false;
        }
        if let Some(request) = self.export.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        if let Some(request) = self.stamps.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        self.compare.show(&ctx, &p);
        if self.compare.take_refresh() {self.refresh_comparison();}
        if let Some(request) = self.sanitize.show_for(&ctx, &p, self.busy == 0,
            self.doc.as_ref().is_some_and(|doc|doc.protected)) {
            self.document_action(request);
        }
        if let Some(request) = self.compression.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        if let Some(index) = self.privacy.jump.take() {
            self.find.active = index;
            self.go_match();
        }
        if let Some(request) = self.privacy.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        if let Some(request) = self.redaction.show(
            &ctx,
            &p,
            self.busy == 0,
            self.doc.as_ref().map_or(0, |d| d.pages.len()),
        ) {
            self.document_action(request);
        }
        if let Some(request) = self.pictures.show(
            &ctx,
            &p,
            self.busy == 0,
            self.doc.as_ref().map_or(0, |d| d.pages.len()),
        ) {
            self.document_action(request);
        }
        if let Some(request) = self.markup.show(
            &ctx,
            &p,
            self.busy == 0,
            self.doc.as_ref().map_or(0, |d| d.pages.len()),
        ) {
            self.document_action(request);
        }
        if let Some(request) = self.organize.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
        }
        if let Some(request) = self.password.show(&ctx, &p, self.busy == 0, ui.id()) {
            self.document_action(request);
        }
        if self.qol.replace_context.as_ref().is_some_and(|context|self.doc.as_ref().is_none_or(|d|*context!=(d.generation,self.current_search_options()))) {self.replace.count=None;}
        let mut replace=std::mem::take(&mut self.replace);
        let replace_request=replace.show_with_options(&ctx,&p,self.busy==0,|ui|self.search_options_ui(ui));
        self.replace=replace;
        if let Some(request) = replace_request {
            self.document_action(request);
        }
        if let Some(request) = self.forms.show(&ctx, &p, self.busy == 0) {
            self.document_action(request);
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
            self.document_action(request);
        }
        self.qol_windows(&ctx, &p);
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
        if !unsaved {self.discard_recovery();}
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

/// Small muted heading between groups of menu entries.
fn menu_caption(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(text).size(12.0).color(p.muted));
    });
    ui.add_space(4.0);
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

/// Close confirmation remains cancellable while worker operations finish.
fn close_tabs_prompt(ctx: &Context, p: &Palette, single: bool, busy: bool) -> Option<bool> {
    let mut choice = None;
    let modal = egui::Modal::new(Id::new("close-tabs-confirm"))
        .frame(modal_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new(if single {
                    "Close this tab?"
                } else {
                    "Close all documents?"
                })
                .font(theme::semibold(17.0)),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new("Unsaved changes will be lost. Cancel to save documents first.")
                    .color(p.muted),
            );
            if busy {
                ui.add_space(4.0);
                widgets::notice(
                    ui,
                    p,
                    widgets::Tone::Warn,
                    "Wait for pending document operations to finish.",
                );
            }
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if Btn::new("Discard and close")
                    .id("close-tabs-discard")
                    .look(Look::Danger)
                    .enabled(!busy)
                    .show(ui, p)
                    .clicked()
                {
                    choice = Some(true);
                }
                if Btn::new("Cancel")
                    .id("close-tabs-cancel")
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
    choice
}

fn reorder_destination(from: usize, hovered: usize, after: bool, count: usize) -> usize {
    let slot=hovered+usize::from(after);
    slot.saturating_sub(usize::from(from<slot)).min(count.saturating_sub(1))
}

fn page_menu(
    ui: &mut Ui,
    p: &Palette,
    page: u16,
    count: usize,
    idle: bool,
    insert: &mut Option<u16>,
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
        *insert = Some(page);
    }
    action
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
/// One tab in the document tab strip. Returns whether the tab was picked
/// and whether its close button was clicked.
fn document_tab(ui: &mut Ui, p: &Palette, name: &str, active: bool, edited: bool) -> (bool, bool) {
    const HEIGHT: f32 = 34.0;
    const CLOSE: f32 = 22.0;
    let mut job = egui::text::LayoutJob::single_section(
        name.to_string(),
        egui::TextFormat::simple(FontId::proportional(13.5), Color32::PLACEHOLDER),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(180.0);
    let galley = ui.painter().layout_job(job);
    let width = 12.0 + 16.0 + 8.0 + galley.size().x + 8.0 + CLOSE + 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, name));
    let hot = ui.rect_contains_pointer(rect);
    let painter = ui.painter();
    let top = CornerRadius {
        nw: 8,
        ne: 8,
        sw: 0,
        se: 0,
    };
    if active {
        painter.rect_filled(rect, top, p.chrome);
        painter.rect_filled(
            Rect::from_min_size(rect.left_top(), vec2(rect.width(), 2.0)),
            top,
            p.accent,
        );
    } else if hot {
        painter.rect_filled(rect, top, p.hover);
    }
    let icon = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.center().y - 8.0),
        vec2(16.0, 16.0),
    );
    crate::icons::paint(
        painter,
        icon,
        Icon::File,
        if active { p.accent_strong } else { p.muted },
    );
    painter.galley(
        pos2(icon.right() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        if active { p.text } else { p.muted },
    );
    let close_rect = Rect::from_min_size(
        pos2(rect.right() - 8.0 - CLOSE, rect.center().y - CLOSE / 2.0),
        vec2(CLOSE, CLOSE),
    );
    // Edited documents show a dot in place of the close button until the
    // pointer is over the tab.
    let closed = if edited && !hot {
        painter.circle_filled(close_rect.center(), 4.0, p.accent_strong);
        false
    } else {
        ui.scope_builder(egui::UiBuilder::new().max_rect(close_rect), |ui| {
            Btn::icon(Icon::Close)
                .height(CLOSE)
                .tip("Close tab")
                .show(ui, p)
                .clicked()
        })
        .inner
    };
    (response.clicked(), closed)
}

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
    let transform=doc.pages.get(edit.page).and_then(|page|page.objects.as_ref()).map(|objects|objects.transform);
    if let Some(transform)=transform
        && let Some((plane,size))=affine_edit::line_plane(&edit.line,&transform,page_rect,scale)
        && (plane.x.y.abs()>0.001 || plane.y.x.abs()>0.001 || plane.x.x<0.0 || plane.y.y<0.0) {
        let [r,g,b,alpha]=first.color;
        let color=if alpha<32 {Color32::BLACK}else{Color32::from_rgb(r,g,b)};
        let [a,b,c,d,_,_]=first.matrix;
        let font=FontId::proportional((first.size*(a*d-b*c).abs().sqrt()*scale).clamp(6.,240.));
        return affine_edit::show(ui,edit,Id::new(("edit-box",edit.page,first.index,first.revision)),(plane,size),font,color,p);
    }

    // Match the visible glyph size: the object's font size scaled by its
    // matrix. Bounds are glyph-tight (cap height without descenders is about
    // 0.7 em), so they only serve as a fallback when the content stream adds
    // its own scaling.
    let [a, b, c, d, _, _] = first.matrix;
    let from_matrix = first.size * (a * d - b * c).abs().sqrt() * scale;
    let height = screen.height().max(4.0);
    let font_px = if edit.line.paragraph.is_some()
        || (from_matrix > height * 0.6 && from_matrix < height * 2.4)
    {
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
    if edit.line.paragraph.is_none() {
        rect.max.x = rect.max.x.max(rect.min.x + text_width + 10.0);
    }
    crate::paragraph::hint(
        ui,
        rect,
        edit.line.paragraph.as_ref(),
        edit.line.fallback.as_deref(),
        p.muted,
    );

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

    if !edit.focus
        && edit.line.paragraph.is_some()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Enter))
    {
        return Some(true);
    }
    if !edit.focus && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        return Some(false);
    }
    let id = Id::new(("edit-box", edit.page, first.index, first.revision));
    let spelling = crate::spelling::prepare(ui, id, &edit.text);
    let spell_font = font.clone();
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, width: f32| {
        spelling.layout(ui, text, width, &spell_font, color)
    };
    let editor = crate::paragraph::text_edit(&mut edit.text, edit.line.paragraph.is_some())
        .layouter(&mut layouter)
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
    spelling.menu(&response, &mut edit.text);
    if edit.focus {
        response.request_focus();
        edit.focus = false;
        return None;
    }
    let clicked_away = response.clicked_elsewhere()
        && ui.input(|i| i.pointer.primary_clicked());
    if (response.lost_focus() || clicked_away)
        && edit.rejected_text.as_ref() != Some(&edit.text)
        && !response.context_menu_opened()
        && !egui::Popup::is_any_open(ui.ctx())
    {
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
    let objects: Vec<_> = items.iter().map(|(o, _)| o.clone()).collect();
    let rectangles: std::collections::HashMap<_, _> =
        items.iter().map(|(o, r)| (o.index, *r)).collect();
    let paragraphs = pdf_editor_core::paragraphs(&objects);
    let membership: std::collections::HashMap<_, _> = paragraphs
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p.objects.iter().map(move |o| (o.index, i)))
        .collect();
    let mut emitted = std::collections::HashSet::new();
    let mut result = Vec::new();
    for mut line in group_lines_fallback(items) {
        let paragraph = line
            .objects
            .iter()
            .find_map(|o| membership.get(&o.index))
            .map(|i| (*i, &paragraphs[*i]));
        if let Some((i, p)) = paragraph {
            if p.fallback.is_none() && p.line_count >= 2 {
                if emitted.insert(i) {
                    let selected = p
                        .objects
                        .iter()
                        .filter_map(|o| rectangles.get(&o.index).map(|r| (o.clone(), *r)))
                        .collect();
                    let mut block = line_from(selected);
                    block.text = p.text.clone();
                    block.paragraph = Some(p.clone());
                    result.push(block);
                }
                continue;
            }
            line.fallback = p.fallback.clone();
        }
        result.push(line);
    }
    result
}
fn group_lines_fallback(items: Vec<(TextObject, Rect)>) -> Vec<Line> {
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
        paragraph: None,
        fallback: None,
        objects: group.into_iter().map(|(o, _)| o).collect(),
        rect,
        text,
    }
}

fn raster_rect(transform: &PageTransform, object: &TextObject) -> Rect {
    bounds_rect(transform, object.bounds)
}

fn resize_cursor(handle: usize) -> CursorIcon {
    match handle {
        1 | 5 => CursorIcon::ResizeVertical,
        3 | 7 => CursorIcon::ResizeHorizontal,
        2 | 6 => CursorIcon::ResizeNeSw,
        _ => CursorIcon::ResizeNwSe,
    }
}

fn resize_handles(bounds: Rect) -> [Pos2; 8] {
    [
        bounds.left_top(),
        bounds.center_top(),
        bounds.right_top(),
        bounds.right_center(),
        bounds.right_bottom(),
        bounds.center_bottom(),
        bounds.left_bottom(),
        bounds.left_center(),
    ]
}

fn resized_rect(mut bounds: Rect, handle: usize, delta: Vec2) -> Rect {
    if matches!(handle, 0 | 6 | 7) {
        bounds.min.x = (bounds.min.x + delta.x).min(bounds.max.x - 1.0);
    }
    if matches!(handle, 2..=4) {
        bounds.max.x = (bounds.max.x + delta.x).max(bounds.min.x + 1.0);
    }
    if matches!(handle, 0..=2) {
        bounds.min.y = (bounds.min.y + delta.y).min(bounds.max.y - 1.0);
    }
    if matches!(handle, 4..=6) {
        bounds.max.y = (bounds.max.y + delta.y).max(bounds.min.y + 1.0);
    }
    bounds
}

fn raster_bounds(transform: &PageTransform, bounds: Rect) -> Option<pdf_editor_core::Bounds> {
    let mut result = Rect::NOTHING;
    for corner in [
        bounds.left_top(),
        bounds.right_top(),
        bounds.right_bottom(),
        bounds.left_bottom(),
    ] {
        let (x, y) = transform.inverse(corner.x, corner.y)?;
        result.extend_with(pos2(x, y));
    }
    Some(pdf_editor_core::Bounds {
        left: result.min.x,
        bottom: result.min.y,
        right: result.max.x,
        top: result.max.y,
    })
}

fn raster_delta(transform: &PageTransform, delta: Vec2) -> Option<(f32, f32)> {
    let origin = transform.inverse(0.0, 0.0)?;
    let end = transform.inverse(delta.x, delta.y)?;
    Some((end.0 - origin.0, end.1 - origin.1))
}

fn bounds_rect(transform: &PageTransform, b: pdf_editor_core::Bounds) -> Rect {
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
    include!("workflow_tests.rs");
    include!("qol_workflow_tests.rs");
    include!("recovery_safety_tests.rs");
    include!("comparison_workflow_tests.rs");
    include!("sanitize_export_workflow_tests.rs");
    include!("tile_workflow_tests.rs");
    include!("rotated_workflow_tests.rs");
    include!("action_workflow_tests.rs");
    include!("drag_workflow_tests.rs");
    include!("font_readout_workflow_tests.rs");
    #[cfg(windows)]
    mod native_acceptance_tests { include!("native_acceptance_tests.rs"); }
    use super::*;

    #[test]
    fn error_banners_dismiss_expire_and_remain_bounded() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _, _) = mock_app();
        let frame = |app: &mut EditorApp, time: f64, events| {
            let mut output = ctx.run_ui(egui::RawInput {
                time: Some(time),
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
                ..Default::default()
            }, |ui| { app.banners(ui.ctx(), 40.0, &theme::palette(ui)); });
            output.textures_delta.clear();
        };
        for _ in 0..100 { app.banner(BannerKind::Error, "Repeated error with enough text to wrap across the available banner width. ".repeat(4)); }
        assert_eq!(app.banners.len(), 1);
        frame(&mut app, 1.0, vec![]);
        frame(&mut app, 1.1, vec![]);
        let key = format!("dismiss-banner-{}", app.banners[0].id);
        let at = tool_center(&ctx, &key);
        let events = click_at(at);
        frame(&mut app, 1.2, events[..2].to_vec());
        frame(&mut app, 1.3, events[2..].to_vec());
        assert!(app.banners.is_empty(), "Dismiss must remove even a long banner: {:?}", ctx.read_response(widgets::tool_id(&key)));
        for n in 0..100 { app.banner(BannerKind::Error, format!("Error {n}")); }
        assert_eq!(app.banners.len(), 4);
        frame(&mut app, 2.0, vec![]);
        frame(&mut app, 15.0, vec![]);
        assert!(app.banners.is_empty(), "Errors must expire without interaction");
    }

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
            quad: [[0.;2];4],
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
                qol: Default::default(),
            surface: Default::default(),
                tabbed: false,
                new_tab_path: None,
            tools_open: true,
            tools_query: String::new(),
            tools_focus: false,
            tools_category: String::new(),
            tools_selected: 0,
            snap_enabled: true,
            drag_preview: Default::default(),
            page_jump: crate::navigation::PageJump::default(),
            picker: crate::dialogs::Picker::default(),
            insert_dialog: Default::default(),
            picker_save: false,
            pending_action: None,
            pending_comparison: None,
            compare_origin: None,
                recent_files: vec![],
                dragging_page: None,
                text_tool: TextTool::default(),
                image_create: Default::default(),
                scan_cleanup: Default::default(),
                signatures: Default::default(),
                forms: Default::default(),
                replace: Default::default(),
                password: Default::default(),
                organize: Default::default(),
                markup: Default::default(),
                pictures: Default::default(),
                privacy: Default::default(),
                redaction: Default::default(),
                compression: Default::default(),
                sanitize: Default::default(),
                compare: Default::default(),
                stamps: Default::default(),
                export: Default::default(),
                tables: Default::default(),
                word: Default::default(),
                crop: Default::default(),
                drawing: Default::default(),
                bookmarks: Default::default(),
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
                integration: Default::default(),
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
    #[test]
    fn sanitize_menu_opens_the_dialog() {
        assert!(
            Tool::MENU
                .iter()
                .any(|(_, tools)| tools.iter().any(|(tool, _, _)| *tool == Tool::Sanitize))
        );
        let (mut app, _, _) = mock_app();
        app.open_tool(Tool::Sanitize);
        assert!(app.sanitize.open);
    }
    fn opened(app: &mut EditorApp, ctx: &Context) {
        app.handle(
            ctx,
            Reply::Opened {
                protected: false,
                can_undo: false, can_redo: false,
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
    fn thousand_page_view_requests_only_nearby_rasters_and_thumbnails() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, inbox, _outbox) = mock_app();
        app.handle(
            &ctx,
            Reply::Opened {
                protected: false,
                can_undo: false, can_redo: false,
                generation: 1,
                path: "generated.pdf".into(),
                name: "generated.pdf".into(),
                suggested: "generated_edited.pdf".into(),
                pages: vec![
                    PageInfo {
                        width: 612.0,
                        height: 792.0
                    };
                    1000
                ],
            },
        );
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1180.0, 820.0))),
                ..Default::default()
            },
            |ui| {
                let p = theme::palette(ui);
                egui::Panel::left("lazy-thumbnails")
                    .exact_size(200.0)
                    .show(ui, |ui| app.thumbnails(ui, &p));
                egui::CentralPanel::default().show(ui, |ui| app.canvas(ui, &p));
            },
        );
        output.textures_delta.clear();
        let requests: Vec<_> = inbox.try_iter().collect();
        let renders = requests
            .iter()
            .filter(|r| matches!(r, Request::Render { .. }))
            .count();
        let thumbnails = requests
            .iter()
            .filter(|r| matches!(r, Request::Thumbnail(_)))
            .count();
        assert!((1..=8).contains(&renders), "render requests: {renders}");
        assert!(
            (1..=12).contains(&thumbnails),
            "thumbnail requests: {thumbnails}"
        );
    }

    #[test]
    fn page_cache_accounts_for_incoming_and_replaced_textures() {
        let ctx = Context::default();
        let (mut app, _inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        let pages = &mut app.doc.as_mut().unwrap().pages;
        for (i, page) in pages.iter_mut().enumerate() {
            page.texture = Some(PageTexture {
                handle: ctx.load_texture(
                    format!("budget-{i}"),
                    egui::ColorImage::filled([4, 4], Color32::WHITE),
                    TextureOptions::LINEAR,
                ),
                scale: 1.0,
                generation: 1,
            });
        }
        // Replacing page zero costs 64 bytes, not 128; its neighbour fits.
        trim_page_cache_to(pages, 0, 64, 128);
        assert!(pages[0].texture.is_none());
        assert!(pages[1].texture.is_some());
        // A larger incoming raster evicts its neighbour before insertion.
        trim_page_cache_to(pages, 0, 128, 128);
        assert!(pages.iter().all(|page| page.texture.is_none()));
    }

    #[test]
    fn superseded_display_raster_keeps_the_preview_and_latest_request() {
        let ctx = Context::default();
        let (mut app, _, _) = mock_app();
        opened(&mut app, &ctx);
        let image = || pdf_editor_core::Image { width: 2, height: 2, rgba: vec![255; 16] };
        app.handle(&ctx, Reply::Rendered { generation: 1, page: 0, scale: 1.0, image: image() });
        let original = app.doc.as_ref().unwrap().pages[0].texture.as_ref().unwrap().handle.id();
        app.doc.as_mut().unwrap().pages[0].pending_render = Some((2.0, 1));
        app.handle(&ctx, Reply::Rendered { generation: 1, page: 0, scale: 1.5, image: image() });
        let page = &app.doc.as_ref().unwrap().pages[0];
        assert_eq!(page.texture.as_ref().unwrap().handle.id(), original);
        assert_eq!(page.pending_render, Some((2.0, 1)));
        app.handle(&ctx, Reply::Rendered { generation: 1, page: 0, scale: 2.0, image: image() });
        let page = &app.doc.as_ref().unwrap().pages[0];
        assert_eq!(page.pending_render, None);
        assert_eq!(page.texture.as_ref().unwrap().scale, 2.0);
    }

    #[test]
    fn content_refresh_keeps_previews_and_position_but_rejects_stale_rasters() {
        let ctx = Context::default();
        let (mut app, _inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        let image = || pdf_editor_core::Image {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        };
        app.handle(
            &ctx,
            Reply::Rendered {
                generation: 1,
                page: 0,
                scale: 1.0,
                image: image(),
            },
        );
        app.handle(
            &ctx,
            Reply::Thumbnail {
                generation: 1,
                page: 0,
                image: image(),
            },
        );
        let doc = app.doc.as_mut().unwrap();
        doc.offset = vec2(18.0, 90.0);
        doc.scroll_to = None;
        let old = doc.pages[0].texture.as_ref().unwrap().handle.id();
        app.find.query = "match".into();
        app.handle(
            &ctx,
            Reply::Changed {
                pages: vec![],
                generation: 2,
                kind: ChangeKind::Edit,
                can_undo: true,
                can_redo: false,
                warning: None,
                objects_changed: 1,
            },
        );
        let doc = app.doc.as_ref().unwrap();
        assert_eq!(doc.pages[0].texture.as_ref().unwrap().handle.id(), old);
        assert!(doc.pages[0].thumbnail.is_some());
        assert_eq!(doc.pages[0].thumbnail_generation, None);
        assert_eq!(doc.offset, vec2(18.0, 90.0));
        assert!(doc.pages[0].objects.is_none());
        app.handle(
            &ctx,
            Reply::Rendered {
                generation: 1,
                page: 0,
                scale: 1.0,
                image: image(),
            },
        );
        assert_eq!(
            app.doc.as_ref().unwrap().pages[0]
                .texture
                .as_ref()
                .unwrap()
                .handle
                .id(),
            old
        );
        app.handle(
            &ctx,
            Reply::Found {
                generation: 2,
                query: "match".into(),
                matches: vec![(1, vec![[0.0, 0.0, 10.0, 10.0]])],
            },
        );
        assert!(app.doc.as_ref().unwrap().scroll_to.is_none());
        app.handle(
            &ctx,
            Reply::Rendered {
                generation: 2,
                page: 0,
                scale: 1.0,
                image: image(),
            },
        );
        assert_eq!(
            app.doc.as_ref().unwrap().pages[0]
                .texture
                .as_ref()
                .unwrap()
                .generation,
            2
        );
    }

    #[test]
    fn resize_handles_keep_opposite_edges_and_map_rotation() {
        let bounds = Rect::from_min_max(pos2(10.0, 20.0), pos2(80.0, 90.0));
        let resized = resized_rect(bounds, 0, vec2(1000.0, 1000.0));
        assert_eq!(resized.size(), vec2(1.0, 1.0));
        assert_eq!(resized.max, bounds.max);
        let mapped = raster_bounds(
            &PageTransform::for_test([0.0, 1.0, -1.0, 0.0, 600.0, 20.0]),
            bounds,
        )
        .unwrap();
        assert_eq!(mapped.left, 0.0);
        assert_eq!(mapped.right, 70.0);
        assert_eq!(mapped.bottom, 520.0);
        assert_eq!(mapped.top, 590.0);
    }

    #[test]
    fn drag_delta_respects_rotated_page_coordinates() {
        assert_eq!(
            raster_delta(
                &PageTransform::for_test([0.0, 1.0, -1.0, 0.0, 600.0, 20.0]),
                vec2(12.0, 8.0)
            ),
            Some((8.0, -12.0))
        );
        assert_eq!(
            raster_delta(
                &PageTransform::for_test([1.0, 0.0, 0.0, -1.0, 0.0, 800.0]),
                vec2(12.0, 8.0)
            ),
            Some((12.0, -8.0))
        );
    }

    #[test]
    fn canvas_click_edits_grouped_text_and_drag_moves_resizes_or_pans() {
        fn frame(app: &mut EditorApp, ctx: &Context, events: Vec<egui::Event>) {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let p = theme::palette(ui);
                    egui::CentralPanel::default().show(ui, |ui| app.canvas(ui, &p));
                },
            );
            output.textures_delta.clear();
        }
        fn button(pos: Pos2, pressed: bool) -> egui::Event {
            egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            }
        }
        for object_mode in [false, true] {
            let ctx = Context::default();
            theme::install(&ctx);
            let (mut app, inbox, _outbox) = mock_app();
            app.snap_enabled = false; // Verify raw drag geometry independently of snapping.
            opened(&mut app, &ctx);
            app.doc.as_mut().unwrap().object_mode = object_mode;
            let mut text = glyph("drag", 20.0, 40.0).0;
            text.index = 0;
            text.bounds = pdf_editor_core::Bounds {
                left: 20.0,
                bottom: 20.0,
                right: 80.0,
                top: 70.0,
            };
            app.handle(
                &ctx,
                Reply::Objects {
                    generation: 1,
                    page: 0,
                    transform: PageTransform::for_test([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
                    objects: vec![text],
                    items: vec![pdf_editor_core::PageItem {
                        page: 0,
                        index: 0,
                        revision: 1,
                        kind: "Path".into(),
                        bounds: pdf_editor_core::Bounds {
                            left: 20.0,
                            bottom: 20.0,
                            right: 80.0,
                            top: 70.0,
                        },
                    }],
                },
            );
            for _ in 0..3 {
                frame(&mut app, &ctx, vec![]);
            }
            inbox.try_iter().for_each(drop);
            let rect = ctx
                .read_response(Id::new(("pdf-page", 0usize)))
                .unwrap()
                .rect;
            let start = rect.min + vec2(40.0, 40.0) * PT_TO_UI;
            if !object_mode {
                let objects = app.doc.as_mut().unwrap().pages[0].objects.as_mut().unwrap();
                let lines = std::mem::take(&mut objects.lines);
                objects.items[0].kind = "XObjectForm".into();
                frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(start), button(start, true)],
                );
                frame(&mut app, &ctx, vec![button(start, false)]);
                frame(&mut app, &ctx, vec![button(start, true)]);
                frame(&mut app, &ctx, vec![button(start, false)]);
                assert!(
                    inbox
                        .try_iter()
                        .any(|r| matches!(r, Request::PrepareGroupedText(_)))
                );
                assert!(app.doc.as_ref().unwrap().reopen.is_some());
                assert_eq!(app.busy, 1);
                // The worker refresh exposes the text at the original click.
                let objects = app.doc.as_mut().unwrap().pages[0].objects.as_mut().unwrap();
                objects.lines = lines;
                objects.items[0].kind = "Text".into();
                app.busy = 0;
                frame(&mut app, &ctx, vec![]);
                assert!(app.doc.as_ref().unwrap().edit.is_some());
                assert!(app.doc.as_ref().unwrap().reopen.is_none());
                app.set_object_mode(true);
                frame(&mut app, &ctx, vec![]);
                assert!(app.doc.as_ref().unwrap().selected_item.is_some());
                assert!(app.doc.as_ref().unwrap().edit.is_none());
                app.set_object_mode(false);
                frame(&mut app, &ctx, vec![]);
                frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(start), button(start, true)],
                );
                frame(&mut app, &ctx, vec![button(start, false)]);
                frame(&mut app, &ctx, vec![button(start, true)]);
                frame(&mut app, &ctx, vec![button(start, false)]);
                assert!(
                    app.doc.as_ref().unwrap().edit.is_some(),
                    "A text double-click must open the editor"
                );
                frame(&mut app, &ctx, vec![egui::Event::Text("updated".into())]);
                assert!(
                    app.doc
                        .as_ref()
                        .unwrap()
                        .edit
                        .as_ref()
                        .unwrap()
                        .text
                        .contains("updated")
                );
                frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::Key {
                        key: Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert!(inbox.try_iter().any(|request| matches!(request,
                    Request::Edit { text, .. } if text.contains("updated")
                )));
                app.busy = 0;
                app.cancel_edit();
                frame(&mut app, &ctx, vec![]);
            }
            frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(start), button(start, true)],
            );
            let end = start + vec2(90.0, 15.0);
            frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
            frame(&mut app, &ctx, vec![button(end, false)]);
            let moves: Vec<_> = inbox
                .try_iter()
                .filter_map(|r| match r {
                    Request::EditItems(items, pdf_editor_core::ObjectAction::Move { dx, dy }) => {
                        Some((items, dx, dy))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(moves.len(), 1);
            assert_eq!(moves[0].0[0].index, 0);
            assert!((moves[0].1 - 90.0 / PT_TO_UI).abs() < 0.01);
            app.busy = 0;
            let blank = rect.min + vec2(160.0, 180.0) * PT_TO_UI;
            frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(blank), button(blank, true)],
            );
            let end = blank - vec2(0.0, 70.0);
            frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
            frame(&mut app, &ctx, vec![button(end, false)]);
            for _ in 0..4 {
                frame(&mut app, &ctx, vec![]);
            }
            assert!(app.doc.as_ref().unwrap().offset.y > 0.0);
            assert!(
                !inbox
                    .try_iter()
                    .any(|r| matches!(r, Request::EditItems(..)))
            );
            app.doc.as_mut().unwrap().object_mode = true;
            let item = app.doc.as_ref().unwrap().pages[0]
                .objects
                .as_ref()
                .unwrap()
                .items[0]
                .clone();
            app.doc.as_mut().unwrap().selected_item = Some(item);
            app.doc.as_mut().unwrap().scroll_to = Some(Vec2::ZERO);
            // Each handle previews without dispatching mutations until release.
            for matrix in [
                [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                [0.0, 1.0, -1.0, 0.0, 100.0, 0.0],
                [-1.0, 0.0, 0.0, -1.0, 100.0, 100.0],
                [0.0, -1.0, 1.0, 0.0, 0.0, 100.0],
            ] {
                for zoom in [0.75, 1.5] {
                    let transform = PageTransform::for_test(matrix);
                    let doc = app.doc.as_mut().unwrap();
                    doc.zoom = zoom;
                    doc.scroll_to = Some(Vec2::ZERO);
                    doc.pages[0].objects.as_mut().unwrap().transform = transform;
                    let scale = doc.scale();
                    let bounds =
                        bounds_rect(&transform, doc.selected_item.as_ref().unwrap().bounds);
                    for handle in 0..8 {
                        for _ in 0..2 {
                            frame(&mut app, &ctx, vec![]);
                        }
                        inbox.try_iter().for_each(drop);
                        let rect = ctx
                            .read_response(Id::new(("pdf-page", 0usize)))
                            .unwrap()
                            .rect;
                        let start = rect.min + resize_handles(bounds)[handle].to_vec2() * scale;
                        frame(
                            &mut app,
                            &ctx,
                            vec![egui::Event::PointerMoved(start), button(start, true)],
                        );
                        let end = start + vec2(12.0, 12.0);
                        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
                        assert!(
                            !inbox
                                .try_iter()
                                .any(|r| matches!(r, Request::EditItems(..)))
                        );
                        frame(&mut app, &ctx, vec![button(end, false)]);
                        let edits: Vec<_> = inbox
                            .try_iter()
                            .filter_map(|r| match r {
                                Request::EditItems(
                                    _,
                                    pdf_editor_core::ObjectAction::Resize { bounds },
                                ) => Some(bounds),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(edits.len(), 1, "handle {handle}");
                        let expected = raster_bounds(
                            &transform,
                            resized_rect(bounds, handle, vec2(12.0, 12.0) / scale),
                        )
                        .unwrap();
                        assert!((edits[0].left - expected.left).abs() < 0.1);
                        assert!((edits[0].top - expected.top).abs() < 0.1);
                        app.busy = 0;
                    }
                }
            }
            ctx.memory_mut(|m| m.request_focus(Id::new(("pdf-page", 0usize))));
            frame(&mut app, &ctx, vec![egui::Event::Copy]);
            assert!(inbox.try_iter().any(|r| matches!(r, Request::CopyItems(_))));
            app.handle(&ctx, Reply::ItemsCopied);
            frame(
                &mut app,
                &ctx,
                vec![egui::Event::Paste("ignored system clipboard".into())],
            );
            assert!(
                inbox
                    .try_iter()
                    .any(|r| matches!(r, Request::PasteItems(_)))
            );
        }
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
        app.text_tool.selection = vec![glyph("chosen", 10.0, 50.0).0];
        tool_frame(&mut app, &ctx, vec![], 1);
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
    fn find_enter_cycles_matches_without_losing_field_focus() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _, _) = mock_app();
        opened(&mut app, &ctx);
        app.find.open = true;
        app.find.focus = true;
        app.find.query = "match".into();
        app.find.matches = vec![
            (0, vec![[0.0, 0.0, 10.0, 10.0]]),
            (1, vec![[0.0, 0.0, 10.0, 10.0]]),
        ];
        tool_frame(&mut app, &ctx, vec![], 2);
        for (shift, expected) in [(false,1),(false,0),(true,1)] {
            tool_frame(&mut app, &ctx, vec![egui::Event::Key {
                key: Key::Enter, physical_key: None, pressed: true, repeat: false,
                modifiers: egui::Modifiers { shift, ..Default::default() },
            }], 2);
            assert_eq!(app.find.active, expected);
            assert!(ctx.memory(|m| m.has_focus(Id::new("find-field"))));
            assert_eq!(app.doc.as_ref().unwrap().explicit_page, Some(expected));
            tool_frame(&mut app, &ctx, vec![], 2);
        }
    }
    #[test]
    fn close_confirmation_can_cancel_while_operations_are_pending() {
        for busy in [false,true] {
            let ctx=Context::default();
            theme::install(&ctx);
            let mut choice=None;
            let mut frame=|events| {
                let mut output=ctx.run_ui(egui::RawInput {events,
                    screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(900.0,700.0))),
                    ..Default::default()},|ui| {
                    let p=theme::palette(ui);
                    choice=close_tabs_prompt(&ctx,&p,false,busy);
                });
                output.textures_delta.clear();
            };
            for _ in 0..5 {frame(vec![]);}
            let cancel=ctx.read_response(widgets::tool_id("close-tabs-cancel")).unwrap();
            assert!(cancel.enabled(), "busy={busy}, sense={:?}, rect={:?}",cancel.sense,cancel.rect);
            let discard=ctx.read_response(widgets::tool_id("close-tabs-discard")).unwrap();
            assert_eq!(discard.sense == Sense::click(),!busy);
            frame(click_at(cancel.rect.center()));
            assert_eq!(choice,Some(false));
        }
    }
    #[test]
    fn page_insertion_destinations_account_for_removed_source() {
        assert_eq!(reorder_destination(0,2,false,4),1);
        assert_eq!(reorder_destination(0,2,true,4),2);
        assert_eq!(reorder_destination(3,1,false,4),1);
        assert_eq!(reorder_destination(3,1,true,4),2);
        assert_eq!(reorder_destination(1,1,false,4),1);
        assert_eq!(reorder_destination(1,1,true,4),1);
    }
    #[test]
    fn thumbnail_drag_dispatches_page_move() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        for (index, page) in app.doc.as_mut().unwrap().pages.iter_mut().enumerate() {
            page.thumbnail_generation = Some(1);
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
            output.textures_delta.clear();
            for key in [
                "open",
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
            for key in ["all-tools", "recent", "compare-visible"] {
                let rect = ctx.read_response(widgets::tool_id(key)).unwrap().rect;
                assert!(rect.min.x >= 0.0 && rect.max.x <= width, "{key}: {rect:?}");
            }
            if width > 1120.0 {
                // Wide windows label the main actions.
                for label in ["Open", "Save", "Undo", "Redo", "Add text", "Find"] {
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
    #[test]
    fn tab_requests_preserve_document_and_warn_for_background_edits() {
        let ctx = Context::default();
        let (mut first, requests, _) = mock_app();
        let (mut second, _, _) = mock_app();
        opened(&mut first, &ctx);
        opened(&mut second, &ctx);
        first.tabbed = true;
        second.tabbed = true;
        first.doc.as_mut().unwrap().changes.edited();
        first.request_open(Some(PathBuf::from("next.pdf")));
        assert_eq!(first.new_tab_path.take(), Some(PathBuf::from("next.pdf")));
        assert!(first.doc.as_ref().unwrap().dirty());
        assert!(first.confirm.is_none());
        assert!(requests.try_recv().is_err());
        let mut tabs = TabbedApp {
            sessions: Default::default(),
            native_guard: None,
            clipboard: Default::default(),
            tabs: vec![(0, first), (1, second)],
            active: 1,
            next_id: 2,
            closing: None,
            allow_exit: false,
            instance: None,
            merge_tab: None,
        };
        assert!(TabbedApp::needs_warning(&tabs.tabs[0].1));
        assert!(!TabbedApp::needs_warning(&tabs.tabs[1].1));
        tabs.remove(1, &ctx);
        assert_eq!(tabs.tabs.len(), 1);
        assert_eq!(tabs.active, 0);
        assert!(tabs.tabs[0].1.doc.as_ref().unwrap().dirty());
    }
    #[test]
    fn page_jump_scrolls_without_changing_document() {
        let ctx = Context::default();
        let (mut app, _, _) = mock_app();
        opened(&mut app, &ctx);
        let dirty = app.doc.as_ref().unwrap().dirty();
        app.doc.as_mut().unwrap().scroll_to = None;
        app.go_to_page(999);
        assert!(app.doc.as_ref().unwrap().scroll_to.is_none());
        app.go_to_page(0);
        assert_eq!(app.doc.as_ref().unwrap().scroll_to.unwrap().y, CANVAS_PAD);
        assert_eq!(app.doc.as_ref().unwrap().dirty(), dirty);
    }

    #[test]
    fn click_away_applies_once_and_failed_edit_keeps_the_draft() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, inbox, _) = mock_app();
        opened(&mut app, &ctx);
        let (object, rect) = glyph("original", 30.0, 40.0);
        app.doc.as_mut().unwrap().edit = Some(EditState {
            rejected_text: None,
            page: 0, text: "original".into(), focus: true,
            line: Line { paragraph: None, fallback: None, objects: vec![object], rect, text: "original".into() },
        });
        let frame = |app: &mut EditorApp, events| {
            let mut outcome = None;
            let mut output = ctx.run_ui(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events, ..Default::default()
            }, |ui| {
                let p = theme::palette(ui);
                outcome = edit_box(ui, app.doc.as_mut().unwrap(),
                    &[Rect::from_min_size(pos2(20.0, 20.0), vec2(500.0, 500.0))], 1.0, &p);
            });
            output.textures_delta.clear();
            outcome
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(&mut app, vec![egui::Event::Text(" draft".into())]);
        let draft = app.doc.as_ref().unwrap().edit.as_ref().unwrap().text.clone();
        assert!(draft.contains("draft"));
        let at = pos2(650.0, 450.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at, pressed, button: egui::PointerButton::Primary, modifiers: egui::Modifiers::NONE,
        };
        let down = frame(&mut app, vec![egui::Event::PointerMoved(at), button(true)]);
        let up = frame(&mut app, vec![button(false)]);
        assert!(down == Some(true) || up == Some(true));
        assert!(app.commit_edit());
        assert!(!app.commit_edit());
        assert_eq!(inbox.try_iter().filter(|r| matches!(r, Request::Edit { .. })).count(), 1);
        app.handle(&ctx, Reply::Failed { message: "Unsupported test font".into(), blocking: true, page: None });
        assert_eq!(app.doc.as_ref().unwrap().edit.as_ref().unwrap().text, draft);
        assert!(app.doc.as_ref().unwrap().pending_edit.is_none());
        assert_eq!(app.busy, 0);
        // Opening a modal or repeatedly clicking elsewhere cannot resubmit
        // the same unsupported draft or keep the worker permanently busy.
        for _ in 0..8 {
            assert!(!app.commit_edit());
            assert_eq!(app.busy, 0);
        }
        assert!(inbox.try_recv().is_err());
        assert_eq!(app.doc.as_ref().unwrap().edit.as_ref().unwrap().text, draft);
        frame(&mut app, vec![]);
        let escape = frame(&mut app, vec![egui::Event::Key {
            key: Key::Escape, physical_key: None, pressed: true, repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(escape, Some(false));
        app.cancel_edit();
        assert!(!app.commit_edit());
    }

    #[test]
    fn save_waits_for_edit_success_and_failure_preserves_the_draft() {
        for encrypted in [false, true] {
            for succeeds in [false, true] {
                let ctx = Context::default();
                let (mut app, inbox, outbox) = mock_app();
                opened(&mut app, &ctx);
                inbox.try_iter().for_each(drop);
                let (object, rect) = glyph("original", 30.0, 40.0);
                app.doc.as_mut().unwrap().edit = Some(EditState {
                    rejected_text: None,
                    page: 0, text: "new draft".into(), focus: true,
                    line: Line { paragraph: None, fallback: None, objects: vec![object], rect, text: "original".into() },
                });
                let request = if encrypted {
                    Request::SavePassword("generated-copy.pdf".into(), "generated-password".into())
                } else {
                    Request::SaveAs("generated-copy.pdf".into())
                };
                app.document_action(request);
                assert!(matches!(inbox.try_recv().unwrap(), Request::Edit { .. }));
                assert!(inbox.try_recv().is_err(), "Save must wait for the edit reply");
                assert!(app.pending_action.is_some());
                app.poll(&ctx);
                assert!(inbox.try_recv().is_err());
                let reply = if succeeds {
                    Reply::Changed {
                        pages: vec![PageInfo { width: 300.0, height: 400.0 }],
                        generation: 2, kind: ChangeKind::Edit, can_undo: true,
                        can_redo: false, warning: None, objects_changed: 1,
                    }
                } else {
                    Reply::Failed { message: "Generated edit rejection".into(), blocking: true, page: None }
                };
                outbox.send(reply).unwrap();
                app.poll(&ctx);
                assert!(app.pending_action.is_none());
                if succeeds {
                    let saved = inbox.try_recv().unwrap();
                    assert!(if encrypted { matches!(saved, Request::SavePassword(..)) }
                        else { matches!(saved, Request::SaveAs(_)) });
                    assert_eq!(app.busy, 1);
                } else {
                    assert!(inbox.try_recv().is_err(), "Rejected edits must not write a file");
                    assert_eq!(app.doc.as_ref().unwrap().edit.as_ref().unwrap().text, "new draft");
                    assert!(app.banners.iter().any(|b| b.text.starts_with("Cancelled because")));
                    assert_eq!(app.busy, 0);
                    app.document_action(Request::SaveAs("retry-copy.pdf".into()));
                    assert!(inbox.try_recv().is_err(),"Retrying Save must not silently omit a rejected draft");
                    assert!(app.pending_action.is_none());
                }
                app.poll(&ctx);
                assert!(inbox.try_recv().is_err(), "Save must dispatch only once");
            }
        }
    }

    #[test]
    fn catalog_lists_every_tool_and_filters_descriptions() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _, _) = mock_app();
        opened(&mut app, &ctx);
        let draw = |app: &mut EditorApp| {
            let mut output = ctx.run_ui(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(360.0, 600.0))),
                ..Default::default()
            }, |ui| { let p = theme::palette(ui); app.tool_catalog(ui, &p); });
            output.textures_delta.clear();
        };
        for (_, tools) in Tool::MENU {
            for &(tool, label, _) in tools {
                app.tools_query = label.to_string();
                draw(&mut app); draw(&mut app);
                let response = ctx.read_response(widgets::tool_id(&format!("catalog-{}",tool.key()))).unwrap();
                assert!(response.enabled());
                assert!(response.rect.top() >= 0.0 && response.rect.bottom() < 600.0);
            }
        }
        assert!(Tool::Forms.matches("Forms", "flatten"));
        assert!(Tool::Tables.matches("Tables", "xlsx"));
        assert!(Tool::Privacy.matches("Search and redact", "IBAN"));
        assert!(!Tool::Print.matches("Print", "signature"));
        app.doc = None;
        app.tools_query = "Compare".into(); draw(&mut app);
        assert!(ctx.read_response(widgets::tool_id("catalog-tool-compare")).unwrap().enabled());
        app.tools_query = "Forms".into(); draw(&mut app);
        assert_eq!(ctx.read_response(widgets::tool_id("catalog-tool-forms")).unwrap().sense, Sense::hover());
    }

    #[test]
    fn visible_modes_and_compare_dispatch_from_toolbar() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _, _) = mock_app();
        opened(&mut app, &ctx);
        let frame = |app: &mut EditorApp, events| {
            let mut output = ctx.run_ui(egui::RawInput {
                events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 420.0))),
                ..Default::default()
            }, |ui| { let p = theme::palette(ui); egui::Panel::top("toolbar").show(ui, |ui| app.toolbar(ui, &p)); });
            output.textures_delta.clear();
        };
        frame(&mut app, vec![]); frame(&mut app, vec![]);
        for requested in [true, false] {
            app.drawing.active = true;
            app.tables.selecting = true;
            app.text_tool.adding = true;
            app.set_object_mode(requested);
            frame(&mut app, vec![]);
            assert!(app.doc.as_ref().unwrap().object_mode);
            assert!(!app.drawing.active && !app.tables.selecting && !app.text_tool.adding);
        }
        frame(&mut app, click_at(tool_center(&ctx, "compare-visible")));
        assert!(app.compare.open);
    }

    #[test]
    fn document_tools_scroll_and_dispatch_print_at_small_height() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        let frame = |app: &mut EditorApp, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 420.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let p = theme::palette(ui);
                    egui::Panel::top("toolbar").show(ui, |ui| app.toolbar(ui, &p));
                },
            );
            output.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(&mut app, click_at(tool_center(&ctx, "recent")));
        frame(&mut app, vec![]);
        let mut print = tool_center(&ctx, "print-document");
        for _ in 0..30 {
            if print.y > 80.0 && print.y < 310.0 {
                break;
            }
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos2(print.x, 180.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        phase: egui::TouchPhase::Move,
                        delta: vec2(0.0, -80.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            // Wheel scrolling animates; let it come to rest before measuring.
            for _ in 0..20 {
                frame(&mut app, vec![]);
            }
            print = tool_center(&ctx, "print-document");
        }
        assert!(
            print.y > 60.0 && print.y < 380.0,
            "Print is outside the viewport: {print:?}"
        );
        frame(&mut app, click_at(print));
        assert!(matches!(inbox.try_recv().unwrap(), Request::Print));
    }
    #[test]
    fn tool_windows_render_in_both_themes() {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        app.password.save(PathBuf::from("copy.pdf"));
        for dark in [false, true] {
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            app.signatures.open = true;
            app.forms.open = true;
            app.replace.open = true;
            app.organize.open = true;
            app.markup.open = true;
            app.pictures.open = true;
            app.redaction.open = true;
            app.privacy.open = true;
            app.compression.open = true;
            app.stamps.open = true;
            app.export.open = true;
            app.crop.open = true;
            app.drawing.open = true;
            app.bookmarks.open = true;
            let mut texts = Vec::new();
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 900.0))),
                        ..Default::default()
                    },
                    |ui| {
                        let p = theme::palette(ui);
                        let ctx = ui.ctx().clone();
                        egui::Panel::left("bookmarks").show(ui, |ui| {
                            assert!(app.bookmarks.show(ui, &p, 0, true).is_none())
                        });
                        app.drawing.show(&ctx, &p);
                        assert!(app.signatures.show(&ctx, &p, None, 1, true).is_none());
                        assert!(app.forms.show(&ctx, &p, true).is_none());
                        assert!(app.replace.show(&ctx, &p, true).is_none());
                        assert!(app.organize.show(&ctx, &p, true).is_none());
                        assert!(app.markup.show(&ctx, &p, true, 1).is_none());
                        assert!(app.pictures.show(&ctx, &p, true, 1).is_none());
                        assert!(app.redaction.show(&ctx, &p, true, 1).is_none());
                        assert!(app.privacy.show(&ctx, &p, true).is_none());
                        assert!(app.compression.show(&ctx, &p, true).is_none());
                        assert!(app.stamps.show(&ctx, &p, true).is_none());
                        assert!(app.export.show(&ctx, &p, true).is_none());
                        assert!(app.crop.show(&ctx, &p, true, 1).is_none());
                        assert!(app.password.show(&ctx, &p, true, ui.id()).is_none());
                    },
                );
                texts = output
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect();
                output.textures_delta.clear();
            }
            for title in [
                "Bookmarks",
                "Draw and shapes",
                "Signatures",
                "Forms",
                "Find and replace",
                "Merge, split and extract",
                "Markup and notes",
                "Images",
                "Secure redaction",
                "Search and redact",
                "Compress images",
                "Headers, footers and Bates",
                "Export page images",
                "Crop page",
                "Save options",
            ] {
                assert!(texts.iter().any(|t| t == title), "{title}: {texts:?}");
            }
        }
    }
}
