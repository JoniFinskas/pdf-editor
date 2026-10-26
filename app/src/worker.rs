//! Background thread that owns the PDF engine and the open document.
//!
//! Every core call runs here so large documents never block the UI thread.
//! Results carry the document generation they were computed for; the UI drops
//! replies whose generation no longer matches.

use pdf_editor_core::{Document, Engine, PageInfo, Substitution, TextObject};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{Receiver, Sender, SyncSender, TryRecvError, channel, sync_channel},
    },
    thread,
};

/// Raster coordinates are sampled at this scale and divided back down, which
/// keeps integer rounding in `page_to_pixel` far below a display pixel.
const SAMPLE_SCALE: f32 = 4.0;
const SAMPLE_SPAN: f32 = 1000.0;

pub enum Request {
    ObjectPreview { id: u64, items: Vec<pdf_editor_core::PageItem>, scale: f32 },
    RenderTiles {page:u16,scale:f32,regions:Vec<[u32;4]>},
    ComparisonSnapshot(u64),
    Characters(u16),
    FindOptions(String, pdf_editor_core::SearchOptions),
    ReplacementCountOptions(String, pdf_editor_core::SearchOptions),
    ReplaceOptions { query:String, text:String, occurrence:Option<usize>, options:pdf_editor_core::SearchOptions },
    FormatText(Vec<TextObject>, pdf_editor_core::TextFormat),
    PageBatch(pdf_editor_core::PageBatch),
    LayoutItems(Vec<pdf_editor_core::PageItem>, pdf_editor_core::ObjectLayout),
    HighlightText(Vec<(u16, pdf_editor_core::Bounds)>),
    SaveWorkingCopy,

    ScanPreview(u16, Option<pdf_editor_core::ScanOptions>),
    CleanupScan(u16, pdf_editor_core::ScanOptions),
    ImagesToPdf(Vec<PathBuf>, pdf_editor_core::ImagePageSize, PathBuf),
    CopyItemsPrepared(CopyJob),
    PasteItemsPrepared(u16, Option<Arc<CopySlot>>),
    Open(PathBuf),
    OpenRecovery { path: PathBuf, source: PathBuf },
    OpenPassword(PathBuf, String),
    SavePassword(PathBuf, String),
    Bookmarks,
    EditBookmarks(u64, pdf_editor_core::BookmarkEdit),
    Print,
    Draw(pdf_editor_core::Drawing),
    Crop(u16, pdf_editor_core::Bounds),
    DetectTable(u16, pdf_editor_core::Bounds),
    ExportWord(PathBuf),
    ExportTable(
        pdf_editor_core::TableGrid,
        pdf_editor_core::TableFormat,
        PathBuf,
    ),
    ExportImages(String, u16, pdf_editor_core::PageImageFormat, PathBuf),
    Stamp(pdf_editor_core::RangeStamp),
    Bates(pdf_editor_core::RangeStamp, pdf_editor_core::BatesNumber),
    Compress(u32, u8),
    Sanitize(pdf_editor_core::SanitizeOptions),
    ExportSanitized(pdf_editor_core::SanitizeOptions, PathBuf),

    PrivacyPreview(pdf_editor_core::PrivacyQuery),
    PrivacyApply(pdf_editor_core::RedactionPreview),
    Redact(u16, pdf_editor_core::Bounds),
    Ocr(u16),
    Pictures,
    MovePicture(pdf_editor_core::PlacedPicture, pdf_editor_core::Placement),
    Annotations,
    AddAnnotation(pdf_editor_core::Markup),
    UpdateAnnotation(pdf_editor_core::Annotation, pdf_editor_core::Markup),
    DeleteAnnotation(pdf_editor_core::Annotation),
    Merge(Vec<PathBuf>),
    Extract(String, PathBuf),
    Split(String, PathBuf),
    FormFields,
    DesignForm(pdf_editor_core::FormDesignEdit),
    FlattenForms,
    FillField(pdf_editor_core::FormField, pdf_editor_core::FieldValue),
    PlacePicture(pdf_editor_core::Picture, pdf_editor_core::Placement),
    Find(String),
    ReplacementCount(String),
    Replace {
        query: String,
        text: String,
        occurrence: Option<usize>,
    },
    Fonts,
    Thumbnail(u16),
    Pages(pdf_editor_core::PageEdit),
    DeleteText(TextObject),
    EditItems(
        Vec<pdf_editor_core::PageItem>,
        pdf_editor_core::ObjectAction,
    ),
    UngroupItem(pdf_editor_core::PageItem),
    PrepareGroupedText(pdf_editor_core::PageItem),
    CopyItems(Vec<pdf_editor_core::PageItem>),
    PasteItems(u16),
    StyleText {
        object: TextObject,
        size: f32,
        color: [u8; 3],
    },
    AddText {
        text: pdf_editor_core::NewText,
        raster: (f32, f32),
    },
    Render {
        page: u16,
        scale: f32,
    },
    Objects {
        page: u16,
    },
    /// Replaces a line of one or more adjacent runs as one edit.
    Edit {
        objects: Vec<TextObject>,
        text: String,
    },
    EditParagraph(pdf_editor_core::Paragraph, String),
    Undo,
    Redo,
    SaveAs(PathBuf),
    /// Writes a recovery copy to a new path; the document is unchanged.
    Snapshot(PathBuf),
    SetSubstitution(Substitution),
}

/// Maps PDF points to unscaled raster points (top-left origin), including
/// page rotation: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Clone, Copy, Debug)]
pub struct PageTransform([f32; 6]);

impl PageTransform {
    #[cfg(test)]
    pub(crate) fn for_test(matrix: [f32; 6]) -> Self {
        Self(matrix)
    }
    #[cfg(test)]
    pub fn identity() -> Self {
        Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }
    pub fn inverse(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let [a, b, c, d, e, f] = self.0;
        let determinant = a * d - b * c;
        if determinant.abs() < 0.000001 {
            return None;
        }
        Some((
            (d * (x - e) - c * (y - f)) / determinant,
            (-b * (x - e) + a * (y - f)) / determinant,
        ))
    }
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

pub enum Reply {
    ObjectPreview { id: u64, generation: u64, result: pdf_editor_core::Result<pdf_editor_core::ObjectPreview> },
    Tiles {generation:u64,page:u16,scale:f32,images:Result<Vec<([u32;4],pdf_editor_core::Image)>,String>},
    SanitizedExported(PathBuf),
    ComparisonSnapshot {id:u64,generation:u64,original:Arc<Document>,current:Arc<Document>},
    Characters {generation:u64,page:u16,characters:Result<Vec<pdf_editor_core::TextCharacter>,String>},
    SearchResults {generation:u64,query:String,options:pdf_editor_core::SearchOptions,hits:Vec<pdf_editor_core::SearchHit>,boxes:Vec<(u16,Vec<[f32;4]>)>},
    WorkingCopySaved {path:PathBuf,encrypted:bool},

