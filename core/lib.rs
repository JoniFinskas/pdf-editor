//! Headless PDF editing. Page indices are zero based; geometry is in PDF points.
mod bookmarks;
pub use bookmarks::{Bookmark, BookmarkEdit, BookmarkSet};

mod annotations;
mod change_state;
pub use annotations::{Annotation, Markup, MarkupKind};
mod compression;
mod content;
mod crop;
pub use change_state::ChangeState;
pub use compression::CompressionReport;
mod drawing;
mod export;
pub use drawing::{Drawing, DrawingKind};
mod features;
pub use export::PageImageFormat;
pub use features::NewText;
mod fonts;
mod forms;
pub use forms::{FieldValue, FormField};
mod images;
mod pictures;
pub use pictures::PlacedPicture;
mod ocr;
mod organize;
mod pages;
pub use organize::page_ranges;
mod passwords;
pub use passwords::PASSWORD_REQUIRED;
mod printing;
pub use images::{Picture, Placement, signature_files, signature_image, store_signature};
pub use pages::PageEdit;
pub use printing::PrintJob;
mod redaction;
mod replace;
mod search;
mod settings;
mod stamps;
mod startup;
pub use search::SearchMatch;
pub use stamps::TextStamp;
pub use startup::startup_path;
#[cfg(test)]
mod tests;

use pdfium_render::prelude::*;
pub use settings::{Settings, Substitution};
use std::{
    fmt, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_FILE: usize = 64 * 1024 * 1024;
const MAX_STREAM: usize = 16 * 1024 * 1024;
const MAX_HISTORY: usize = 256 * 1024 * 1024;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self("File operation failed; check access and available disk space".into())
    }
}
impl From<lopdf::Error> for Error {
    fn from(_: lopdf::Error) -> Self {
        Self("Malformed or unsupported PDF structure".into())
    }
}
impl From<PdfiumError> for Error {
    fn from(_: PdfiumError) -> Self {
        Self("PDFium could not process this document".into())
    }
}
fn err(message: &str) -> Error {
    Error(message.into())
}
fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|_| err("Malformed document or font could not be processed"))?
}
fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(err("File exceeds the supported size limit"));
    }
    Ok(bytes)
}
fn parse(bytes: &[u8]) -> Result<lopdf::Document> {
    if bytes.len() > MAX_FILE {
        return Err(err("PDF exceeds 64 MiB"));
    }
    let doc = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions::with_max_decompressed_size(MAX_STREAM),
    )
    .map_err(|e| match e {
        lopdf::Error::InvalidPassword | lopdf::Error::Decryption(_) => {
            err("Encrypted PDFs are not supported")
        }
        other => other.into(),
    })?;
    if doc.is_encrypted() || doc.was_encrypted() || doc.trailer.has(b"Encrypt") {
        return Err(err("Encrypted PDFs are not supported"));
    }
    let count = doc.get_pages().len();
    if count == 0 || count > 10_000 {
        return Err(err("Invalid or excessive page count"));
    }
    Ok(doc)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}
