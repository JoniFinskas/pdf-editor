use crate::*;

/// Parse one-based comma-separated page numbers and ascending ranges into indices.
pub fn page_ranges(input: &str, count: usize) -> Result<Vec<u16>> {
    if input.len() > 4096 || count == 0 || count > 10000 {
        return Err(err("Invalid page range"));
    }
    let mut pages = vec![];
    for part in input.split(',') {
        let pair: Vec<_> = part.trim().split('-').collect();
        if pair.is_empty() || pair.len() > 2 {
            return Err(err("Use ranges such as 1-3,5"));
        }
        let start = pair[0]
            .trim()
            .parse::<usize>()
            .map_err(|_| err("Invalid page number"))?;
        let end = if pair.len() == 2 {
            pair[1]
                .trim()
                .parse::<usize>()
                .map_err(|_| err("Invalid page number"))?
        } else {
            start
        };
        if start == 0 || start > end || end > count {
            return Err(err("Page range is outside the document"));
        }
        for page in start..=end {
            if pages.contains(&((page - 1) as u16)) {
                return Err(err("A page appears more than once"));
            }
            pages.push((page - 1) as u16);
        }
    }
    Ok(pages)
}
impl Engine {
    pub fn merge_pdfs(&self, doc: &mut Document, paths: &[PathBuf]) -> Result<()> {
        guarded(|| {
            if paths.is_empty() || paths.len() > 100 {
                return Err(err("Select 1 to 100 PDFs to merge"));
            }
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
            for path in paths {
                let before = self.pages(&candidate)?.len() as u16;
                self.edit_pages(
                    &mut candidate,
                    &PageEdit::Insert {
                        path: path.clone(),
                        before,
                    },
                )?;
                candidate.undo.clear();
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
    pub fn extract_pages(&self, doc: &Document, pages: &[u16], path: &Path) -> Result<()> {
        guarded(|| self.extracted(doc, pages)?.save_as(path))
    }
    fn extracted(&self, doc: &Document, pages: &[u16]) -> Result<Document> {
        if pages.is_empty() || pages.len() > 10000 {
            return Err(err("Select at least one page"));
        }
        let source = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
        let mut destination = self.pdfium.create_new_pdf()?;
        for (index, page) in pages.iter().enumerate() {
            if i32::from(*page) >= source.pages().len() {
                return Err(err("Invalid selected page"));
            }
            destination.pages_mut().copy_page_range_from_document(
                &source,
                i32::from(*page)..=i32::from(*page),
                index as i32,
            )?;
        }
        self.candidate(doc, parse(&destination.save_to_bytes()?)?)
    }
    /// Each semicolon-separated range produces a separate, exclusively-created file.
    pub fn split_pdf(&self, doc: &Document, ranges: &str, folder: &Path) -> Result<Vec<PathBuf>> {
        guarded(|| {
            let count = self.pages(doc)?.len();
            let groups: Vec<_> = ranges.split(';').collect();
            if groups.is_empty() || groups.len() > 100 {
                return Err(err("Use up to 100 page groups separated by semicolons"));
            }
            let mut outputs = vec![];
            // Validate every range and destination before creating the first output.
            for (i, group) in groups.iter().enumerate() {
                let pages = page_ranges(group, count)?;
                let path = folder.join(format!("split-{}.pdf", i + 1));
                if path.exists() {
                    return Err(err("Split filenames already exist; choose an empty folder"));
                }
                outputs.push((pages, path));
            }
            let mut created = vec![];
            for (pages, path) in outputs {
                if let Err(error) = self.extract_pages(doc, &pages, &path) {
                    for path in &created {
                        let _ = fs::remove_file(path);
                    }
                    return Err(error);
                }
                created.push(path);
            }
            Ok(created)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_range_validation() {
        assert_eq!(page_ranges("3, 1-2", 3).unwrap(), vec![2, 0, 1]);
        for input in ["", "0", "4", "2-1", "1,1", "1-3-4"] {
            assert!(page_ranges(input, 3).is_err(), "{input}");
        }
    }
}
