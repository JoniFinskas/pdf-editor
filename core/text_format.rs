use crate::*;

#[derive(Clone, Debug)]
pub struct TextFormat {
    pub font: String,
    pub bold: bool,
    pub italic: bool,
    pub size: f32,
    pub color: [u8; 3],
}

impl Engine {
    /// Applies one installed embeddable family/style to compatible selected runs.
    /// The entire selection commits as one undo step, after independent extraction.
    pub fn format_text(
        &self,
        doc: &mut Document,
        selected: &[TextObject],
        format: &TextFormat,
    ) -> Result<EditResult> {
        guarded(|| {
            if selected.is_empty() {
                return Err(err("Select text to format"));
            }
            if !format.size.is_finite() || !(1.0..=1000.0).contains(&format.size) {
                return Err(err("Font size must be from 1 to 1000 points"));
            }
            if selected.iter().any(|o| o.revision != doc.revision) {
                return Err(err("Selection is stale; select the objects again"));
            }
            let mut groups: std::collections::BTreeMap<u16, Vec<usize>> = Default::default();
            let mut source = std::collections::BTreeMap::new();
            let mut wording = String::new();
            for selection in selected {
                let objects = if let Some(objects) = source.get(&selection.page) {
                    objects
                } else {
                    source.insert(selection.page, self.text_objects(doc, selection.page)?);
                    &source[&selection.page]
                };
                let ordinal = objects
                    .iter()
                    .position(|o| o.index == selection.index)
                    .ok_or_else(|| err("Text object no longer exists"))?;
                let actual = &objects[ordinal];
                if actual.text != selection.text
                    || actual.matrix != selection.matrix
                    || actual.font != selection.font
                    || actual.size != selection.size
                    || actual.color != selection.color
                {
                    return Err(err("Selection no longer matches the document"));
                }
                let entries = groups.entry(selection.page).or_default();
                if !entries.contains(&ordinal) {
                    entries.push(ordinal);
                    wording.push_str(&actual.text);
                }
            }
            let font = fonts::find_style(&format.font, format.bold, format.italic, &wording)?;
            let pdf = parse(&doc.bytes)?;
            let matching_style = groups.iter().all(|(page, ordinals)| ordinals.iter().all(|i| {
                let object = &source[page][*i];
                object.size == format.size && object.color[..3] == format.color
                    && fonts::base_name(&object.font).eq_ignore_ascii_case(&font.name)
            }));
            if matching_style {
                let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                let mut unchanged = true;
                let pages = pdf.get_pages();
                for (progress, (page, ordinals)) in groups.iter().enumerate() {
                    operation::checkpoint("Checking text format", progress, groups.len())?;
                    let id = *pages.get(&(u32::from(*page) + 1)).ok_or_else(|| err("Invalid page"))?;
                    let operations = lopdf::content::Content::decode(&pdf.get_page_content_with_limit(id, MAX_STREAM)?)?;
                    let runs = content::runs(&pdf, id, &operations)?;
                    content::verify_mapping(&runs, &source[page])?;
                    let wording: String = ordinals.iter().map(|i| source[page][*i].text.as_str()).collect();
                    let mut reference = lopdf::Document::with_version("1.7");
                    let expected = fonts::embed(&mut reference, &font, &wording)?;
                    let native_page = native.pages().get(i32::from(*page))?;
                    let mut checked = std::collections::HashMap::new();
                    for i in ordinals {
                        let run = &runs[*i];
                        let same_font = *checked.entry(&run.key).or_insert_with(|| fonts::same_resource(
                            &pdf, &lopdf::Object::Dictionary(run.font.clone()),
                            &reference, &lopdf::Object::Reference(expected.id),
                        ));
                        let object = native_page.objects().get(source[page][*i].index)?;
                        unchanged &= same_font && object.as_text_object().is_some_and(|text|
                            features::stroke_matches(text, format.color));
                    }
                    if !unchanged { break; }
                }
                if unchanged {
                    operation::checkpoint("Applying text format", groups.len(), groups.len())?;
                    return Ok(EditResult { warning: None, objects_changed: 0 });
                }
            }
            let mut candidate = self.candidate(doc, pdf)?;
            let mut count = 0;
            let page_total = groups.len();
            for (page_index, (page, ordinals)) in groups.into_iter().enumerate() {
                operation::checkpoint("Formatting text", page_index, page_total)?;
                let before = &source[&page];
                let targets: Vec<_> = ordinals
                    .iter()
                    .map(|i| (*i, before[*i].text.clone()))
                    .collect();
                let (bytes, result) = content::edit_formatted(
                    self,
                    &candidate,
                    &before[ordinals[0]],
                    &targets,
                    &font,
                    format.size,
                    format.color,
                )?;
                candidate = self.candidate(doc, parse(&bytes)?)?;
                let after = self.text_objects(&candidate, page)?;
                if before.len() != after.len() {
                    return Err(err(
                        "Formatting changed the text structure; document unchanged",
                    ));
                }
                for (index, (old, new)) in before.iter().zip(&after).enumerate() {
                    let targeted = ordinals.contains(&index);
                    if old.text != new.text
                        || old
                            .matrix
                            .iter()
                            .zip(new.matrix)
                            .any(|(a, b)| (a - b).abs() > 0.001)
                        || if targeted {
                            fonts::base_name(&new.font).to_lowercase() != font.name.to_lowercase()
                                || (new.size - format.size).abs() > 0.01
                                || new.color[..3] != format.color
                        } else {
                            old.font != new.font
                                || old.size != new.size
                                || old.color != new.color
                                || old.bounds != new.bounds
                        }
                    {
                        return Err(err(
                            "Formatting failed preservation verification; document unchanged",
                        ));
                    }
                }
                count += result.objects_changed;
            }
            operation::checkpoint("Applying text format", page_total, page_total)?;
            doc.accept(candidate.bytes);
            Ok(EditResult {
                warning: None,
                objects_changed: count,
            })
        })
    }
}