    TableDetected(pdf_editor_core::TableGrid),

    ScanPreview {
        generation: u64,
        page: u16,
        picture: pdf_editor_core::Picture,
        corrected: bool,
    },

    PrivacyPreview(pdf_editor_core::RedactionPreview, Vec<(u16, Vec<[f32; 4]>)>),
    ItemsCopied,
    Bookmarks(pdf_editor_core::BookmarkSet),
    Printed(bool),
    Pictures(Vec<pdf_editor_core::PlacedPicture>),
    Annotations(Vec<pdf_editor_core::Annotation>),
    Exported(String),
    PasswordRequired(PathBuf),
    ReplacementCount(String, usize),
    FormFields(
        Vec<pdf_editor_core::FormField>,
        Vec<pdf_editor_core::DesignedField>,
    ),
    Thumbnail {
        generation: u64,
        page: u16,
        image: pdf_editor_core::Image,
    },
    Fonts(Vec<String>),
    Found {
        generation: u64,
        query: String,
        matches: Vec<(u16, Vec<[f32; 4]>)>,
    },
    Opened {
        generation: u64,
        path: PathBuf,
        name: String,
        pages: Vec<PageInfo>,
        suggested: PathBuf,
        protected: bool,
        can_undo: bool,
        can_redo: bool,
    },
    Rendered {
        generation: u64,
        page: u16,
        scale: f32,
        image: pdf_editor_core::Image,
    },
    Objects {
        generation: u64,
        page: u16,
        transform: PageTransform,
        objects: Vec<TextObject>,
        items: Vec<pdf_editor_core::PageItem>,
    },
    Changed {
        pages: Vec<PageInfo>,
        generation: u64,
        kind: ChangeKind,
        can_undo: bool,
        can_redo: bool,
        warning: Option<String>,
        objects_changed: usize,
    },
    Saved(PathBuf),
    Snapshot {
        generation: u64,
        path: PathBuf,
        error: Option<String>,
    },
    /// A request failed. `blocking` marks edit, undo, redo, open and save;
    /// `page` marks a failed render or object listing for that page.
    Failed {
        message: String,
        blocking: bool,
        page: Option<u16>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Edit,
    Undo,
    Redo,
}

/// Shared by tabs in one window; retains one immutable snapshot after a source tab closes.
#[derive(Clone, Default)]
pub struct SharedClipboard(Arc<Mutex<Option<Arc<CopySlot>>>>);

#[derive(Default)]
pub struct CopySlot {
    value: Mutex<Option<Result<Arc<pdf_editor_core::ObjectClipboard>, String>>>,
    ready: Condvar,
}

pub struct CopyJob {
    items: Vec<pdf_editor_core::PageItem>,
    slot: Arc<CopySlot>,
}

impl CopySlot {
    fn finish(&self, value: Result<Arc<pdf_editor_core::ObjectClipboard>, String>) {
        let mut state = self.value.lock().unwrap_or_else(|e| e.into_inner());
        if state.is_none() {
            *state = Some(value);
            self.ready.notify_all();
        }
    }

    fn wait(&self) -> Result<Arc<pdf_editor_core::ObjectClipboard>, String> {
        let state = self.value.lock().unwrap_or_else(|e| e.into_inner());
        self.ready
            .wait_while(state, |state| state.is_none())
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .unwrap()
            .clone()
    }
}

impl Drop for CopyJob {
    fn drop(&mut self) {
        self.slot.finish(Err("Object copy did not complete".into()));
    }
}

impl SharedClipboard {
    fn prepare(&self, request: Request) -> Request {
        match request {
            Request::CopyItems(items) => {
                let slot = Arc::new(CopySlot::default());
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(slot.clone());
                Request::CopyItemsPrepared(CopyJob { items, slot })
            }
            Request::PasteItems(page) => Request::PasteItemsPrepared(
                page,
                self.0.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            ),
            other => other,
        }
    }
}

#[derive(Clone, Default)]
struct ActiveJob(Arc<Mutex<Option<pdf_editor_core::OperationControl>>>);
impl ActiveJob {
    fn start(&self) -> pdf_editor_core::OperationControl {
        let control = pdf_editor_core::OperationControl::default();
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(control.clone());
        control
    }
    fn finish(&self) { *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None; }
    fn cancel(&self) -> bool {
        let current = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(control) = current.as_ref() { control.cancel(); true } else { false }
    }
    fn progress(&self) -> Option<pdf_editor_core::OperationProgress> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.progress()
    }
}

pub struct Worker {
    active: ActiveJob,
    clipboard: Option<SharedClipboard>,
    requests: Sender<Request>,
    replies: Receiver<Reply>,
}

impl Worker {
    pub fn spawn(
        ctx: eframe::egui::Context,
        library: PathBuf,
        policy: Substitution,
        clipboard: SharedClipboard,
    ) -> Self {
        let (requests, inbox) = channel();
        let (outbox, replies) = sync_channel(1);
        let shared = clipboard.clone();
        let active = ActiveJob::default();
        let job = active.clone();
        // If spawning fails the closure and its sender are dropped, so the
        // first poll reports a disconnected engine.
        let _ = thread::Builder::new()
            .name("pdf-engine".into())
            .spawn(move || run(ctx, library, policy, clipboard, inbox, outbox, job));
        Self {
            active,
            clipboard: Some(shared),
            requests,
            replies,
        }
    }

    /// Progress for the currently executing job only.
    pub fn progress(&self) -> Option<pdf_editor_core::OperationProgress> { self.active.progress() }
    pub fn cancel(&self) -> bool { self.active.cancel() }

    /// Returns false when the engine thread is gone.
    pub fn send(&self, request: Request) -> bool {
        let request = if let Some(clipboard) = &self.clipboard {
            clipboard.prepare(request)
        } else {
            request
        };
        self.requests.send(request).is_ok()
    }

