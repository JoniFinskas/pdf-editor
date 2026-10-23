use crate::*;

#[derive(Clone, Debug)]
pub struct SearchMatch {
    pub page: u16,
    pub bounds: Vec<Bounds>,
}

impl Engine {
    /// Case-insensitive literal search across page text, including adjacent runs.
    pub fn find_text(&self, doc: &Document, query: &str) -> Result<Vec<SearchMatch>> {
        guarded(|| {
            if query.is_empty() {
                return Ok(vec![]);
            }
            if query.len() > 4096 || query.contains('\0') {
                return Err(err("Invalid search query"));
            }
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let mut matches = Vec::new();
            for (index, page) in pdf.pages().iter().enumerate() {
                let text = page.text()?;
                let search = text.search(query, &PdfSearchOptions::new())?;
                while let Some(segments) = search.find_next() {
                    if matches.len() >= 100_000 {
                        return Err(err("Too many search matches; narrow the query"));
                    }
                    matches.push(SearchMatch {
                        page: index as u16,
                        bounds: segments
                            .iter()
                            .map(|s| {
                                let b = s.bounds();
                                Bounds {
                                    left: b.left().value,
                                    bottom: b.bottom().value,
                                    right: b.right().value,
                                    top: b.top().value,
                                }
                            })
                            .collect(),
                    });
                }
            }
            Ok(matches)
        })
    }
}