impl Bounds {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.bottom && y <= self.top
    }
}
#[derive(Clone, Debug)]
pub struct TextObject {
    pub page: u16,
    pub index: usize,
    pub revision: u64,
    pub text: String,
    pub font: String,
    pub size: f32,
    pub bounds: Bounds,
    pub color: [u8; 4],
    pub matrix: [f32; 6],
}
#[derive(Clone, Debug)]
pub struct PageInfo {
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct EditResult {
    pub warning: Option<String>,
    pub objects_changed: usize,
}

/// A PDFium runtime. Load only an explicitly trusted, absolute DLL path.
pub struct Engine {
    pdfium: &'static Pdfium,
    raw: &'static dyn PdfiumLibraryBindings,
}
static RUNTIME: std::sync::OnceLock<(PathBuf, Pdfium, Box<dyn PdfiumLibraryBindings>)> =
    std::sync::OnceLock::new();
static INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Editable state uses immutable byte snapshots so failed operations cannot partially apply.
pub struct Document {
    bytes: Vec<u8>,
    original: PathBuf,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
    revision: u64,
    protected: bool,
    pub substitution: Substitution,
}
impl Engine {
    pub fn new(library: &Path) -> Result<Self> {
        if !library.is_absolute() {
            return Err(err("PDFium library path must be absolute"));
        }
        let library = library.canonicalize()?;
        let _lock = INIT
            .lock()
            .map_err(|_| err("PDFium initialization failed"))?;
        if RUNTIME.get().is_none() {
            let raw = Pdfium::bind_to_library(&library)?;
            let pdfium = Pdfium::new(Pdfium::bind_to_library(&library)?);
            RUNTIME
                .set((library.clone(), pdfium, raw))
                .map_err(|_| err("PDFium initialization failed"))?;
        }
        let (loaded, pdfium, raw) = RUNTIME
            .get()
            .ok_or_else(|| err("PDFium initialization failed"))?;
        if loaded != &library {
            return Err(err("A different PDFium library is already loaded"));
        }
        Ok(Self {
            pdfium,
            raw: raw.as_ref(),
        })
    }
    pub fn open(&self, path: &Path) -> Result<Document> {
        self.open_with_password(path, "")
    }
    pub fn pages(&self, doc: &Document) -> Result<Vec<PageInfo>> {
        guarded(|| {
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            pdf.pages()
                .iter()
                .map(|p| {
                    Ok(PageInfo {
                        width: p.width().value,
                        height: p.height().value,
                    })
                })
                .collect()
        })
    }
    pub fn text_objects(&self, doc: &Document, page: u16) -> Result<Vec<TextObject>> {
        guarded(|| {
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let p = pdf.pages().get(i32::from(page))?;
            let page_text = p.text()?;
            let mut result = vec![];
            for (index, object) in p.objects().iter().enumerate() {
                if let Some(text) = object.as_text_object() {
                    let b = text.bounds()?;
                    let c = text.fill_color()?;
                    let m = text.matrix()?;
                    result.push(TextObject {
                        page,
                        index,
                        revision: doc.revision,

                        text: native_text(text, &page_text)?,
                        font: text.font().name(),
                        size: text.unscaled_font_size().value,
                        bounds: Bounds {
                            left: b.left().value,
                            bottom: b.bottom().value,
                            right: b.right().value,
                            top: b.top().value,
                        },
                        color: [c.red(), c.green(), c.blue(), c.alpha()],
                        matrix: [m.a(), m.b(), m.c(), m.d(), m.e(), m.f()],
                    });
                }
            }
            Ok(result)
        })
    }
    pub fn hit_test(
        &self,
        doc: &Document,
        page: u16,
        x: f32,
        y: f32,
    ) -> Result<Option<TextObject>> {
        Ok(self
            .text_objects(doc, page)?
            .into_iter()
            .rev()
            .find(|o| o.bounds.contains(x, y)))
    }
    pub fn render(&self, doc: &Document, page: u16, scale: f32) -> Result<Image> {
        guarded(|| {
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let p = pdf.pages().get(i32::from(page))?;
            let config = render_config(&p, scale)?;
            let bitmap = p.render_with_config(&config)?;
            Ok(Image {
                width: bitmap.width() as u32,
                height: bitmap.height() as u32,
                rgba: bitmap.as_rgba_bytes(),
            })
        })
    }
    pub fn pixel_to_page(
        &self,
        doc: &Document,
        page: u16,
        scale: f32,
        x: i32,
        y: i32,
    ) -> Result<(f32, f32)> {
        guarded(|| {
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let p = pdf.pages().get(i32::from(page))?;
            let (x, y) = p.pixels_to_points(x, y, &render_config(&p, scale)?)?;
            Ok((x.value, y.value))
        })
    }
    pub fn page_to_pixel(
        &self,
        doc: &Document,
        page: u16,
        scale: f32,
        x: f32,
        y: f32,
    ) -> Result<(i32, i32)> {
        guarded(|| {
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let p = pdf.pages().get(i32::from(page))?;
            Ok(p.points_to_pixels(
                PdfPoints::new(x),
                PdfPoints::new(y),
                &render_config(&p, scale)?,
            )?)
        })
    }
    pub fn edit(
        &self,
        doc: &mut Document,
        selected: &TextObject,
        replacement: &str,
    ) -> Result<EditResult> {
        self.edit_group(doc, std::slice::from_ref(selected), replacement)
    }
    /// Replaces adjacent runs on one page as a single edit: the first run in
    /// paint order receives `replacement` and the others are removed.
    pub fn edit_group(
        &self,
        doc: &mut Document,
        selected: &[TextObject],
        replacement: &str,
    ) -> Result<EditResult> {
        guarded(|| {
            let first = selected.first().ok_or_else(|| err("Nothing selected"))?;
            if selected.len() > 4096 || selected.iter().any(|o| o.page != first.page) {
                return Err(err("Select runs from one page"));
            }
            if selected.iter().any(|o| o.revision != doc.revision) {
                return Err(err("Selection is stale; select the object again"));
            }
            if replacement.chars().count() > 16_384 || replacement.chars().any(char::is_control) {
                return Err(err(
                    "Replacement must be a single run of at most 16384 printable characters",
                ));
            }
            let objects = self.text_objects(doc, first.page)?;
            let mut ordinals = selected
                .iter()
                .map(|s| {
                    objects
                        .iter()
                        .position(|o| o.index == s.index)
                        .ok_or_else(|| err("Text object no longer exists"))
                })
                .collect::<Result<Vec<_>>>()?;
            ordinals.sort_unstable();
            ordinals.dedup();
            if ordinals.len() != selected.len() {
                return Err(err("The same run was selected twice"));
            }
            let ordinal = ordinals[0];
            let actual = &objects[ordinal];
            if ordinals.len() == 1 && actual.text == replacement {
                return Ok(EditResult {
                    warning: None,
                    objects_changed: 0,
                });
            }
            let targets: Vec<(usize, String)> = ordinals
                .iter()
                .enumerate()
                .map(|(i, &o)| {
                    (
                        o,
                        if i == 0 {
                            replacement.to_string()
                        } else {
                            String::new()
                        },
                    )
                })
                .collect();
            let (bytes, result) = content::edit(self, doc, actual, &targets)?;
            if bytes.len() > MAX_FILE {
                return Err(err("Edited PDF exceeds 64 MiB"));
            }
            // Verify through the independent native parser before committing the transaction.
            let candidate = Document {
                bytes,
                original: doc.original.clone(),
                undo: vec![],
                redo: vec![],
                revision: doc.revision,
                protected: doc.protected,
                substitution: doc.substitution,
            };
            let reopened = self.text_objects(&candidate, first.page)?;
            let removed = targets.iter().filter(|(_, t)| t.is_empty()).count();
            if reopened.len() + removed != objects.len()
                || (!replacement.is_empty()
                    && reopened.get(ordinal).map(|o| o.text.as_str()) != Some(replacement))
            {
                return Err(err(
                    "Edited text failed round-trip verification; document unchanged",
                ));
            }
            let old = std::mem::replace(&mut doc.bytes, candidate.bytes);
            doc.undo.push(old);
            doc.redo.clear();
            while doc.undo.iter().map(Vec::len).sum::<usize>() > MAX_HISTORY {
                doc.undo.remove(0);
            }
            doc.revision = doc.revision.wrapping_add(1);
            Ok(result)
        })
    }
}
fn native_text<'a>(
    object: &PdfPageTextObject<'a>,
    page_text: &'a PdfPageText<'a>,
) -> Result<String> {
    let mut result = String::new();
    for ch in object.chars(page_text)?.iter() {
        if !ch.is_generated()? {
            result.push(
                ch.unicode_char()
                    .ok_or_else(|| err("Invalid text character"))?,
            );
        }
    }
    Ok(result)
}
fn render_config(page: &PdfPage<'_>, scale: f32) -> Result<PdfRenderConfig> {
    let width = (page.width().value * scale).ceil();
    let height = (page.height().value * scale).ceil();
    if !scale.is_finite()
        || scale <= 0.0
        || !width.is_finite()
        || !height.is_finite()
        || width < 1.0
        || height < 1.0
        || width * height > 16_000_000.0
    {
        return Err(err("Render dimensions exceed the 16 megapixel limit"));
    }
    Ok(PdfRenderConfig::new()
        .set_target_size(width as i32, height as i32)
        .render_form_data(true)
        .render_annotations(true))
}
impl Document {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self) -> bool {
        if let Some(bytes) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.bytes, bytes));
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(bytes) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.bytes, bytes));
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }
    pub fn suggested_save_path(&self) -> PathBuf {
        let mut name = self.original.file_stem().unwrap_or_default().to_os_string();
        name.push("_edited.pdf");
        self.original.with_file_name(name)
    }
    pub fn save_as(&self, path: &Path) -> Result<()> {
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            || path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains(':'))
        {
            return Err(err("Save As requires a regular .pdf filename"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let name = path
            .file_name()
            .ok_or_else(|| err("Invalid output filename"))?;
        let resolved = parent.canonicalize()?.join(name);
        if resolved
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&self.original.as_os_str().to_string_lossy())
        {
            return Err(err("Save As cannot use the original path"));
        }
        // Exclusive creation rejects originals, existing files, hard links and symlinks alike.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| {
                err("Save As requires a new filename; existing files are never overwritten")
            })?;
        file.write_all(&self.bytes)?;
        file.sync_all()?;
        Ok(())
    }
}
