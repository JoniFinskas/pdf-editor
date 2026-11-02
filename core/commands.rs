//! Local JSON-line document editing. No shell, network, dialogs or printer access.
use crate::*;
use serde_json::{Value as V, json};
use std::{collections::BTreeSet, path::Component};
#[path = "commands_edit.rs"]
mod edit;
#[path = "commands_inspect.rs"]
mod inspect;
#[path = "commands_schema.rs"]
mod schema;

pub const MAX_COMMAND_BYTES: usize = 1_048_576;
// Field lists are also the capability contract; reject misspelled arguments.
const OPS: &[(&str, &str, bool)] = &[
    ("help", "", false),
    ("new", "width height pages preset landscape discard", false),
    (
        "cv.create",
        "name headline contact sections letter discard",
        false,
    ),
    ("open", "path password discard", false),
    ("images.create", "paths size discard", false),
    ("inspect", "view page offset limit", false),
    ("fonts", "", false),
    ("cv.check", "", false),
    ("document.policy", "substitution", true),
    ("save", "path password", false),
    ("save.update", "", false),
    ("text.add", "page x y text font size color", true),
    ("text.edit", "page indices text", true),
    ("text.delete", "page indices", true),
    ("text.style", "page index size color", true),
    (
        "text.format",
        "page indices font bold italic size color",
        true,
    ),
    ("paragraph.edit", "page index text", true),
    (
        "text.replace",
        "find text occurrence case_sensitive whole_word pages regions",
        true,
    ),
    (
        "text.find",
        "find case_sensitive whole_word pages regions",
        false,
    ),
    ("text.spelling", "page index language", false),
    ("text.highlight", "regions", true),
    ("text.characters", "page offset limit", false),
    ("objects.edit", "page indices action dx dy bounds", true),
    ("objects.layout", "page indices alignment", true),
    ("objects.copy", "page indices", false),
    ("objects.paste", "page", true),
    ("objects.ungroup", "page index", true),
    ("objects.prepare_text", "page index", true),
    ("page.blank", "before width height", true),
    ("page.rotate", "pages", true),
    ("page.delete", "pages", true),
    ("page.move", "pages before", true),
    ("page.duplicate", "pages before", true),
    ("page.import", "path before", true),
    ("page.merge", "paths", true),
    ("page.crop", "page bounds", true),
    ("page.extract", "pages path", false),
    ("page.split", "ranges folder", false),
    ("page.split_bookmarks", "folder", false),
    ("image.add", "page path left bottom width height", true),
    ("image.move", "page name left bottom width height", true),
    ("draw", "page kind points color width opacity", true),
    (
        "annotation.add",
        "page kind bounds color opacity text",
        true,
    ),
    (
        "annotation.update",
        "page index kind bounds color opacity text",
        true,
    ),
    ("annotation.delete", "page index", true),
    ("link.add", "page bounds uri target_page", true),
    ("link.update", "page index bounds uri target_page", true),
    ("link.delete", "page index", true),
    ("form.add", "page name kind bounds required options", true),
    ("form.update", "page id name bounds required", true),
    ("form.delete", "page id", true),
    ("form.fill", "page index text checked selected", true),
    ("form.flatten", "", true),
    ("bookmark.add", "title page parent", true),
    ("bookmark.rename", "id title", true),
    ("bookmark.delete", "id", true),
    ("bookmark.move", "id position", true),
    (
        "stamp",
        "text pages font size color opacity x y date placement",
        true,
    ),
    (
        "bates",
        "prefix start digits text pages font size color opacity x y date placement",
        true,
    ),
    ("ocr", "pages", true),
    ("compress", "edge quality", true),
    ("scan.cleanup", "page corners deskew color ocr", true),
    ("scan.preview", "page path", false),
    ("privacy.preview", "kind text", false),
    ("privacy.apply", "preview", true),
    ("redact", "regions", true),
    ("sanitize", "comments links flatten", true),
    ("export.sanitized", "comments links flatten path", false),
    ("render", "page path dpi format", false),
    ("export.images", "pages folder dpi format", false),
    ("export.word", "path", false),
    ("table.detect", "page bounds", false),
    ("table.edit", "cells", false),
    ("table.export", "path format", false),
    ("compare.load", "path password", false),
    ("compare", "page revised_page visual", false),
    ("history.undo", "", true),
    ("history.redo", "", true),
];
pub fn command_schema() -> String {
    schema::describe().to_string()
}
pub struct CommandResponse {
    pub ok: bool,
    pub json: String,
}
pub struct CommandSession<'a> {
    engine: &'a Engine,
    root: PathBuf,
    doc: Option<Document>,
    number: u64,
    count: usize,
    changes: ChangeState,
    copy: Option<WorkingCopy>,
    clipboard: Option<ObjectClipboard>,
    reference: Option<Document>,
    preview: Option<(String, RedactionPreview)>,
    preview_number: u64,
    table: Option<(u64, TableGrid)>,
}
impl<'a> CommandSession<'a> {
    pub fn new(engine: &'a Engine, root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(err("Command workspace must be an existing directory"));
        }
        Ok(Self {
            engine,
            root,
            doc: None,
            number: 0,
            count: 0,
            changes: ChangeState::default(),
            copy: None,
            clipboard: None,
            reference: None,
            preview: None,
            preview_number: 0,
            table: None,
        })
    }
    pub fn input_file(&self, value: &str) -> Result<PathBuf> {
        self.path(value, false, false)
    }
    fn path(&self, value: &str, output: bool, folder: bool) -> Result<PathBuf> {
        let meta = fs::symlink_metadata(&self.root)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(err("Command workspace was redirected"));
            }
        }
        if !meta.is_dir() || meta.file_type().is_symlink() || self.root.canonicalize()? != self.root
        {
            return Err(err("Command workspace changed"));
        }
        if folder && value == "." {
            return Ok(self.root.clone());
        }
        if value.is_empty()
            || value.len() > 4096
            || value.chars().any(char::is_control)
            || value.contains(':')
        {
            return Err(err("Use a relative workspace path"));
        }
        let relative = Path::new(value);
        if relative.is_absolute() {
            return Err(err("Absolute command paths are not allowed"));
        }
        let mut path = self.root.clone();
        let components: Vec<_> = relative.components().collect();
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err(err("Path traversal is not allowed"));
            };
            let name_text = name.to_string_lossy();
            if name_text.ends_with(['.', ' ']) {
                return Err(err("Ambiguous path component"));
            }
            path.push(name);
            match fs::symlink_metadata(&path) {
                Ok(meta) => {
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        if meta.file_attributes() & 0x400 != 0 {
                            return Err(err("Redirected paths are not allowed"));
                        }
                    }
                    if meta.file_type().is_symlink() {
                        return Err(err("Symbolic links are not allowed"));
                    }
                    if output && !folder && index + 1 == components.len() {
                        return Err(err("Output already exists"));
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::NotFound
                        && output
                        && !folder
                        && index + 1 == components.len() => {}
                Err(_) => return Err(err("Workspace path is unavailable")),
            }
        }
        let parent = if output && !folder {
            path.parent()
                .ok_or_else(|| err("Invalid output path"))?
                .canonicalize()?
        } else {
            path.canonicalize()?
        };
        if !parent.starts_with(&self.root) {
            return Err(err("Path escaped the workspace"));
        }
        if folder && !path.is_dir() || !output && !folder && !path.is_file() {
            return Err(err("Wrong workspace file type"));
        }
        Ok(path)
    }
    pub fn execute(&mut self, line: &str) -> CommandResponse {
        self.execute_with_control(line, &OperationControl::default())
    }
    pub fn execute_with_control(
        &mut self,
        line: &str,
        control: &OperationControl,
    ) -> CommandResponse {
        self.count = self.count.saturating_add(1);
        let parsed = if line.len() > MAX_COMMAND_BYTES || self.count > 10000 {
            Err(err("Command limit exceeded"))
        } else {
            serde_json::from_str::<V>(line).map_err(|_| err("Invalid JSON command"))
        };
        let id = parsed
            .as_ref()
            .ok()
            .and_then(|v| v.get("request_id"))
            .filter(|v| v.is_number() || v.as_str().is_some_and(|s| s.len() <= 128))
            .cloned()
            .unwrap_or(V::Null);
        let engine = self.engine;
        let result = parsed.and_then(|v| engine.with_operation(control, || self.run(&v)));
        let ok = result.is_ok();
        let mut output = match result {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(e) => json!({"ok":false,"error":e.0}),
        };
        output["request_id"] = id;
        output["document"] = json!(self.number);
        output["revision"] = json!(self.doc.as_ref().map(Document::revision));
        output["dirty"] = json!(self.doc.is_some() && self.changes.is_dirty());
        let mut json = output.to_string();
        if json.len() > 8_388_608 {
            output
                .as_object_mut()
                .expect("Response is an object")
                .remove("result");
            output["ok"] = json!(false);
            output["error"] = json!("Response exceeds limit; inspect a smaller range");
            json = output.to_string();
            return CommandResponse { ok: false, json };
        }
        CommandResponse { ok, json }
    }
    fn install(&mut self, doc: Document, new: bool) -> V {
        self.number += 1;
        self.reference = Some(doc.comparison_snapshot());
        self.doc = Some(doc);
        self.copy = None;
        self.preview = None;
        self.table = None;
        self.changes = if new {
            ChangeState::recovered()
        } else {
            ChangeState::default()
        };
        json!({"created":new})
    }
    fn run(&mut self, value: &V) -> Result<V> {
        let a = Args(value);
        let op = a.s("op")?;
        let (_, fields, revision) = OPS
            .iter()
            .find(|(name, _, _)| *name == op)
            .ok_or_else(|| err("Unknown editing command; use help"))?;
        let object = value
            .as_object()
            .ok_or_else(|| err("Command must be an object"))?;
        if let Some(id) = object.get("request_id")
            && !(id.is_number() || id.as_str().is_some_and(|s| s.len() <= 128))
        {
            return Err(err(
                "request_id must be a number or a string up to 128 bytes",
            ));
        }
        if object.keys().any(|key| {
            !matches!(key.as_str(), "op" | "request_id" | "document" | "revision")
                && !fields.split_whitespace().any(|field| field == key)
        }) {
            return Err(err("Unknown command field"));
        }
        if (*revision || a.0.get("document").is_some() || a.0.get("revision").is_some())
            && (a.u("document")? != self.number
                || Some(a.u("revision")?) != self.doc.as_ref().map(Document::revision))
        {
            return Err(err(
                "Stale document or revision; inspect the current document",
            ));
        }
        match op {
            "help" => {
                return serde_json::from_str(&command_schema())
                    .map_err(|_| err("Invalid capability schema"));
            }
            "fonts" => return Ok(json!(self.engine.system_fonts()?)),
            "new" | "cv.create" | "open" | "images.create" => {
                if self.doc.is_some() && self.changes.is_dirty() && !a.b("discard", false)? {
                    return Err(err(
                        "Save the current document or explicitly set discard: true",
                    ));
                }
                let doc = match op {
                    "new" => self.engine.blank_document(blank(&a)?)?,
                    "cv.create" => self.engine.create_cv(&cv(&a)?)?,
                    "open" => self.engine.open_with_password(
                        &self.path(a.s("path")?, false, false)?,
                        a.text("password", "")?,
                    )?,
                    _ => {
                        let paths = a
                            .strings("paths")?
                            .iter()
                            .map(|p| self.path(p, false, false))
                            .collect::<Result<Vec<_>>>()?;
                        self.engine
                            .images_to_pdf(&paths, image_size(a.text("size", "a4")?)?)?
                    }
                };
                return Ok(self.install(doc, op != "open"));
            }
            _ => {}
        }
        let mut doc = self
            .doc
            .take()
            .ok_or_else(|| err("Create or open a document first"))?;
        let before = doc.revision();
        let result = self.with_document(op, &a, &mut doc);
        if doc.revision() != before {
            match op {
                "history.undo" => self.changes.undone(),
                "history.redo" => self.changes.redone(),
                _ => self.changes.edited(),
            };
            self.preview = None;
            self.table = None;
        }
        self.doc = Some(doc);
        result
    }
    fn with_document(&mut self, op: &str, a: &Args<'_>, doc: &mut Document) -> Result<V> {
        match op {
            "inspect" => inspect::inspect(self.engine, doc, a),
            "cv.check" => {
                let check = self.engine.check_cv(doc)?;
                Ok(
                    json!({"pages":check.pages,"bytes":check.bytes,"text":check.text,"warnings":check.warnings,"scope":"Text extraction checks, not an ATS ranking or acceptance guarantee"}),
                )
            }
            "save" => {
                if doc.is_password_protected() && a.0.get("password").is_none() {
                    return Err(err(
                        "Choose an explicit output password, or an empty password for an unencrypted copy",
                    ));
                }
                let path = self.path(a.s("path")?, true, false)?;
                let copy = doc.create_working_copy(&path, a.text("password", "")?)?;
                let encrypted = copy.encrypted();
                self.copy = Some(copy);
                self.changes.mark_saved();
                Ok(json!({"saved":true,"encrypted":encrypted}))
            }
            "save.update" => {
                let relative = self
                    .copy
                    .as_ref()
                    .ok_or_else(|| err("Save a new working copy first"))?
                    .path()
                    .strip_prefix(&self.root)
                    .map_err(|_| err("Working copy escaped the workspace"))?
                    .to_str()
                    .ok_or_else(|| err("Invalid working-copy path"))?
                    .to_owned();
                self.path(&relative, false, false)?;
                let copy = self
                    .copy
                    .as_mut()
                    .ok_or_else(|| err("Save a new working copy first"))?;
                doc.update_working_copy(copy)?;
                self.changes.mark_saved();
                Ok(json!({"saved":true}))
            }
            "history.undo" => Ok(json!({"changed":doc.undo()})),
            "history.redo" => Ok(json!({"changed":doc.redo()})),
            _ => edit::execute(self, doc, op, a),
        }
    }
}
struct Args<'a>(&'a V);
impl<'a> Args<'a> {
    fn s(&self, key: &str) -> Result<&'a str> {
        self.0
            .get(key)
            .and_then(V::as_str)
            .ok_or_else(|| err(&format!("Expected string: {key}")))
    }
    fn text(&self, key: &str, default: &'a str) -> Result<&'a str> {
        if self.0.get(key).is_none() {
            Ok(default)
        } else {
            self.s(key)
        }
    }
    fn u(&self, key: &str) -> Result<u64> {
        self.0
            .get(key)
            .and_then(V::as_u64)
            .ok_or_else(|| err(&format!("Expected nonnegative integer: {key}")))
    }
    fn integer(&self, key: &str, default: u64) -> Result<u64> {
        if self.0.get(key).is_none() {
            Ok(default)
        } else {
            self.u(key)
        }
    }
    fn n(&self, key: &str, default: f32) -> Result<f32> {
        match self.0.get(key) {
            None => Ok(default),
            Some(v) => v
                .as_f64()
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .ok_or_else(|| err(&format!("Expected finite number: {key}"))),
        }
    }
    fn b(&self, key: &str, default: bool) -> Result<bool> {
        match self.0.get(key) {
            None => Ok(default),
            Some(v) => v
                .as_bool()
                .ok_or_else(|| err(&format!("Expected boolean: {key}"))),
        }
    }
    fn index(&self, key: &str) -> Result<usize> {
        usize::try_from(self.u(key)?).map_err(|_| err("Index is too large"))
    }
    fn page(&self) -> Result<u16> {
        self.0.get("page").map_or(Ok(0), |_| self.page_key("page"))
    }
    fn page_key(&self, key: &str) -> Result<u16> {
        u16::try_from(self.u(key)?).map_err(|_| err("Page is too large"))
    }
    fn array(&self, key: &str) -> Result<&'a Vec<V>> {
        self.0
            .get(key)
            .and_then(V::as_array)
            .filter(|a| a.len() <= 10000)
            .ok_or_else(|| err(&format!("Expected bounded array: {key}")))
    }
    fn strings(&self, key: &str) -> Result<Vec<String>> {
        self.array(key)?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| err("Expected strings"))
            })
            .collect()
    }
    fn indices(&self) -> Result<Vec<usize>> {
        let values = self
            .array("indices")?
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| err("Expected object indices"))
            })
            .collect::<Result<Vec<_>>>()?;
        if values.is_empty()
            || values.iter().copied().collect::<BTreeSet<_>>().len() != values.len()
        {
            return Err(err("Select unique object indices"));
        }
        Ok(values)
    }
    fn pages(&self) -> Result<Vec<u16>> {
        self.array("pages")?
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| err("Expected page indices"))
            })
            .collect()
    }
    fn bounds(&self) -> Result<Bounds> {
        bounds(self.0.get("bounds").ok_or_else(|| err("Missing bounds"))?)
    }
    fn color(&self) -> Result<[u8; 3]> {
        match self.0.get("color") {
            None => Ok([0, 0, 0]),
            Some(V::Array(a)) if a.len() == 3 => Ok([byte(&a[0])?, byte(&a[1])?, byte(&a[2])?]),
            _ => Err(err("Color must be [red, green, blue] bytes")),
        }
    }
}
fn byte(v: &V) -> Result<u8> {
    v.as_u64()
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(|| err("Expected byte"))
}
fn number(v: &V) -> Result<f32> {
    v.as_f64()
        .map(|n| n as f32)
        .filter(|n| n.is_finite())
        .ok_or_else(|| err("Expected finite coordinate"))
}
fn bounds(v: &V) -> Result<Bounds> {
    let a = v
        .as_array()
        .filter(|v| v.len() == 4)
        .ok_or_else(|| err("Bounds must be [left, bottom, right, top]"))?;
    Ok(Bounds {
        left: number(&a[0])?,
        bottom: number(&a[1])?,
        right: number(&a[2])?,
        top: number(&a[3])?,
    })
}
fn rect(b: Bounds) -> V {
    json!([b.left, b.bottom, b.right, b.top])
}
fn blank(a: &Args<'_>) -> Result<BlankPdf> {
    let mut spec = match a.text("preset", "a4")? {
        "a4" => BlankPdf::default(),
        "letter" => BlankPdf {
            width: 612.,
            height: 792.,
            pages: 1,
        },
        "a5" => BlankPdf {
            width: 419.528,
            height: 595.276,
            pages: 1,
        },
        "custom" => BlankPdf::default(),
        _ => return Err(err("Preset must be a4, a5, letter or custom")),
    };
    spec.width = a.n("width", spec.width)?;
    spec.height = a.n("height", spec.height)?;
    if a.b("landscape", false)? {
        std::mem::swap(&mut spec.width, &mut spec.height);
    }
    spec.pages = match a.0.get("pages") {
        None => 1,
        Some(_) => a.page_key("pages")?,
    };
    spec.validate()?;
    Ok(spec)
}
fn cv(a: &Args<'_>) -> Result<CvSpec> {
    let sections = a
        .array("sections")?
        .iter()
        .map(|v| {
            let s = Args(v);
            if v.as_object()
                .is_none_or(|o| o.keys().any(|k| k != "heading" && k != "body"))
            {
                return Err(err("CV section fields are heading and body"));
            }
            Ok(CvSection {
                heading: s.s("heading")?.into(),
                body: s.s("body")?.into(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(CvSpec {
        name: a.s("name")?.into(),
        headline: a.text("headline", "")?.into(),
        contact: if a.0.get("contact").is_some() {
            a.strings("contact")?
        } else {
            vec![]
        },
        sections,
        letter: a.b("letter", false)?,
    })
}
fn image_size(value: &str) -> Result<ImagePageSize> {
    match value {
        "a4" => Ok(ImagePageSize::A4),
        "letter" => Ok(ImagePageSize::Letter),
        "image" => Ok(ImagePageSize::Image),
        _ => Err(err("Image size must be a4, letter or image")),
    }
}
fn texts(e: &Engine, doc: &Document, a: &Args<'_>) -> Result<Vec<TextObject>> {
    let objects = e.text_objects(doc, a.page()?)?;
    a.indices()?
        .into_iter()
        .map(|index| {
            objects
                .iter()
                .find(|o| o.index == index)
                .cloned()
                .ok_or_else(|| err("Text object not found"))
        })
        .collect()
}
fn items(e: &Engine, doc: &Document, a: &Args<'_>) -> Result<Vec<PageItem>> {
    let objects = e.page_items(doc, a.page()?)?;
    a.indices()?
        .into_iter()
        .map(|index| {
            objects
                .iter()
                .find(|o| o.index == index)
                .cloned()
                .ok_or_else(|| err("Page object not found"))
        })
        .collect()
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
