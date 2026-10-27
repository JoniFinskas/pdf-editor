use crate::*;

fn bookmark_ranges(bookmarks: &BookmarkSet, count: usize) -> Result<String> {
    if count == 0 || count > 10000 { return Err(err("Invalid page count")); }
    let mut starts: std::collections::BTreeSet<_> = bookmarks.items.iter()
        .filter(|bookmark| bookmark.depth == 0)
        .filter_map(|bookmark| bookmark.page.map(usize::from))
        .filter(|page| *page < count).collect();
    if starts.is_empty() { return Err(err("No top-level bookmarks point to pages; use page ranges")); }
    starts.insert(0);
    if starts.len() > 100 { return Err(err("Bookmark split exceeds 100 files; use page ranges")); }
    let starts: Vec<_> = starts.into_iter().collect();
    Ok(starts.iter().enumerate().map(|(index, &start)| {
        let end = starts.get(index + 1).copied().unwrap_or(count);
        if end == start + 1 { end.to_string() } else { format!("{}-{end}", start + 1) }
    }).collect::<Vec<_>>().join(";"))
}

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
    /// Split at top-level bookmark destinations, retaining leading pages separately.
    pub fn split_pdf_at_bookmarks(&self, doc: &Document, folder: &Path) -> Result<Vec<PathBuf>> {
        guarded(|| {
            let bookmarks = self.bookmarks(doc)?;
            let ranges = bookmark_ranges(&bookmarks, self.pages(doc)?.len())?;
            self.split_pdf(doc, &ranges, folder)
        })
    }
    pub fn merge_pdfs(&self, doc: &mut Document, paths: &[PathBuf]) -> Result<()> {
        guarded(|| {
            if paths.is_empty() || paths.len() > 100 {
                return Err(err("Select 1 to 100 PDFs to merge"));
            }
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
            for (index, path) in paths.iter().enumerate() {
                operation::checkpoint("Merging PDFs", index, paths.len())?;
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
            operation::checkpoint("Merging PDFs", paths.len(), paths.len())?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
    pub fn extract_pages(&self, doc: &Document, pages: &[u16], path: &Path) -> Result<()> {
        guarded(|| {
            let extracted = self.extracted(doc, pages)?;
            operation::checkpoint("Extracting pages", pages.len(), pages.len())?;
            extracted.save_as(path)
        })
    }
    fn extracted(&self, doc: &Document, pages: &[u16]) -> Result<Document> {
        if pages.is_empty() || pages.len() > 10000 {
            return Err(err("Select at least one page"));
        }
        let source = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
        let mut destination = self.pdfium.create_new_pdf()?;
        for (index, page) in pages.iter().enumerate() {
            operation::checkpoint("Extracting pages", index, pages.len())?;
            if i32::from(*page) >= source.pages().len() {
                return Err(err("Invalid selected page"));
            }
            destination.pages_mut().copy_page_range_from_document(
                &source,
                i32::from(*page)..=i32::from(*page),
                index as i32,
            )?;
        }
        let mut pdf = parse(&destination.save_to_bytes()?)?;
        let source = parse(&doc.bytes)?;
        let source_pages: Vec<_> = source.get_pages().into_values().collect();
        let target_pages: Vec<_> = pdf.get_pages().into_values().collect();
        let mapping: Vec<_> = pages.iter().zip(target_pages)
            .map(|(page, target)| (source_pages[*page as usize], target)).collect();
        crate::page_forms::restore(&source, &mut pdf, &mapping)?;
        let candidate = self.candidate(doc, pdf)?;
        let expected = self.form_fields(doc)?.iter().filter(|field| pages.contains(&field.page)).count();
        if self.form_fields(&candidate)?.len() != expected {
            return Err(err("Page extraction could not preserve form fields; no file was created"));
        }
        Ok(candidate)
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
            let count = outputs.len();
            for (index, (pages, path)) in outputs.into_iter().enumerate() {
                let result = operation::checkpoint("Splitting PDF", index, count)
                    .and_then(|_| self.extract_pages(doc, &pages, &path));
                if let Err(error) = result {
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
    fn marks(entries: &[(Option<u16>, usize)]) -> BookmarkSet {
        BookmarkSet { revision: 0, items: entries.iter().enumerate().map(|(index, &(page, depth))| Bookmark {
            id: (index as u32 + 1, 0), title: "Chapter".into(), page, depth, sibling: index, siblings: entries.len(),
        }).collect() }
    }
    #[test]
    fn bookmark_groups_sort_deduplicate_and_preserve_every_page() {
        let bookmarks = marks(&[(Some(5),0), (Some(1),1), (Some(2),0), (Some(2),0), (None,0), (Some(99),0)]);
        let ranges = bookmark_ranges(&bookmarks, 8).unwrap();
        assert_eq!(ranges, "1-2;3-5;6-8");
        let pages: Vec<_> = ranges.split(';').flat_map(|range| page_ranges(range, 8).unwrap()).collect();
        assert_eq!(pages, (0..8).collect::<Vec<_>>());
        assert_eq!(bookmark_ranges(&marks(&[(Some(0),0), (Some(1),0)]), 3).unwrap(), "1;2-3");
        assert_eq!(bookmark_ranges(&marks(&[(Some(0),0)]), 1).unwrap(), "1");
    }
    #[test]
    fn unusable_or_excessive_bookmark_groups_fail_before_export() {
        for entries in [vec![], vec![(None,0)], vec![(Some(2),1)], vec![(Some(99),0)]] {
            assert!(bookmark_ranges(&marks(&entries), 8).is_err());
        }
        assert!(bookmark_ranges(&marks(&[(Some(0),0)]), 0).is_err());
        assert!(bookmark_ranges(&marks(&[(Some(0),0)]), 10001).is_err());
        let entries: Vec<_> = (1..=100).map(|page| (Some(page),0)).collect();
        assert!(bookmark_ranges(&marks(&entries), 102).is_err());
        let entries: Vec<_> = (0..100).map(|page| (Some(page),0)).collect();
        assert_eq!(bookmark_ranges(&marks(&entries), 100).unwrap().split(';').count(), 100);
    }
    #[test]
    fn page_range_validation() {
        assert_eq!(page_ranges("3, 1-2", 3).unwrap(), vec![2, 0, 1]);
        for input in ["", "0", "4", "2-1", "1,1", "1-3-4"] {
            assert!(page_ranges(input, 3).is_err(), "{input}");
        }
    }
}
