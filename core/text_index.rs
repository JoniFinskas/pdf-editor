//! One-pass character ownership index for top-level page objects.
use crate::*;
use std::collections::HashMap;

struct TextPage<'a> {
    raw: &'a dyn PdfiumLibraryBindings,
    document: FPDF_DOCUMENT,
    page: FPDF_PAGE,
    text: FPDF_TEXTPAGE,
}
impl Drop for TextPage<'_> {
    fn drop(&mut self) {
        // Handles are owned here and closed in reverse order on every return path.
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

pub(crate) fn index(
    raw: &dyn PdfiumLibraryBindings,
    bytes: &[u8],
    page: u16,
) -> Result<Vec<String>> {
    let mut native = TextPage {
        raw,
        document: std::ptr::null_mut(),
        page: std::ptr::null_mut(),
        text: std::ptr::null_mut(),
    };
    // The input bytes outlive all native handles. No actions or form callbacks run.
    unsafe {
        native.document = raw.FPDF_LoadMemDocument64(bytes, None);
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
        let count = raw.FPDFPage_CountObjects(native.page);
        let chars = raw.FPDFText_CountChars(native.text);
        if count < 0 || chars < 0 {
            return Err(err("Cannot read page text structure"));
        }
        let mut strings = vec![String::new(); count as usize];
        let mut owners = HashMap::new();
        for i in 0..count {
            let object = raw.FPDFPage_GetObject(native.page, i);
            if object.is_null() {
                return Err(err("Cannot read page object"));
            }
            owners.insert(object, i as usize);
        }
        for i in 0..chars {
            let object = raw.FPDFText_GetTextObject(native.text, i);
            // Nested objects and generated characters without an owner are not
            // top-level editable runs, matching chars_for_object behavior.
            let Some(&owner) = owners.get(&object) else {
                continue;
            };
            match raw.FPDFText_IsGenerated(native.text, i) {
                1 => continue,
                0 => {}
                _ => return Err(err("Invalid generated text character")),
            }
            let ch = char::from_u32(raw.FPDFText_GetUnicode(native.text, i))
                .ok_or_else(|| err("Invalid text character"))?;
            strings[owner].push(ch);
        }
        Ok(strings)
    }
}
