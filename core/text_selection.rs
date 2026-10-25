use crate::*;

/// One PDFium character in page reading order. Generated spaces and line breaks
/// are retained; indices are page-local and do not identify editable objects.
#[derive(Clone, Debug)]
pub struct TextCharacter {
    pub page: u16,
    pub index: usize,
    pub text: String,
    pub bounds: Bounds,
}

#[derive(Clone, Debug, Default)]
pub struct TextSelection {
    pub text: String,
    pub bounds: Vec<Bounds>,
}

/// Copies inclusive endpoints, accepting either drag direction. PDF line endings
/// are normalized to a single newline without trimming intentional whitespace.
pub fn selected_text(chars: &[TextCharacter], start: usize, end: usize) -> TextSelection {
    let mut result = TextSelection::default();
    let (start, end) = (start.min(end), start.max(end));
    let mut previous_cr = false;
    for character in chars.iter().filter(|c| (start..=end).contains(&c.index)) {
        for ch in character.text.chars() {
            if ch == '\r' {
                result.text.push('\n');
            } else if ch != '\n' || !previous_cr {
                result.text.push(ch);
            }
            previous_cr = ch == '\r';
        }
        if !character.text.chars().all(char::is_whitespace)
            && character.bounds.right > character.bounds.left
            && character.bounds.top > character.bounds.bottom
        {
            result.bounds.push(character.bounds);
        }
    }
    result
}

struct NativeText<'a> {
    raw: &'a dyn PdfiumLibraryBindings,
    document: FPDF_DOCUMENT,
    page: FPDF_PAGE,
    text: FPDF_TEXTPAGE,
}
impl Drop for NativeText<'_> {
    fn drop(&mut self) {
        unsafe {
            if !self.text.is_null() {
                self.raw.FPDFText_ClosePage(self.text);
            }
            if !self.page.is_null() {
                self.raw.FPDF_ClosePage(self.page);
            }
            if !self.document.is_null() {
                self.raw.FPDF_CloseDocument(self.document);
            }
        }
    }
}

pub(crate) struct IndexedCharacter {
    pub character: TextCharacter,
    pub owner: Option<usize>,
}

pub(crate) fn extract(engine: &Engine, doc: &Document, page: u16) -> Result<Vec<IndexedCharacter>> {
    let raw = engine.raw;
    let mut native = NativeText {
        raw,
        document: std::ptr::null_mut(),
        page: std::ptr::null_mut(),
        text: std::ptr::null_mut(),
    };
    // No document actions, JavaScript, form initialization, or callbacks run.
    unsafe {
        native.document = raw.FPDF_LoadMemDocument64(&doc.bytes, None);
        if native.document.is_null() {
            return Err(err("Cannot load text document"));
        }
        native.page = raw.FPDF_LoadPage(native.document, i32::from(page));
        if native.page.is_null() {
            return Err(err("Invalid text page"));
        }
        native.text = raw.FPDFText_LoadPage(native.page);
        if native.text.is_null() {
            return Err(err("Cannot load page text"));
        }
        let count = raw.FPDFText_CountChars(native.text);
        let object_count = raw.FPDFPage_CountObjects(native.page);
        if !(0..=1_000_000).contains(&count) || object_count < 0 {
            return Err(err("Invalid or excessive page text"));
        }
        let mut owners = std::collections::HashMap::new();
        for index in 0..object_count {
            let object = raw.FPDFPage_GetObject(native.page, index);
            if object.is_null() {
                return Err(err("Cannot read page object"));
            }
            owners.insert(object, index as usize);
        }
        let mut result = Vec::with_capacity(count as usize);
        for index in 0..count {
            operation::checkpoint("Reading text", index as usize, count as usize)?;
            let ch = char::from_u32(raw.FPDFText_GetUnicode(native.text, index))
                .ok_or_else(|| err("Invalid text character"))?;
            let generated = raw.FPDFText_IsGenerated(native.text, index);
            if generated != 0 && generated != 1 {
                return Err(err("Invalid generated character"));
            }
            let (mut left, mut right, mut bottom, mut top) = (0., 0., 0., 0.);
            if raw.FPDFText_GetCharBox(
                native.text,
                index,
                &mut left,
                &mut right,
                &mut bottom,
                &mut top,
            ) == 0
                || [left, right, bottom, top]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > f32::MAX as f64)
                || left > right
                || bottom > top
            {
                return Err(err("Invalid character bounds"));
            }
            let owner = if generated == 0 {
                owners
                    .get(&raw.FPDFText_GetTextObject(native.text, index))
                    .copied()
            } else {
                None
            };
            result.push(IndexedCharacter {
                character: TextCharacter {
                    page,
                    index: index as usize,
                    text: ch.to_string(),
                    bounds: Bounds {
                        left: left as f32,
                        right: right as f32,
                        bottom: bottom as f32,
                        top: top as f32,
                    },
                },
                owner,
            });
        }
        Ok(result)
    }
}

impl Engine {
    pub fn page_text_characters(&self, doc: &Document, page: u16) -> Result<Vec<TextCharacter>> {
        guarded(|| {
            Ok(extract(self, doc, page)?
                .into_iter()
                .map(|c| c.character)
                .collect())
        })
    }
}

impl Engine {
    pub fn highlight_text_selection(
        &self,
        doc: &mut Document,
        regions: &[(u16, Bounds)],
    ) -> Result<()> {
        guarded(|| {
            if regions.is_empty() || regions.len() > 10000 {
                return Err(err("Select text to highlight"));
            }
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
            for (index, (page, bounds)) in regions.iter().enumerate() {
                operation::checkpoint("Highlighting selection", index, regions.len())?;
                self.add_annotation(
                    &mut candidate,
                    &Markup {
                        page: *page,
                        kind: MarkupKind::Highlight,
                        bounds: *bounds,
                        color: [255, 220, 40],
                        opacity: 0.45,
                        text: String::new(),
                    },
                )?;
                candidate.undo.clear();
            }
            operation::checkpoint("Applying highlights", regions.len(), regions.len())?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
