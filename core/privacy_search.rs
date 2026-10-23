use crate::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivacyQuery {
    Text(String),
    Email,
    Phone,
    Iban,
    FinnishId,
    CreditCard,
}
#[derive(Clone, Debug)]
pub struct RedactionPreview {
    document_bytes: std::sync::Arc<[u8]>,
    pub revision: u64,
    pub matches: Vec<SearchMatch>,
}
impl Engine {
    pub fn preview_redactions(
        &self,
        doc: &Document,
        query: &PrivacyQuery,
    ) -> Result<RedactionPreview> {
        guarded(|| {
            let mut matches = Vec::new();
            if let PrivacyQuery::Text(text) = query {
                matches = self.find_text(doc, text)?;
            } else {
                let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                for (index, page) in pdf.pages().iter().enumerate() {
                    let text = page.text()?;
                    let extracted = text.all();
                    if extracted.len() > 1_000_000 {
                        return Err(err("Page text exceeds privacy scan limit"));
                    }
                    let candidates = candidates(&extracted, query);
                    if candidates.len() > 1000 {
                        return Err(err("Too many privacy candidates on one page"));
                    }
                    for candidate in candidates {
                        let search = text.search(&candidate, &PdfSearchOptions::new())?;
                        while let Some(segments) = search.find_next() {
                            if matches.len() >= 100_000 {
                                return Err(err("Too many redaction matches"));
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
                }
            }
            check_region_limit(&matches)?;
            Ok(RedactionPreview {
                revision: doc.revision,
                document_bytes: doc.bytes.clone().into(),
                matches,
            })
        })
    }
    pub fn apply_redaction_preview(
        &self,
        doc: &mut Document,
        preview: &RedactionPreview,
    ) -> Result<()> {
        if doc.revision != preview.revision
            || doc.bytes.as_slice() != preview.document_bytes.as_ref()
        {
            return Err(err("Document changed; preview matches again"));
        }
        check_region_limit(&preview.matches)?;
        let regions = preview
            .matches
            .iter()
            .flat_map(|m| m.bounds.iter().map(move |b| (m.page, *b)))
            .collect::<Vec<_>>();
        if regions.is_empty() {
            return Ok(());
        }
        self.redact_regions(doc, &regions)
    }
}
fn check_region_limit(matches: &[SearchMatch]) -> Result<()> {
    if matches
        .iter()
        .fold(0usize, |n, m| n.saturating_add(m.bounds.len()))
        > 10_000
    {
        return Err(err(
            "Preview exceeds 10000 redaction rectangles; narrow the search",
        ));
    }
    Ok(())
}
fn candidates(text: &str, query: &PrivacyQuery) -> Vec<String> {
    let mut result = std::collections::BTreeSet::new();
    // Candidate windows include spaces inside account and telephone numbers.
    let words = text.split_whitespace().collect::<Vec<_>>();
    for start in 0..words.len() {
        let mut value = String::new();
        let mut longest = None;
        for word in words.iter().skip(start).take(8) {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(word);
            let candidate = value.trim_matches(|c: char| {
                matches!(c, ',' | ';' | ':' | '"' | '\'' | '(' | ')' | '.')
            });
            if valid(candidate, query) {
                longest = Some(candidate.to_string());
            }
        }
        if let Some(candidate) = longest {
            result.insert(candidate);
        }
    }
    result.into_iter().collect()
}
fn valid(s: &str, q: &PrivacyQuery) -> bool {
    let compact = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>();
    let digits = compact.chars().filter(char::is_ascii_digit).count();
    match q {
        PrivacyQuery::Email => s.split_once('@').is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.starts_with('.')
                && b.contains('.')
                && !b.ends_with('.')
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
                && !b.contains('@')
        }),
        PrivacyQuery::Phone => {
            (7..=15).contains(&digits)
                && s.chars().all(|c| c.is_ascii_digit() || "+- ()".contains(c))
                && (s.starts_with('+') || s.starts_with('0'))
        }
        PrivacyQuery::Iban => {
            (15..=34).contains(&compact.len())
                && compact.as_bytes()[..2].iter().all(u8::is_ascii_alphabetic)
                && compact.as_bytes()[2..4].iter().all(u8::is_ascii_digit)
                && compact.chars().all(|c| c.is_ascii_alphanumeric())
        }
        PrivacyQuery::FinnishId => {
            s.len() == 11
                && s.as_bytes()[..6].iter().all(u8::is_ascii_digit)
                && b"+-ABCDEFUVWXY".contains(&s.as_bytes()[6])
                && s.as_bytes()[7..10].iter().all(u8::is_ascii_digit)
                && s.as_bytes()[10].is_ascii_alphanumeric()
        }
        PrivacyQuery::CreditCard => {
            (13..=19).contains(&compact.len()) && compact.chars().all(|c| c.is_ascii_digit())
        }
        PrivacyQuery::Text(_) => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangle_limit_counts_all_segments() {
        let bounds = Bounds {
            left: 0.0,
            bottom: 0.0,
            right: 1.0,
            top: 1.0,
        };
        let mut matches = vec![SearchMatch {
            page: 0,
            bounds: vec![bounds; 10000],
        }];
        assert!(check_region_limit(&matches).is_ok());
        matches.push(SearchMatch {
            page: 1,
            bounds: vec![bounds],
        });
        assert!(check_region_limit(&matches).is_err());
    }
    #[test]
    fn presets_detect_patterns() {
        for (query, text) in [
            (PrivacyQuery::Email, "person@example.fi"),
            (PrivacyQuery::Phone, "+358 40 1234567"),
            (PrivacyQuery::Iban, "FI21 1234 5600 0007 85"),
            (PrivacyQuery::FinnishId, "131052-308T"),
            (PrivacyQuery::CreditCard, "4111 1111 1111 1111"),
        ] {
            assert!(!candidates(text, &query).is_empty(), "{query:?}");
            assert!(candidates("ordinary words", &query).is_empty());
        }
    }
}
