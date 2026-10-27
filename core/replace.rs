use crate::*;

struct PlannedMatch {
    ranges: Vec<(usize, usize, usize)>,
    unchanged: bool,
}
pub(crate) struct PageMatches {
    page: u16,
    runs: Vec<TextObject>,
    matches: Vec<PlannedMatch>,
}

/// Unicode lowercase matches always end on original UTF-8 boundaries.
fn literal_matches(text: &str, query: &str, options: &SearchOptions) -> Vec<(usize, usize)> {
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

impl Engine {
    pub(crate) fn replacement_plan(
        &self,
        doc: &Document,
        query: &str,
        options: &SearchOptions,
        replacement: Option<&str>,
    ) -> Result<Vec<PageMatches>> {
        if query.is_empty() || query.len() > 4096 || query.chars().any(char::is_control) {
            return Err(err(
                "Enter a single-line search phrase of at most 4096 bytes",
            ));
        }
        let page_count = self.pages(doc)?.len();
        options.validate(page_count)?;
        let mut pages = vec![];
        let mut count = 0;
        for page in 0..page_count as u16 {
            operation::checkpoint("Searching pages", page as usize, page_count)?;
            if !options.includes_page(page) {
                continue;
            }
            let runs = self.text_objects(doc, page)?;
            let ordinals: std::collections::HashMap<_, _> =
                runs.iter().enumerate().map(|(i, o)| (o.index, i)).collect();
            let chars = text_selection::extract(self, doc, page)?;
            let mut offsets = vec![0; runs.len()];
            let mut reconstructed = vec![String::new(); runs.len()];
            let mut text = String::new();
            let mut positions = vec![];
            for item in &chars {
                let start = text.len();
                text.push_str(&item.character.text);
                let range = item
                    .owner
                    .and_then(|o| ordinals.get(&o).copied())
                    .map(|ordinal| {
                        let at = offsets[ordinal];
                        offsets[ordinal] += item.character.text.len();
                        reconstructed[ordinal].push_str(&item.character.text);
                        (ordinal, at, offsets[ordinal])
                    });
                positions.push((start, text.len(), range, item.character.bounds));
            }
            let mut matches = vec![];
            for (start, end) in literal_matches(&text, query, options) {
                let first = positions.partition_point(|(_, b, _, _)| *b <= start);
                let last = positions.partition_point(|(a, _, _, _)| *a < end);
                let selected: Vec<_> = positions[first..last].iter().collect();
                let bounds: Vec<_> = selected
                    .iter()
                    .filter(|(_, _, r, b)| r.is_some() && b.right > b.left && b.top > b.bottom)
                    .map(|p| p.3)
                    .collect();
                if !options.includes_bounds(page, &bounds) {
                    continue;
                }
                let mut ranges: Vec<(usize, usize, usize)> = vec![];
                let mut editable = true;
                for (a, b, range, _) in selected {
                    if let Some((ordinal, from, to)) = range {
                        if reconstructed[*ordinal] != runs[*ordinal].text {
                            editable = false;
                            break;
                        }
                        if let Some(previous) = ranges.last_mut()
                            && previous.0 == *ordinal
                            && previous.2 == *from
                        {
                            previous.2 = *to;
                        } else {
                            ranges.push((*ordinal, *from, *to));
                        }
                    } else if !text[*a..*b].chars().all(char::is_whitespace) {
                        editable = false;
                        break;
                    }
                }
                // Count and replace share the safely editable text surface.
                if !editable || ranges.is_empty() {
                    continue;
                }
                count += 1;
                if count > 100_000 {
                    return Err(err("Too many replacement matches"));
                }
                matches.push(PlannedMatch {
                    ranges,
                    unchanged: replacement.is_some_and(|value| &text[start..end] == value),
                });
            }
            if !matches.is_empty() {
                pages.push(PageMatches {
                    page,
                    runs,
                    matches,
                });
            }
        }
        Ok(pages)
    }
    pub fn replacement_count(&self, doc: &Document, query: &str) -> Result<usize> {
        self.replacement_count_with_options(doc, query, &SearchOptions::default())
    }
    pub fn replacement_count_with_options(
        &self,
        doc: &Document,
        query: &str,
        options: &SearchOptions,
    ) -> Result<usize> {
        guarded(|| {
            Ok(self
                .replacement_plan(doc, query, options, None)?
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
        self.replace_text_with_options(
            doc,
            query,
            replacement,
            occurrence,
            &SearchOptions::default(),
        )
    }
    pub fn replace_text_with_options(
        &self,
        doc: &mut Document,
        query: &str,
        replacement: &str,
        occurrence: Option<usize>,
        options: &SearchOptions,
    ) -> Result<usize> {
        guarded(|| {
            if replacement.chars().any(char::is_control) || replacement.len() > 16384 {
                return Err(err(
                    "Replacement must contain at most 16384 printable bytes",
                ));
            }
            let plan = self.replacement_plan(doc, query, options, Some(replacement))?;
            let total: usize = plan.iter().map(|p| p.matches.len()).sum();
            if occurrence.is_some_and(|i| i >= total) {
                return Err(err("Replacement occurrence is out of range"));
            }
            if total == 0 || plan.iter().all(|page| page.matches.iter().all(|matched| matched.unchanged)) {
                return Ok(0);
            }
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
            let mut offset = 0;
            let mut replaced = 0;
            let page_total = plan.len();
            for (page_index, page) in plan.into_iter().enumerate() {
                operation::checkpoint("Replacing text", page_index, page_total)?;
                let mut texts: Vec<_> = page.runs.iter().map(|r| r.text.clone()).collect();
                for (index, matched) in page.matches.iter().enumerate().rev() {
                    if matched.unchanged || occurrence.is_some_and(|i| i != offset + index) {
                        continue;
                    }
                    for (range_index, (ordinal, start, end)) in
                        matched.ranges.iter().enumerate().rev()
                    {
                        texts[*ordinal].replace_range(
                            *start..*end,
                            if range_index == 0 { replacement } else { "" },
                        );
                    }
                    replaced += 1;
                }
                offset += page.matches.len();
                let targets: Vec<_> = texts
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| **t != page.runs[*i].text)
                    .map(|(i, t)| (i, t.clone()))
                    .collect();
                if targets.is_empty() {
                    continue;
                }
                let objects = self.text_objects(&candidate, page.page)?;
                let anchor = objects
                    .get(targets[0].0)
                    .ok_or_else(|| err("Replacement target disappeared"))?;
                let (bytes, _) = content::edit(self, &candidate, anchor, &targets)?;
                candidate = self.candidate(doc, parse(&bytes)?)?;
                let actual: Vec<_> = self
                    .text_objects(&candidate, page.page)?
                    .into_iter()
                    .map(|o| o.text)
                    .filter(|t| !t.is_empty())
                    .collect();
                let expected: Vec<_> = texts.into_iter().filter(|t| !t.is_empty()).collect();
                if actual != expected {
                    return Err(err(
                        "Replacement failed round-trip verification; document unchanged",
                    ));
                }
            }
            operation::checkpoint("Applying replacements", page_total, page_total)?;
            if replaced > 0 { doc.accept(candidate.bytes); }
            Ok(replaced)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_offsets_preserve_unicode_boundaries() {
        let o = SearchOptions::default();
        assert_eq!(literal_matches("Ää Ää", "ää", &o), vec![(0, 4), (5, 9)]);
        assert_eq!(literal_matches("banana", "ana", &o), vec![(1, 4)]);
        assert!(literal_matches("İ", "i", &o).is_empty());
        let whole = SearchOptions {
            whole_word: true,
            ..o.clone()
        };
        assert_eq!(
            literal_matches("ää ääni xää ää_x ää", "ÄÄ", &whole),
            vec![(0, 4), (25, 29)]
        );
        assert_eq!(
            literal_matches(
                "Word word",
                "Word",
                &SearchOptions {
                    case_sensitive: true,
                    ..o
                }
            ),
            vec![(0, 4)]
        );
    }
}
