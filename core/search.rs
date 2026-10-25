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
            let total = pdf.pages().len() as usize;
            for (index, page) in pdf.pages().iter().enumerate() {
                operation::checkpoint("Finding text", index, total)?;
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

/// Restricts literal matching to selected pages and rectangles. Empty regions
/// mean the full page; otherwise a match must fit wholly inside one rectangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub pages: Option<Vec<u16>>,
    pub regions: Vec<(u16, Bounds)>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub page: u16,
    pub bounds: Vec<Bounds>,
    pub excerpt: String,
}

impl SearchOptions {
    pub(crate) fn validate(&self, page_count: usize) -> Result<()> {
        if self
            .pages
            .as_ref()
            .is_some_and(|p| p.iter().any(|p| *p as usize >= page_count))
            || self.regions.iter().any(|(page, b)| {
                *page as usize >= page_count
                    || [b.left, b.right, b.bottom, b.top]
                        .iter()
                        .any(|v| !v.is_finite())
                    || b.left >= b.right
                    || b.bottom >= b.top
            })
        {
            return Err(err("Invalid search page or region"));
        }
        Ok(())
    }
    pub(crate) fn includes_page(&self, page: u16) -> bool {
        self.pages
            .as_ref()
            .is_none_or(|pages| pages.contains(&page))
            && (self.regions.is_empty() || self.regions.iter().any(|(p, _)| *p == page))
    }
    pub(crate) fn includes_bounds(&self, page: u16, bounds: &[Bounds]) -> bool {
        self.regions.is_empty()
            || self.regions.iter().any(|(p, r)| {
                *p == page
                    && !bounds.is_empty()
                    && bounds.iter().all(|b| {
                        b.left >= r.left
                            && b.right <= r.right
                            && b.bottom >= r.bottom
                            && b.top <= r.top
                    })
            })
    }
}
impl Engine {
    pub fn find_text_with_options(
        &self,
        doc: &Document,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchHit>> {
        guarded(|| {
            if query.is_empty() {
                return Ok(vec![]);
            }
            if query.len() > 4096 || query.chars().any(char::is_control) {
                return Err(err(
                    "Enter a single-line search phrase of at most 4096 bytes",
                ));
            }
            let page_count = self.pages(doc)?.len();
            options.validate(page_count)?;
            let mut hits = vec![];
            for page in 0..page_count as u16 {
                operation::checkpoint("Finding text", page as usize, page_count)?;
                if !options.includes_page(page) {
                    continue;
                }
                // Native reading-order text includes nested forms and other text
                // that cannot be mapped safely to editable top-level runs.
                let characters = self.page_text_characters(doc, page)?;
                let mut text = String::new();
                let mut positions = Vec::with_capacity(characters.len());
                for character in &characters {
                    let start = text.len();
                    text.push_str(&character.text);
                    positions.push((start, text.len()));
                }
                for (start, end) in find_literal_matches(&text, query, options) {
                    let first = positions.partition_point(|(_, b)| *b <= start);
                    let last = positions.partition_point(|(a, _)| *a < end);
                    let bounds: Vec<_> = characters[first..last]
                        .iter()
                        .filter(|c| {
                            !c.text.chars().all(char::is_whitespace)
                                && c.bounds.right > c.bounds.left
                                && c.bounds.top > c.bounds.bottom
                        })
                        .map(|c| c.bounds)
                        .collect();
                    if !options.includes_bounds(page, &bounds) {
                        continue;
                    }
                    if hits.len() >= 100_000 {
                        return Err(err("Too many search matches; narrow the query"));
                    }
                    hits.push(SearchHit {
                        page,
                        bounds,
                        excerpt: find_excerpt(&text, start, end),
                    });
                }
            }
            operation::checkpoint("Finding text", page_count, page_count)?;
            Ok(hits)
        })
    }
}

// The same UTF-8 boundary and word rules are used by the editable replacement
// planner. Find deliberately does not require an editable object mapping.
fn find_literal_matches(text: &str, query: &str, options: &SearchOptions) -> Vec<(usize, usize)> {
    let mut folded = String::new();
    let mut boundaries = std::collections::BTreeMap::new();
    for (at, ch) in text.char_indices() {
        boundaries.insert(folded.len(), at);
        if options.case_sensitive {
            folded.push(ch);
        } else {
            folded.extend(ch.to_lowercase());
        }
    }
    boundaries.insert(folded.len(), text.len());
    let query = if options.case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let word = |ch: char| {
        ch.is_alphanumeric()
            || ch == '_'
            || matches!(ch as u32,
        0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
    };
    folded
        .match_indices(&query)
        .filter_map(|(start, _)| {
            let end = *boundaries.get(&(start + query.len()))?;
            let start = *boundaries.get(&start)?;
            if options.whole_word
                && (text[..start].chars().next_back().is_some_and(word)
                    || text[end..].chars().next().is_some_and(word))
            {
                return None;
            }
            Some((start, end))
        })
        .collect()
}

fn find_excerpt(text: &str, start: usize, end: usize) -> String {
    let prefix: String = text[..start]
        .chars()
        .rev()
        .take(36)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let matched: String = text[start..end].chars().take(88).collect();
    let suffix: String = text[end..].chars().take(36).collect();
    format!(
        "{}{}{}{}{}",
        if prefix.len() < text[..start].len() {
            "…"
        } else {
            ""
        },
        prefix,
        matched,
        if text[start..end].chars().count() > 88 {
            "…"
        } else {
            ""
        },
        suffix
    )
    .replace(['\r', '\n'], " ")
}
