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
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
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
