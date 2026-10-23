//! Background thread that owns the PDF engine and the open document.
//!
//! Every core call runs here so large documents never block the UI thread.
//! Results carry the document generation they were computed for; the UI drops
//! replies whose generation no longer matches.

use pdf_editor_core::{Document, Engine, PageInfo, Substitution, TextObject};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::mpsc::{Receiver, Sender, TryRecvError, channel},
    thread,
};

/// Raster coordinates are sampled at this scale and divided back down, which
/// keeps integer rounding in `page_to_pixel` far below a display pixel.
const SAMPLE_SCALE: f32 = 4.0;
const SAMPLE_SPAN: f32 = 1000.0;

pub enum Request {
    Open(PathBuf),
    Print,
    FormFields,
    FillField(pdf_editor_core::FormField, pdf_editor_core::FieldValue),
    PlacePicture(pdf_editor_core::Picture, pdf_editor_core::Placement),
    Find(String),
    Fonts,
    Thumbnail(u16),
    Pages(pdf_editor_core::PageEdit),
    DeleteText(TextObject),
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
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

pub enum Reply {
    Printed(bool),
    FormFields(Vec<pdf_editor_core::FormField>),
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

pub struct Worker {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
}

impl Worker {
    pub fn spawn(ctx: eframe::egui::Context, library: PathBuf, policy: Substitution) -> Self {
        let (requests, inbox) = channel();
        let (outbox, replies) = channel();
        // If spawning fails the closure and its sender are dropped, so the
        // first poll reports a disconnected engine.
        let _ = thread::Builder::new()
            .name("pdf-engine".into())
            .spawn(move || run(ctx, library, policy, inbox, outbox));
        Self { requests, replies }
    }

    /// Returns false when the engine thread is gone.
    pub fn send(&self, request: Request) -> bool {
        self.requests.send(request).is_ok()
    }

    /// Drains pending replies. Returns None when the engine thread has stopped.
    pub fn poll(&self) -> Option<Vec<Reply>> {
        let mut out = Vec::new();
        loop {
            match self.replies.try_recv() {
                Ok(reply) => out.push(reply),
                Err(TryRecvError::Empty) => return Some(out),
                Err(TryRecvError::Disconnected) => {
                    return if out.is_empty() { None } else { Some(out) };
                }
            }
        }
    }
}

struct State {
    engine: Option<Engine>,
    engine_error: String,
    doc: Option<Document>,
    generation: u64,
    policy: Substitution,
}

fn run(
    ctx: eframe::egui::Context,
    library: PathBuf,
    policy: Substitution,
    inbox: Receiver<Request>,
    outbox: Sender<Reply>,
) {
    let (engine, engine_error) = match Engine::new(&library) {
        Ok(engine) => (Some(engine), String::new()),
        Err(e) => (
            None,
            format!("The PDF engine could not start: {e}. Place pdfium.dll next to the program."),
        ),
    };
    let mut state = State {
        engine,
        engine_error,
        doc: None,
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
            let reply = catch_unwind(AssertUnwindSafe(|| handle(&mut state, request)))
                .unwrap_or_else(|_| {
                    Some(Reply::Failed {
                        message: "An internal error occurred while processing the document.".into(),
                        blocking: true,
                        page: None,
                    })
                });
            if let Some(reply) = reply {
                if outbox.send(reply).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        }
    }
}

fn coalesce(batch: Vec<Request>) -> Vec<Request> {
    let mut out: Vec<Request> = Vec::with_capacity(batch.len());
    for request in batch {
        match &request {
            Request::Render { page, .. } => {
                out.retain(|r| !matches!(r, Request::Render { page: p, .. } if p == page));
            }
            Request::Objects { page } => {
                out.retain(|r| !matches!(r, Request::Objects { page: p } if p == page));
            }
            _ => {}
        }
        out.push(request);
    }
    out
}

fn handle(state: &mut State, request: Request) -> Option<Reply> {
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
    let blocking = page.is_none() && !matches!(request, Request::Find(_) | Request::Fonts);
    let failed = |e: pdf_editor_core::Error| {
        Some(Reply::Failed {
            message: e.to_string(),
            blocking,
            page,
        })
    };

    match request {
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
        Request::StyleText {
            object,
            size,
            color,
        } => {
            let doc = state.doc.as_mut()?;
            match engine.style_text(doc, &object, size, color) {
                Ok(()) => {
                    state.generation += 1;
                    Some(changed(state, ChangeKind::Edit, None, 1))
                }
                Err(e) => failed(e),
            }
        }
        Request::DeleteText(object) => {
            let doc = state.doc.as_mut()?;
            match engine.delete_text(doc, &object) {
                Ok(result) => {
                    state.generation += 1;
                    Some(changed(
                        state,
                        ChangeKind::Edit,
                        result.warning,
                        result.objects_changed,
                    ))
                }
                Err(e) => failed(e),
            }
        }
        Request::FormFields => match engine.form_fields(state.doc.as_ref()?) {
            Ok(fields) => Some(Reply::FormFields(fields)),
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
        Request::Open(path) => {
            let mut doc = match engine.open(&path) {
                Ok(doc) => doc,
                Err(e) => return failed(e),
            };
            let pages = match engine.pages(&doc) {
                Ok(pages) => pages,
                Err(e) => return failed(e),
            };
            doc.substitution = state.policy;
            let suggested = doc.suggested_save_path();
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
            })
        }
        Request::Edit { objects, text } => {
            let doc = state.doc.as_mut()?;
            match engine.edit_group(doc, &objects, &text) {
                Ok(result) => {
                    if result.objects_changed > 0 {
                        state.generation += 1;
                    }
                    Some(changed(
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
        Request::SaveAs(path) => {
            let doc = state.doc.as_ref()?;
            match doc.save_as(&path) {
                Ok(()) => Some(Reply::Saved(path)),
                Err(e) => failed(e),
            }
        }
        Request::Snapshot(path) => {
            let doc = state.doc.as_ref()?;
            let error = doc.save_as(&path).err().map(|e| e.to_string());
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
        Request::Render { page, .. } | Request::Objects { page } | Request::Thumbnail(page) => {
            Some(*page)
        }
        _ => None,
    }
}

fn changed(
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
        pages: state
            .engine
            .as_ref()
            .zip(state.doc.as_ref())
            .and_then(|(e, d)| e.pages(d).ok())
            .unwrap_or_default(),
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
    (Worker { requests, replies }, inbox, outbox)
}