    /// Drains pending replies. Returns None when the engine thread has stopped.
    pub fn poll(&self) -> Option<Vec<Reply>> {
        let mut out = Vec::new();
        loop {
            match self.replies.try_recv() {
                Ok(reply) => {
                    out.push(reply);
                    if out.len() == 4 {
                        return Some(out);
                    }
                }
                Err(TryRecvError::Empty) => return Some(out),
                Err(TryRecvError::Disconnected) => {
                    return if out.is_empty() { None } else { Some(out) };
                }
            }
        }
    }
}

struct State {
    comparison_original: Option<Arc<Document>>,
    clipboard: SharedClipboard,
    engine: Option<Engine>,
    engine_error: String,
    doc: Option<Document>,
    working_copy: Option<pdf_editor_core::WorkingCopy>,
    generation: u64,
    policy: Substitution,
}

fn run(
    ctx: eframe::egui::Context,
    library: PathBuf,
    policy: Substitution,
    clipboard: SharedClipboard,
    inbox: Receiver<Request>,
    outbox: SyncSender<Reply>,
    active: ActiveJob,
) {
    let (engine, engine_error) = match Engine::new(&library) {
        Ok(engine) => (Some(engine), String::new()),
        Err(e) => (
            None,
            format!("The PDF engine could not start: {e}. Place pdfium.dll next to the program."),
        ),
    };
    let mut state = State {
        comparison_original: None,
        clipboard,
        engine,
        engine_error,
        doc: None,
        working_copy: None,
        generation: 0,
        policy,
    };
    while let Ok(first) = inbox.recv() {
        // Coalesce queued work: renders and object lists for the same page
        // only need their latest request; everything else keeps its order.
        let mut batch = vec![first];
        while let Ok(next) = inbox.try_recv() {
            batch.push(next);
        }
        let batch = coalesce(batch);
        for request in batch {
            let cancellable=matches!(&request,Request::Sanitize(..)|Request::ExportSanitized(..)|Request::CleanupScan(..)|Request::ImagesToPdf(..)|Request::Merge(..)|Request::Extract(..)|Request::Split(..)|Request::ExportWord(..)|Request::ExportImages(..)|Request::Compress(..)|Request::Ocr(..)|Request::PageBatch(..)|Request::LayoutItems(..)|Request::HighlightText(..)|Request::FormatText(..)|Request::Replace {..}|Request::ReplaceOptions {..}|Request::EditParagraph(..)|Request::Print);
            let control = if cancellable {active.start()} else {Default::default()};
            let reply = catch_unwind(AssertUnwindSafe(|| {
                pdf_editor_core::operation::with_control(&control, || {
                    if let Err(error) = pdf_editor_core::operation::checkpoint("Processing document", 0, 1) {
                        return Some(Reply::Failed { message: error.to_string(), blocking: page_of(&request).is_none(), page: page_of(&request) });
                    }
                    handle(&mut state, request)
                })
            }))
                .unwrap_or_else(|_| {
                    Some(Reply::Failed {
                        message: "An internal error occurred while processing the document.".into(),
                        blocking: true,
                        page: None,
                    })
                });
            active.finish();
            if let Some(reply) = reply {
                // Wake the UI before a bounded send can wait for a free slot.
                ctx.request_repaint();
                if outbox.send(reply).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        }
    }
}

fn coalesce(batch: Vec<Request>) -> Vec<Request> {
    fn key(request: &Request) -> Option<(u8,u16)> {
        match request {
            Request::ObjectPreview {items,..} => items.first().map(|item|(0,item.page)),
            Request::Objects { page } => Some((1,*page)),
            Request::Render { page,.. } => Some((2,*page)),
            Request::Thumbnail(page) => Some((3,*page)),
            Request::RenderTiles {page,..} => Some((4,*page)),
            _ => None,
        }
    }
    fn flush(pending: &mut Vec<Request>, out: &mut Vec<Request>) {
        let mut seen = std::collections::HashSet::new();
        // Keep the latest scale for each page, without cancelling a preview
        // across an edit, open, save or other ordered operation.
        pending.reverse();
        pending.retain(|request| seen.insert(key(request).unwrap()));
        pending.reverse();
        pending.sort_by_key(|request| key(request).unwrap().0);
        out.append(pending);
    }
    let mut out = Vec::with_capacity(batch.len());
    let mut pending = Vec::new();
    for request in batch {
        if key(&request).is_some() {
            pending.push(request);
        } else {
            flush(&mut pending,&mut out);
            out.push(request);
        }
    }
    flush(&mut pending,&mut out);
    out
}

fn handle(state: &mut State, request: Request) -> Option<Reply> {
    let request = state.clipboard.prepare(request);
    if let Request::SetSubstitution(policy) = request {
        state.policy = policy;
        if let Some(doc) = state.doc.as_mut() {
            doc.substitution = policy;
        }
        return None;
    }
    let Some(engine) = state.engine.as_ref() else {
        let page = page_of(&request);
        return Some(Reply::Failed {
            message: state.engine_error.clone(),
            blocking: page.is_none(),
            page,
        });
    };
    let page = page_of(&request);
    let blocking = page.is_none() && !matches!(request, Request::Find(_) | Request::FindOptions(..) | Request::Characters(_) | Request::Fonts | Request::ObjectPreview {..});
    let failed = |e: pdf_editor_core::Error| {
        Some(Reply::Failed {
            message: e.to_string(),
            blocking,
            page,
        })
    };

    match request {
        Request::ObjectPreview {id,items,scale} => Some(Reply::ObjectPreview {id,generation:state.generation,result:engine.object_preview(state.doc.as_ref()?,&items,scale)}),
        Request::RenderTiles {page,scale,regions}=>{
            let doc=state.doc.as_ref()?;
            let images=engine.render_tiles(doc,page,scale,&regions).map(|images|regions.into_iter().zip(images).collect()).map_err(|e|e.to_string());
            Some(Reply::Tiles {generation:state.generation,page,scale,images})
        }
        Request::ComparisonSnapshot(id)=>{
            let doc=state.doc.as_ref()?;
            let original=state.comparison_original.clone().unwrap_or_else(||Arc::new(doc.comparison_snapshot()));
            Some(Reply::ComparisonSnapshot {id,generation:state.generation,original,current:Arc::new(doc.comparison_snapshot())})
        }
        Request::Characters(page) => Some(Reply::Characters { generation:state.generation,page,
            characters:engine.page_text_characters(state.doc.as_ref()?,page).map_err(|e|e.to_string()) }),
        Request::FindOptions(query, options) => {
            let doc=state.doc.as_ref()?;
            match engine.find_text_with_options(doc,&query,&options).and_then(|hits| {
                let boxes=search_boxes(engine,doc,&hits)?; Ok((hits,boxes))
            }) {
                Ok((hits,boxes))=>Some(Reply::SearchResults {generation:state.generation,query,options,hits,boxes}),
                Err(e)=>failed(e),
            }
        }
        Request::ReplacementCountOptions(query,options) => match engine.replacement_count_with_options(state.doc.as_ref()?,&query,&options) {
            Ok(count)=>Some(Reply::ReplacementCount(query,count)), Err(e)=>failed(e),
        },
        Request::ReplaceOptions {query,text,occurrence,options} => match engine.replace_text_with_options(state.doc.as_mut()?,&query,&text,occurrence,&options) {
            Ok(count)=>{state.generation+=u64::from(count>0);Some(changed(state,ChangeKind::Edit,Some(format!("Replaced {count} matches.")),count))},
            Err(e)=>failed(e),
        },
        Request::FormatText(objects,format)=>match engine.format_text(state.doc.as_mut()?,&objects,&format) {
            Ok(result)=>{state.generation+=u64::from(result.objects_changed>0);Some(content_changed(state,ChangeKind::Edit,result.warning,result.objects_changed))},
            Err(e)=>failed(e),
        },
        Request::PageBatch(edit)=>{
            let doc=state.doc.as_mut()?;let before=doc.revision();
            match engine.edit_page_batch(doc,&edit) {
                Ok(())=>{let change=usize::from(state.doc.as_ref()?.revision()!=before);state.generation+=change as u64;Some(changed(state,ChangeKind::Edit,None,change))},
                Err(e)=>failed(e),
            }
        }
        Request::LayoutItems(items,layout)=>{
            let doc=state.doc.as_mut()?;let before=doc.revision();
            match engine.layout_page_items(doc,&items,layout) {
                Ok(())=>{let change=usize::from(state.doc.as_ref()?.revision()!=before);state.generation+=change as u64;Some(content_changed(state,ChangeKind::Edit,None,change))},
                Err(e)=>failed(e),
            }
        }
        Request::HighlightText(regions)=>match engine.highlight_text_selection(state.doc.as_mut()?,&regions) {
            Ok(())=>{state.generation+=1;Some(content_changed(state,ChangeKind::Edit,None,1))},Err(e)=>failed(e),
        },
        Request::SaveWorkingCopy=>{
            let Some(copy)=state.working_copy.as_mut() else {return failed(pdf_editor_core::Error("Create a working copy with Save As first".into()));};
            match state.doc.as_ref()?.update_working_copy(copy) {
                Ok(())=>Some(Reply::WorkingCopySaved{path:copy.path().to_owned(),encrypted:copy.encrypted()}),Err(e)=>failed(e),
            }
        }
        Request::ScanPreview(page, options) => match engine
            .scan_preview(state.doc.as_ref()?, page)
            .and_then(|picture| match &options {
                Some(options) => pdf_editor_core::clean_scan(&picture, options),
                None => Ok(picture),
            }) {
            Ok(picture) => Some(Reply::ScanPreview {
                generation: state.generation,
                page,
                picture,
                corrected: options.is_some(),
            }),
            Err(e) => failed(e),
        },
        Request::CleanupScan(page, options) => {
            match engine.cleanup_scan(state.doc.as_mut()?, page, &options) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::ImagesToPdf(paths, size, path) => match engine
            .images_to_pdf(&paths, size)
            .and_then(|doc| doc.save_as(&path))
        {
            Ok(()) => Some(Reply::Exported(format!("Created PDF: {}", path.display()))),
            Err(e) => failed(e),
        },
        Request::Merge(paths) => match engine.merge_pdfs(state.doc.as_mut()?, &paths) {
            Ok(()) => {
                state.generation += 1;
                Some(changed(state, ChangeKind::Edit, None, 1))
            }
            Err(e) => failed(e),
        },
        Request::Extract(range, path) => {
            let doc = state.doc.as_ref()?;
            match engine
                .pages(doc)
                .and_then(|p| pdf_editor_core::page_ranges(&range, p.len()))
                .and_then(|pages| engine.extract_pages(doc, &pages, &path))
            {
                Ok(()) => Some(Reply::Exported("Selected pages exported.".into())),
                Err(e) => failed(e),
            }
        }
        Request::Split(ranges, folder) => {
            match engine.split_pdf(state.doc.as_ref()?, &ranges, &folder) {
                Ok(paths) => Some(Reply::Exported(format!(
                    "Created {} split PDFs.",
                    paths.len()
                ))),
                Err(e) => failed(e),
            }
        }
        Request::Annotations => match engine.annotations(state.doc.as_ref()?) {
            Ok(items) => Some(Reply::Annotations(items)),
            Err(e) => failed(e),
        },
        Request::AddAnnotation(markup) => match engine.add_annotation(state.doc.as_mut()?, &markup)
        {
            Ok(()) => {
                state.generation += 1;
                Some(changed(state, ChangeKind::Edit, None, 1))
            }
            Err(e) => failed(e),
        },
        Request::UpdateAnnotation(item, markup) => {
            match engine.update_annotation(state.doc.as_mut()?, &item, &markup) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::DeleteAnnotation(item) => {
            match engine.delete_annotation(state.doc.as_mut()?, &item) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::Pictures => match engine.placed_pictures(state.doc.as_ref()?) {
            Ok(items) => Some(Reply::Pictures(items)),
            Err(e) => failed(e),
        },
        Request::MovePicture(item, placement) => {
            match engine.move_picture(state.doc.as_mut()?, &item, placement) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::Ocr(page) => match engine.ocr_page(state.doc.as_mut()?, page) {
            Ok(words) => {
                state.generation += 1;
                Some(changed(
                    state,
                    ChangeKind::Edit,
                    Some(format!("Added {words} searchable words using Windows OCR.")),
                    words,
                ))
            }
            Err(e) => failed(e),
        },
        Request::Redact(page, bounds) => match engine.redact(state.doc.as_mut()?, page, bounds) {
            Ok(()) => {
                state.generation += 1;
                Some(changed(state,ChangeKind::Edit,Some("Redaction applied. The document is flattened; Save As writes a sanitized copy.".into()),1))
            }
            Err(e) => failed(e),
        },
        Request::ExportSanitized(options,path) => match engine.export_sanitized(state.doc.as_ref()?,options,&path) {
            Ok(_) => Some(Reply::SanitizedExported(path)),
            Err(e) => failed(e),
        },
        Request::Sanitize(options) => match engine.sanitize(state.doc.as_mut()?, options) {
            Ok(report) => {
                state.generation += 1;
                Some(changed(
                    state,
                    ChangeKind::Edit,
                    Some(report.to_string()),
                    1,
                ))
            }
            Err(e) => failed(e),
        },
        Request::Compress(edge, quality) => {
            match engine.compress_images(state.doc.as_mut()?, edge, quality) {
                Ok(report) => {
                    state.generation += u64::from(report.images > 0);
                    Some(changed(
                        state,
                        ChangeKind::Edit,
                        Some(format!(
                            "Compression: {} → {} bytes; {} images changed, {} unsupported images skipped.",
                            report.before, report.after, report.images, report.skipped
                        )),
                        usize::from(report.images > 0),
                    ))
                }
                Err(e) => failed(e),
            }
        }
        Request::Bates(stamp, bates) => {
            match engine.stamp_bates(state.doc.as_mut()?, &stamp, &bates) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::Stamp(stamp) => match engine.stamp_page_range(state.doc.as_mut()?, &stamp) {
            Ok(()) => {
                state.generation += 1;
                Some(changed(state, ChangeKind::Edit, None, 1))
            }
            Err(e) => failed(e),
        },
        Request::ExportWord(path) => match engine.export_word(state.doc.as_ref()?, &path) {
            Ok(()) => Some(Reply::Exported("Word document exported.".into())),
            Err(e) => failed(e),
        },
        Request::DetectTable(page, area) => {
            match engine.detect_table(state.doc.as_ref()?, page, area) {
                Ok(grid) => Some(Reply::TableDetected(grid)),
                Err(e) => failed(e),
            }
        }
        Request::ExportTable(grid, format, path) => match grid.export(&path, format) {
            Ok(()) => Some(Reply::Exported("Table exported.".into())),
            Err(e) => failed(e),
        },
        Request::ExportImages(ranges, dpi, format, folder) => {
            let doc = state.doc.as_ref()?;
            match engine
                .pages(doc)
                .and_then(|p| pdf_editor_core::page_ranges(&ranges, p.len()))
                .and_then(|pages| engine.export_page_images(doc, &pages, dpi, format, &folder))
            {
                Ok(paths) => Some(Reply::Exported(format!(
                    "Exported {} page images at {dpi} DPI.",
                    paths.len()
                ))),
                Err(e) => failed(e),
            }
        }
        Request::Crop(page, bounds) => match engine.crop_page(state.doc.as_mut()?, page, bounds) {
            Ok(()) => {
                state.generation += 1;
                Some(changed(state, ChangeKind::Edit, None, 1))
            }
            Err(e) => failed(e),
        },
        Request::Draw(drawing) => match engine.add_drawing(state.doc.as_mut()?, &drawing) {
            Ok(()) => {
                state.generation += 1;
                Some(changed(state, ChangeKind::Edit, None, 1))
            }
            Err(e) => failed(e),
        },
        Request::Bookmarks => match engine.bookmarks(state.doc.as_ref()?) {
            Ok(items) => Some(Reply::Bookmarks(items)),
            Err(e) => failed(e),
        },
        Request::EditBookmarks(revision, edit) => {
            match engine.edit_bookmarks(state.doc.as_mut()?, revision, edit) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::Print => {
            let doc = state.doc.as_ref()?;
            let result = (|| -> pdf_editor_core::Result<bool> {
                let job = engine.prepare_print(doc)?;
                let windows = std::env::var_os("SystemRoot").ok_or_else(|| {
                    pdf_editor_core::Error("Windows directory unavailable".into())
                })?;
                let program =
                    PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe");
                let mut command = std::process::Command::new(program);
                command.args([
                    "-NoProfile",
                    "-STA",
                    "-Command",
                    include_str!("printing.ps1"),
                ]);
                command.env("PDF_EDITOR_PRINT_DIRECTORY", job.directory());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                let status = command.status()?;
                match status.code() {
                    Some(0) => Ok(true),
                    Some(2) => Ok(false),
                    _ => Err(pdf_editor_core::Error(
                        "Printing failed; check the selected printer".into(),
                    )),
                }
            })();
            match result {
                Ok(printed) => Some(Reply::Printed(printed)),
                Err(e) => failed(e),
            }
        }
        Request::Pages(edit) => {
            let doc = state.doc.as_mut()?;
            match engine.edit_pages(doc, &edit) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::Thumbnail(page) => {
            let doc = state.doc.as_ref()?;
            match engine.thumbnail(doc, page, 150) {
                Ok(image) => Some(Reply::Thumbnail {
                    generation: state.generation,
                    page,
                    image,
                }),
                Err(e) => failed(e),
            }
        }
        Request::UngroupItem(item) => match engine.ungroup_page_item(state.doc.as_mut()?, &item) {
            Ok(()) => {
                state.generation += 1;
                Some(content_changed(state, ChangeKind::Edit, None, 1))
            }
            Err(e) => failed(e),
        },
        Request::PrepareGroupedText(item) => {
            match engine.prepare_grouped_text(state.doc.as_mut()?, &item) {
                Ok(()) => {
                    state.generation += 1;
                    Some(content_changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::CopyItemsPrepared(job) => {
            match engine.copy_page_items(state.doc.as_ref()?, &job.items) {
                Ok(clipboard) => {
                    job.slot.finish(Ok(Arc::new(clipboard)));
                    Some(Reply::ItemsCopied)
                }
                Err(e) => {
                    job.slot.finish(Err(e.to_string()));
                    failed(e)
                }
            }
        }
        Request::PasteItemsPrepared(page, slot) => {
            let Some(slot) = slot else {
                return Some(Reply::Failed {
                    message: "Copy an object first".into(),
                    blocking: true,
                    page: None,
                });
            };
            let clipboard = match slot.wait() {
                Ok(clipboard) => clipboard,
                Err(message) => {
                    return Some(Reply::Failed {
                        message,
                        blocking: true,
                        page: None,
                    });
                }
            };
            match engine.paste_page_items(state.doc.as_mut()?, &clipboard, page) {
                Ok(()) => {
                    state.generation += 1;
                    Some(content_changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::CopyItems(_) | Request::PasteItems(_) => {
            unreachable!("clipboard request prepared")
        }
        Request::EditItems(items, action) => {
            match engine.edit_page_items(state.doc.as_mut()?, &items, action) {
                Ok(()) => {
                    state.generation += 1;
                    Some(content_changed(state, ChangeKind::Edit, None, items.len()))
                }
                Err(e) => failed(e),
            }
        }
        Request::StyleText {
            object,
            size,
            color,
        } => {
            let doc = state.doc.as_mut()?;
            match engine.style_text(doc, &object, size, color) {
                Ok(()) => {
                    state.generation += 1;
                    Some(content_changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::DeleteText(object) => {
            let doc = state.doc.as_mut()?;
            match engine.delete_text(doc, &object) {
                Ok(result) => {
                    state.generation += 1;
                    // Text edits cannot change page order or dimensions. Keep the
                    // existing page previews while their new rasters are rendered.
                    Some(content_changed(
                        state,
                        ChangeKind::Edit,
                        result.warning,
                        result.objects_changed,
                    ))
                }
                Err(e) => failed(e),
            }
        }
        Request::PrivacyPreview(query) => match engine
            .preview_redactions(state.doc.as_ref()?, &query)
        {
            Ok(preview) => {
                let doc = state.doc.as_ref()?;
                let mut transforms = std::collections::BTreeMap::new();
                let converted = preview
                    .matches
                    .iter()
                    .map(|m| {
                        let t = if let Some(transform) = transforms.get(&m.page) {
                            *transform
                        } else {
                            let transform = page_transform(engine, doc, m.page)?;
                            transforms.insert(m.page, transform);
                            transform
                        };
                        Ok((
                            m.page,
                            m.bounds
                                .iter()
                                .map(|b| {
                                    let points = [
                                        t.apply(b.left, b.bottom),
                                        t.apply(b.right, b.bottom),
                                        t.apply(b.left, b.top),
                                        t.apply(b.right, b.top),
                                    ];
                                    [
                                        points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min),
                                        points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min),
                                        points
                                            .iter()
                                            .map(|p| p.0)
                                            .fold(f32::NEG_INFINITY, f32::max),
                                        points
                                            .iter()
                                            .map(|p| p.1)
                                            .fold(f32::NEG_INFINITY, f32::max),
                                    ]
                                })
                                .collect(),
                        ))
                    })
                    .collect::<pdf_editor_core::Result<Vec<_>>>();
                match converted {
                    Ok(boxes) => Some(Reply::PrivacyPreview(preview, boxes)),
                    Err(e) => failed(e),
                }
            }
            Err(e) => failed(e),
        },
        Request::PrivacyApply(preview) => {
            match engine.apply_redaction_preview(state.doc.as_mut()?, &preview) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(
                        state,
                        ChangeKind::Edit,
                        None,
                        preview.matches.len(),
                    ))
                }
                Err(e) => failed(e),
            }
        }
        Request::ReplacementCount(query) => {
            match engine.replacement_count(state.doc.as_ref()?, &query) {
                Ok(count) => Some(Reply::ReplacementCount(query, count)),
                Err(e) => failed(e),
            }
        }
        Request::Replace {
            query,
            text,
            occurrence,
        } => match engine.replace_text(state.doc.as_mut()?, &query, &text, occurrence) {
            Ok(count) => {
                state.generation += u64::from(count > 0);
                Some(changed(
                    state,
                    ChangeKind::Edit,
                    Some(format!("Replaced {count} matches.")),
                    count,
                ))
            }
            Err(e) => failed(e),
        },
        Request::FormFields => match engine.form_fields(state.doc.as_ref()?) {
            Ok(fields) => match engine.designed_fields(state.doc.as_ref()?) {
                Ok(designed) => Some(Reply::FormFields(fields, designed)),
                Err(e) => failed(e),
            },
            Err(e) => failed(e),
        },
        Request::FlattenForms => match engine.flatten_forms(state.doc.as_mut()?) {
            Ok(count) => {
                if count > 0 {
                    state.generation += 1;
                }
                Some(changed(
                    state,
                    ChangeKind::Edit,
                    Some(format!("Flattened {count} form fields")),
                    count,
                ))
            }
            Err(e) => failed(e),
        },
        Request::DesignForm(edit) => match engine.design_form(state.doc.as_mut()?, edit) {
            Ok(()) => {
                state.generation += 1;
                Some(changed(state, ChangeKind::Edit, None, 1))
            }
            Err(e) => failed(e),
        },
        Request::FillField(field, value) => {
            match engine.fill_field(state.doc.as_mut()?, &field, &value) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::PlacePicture(picture, placement) => {
            let doc = state.doc.as_mut()?;
            match engine.place_picture(doc, &picture, placement) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::Fonts => match engine.system_fonts() {
            Ok(fonts) => Some(Reply::Fonts(fonts)),
            Err(e) => failed(e),
        },
        Request::AddText { mut text, raster } => {
            let doc = state.doc.as_mut()?;
            match engine
                .pixel_to_page(
                    doc,
                    text.page,
                    4.0,
                    (raster.0 * 4.0) as i32,
                    (raster.1 * 4.0) as i32,
                )
                .and_then(|(x, y)| {
                    text.x = x;
                    text.y = y;
                    engine.add_text(doc, &text)
                }) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::Find(query) => {
            let doc = state.doc.as_ref()?;
            let result = (|| -> pdf_editor_core::Result<_> {
                let mut out = Vec::new();
                let mut transforms = std::collections::BTreeMap::new();
                for found in engine.find_text(doc, &query)? {
                    let t = match transforms.entry(found.page) {
                        std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            *entry.insert(page_transform(engine, doc, found.page)?)
                        }
                    };
                    let rects = found
                        .bounds
                        .iter()
                        .map(|b| {
                            let points = [
                                t.apply(b.left, b.bottom),
                                t.apply(b.right, b.bottom),
                                t.apply(b.left, b.top),
                                t.apply(b.right, b.top),
                            ];
                            [
                                points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min),
                                points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min),
                                points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max),
                                points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max),
                            ]
                        })
                        .collect();
                    out.push((found.page, rects));
                }
                Ok(out)
            })();
            match result {
                Ok(matches) => Some(Reply::Found {
                    generation: state.generation,
                    query,
                    matches,
                }),
                Err(e) => failed(e),
            }
        }
        Request::OpenRecovery { path, source } => {
            let mut doc = match engine.open_recovery(&path, &source) {
                Ok(doc) => doc, Err(error) => return failed(error),
            };
            let pages = match engine.pages(&doc) { Ok(pages) => pages, Err(error) => return failed(error) };
            doc.substitution = state.policy;
            let suggested = doc.suggested_save_path();
            let can_undo = doc.can_undo();
            let can_redo = doc.can_redo();
            state.comparison_original=Some(Arc::new(engine.open(&source).unwrap_or_else(|_|doc.comparison_snapshot())));
            state.working_copy = None;
            state.doc = Some(doc);
            state.generation += 1;
            Some(Reply::Opened { generation: state.generation, name: source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Recovered.pdf".into()), path: source, pages, suggested, protected: false, can_undo, can_redo })
        }
        Request::Open(path) => handle(state, Request::OpenPassword(path, String::new())),
        Request::OpenPassword(path, password) => {
            let mut doc = match engine.open_with_password(&path, &password) {
                Ok(doc) => doc,
                Err(e) if e.0 == pdf_editor_core::PASSWORD_REQUIRED => {
                    return Some(Reply::PasswordRequired(path));
                }
                Err(e) => return failed(e),
            };
            let protected = doc.is_password_protected();
            let pages = match engine.pages(&doc) {
                Ok(pages) => pages,
                Err(e) => return failed(e),
            };
            doc.substitution = state.policy;
            let suggested = doc.suggested_save_path();
            state.comparison_original=Some(Arc::new(doc.comparison_snapshot()));
            state.working_copy = None;
            state.doc = Some(doc);
            state.generation += 1;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled.pdf".into());
            Some(Reply::Opened {
                generation: state.generation,
                path,
                name,
                pages,
                suggested,
                protected,
                can_undo: false,
                can_redo: false,
            })
        }
        Request::Render { page, scale } => {
            let doc = state.doc.as_ref()?;
            match engine.render(doc, page, scale) {
                Ok(image) => Some(Reply::Rendered {
                    generation: state.generation,
                    page,
                    scale,
                    image,
                }),
                Err(e) => failed(e),
            }
        }
        Request::Objects { page } => {
            let doc = state.doc.as_ref()?;
            let objects = match engine.text_objects(doc, page) {
                Ok(objects) => objects,
                Err(e) => return failed(e),
            };
            let transform = match page_transform(engine, doc, page) {
                Ok(t) => t,
                Err(e) => return failed(e),
            };
            Some(Reply::Objects {
                generation: state.generation,
                page,
                transform,
                objects,
                items: engine.page_items(doc, page).unwrap_or_default(),
            })
        }
        Request::Edit { .. } | Request::EditParagraph(..) => {
            let doc = state.doc.as_mut()?;
            let result = match request {
                Request::Edit { objects, text } => engine.edit_group(doc, &objects, &text),
                Request::EditParagraph(paragraph, text) => {
                    engine.edit_paragraph(doc, &paragraph, &text)
                }
                _ => unreachable!(),
            };
            match result {
                Ok(result) => {
                    if result.objects_changed > 0 {
                        state.generation += 1;
                    }
                    Some(content_changed(
                        state,
                        ChangeKind::Edit,
                        result.warning,
                        result.objects_changed,
                    ))
                }
                Err(e) => failed(e),
            }
        }
        Request::Undo | Request::Redo => {
            let kind = if matches!(request, Request::Undo) {
                ChangeKind::Undo
            } else {
                ChangeKind::Redo
            };
            let doc = state.doc.as_mut()?;
            let moved = if kind == ChangeKind::Undo {
                doc.undo()
            } else {
                doc.redo()
            };
            if moved {
                state.generation += 1;
            }
            Some(changed(state, kind, None, usize::from(moved)))
        }
        Request::SavePassword(path,password)=>match state.doc.as_ref()?.create_working_copy(&path,&password) {
            Ok(copy)=>{let encrypted=copy.encrypted();state.working_copy=Some(copy);Some(Reply::WorkingCopySaved{path,encrypted})},Err(e)=>failed(e),
        },
        Request::SaveAs(path)=>match state.doc.as_ref()?.create_working_copy(&path,"") {
            Ok(copy)=>{state.working_copy=Some(copy);Some(Reply::WorkingCopySaved{path,encrypted:false})},Err(e)=>failed(e),
        },
        Request::Snapshot(path) => {
            let doc = state.doc.as_ref()?;
            let error = if doc.is_password_protected() || state.working_copy.as_ref().is_some_and(|copy|copy.encrypted()) {
                Some("Recovery copies are disabled for password-protected documents".into())
            } else {
                doc.save_recovery(&path).err().map(|e| e.to_string())
            };
            Some(Reply::Snapshot {
                generation: state.generation,
                path,
                error,
            })
        }
        Request::SetSubstitution(_) => None,
    }
}

fn page_of(request: &Request) -> Option<u16> {
    match request {
        Request::Render { page, .. } | Request::RenderTiles {page,..} | Request::Objects { page } | Request::Thumbnail(page) => {
            Some(*page)
        }
        _ => None,
    }
}

fn search_boxes(engine:&Engine,doc:&Document,hits:&[pdf_editor_core::SearchHit])->pdf_editor_core::Result<Vec<(u16,Vec<[f32;4]>)>> {
    let mut transforms=std::collections::BTreeMap::new();
    hits.iter().map(|hit| {
        let transform=if let Some(t)=transforms.get(&hit.page) {*t} else {
            let t=page_transform(engine,doc,hit.page)?;transforms.insert(hit.page,t);t
        };
        let boxes=hit.bounds.iter().map(|b| {
            let points=[transform.apply(b.left,b.bottom),transform.apply(b.right,b.bottom),transform.apply(b.left,b.top),transform.apply(b.right,b.top)];
            [points.iter().map(|p|p.0).fold(f32::INFINITY,f32::min),points.iter().map(|p|p.1).fold(f32::INFINITY,f32::min),points.iter().map(|p|p.0).fold(f32::NEG_INFINITY,f32::max),points.iter().map(|p|p.1).fold(f32::NEG_INFINITY,f32::max)]
        }).collect();
        Ok((hit.page,boxes))
    }).collect()
}

fn changed(
    state: &State,
    kind: ChangeKind,
    warning: Option<String>,
    objects_changed: usize,
) -> Reply {
    let mut reply = content_changed(state, kind, warning, objects_changed);
    if let Reply::Changed { pages, .. } = &mut reply {
        *pages = state
            .engine
            .as_ref()
            .zip(state.doc.as_ref())
            .and_then(|(e, d)| e.pages(d).ok())
            .unwrap_or_default();
    }
    reply
}

fn content_changed(
    state: &State,
    kind: ChangeKind,
    warning: Option<String>,
    objects_changed: usize,
) -> Reply {
    let (can_undo, can_redo) = state
        .doc
        .as_ref()
        .map_or((false, false), |d| (d.can_undo(), d.can_redo()));
    Reply::Changed {
        pages: Vec::new(),
        generation: state.generation,
        kind,
        can_undo,
        can_redo,
        warning,
        objects_changed,
    }
}

/// Derives the affine PDF-point to raster-point mapping from three samples.
fn page_transform(
    engine: &Engine,
    doc: &Document,
    page: u16,
) -> pdf_editor_core::Result<PageTransform> {
    let sample = |x: f32, y: f32| -> pdf_editor_core::Result<(f32, f32)> {
        let (px, py) = engine.page_to_pixel(doc, page, SAMPLE_SCALE, x, y)?;
        Ok((px as f32 / SAMPLE_SCALE, py as f32 / SAMPLE_SCALE))
    };
    let (e, f) = sample(0.0, 0.0)?;
    let (x1, y1) = sample(SAMPLE_SPAN, 0.0)?;
    let (x2, y2) = sample(0.0, SAMPLE_SPAN)?;
    Ok(PageTransform([
        (x1 - e) / SAMPLE_SPAN,
        (y1 - f) / SAMPLE_SPAN,
        (x2 - e) / SAMPLE_SPAN,
        (y2 - f) / SAMPLE_SPAN,
        e,
        f,
    ]))
}

#[cfg(test)]
pub(crate) fn test_channel() -> (Worker, Receiver<Request>, Sender<Reply>) {
    let (requests, inbox) = channel();
    let (outbox, replies) = channel();
    (
        Worker {
            active: ActiveJob::default(),
            clipboard: None,
            requests,
            replies,
        },
        inbox,
        outbox,
    )
}

#[cfg(test)]
mod clipboard_tests {
    use super::*;

    #[test]
    fn immediate_cross_tab_paste_waits_for_the_copy_queued_before_it() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let library = root.join("target/debug/pdfium.dll");
        let engine = Engine::new(&library).unwrap();
        let doc = engine
            .open(&root.join("tests/fixtures/generated/unicode-cid.pdf"))
            .unwrap();
        let items = engine.page_items(&doc, 0).unwrap();
        let expected = engine.text_objects(&doc, 0).unwrap()[0].text.clone();
        let shared = SharedClipboard::default();
        let mut source = State {
        comparison_original: None,
            working_copy: None,
            clipboard: shared.clone(),
            engine: Some(engine),
            engine_error: String::new(),
            doc: Some(doc),
            generation: 1,
            policy: Substitution::default(),
        };
        // A prior copy must not leak into a paste while a newer copy is pending.
        assert!(matches!(
            handle(&mut source, Request::CopyItems(vec![items[1].clone()])),
            Some(Reply::ItemsCopied)
        ));
        let (mut source_worker, source_inbox, _) = test_channel();
        source_worker.clipboard = Some(shared.clone());
        let (mut destination_worker, destination_inbox, _) = test_channel();
        destination_worker.clipboard = Some(shared.clone());
        assert!(source_worker.send(Request::CopyItems(vec![items[0].clone()])));
        assert!(destination_worker.send(Request::PasteItems(0)));
        let paste = destination_inbox.recv().unwrap();
        let path = root.join("tests/fixtures/generated/rotated.pdf");
        let (started_tx, started_rx) = channel();
        let (done_tx, done_rx) = channel();
        let thread = std::thread::spawn(move || {
            let engine = Engine::new(&library).unwrap();
            let doc = engine.open(&path).unwrap();
            let mut destination = State {
        comparison_original: None,
            working_copy: None,
                clipboard: shared,
                engine: Some(engine),
                engine_error: String::new(),
                doc: Some(doc),
                generation: 1,
                policy: Substitution::default(),
            };
            started_tx.send(()).unwrap();
            let reply = handle(&mut destination, paste);
            assert!(matches!(reply, Some(Reply::Changed { .. })));
            let pasted = destination
                .engine
                .as_ref()
                .unwrap()
                .text_objects(destination.doc.as_ref().unwrap(), 0)
                .unwrap()
                .pop()
                .unwrap()
                .text;
            done_tx.send(pasted).unwrap();
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert!(matches!(
            done_rx.recv_timeout(std::time::Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(
            handle(&mut source, source_inbox.recv().unwrap()),
            Some(Reply::ItemsCopied)
        ));
        assert_eq!(
            done_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap(),
            expected
        );
        thread.join().unwrap();
    }

    #[test]
    fn cancelled_copy_releases_waiting_paste() {
        let shared = SharedClipboard::default();
        let copy = shared.prepare(Request::CopyItems(Vec::new()));
        let Request::PasteItemsPrepared(_, Some(slot)) = shared.prepare(Request::PasteItems(0))
        else {
            panic!("paste must capture pending copy");
        };
        drop(copy);
        assert!(slot.wait().is_err());
    }

    #[test]
    fn copy_between_workers_survives_source_close_and_preserves_destination_undo() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let library = root.join("target/debug/pdfium.dll");
        let source_engine = Engine::new(&library).unwrap();
        let source_doc = source_engine
            .open(&root.join("tests/fixtures/generated/unicode-cid.pdf"))
            .unwrap();
        let items = source_engine.page_items(&source_doc, 0).unwrap();
        let shared = SharedClipboard::default();
        let mut source = State {
        comparison_original: None,
            working_copy: None,
            clipboard: shared.clone(),
            engine: Some(source_engine),
            engine_error: String::new(),
            doc: Some(source_doc),
            generation: 1,
            policy: Substitution::default(),
        };
        assert!(matches!(
            handle(&mut source, Request::CopyItems(vec![items[0].clone()])),
            Some(Reply::ItemsCopied)
        ));
        assert!(!source.doc.as_ref().unwrap().can_undo());
        drop(source);
        let destination_engine = Engine::new(&library).unwrap();
        let destination_doc = destination_engine
            .open(&root.join("tests/fixtures/generated/rotated.pdf"))
            .unwrap();
        let count = destination_engine
            .page_items(&destination_doc, 0)
            .unwrap()
            .len();
        let mut destination = State {
        comparison_original: None,
            working_copy: None,
            clipboard: shared.clone(),
            engine: Some(destination_engine),
            engine_error: String::new(),
            doc: Some(destination_doc),
            generation: 1,
            policy: Substitution::default(),
        };
        assert!(matches!(
            handle(&mut destination, Request::PasteItems(0)),
            Some(Reply::Changed { can_undo: true, .. })
        ));
        assert_eq!(
            destination
                .engine
                .as_ref()
                .unwrap()
                .page_items(destination.doc.as_ref().unwrap(), 0)
                .unwrap()
                .len(),
            count + 1
        );
        assert!(matches!(
            handle(&mut destination, Request::Undo),
            Some(Reply::Changed { can_redo: true, .. })
        ));
        assert_eq!(
            destination
                .engine
                .as_ref()
                .unwrap()
                .page_items(destination.doc.as_ref().unwrap(), 0)
                .unwrap()
                .len(),
            count
        );
        // Closing the source and undoing a paste do not consume the shared copy.
        assert!(matches!(
            handle(&mut destination, Request::PasteItems(0)),
            Some(Reply::Changed { .. })
        ));
    }
}

#[cfg(test)]
#[path = "latency_tests.rs"]
mod latency_tests;

#[cfg(test)]
mod operation_tests {
    use super::*;
    #[test]
    fn cancelling_the_active_job_never_cancels_a_later_job() {
        let active = ActiveJob::default();
        assert!(!active.cancel());
        let first = active.start();
        assert!(active.cancel()); assert!(first.is_cancelled());
        active.finish(); assert!(!active.cancel()); assert!(active.progress().is_none());
        let second = active.start(); assert!(!second.is_cancelled());
        active.finish();
    }
}
