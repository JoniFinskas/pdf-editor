use crate::*;

struct RunSpan {
    start: usize,
    end: usize,
    object: TextObject,
}
struct PageMatches {
    page: u16,
    runs: Vec<RunSpan>,
    matches: Vec<(usize, usize)>,
}

/// Unicode-aware, case-insensitive literal matching with byte offsets into the original.
fn literal_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    let mut folded = String::new();
    let mut boundaries = std::collections::BTreeMap::new();
    for (at, ch) in text.char_indices() {
        boundaries.insert(folded.len(), at);
        folded.extend(ch.to_lowercase());
    }
    boundaries.insert(folded.len(), text.len());
    let query = query.to_lowercase();
    folded
        .match_indices(&query)
        .filter_map(|(start, _)| {
            Some((
                *boundaries.get(&start)?,
                *boundaries.get(&(start + query.len()))?,
            ))
        })
        .collect()
}
impl Engine {
    fn replacement_plan(&self, doc: &Document, query: &str) -> Result<Vec<PageMatches>> {
        if query.is_empty() || query.len() > 4096 || query.chars().any(char::is_control) {
            return Err(err(
                "Enter a single-line search phrase of at most 4096 bytes",
            ));
        }
        let mut pages = vec![];
        let mut count = 0;
        for page in 0..self.pages(doc)?.len() {
            let mut text = String::new();
            let mut runs: Vec<RunSpan> = vec![];
            for object in self.text_objects(doc, page as u16)? {
                if let Some(previous) = runs.last() {
                    // Avoid joining unrelated lines or distant columns. Adjacent runs retain
                    // their own fonts and styles when a match crosses the run boundary.
                    let a = &previous.object;
                    if (a.matrix[5] - object.matrix[5]).abs() > a.size.max(object.size) * 0.25
                        || (a.bounds.right - object.bounds.left).abs() > a.size.max(object.size)
                    {
                        text.push('\n');
                    }
                }
                let start = text.len();
                text.push_str(&object.text);
                runs.push(RunSpan {
                    start,
                    end: text.len(),
                    object,
                });
            }
            let matches = literal_matches(&text, query);
            count += matches.len();
            if count > 100_000 {
                return Err(err("Too many replacement matches"));
            }
            if !matches.is_empty() {
                pages.push(PageMatches {
                    page: page as u16,
                    runs,
                    matches,
                });
            }
        }
        Ok(pages)
    }
    pub fn replacement_count(&self, doc: &Document, query: &str) -> Result<usize> {
        guarded(|| {
            Ok(self
                .replacement_plan(doc, query)?
                .iter()
                .map(|p| p.matches.len())
                .sum())
        })
    }
    /// Replace a zero-based occurrence, or all occurrences with None, as one undo step.
    pub fn replace_text(
        &self,
        doc: &mut Document,
        query: &str,
        replacement: &str,
        occurrence: Option<usize>,
    ) -> Result<usize> {
        guarded(|| {
            if replacement.chars().any(char::is_control) || replacement.len() > 16384 {
                return Err(err(
                    "Replacement must contain at most 16384 printable bytes",
                ));
            }
            let plan = self.replacement_plan(doc, query)?;
            let total: usize = plan.iter().map(|p| p.matches.len()).sum();
            if occurrence.is_some_and(|i| i >= total) {
                return Err(err("Replacement occurrence is out of range"));
            }
            if total == 0 {
                return Ok(0);
            }
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
            let mut offset = 0;
            let mut replaced = 0;
            for page in plan {
                let mut texts: Vec<_> = page.runs.iter().map(|r| r.object.text.clone()).collect();
                for (index, (start, end)) in page.matches.iter().enumerate().rev() {
                    if occurrence.is_some_and(|i| i != offset + index) {
                        continue;
                    }
                    let mut first = true;
                    for (run, text) in page.runs.iter().zip(&mut texts) {
                        if *start < run.end && *end > run.start {
                            let a = start.saturating_sub(run.start);
                            let b = (*end).min(run.end) - run.start;
                            text.replace_range(a..b, if first { replacement } else { "" });
                            first = false;
                        }
                    }
                    replaced += 1;
                }
                offset += page.matches.len();
                let targets: Vec<_> = texts
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| **t != page.runs[*i].object.text)
                    .map(|(i, t)| (i, t.clone()))
                    .collect();
                if targets.is_empty() {
                    continue;
                }
                let objects = self.text_objects(&candidate, page.page)?;
                let anchor = &objects[targets[0].0];
                let (bytes, _) = content::edit(self, &candidate, anchor, &targets)?;
                candidate = self.candidate(doc, parse(&bytes)?)?;
                let actual: Vec<_> = self
                    .text_objects(&candidate, page.page)?
                    .into_iter()
                    .map(|o| o.text)
                    .collect();
                let expected: Vec<_> = texts.into_iter().filter(|t| !t.is_empty()).collect();
                if actual != expected {
                    return Err(err(
                        "Replacement failed round-trip verification; document unchanged",
                    ));
                }
            }
            doc.accept(candidate.bytes);
            Ok(replaced)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_offsets_preserve_unicode_boundaries() {
        assert_eq!(literal_matches("Ää Ää", "ää"), vec![(0, 4), (5, 9)]);
        assert_eq!(literal_matches("banana", "ana"), vec![(1, 4)]);
        assert!(literal_matches("İ", "i").is_empty());
    }
}
