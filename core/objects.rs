use crate::*;

/// A top-level page object. Form objects represent grouped artwork.
#[derive(Clone, Debug)]
pub struct PageItem {
    pub page: u16,
    pub index: usize,
    pub revision: u64,
    pub kind: String,
    pub bounds: Bounds,
}

#[derive(Clone, Copy, Debug)]
pub enum ObjectAction {
    Move {
        dx: f32,
        dy: f32,
    },
    Duplicate {
        dx: f32,
        dy: f32,
    },
    /// Scale a single object to PDF-point bounds without reflowing text.
    Resize {
        bounds: Bounds,
    },
    Delete,
}

/// In-memory PDF object clipboard; never sent to the operating-system clipboard.
pub struct ObjectClipboard {
    bytes: Vec<u8>,
    items: Vec<PageItem>,
}

impl Engine {
    pub fn copy_page_items(&self, doc: &Document, items: &[PageItem]) -> Result<ObjectClipboard> {
        let first = items.first().ok_or_else(|| err("Select an object first"))?;
        let current = self.page_items(doc, first.page)?;
        if items.len() > 20000
            || items.iter().any(|i| {
                i.revision != doc.revision
                    || i.page != first.page
                    || !current.iter().any(|c| c.index == i.index)
            })
        {
            return Err(err("Selection is stale or spans pages"));
        }
        let mut selected = items.to_vec();
        selected.sort_by_key(|i| i.index);
        selected.dedup_by_key(|i| i.index);
        Ok(ObjectClipboard {
            bytes: doc.bytes.clone(),
            items: selected,
        })
    }

    pub fn paste_page_items(
        &self,
        doc: &mut Document,
        clipboard: &ObjectClipboard,
        target: u16,
    ) -> Result<()> {
        guarded(|| {
            let bytes = {
                let source = self
                    .pdfium
                    .load_pdf_from_byte_slice(&clipboard.bytes, None)?;
                let destination = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                let source_page = source.pages().get(i32::from(clipboard.items[0].page))?;
                let mut page = destination.pages().get(i32::from(target))?;
                page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
                for item in &clipboard.items {
                    source_page
                        .objects()
                        .get(item.index)?
                        .copy_to_page(&mut page)?
                        .translate(PdfPoints::new(12.0), PdfPoints::new(-12.0))?;
                }
                page.regenerate_content()?;
                drop(page);
                destination.save_to_bytes()?
            };
            if bytes.len() > MAX_FILE {
                return Err(err("Edited PDF exceeds size limit"));
            }
            let candidate = self.candidate(doc, parse(&bytes)?)?;
            self.page_items(&candidate, target)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
    pub fn page_items(&self, doc: &Document, page: u16) -> Result<Vec<PageItem>> {
        guarded(|| {
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let native = pdf.pages().get(i32::from(page))?;
            if native.objects().len() > 20000 {
                return Err(err("Too many page objects"));
            }
            (0..native.objects().len())
                .map(|index| {
                    let object = native.objects().get(index)?;
                    let b = object.bounds()?;
                    if [
                        b.left().value,
                        b.right().value,
                        b.top().value,
                        b.bottom().value,
                    ]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 100000.0)
                    {
                        return Err(err("Invalid page object bounds"));
                    }
                    Ok(PageItem {
                        page,
                        index,
                        revision: doc.revision,
                        kind: format!("{:?}", object.object_type()),
                        bounds: Bounds {
                            left: b.left().value,
                            right: b.right().value,
                            bottom: b.bottom().value,
                            top: b.top().value,
                        },
                    })
                })
                .collect()
        })
    }

    /// Mutate selected top-level objects atomically; unsupported copies fail cleanly.
    pub fn edit_page_items(
        &self,
        doc: &mut Document,
        items: &[PageItem],
        action: ObjectAction,
    ) -> Result<()> {
        guarded(|| {
            let first = items.first().ok_or_else(|| err("Select an object first"))?;
            if items.len() > 20000
                || items
                    .iter()
                    .any(|i| i.page != first.page || i.revision != doc.revision)
            {
                return Err(err("Selection is stale or spans pages"));
            }
            let (dx, dy) = match action {
                ObjectAction::Move { dx, dy } | ObjectAction::Duplicate { dx, dy } => (dx, dy),
                ObjectAction::Delete | ObjectAction::Resize { .. } => (0.0, 0.0),
            };
            if !dx.is_finite() || !dy.is_finite() || dx.abs() > 100000.0 || dy.abs() > 100000.0 {
                return Err(err("Invalid object movement"));
            }
            if matches!(action, ObjectAction::Move { .. }) && dx == 0.0 && dy == 0.0 {
                return Ok(());
            }
            let resize = if let ObjectAction::Resize { bounds } = action {
                if items.len() != 1 {
                    return Err(err("Resize requires one object"));
                }
                let current = self.page_items(doc, first.page)?;
                let old = current
                    .iter()
                    .find(|i| i.index == first.index)
                    .ok_or_else(|| err("Object no longer exists"))?
                    .bounds;
                let dimensions = [
                    old.right - old.left,
                    old.top - old.bottom,
                    bounds.right - bounds.left,
                    bounds.top - bounds.bottom,
                ];
                if [bounds.left, bounds.bottom, bounds.right, bounds.top]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 100000.0)
                    || dimensions.iter().any(|v| !v.is_finite() || *v < 0.01)
                {
                    return Err(err("Invalid object size"));
                }
                let sx = dimensions[2] / dimensions[0];
                let sy = dimensions[3] / dimensions[1];
                if !(0.0001..=10000.0).contains(&sx) || !(0.0001..=10000.0).contains(&sy) {
                    return Err(err("Object scale exceeds limits"));
                }
                Some((
                    sx,
                    sy,
                    bounds.left - old.left * sx,
                    bounds.bottom - old.bottom * sy,
                ))
            } else {
                None
            };
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let source = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let mut page = pdf.pages().get(i32::from(first.page))?;
            // Regenerate once after the batch, not once per glyph in a paragraph.
            page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let source_page = source.pages().get(i32::from(first.page))?;
            let mut indices: Vec<_> = items.iter().map(|i| i.index).collect();
            indices.sort_unstable();
            indices.dedup();
            if indices.len() != items.len() {
                return Err(err("Duplicate object selection"));
            }
            if matches!(action, ObjectAction::Delete) {
                indices.reverse();
            }
            for index in indices {
                match action {
                    ObjectAction::Resize { .. } => {
                        let (sx, sy, tx, ty) = resize.unwrap();
                        page.objects()
                            .get(index)?
                            .transform(sx, 0.0, 0.0, sy, tx, ty)?;
                    }
                    ObjectAction::Delete => {
                        page.objects_mut().remove_object_at_index(index)?;
                    }
                    ObjectAction::Move { .. } => {
                        page.objects()
                            .get(index)?
                            .translate(PdfPoints::new(dx), PdfPoints::new(dy))?;
                    }
                    ObjectAction::Duplicate { .. } => {
                        source_page
                            .objects()
                            .get(index)?
                            .copy_to_page(&mut page)?
                            .translate(PdfPoints::new(dx), PdfPoints::new(dy))?;
                    }
                }
            }
            page.regenerate_content()?;
            drop(page);
            let bytes = pdf.save_to_bytes()?;
            drop(source_page);
            drop(source);
            drop(pdf);
            if bytes.len() > MAX_FILE {
                return Err(err("Edited PDF exceeds size limit"));
            }
            let candidate = self.candidate(doc, parse(&bytes)?)?;
            self.page_items(&candidate, first.page)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
