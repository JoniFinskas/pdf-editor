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
    /// Scale a selection to PDF-point bounds without reflowing text.
    Resize {
        bounds: Bounds,
    },
    Delete,
    BringToFront,
    SendToBack,
}

/// In-memory PDF object clipboard; never sent to the operating-system clipboard.
pub struct ObjectClipboard {
    bytes: Vec<u8>,
    items: Vec<PageItem>,
}

impl Engine {
    pub(crate) fn transform_clipped_text(&self, doc: &Document, page: u16,
        indices: &[usize], matrix: [f32; 6]) -> Result<Vec<u8>> {
        let mut native = forms::NativeForm::open(self.raw, &doc.bytes, page)?;
        let [a, b, c, d, e, f] = matrix.map(f64::from);
        for &index in indices {
            let index = i32::try_from(index).map_err(|_| err("Invalid page object index"))?;
            // The page and its objects stay owned by NativeForm through generation/save.
            unsafe {
                let object = self.raw.FPDFPage_GetObject(native.page, index);
                if object.is_null() { return Err(err("Object no longer exists")); }
                self.raw.FPDFPageObj_Transform(object, a, b, c, d, e, f);
                self.raw.FPDFPageObj_TransformClipPath(object, a, b, c, d, e, f);
            }
        }
        if unsafe { self.raw.FPDFPage_GenerateContent(native.page) } == 0 {
            return Err(err("Clipped text could not be transformed"));
        }
        native.save()
    }

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
        if selected.len() != items.len() {
            return Err(err("Duplicate object selection"));
        }
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
            let mut expected_text:Vec<_>=self.text_objects(doc,target)?.into_iter().map(|text|text.text).collect();
            let source_doc=self.candidate(doc,parse(&clipboard.bytes)?)?;
            expected_text.extend(self.text_objects(&source_doc,clipboard.items[0].page)?.into_iter()
                .filter(|text|clipboard.items.iter().any(|item|item.index==text.index)).map(|text|text.text));
            let mut wrappers = Vec::new();
            let bytes = {
                let source = self
                    .pdfium
                    .load_pdf_from_byte_slice(&clipboard.bytes, None)?;
                let destination = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                let source_page = source.pages().get(i32::from(clipboard.items[0].page))?;
                let mut page = destination.pages().get(i32::from(target))?;
                page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
                // Form copying temporarily moves/restores native objects. Capture handles
                // first so restoration order cannot change later selection ordinals.
                let objects = clipboard
                    .items
                    .iter()
                    .map(|item| source_page.objects().get(item.index))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                for mut object in objects {
                    if object.object_type() != PdfPageObjectType::XObjectForm {
                        wrappers.push(page.objects().len());
                    }
                    let mut copied = copy_native_object(&mut object, &source_page, &mut page)?;
                    copied.translate(PdfPoints::new(12.0), PdfPoints::new(-12.0))?;
                }
                page.regenerate_content()?;
                drop(page);
                destination.save_to_bytes()?
            };
            if bytes.len() > MAX_FILE {
                return Err(err("Edited PDF exceeds size limit"));
            }
            let mut candidate = self.candidate(doc, parse(&bytes)?)?;
            self.expand_copy_wrappers(&mut candidate, target, wrappers)?;
            self.page_items(&candidate, target)?;
            let mut actual:Vec<_>=self.text_objects(&candidate,target)?.into_iter()
                .map(|text|text.text).filter(|text|!text.is_empty()).collect();
            expected_text.retain(|text|!text.is_empty());
            expected_text.sort();actual.sort();
            if actual!=expected_text {
                return Err(err("Paste cannot preserve the page text; document unchanged"));
            }
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
            let current = self.page_items(doc, first.page)?;
            let mut indices: Vec<_> = items.iter().map(|item| item.index).collect();
            indices.sort_unstable();
            indices.dedup();
            if indices.len() != items.len() {
                return Err(err("Duplicate object selection"));
            }
            if indices.iter().any(|index| !current.iter().any(|item| item.index == *index)) {
                return Err(err("Object no longer exists"));
            }
            let (dx, dy) = match action {
                ObjectAction::Move { dx, dy } | ObjectAction::Duplicate { dx, dy } => (dx, dy),
                ObjectAction::Delete | ObjectAction::Resize { .. } | ObjectAction::BringToFront | ObjectAction::SendToBack => (0.0, 0.0),
            };
            if !dx.is_finite() || !dy.is_finite() || dx.abs() > 100000.0 || dy.abs() > 100000.0 {
                return Err(err("Invalid object movement"));
            }
            if matches!(action, ObjectAction::Move { .. }) && dx == 0.0 && dy == 0.0 {
                return Ok(());
            }
            let resize = if let ObjectAction::Resize { bounds } = action {
                let current = self.page_items(doc, first.page)?;
                let mut bounds_iter=items.iter().map(|item| current.iter().find(|c|c.index==item.index)
                    .map(|c|c.bounds).ok_or_else(||err("Object no longer exists")));
                let mut old=bounds_iter.next().ok_or_else(||err("Select an object first"))??;
                for bound in bounds_iter {
                    let bound=bound?;
                    old.left=old.left.min(bound.left);old.right=old.right.max(bound.right);
                    old.bottom=old.bottom.min(bound.bottom);old.top=old.top.max(bound.top);
                }
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
                if [bounds.left-old.left, bounds.bottom-old.bottom,
                    bounds.right-old.right, bounds.top-old.top]
                    .iter().all(|difference| difference.abs() < 0.0001)
                {
                    return Ok(());
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
            if matches!(action,ObjectAction::Move {..}|ObjectAction::Resize {..}|ObjectAction::Delete) {
                let text=self.text_objects(doc,first.page)?;
                let ordinals:Option<Vec<_>>=items.iter().map(|item|text.iter().position(|o|o.index==item.index)).collect();
                if let Some(mut ordinals)=ordinals {
                    ordinals.sort_unstable();
                    if ordinals.windows(2).any(|pair|pair[0]==pair[1]) {return Err(err("Duplicate object selection"));}
                    let bytes=if matches!(action,ObjectAction::Delete) {
                        let targets:Vec<_>=ordinals.iter().map(|ordinal|(*ordinal,String::new())).collect();
                        content::edit(self,doc,&text[ordinals[0]],&targets)?.0
                    } else {
                        let matrix=resize.map_or([1.0,0.0,0.0,1.0,dx,dy],
                            |(sx,sy,tx,ty)|[sx,0.0,0.0,sy,tx,ty]);
                        content::transform_text(self,doc,first.page,&ordinals,matrix)?
                    };
                    doc.accept(bytes);
                    return Ok(());
                }
            }
            let text_before=self.text_objects(doc,first.page)?;
            let mut expected_text:Vec<_>=text_before.iter()
                .filter(|text|!matches!(action,ObjectAction::Delete) || !items.iter().any(|item|item.index==text.index))
                .map(|text|text.text.clone()).collect();
            if matches!(action,ObjectAction::Duplicate {..}) {
                expected_text.extend(text_before.iter().filter(|text|items.iter().any(|item|item.index==text.index))
                    .map(|text|text.text.clone()));
            }
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let source = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let source_page = source.pages().get(i32::from(first.page))?;
            let mut page = pdf.pages().get(i32::from(first.page))?;
            // Regenerate once after the batch, not once per glyph in a paragraph.
            page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let mut indices: Vec<_> = items.iter().map(|i| i.index).collect();
            indices.sort_unstable();
            indices.dedup();
            if indices.len() != items.len() {
                return Err(err("Duplicate object selection"));
            }
            if matches!(action, ObjectAction::Delete) {
                indices.reverse();
            }
            let mut copies = if matches!(action, ObjectAction::Duplicate { .. }) {
                indices
                    .iter()
                    .map(|index| source_page.objects().get(*index))
                    .collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            }
            .into_iter();
            let mut wrappers = Vec::new();
            if matches!(action, ObjectAction::BringToFront | ObjectAction::SendToBack) {
                let count = page.objects().len();
                if count > 20000 || indices.iter().any(|i| *i >= count) {
                    return Err(err("Invalid object order selection"));
                }
                let selected: std::collections::HashSet<_> = indices.iter().copied().collect();
                let mut order: Vec<_> = (0..count).collect();
                order.sort_by_key(|i| selected.contains(i) == matches!(action, ObjectAction::BringToFront));
                if order.iter().copied().eq(0..count) { return Ok(()); }
                let mut objects = Vec::with_capacity(count);
                for index in (0..count).rev() {
                    objects.push(Some(page.objects_mut().remove_object_at_index(index)?));
                }
                objects.reverse();
                for index in order {
                    page.objects_mut().add_object(objects[index].take().ok_or_else(|| err("Invalid object order"))?)?;
                }
                indices.clear();
            }
            for index in indices {
                match action {
                    ObjectAction::BringToFront | ObjectAction::SendToBack => unreachable!(),
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
                        let mut object = copies
                            .next()
                            .ok_or_else(|| err("Object copy selection is incomplete"))?;
                        if object.object_type() != PdfPageObjectType::XObjectForm {
                            wrappers.push(page.objects().len());
                        }
                        copy_native_object(&mut object, &source_page, &mut page)?
                            .translate(PdfPoints::new(dx), PdfPoints::new(dy))?;
                    }
                }
            }
            drop(copies);
            page.regenerate_content()?;
            drop(page);
            let bytes = pdf.save_to_bytes()?;
            drop(source_page);
            drop(source);
            drop(pdf);
            if bytes.len() > MAX_FILE {
                return Err(err("Edited PDF exceeds size limit"));
            }
            let mut candidate = self.candidate(doc, parse(&bytes)?)?;
            self.expand_copy_wrappers(&mut candidate, first.page, wrappers)?;
            self.page_items(&candidate, first.page)?;
            let mut actual:Vec<_>=self.text_objects(&candidate,first.page)?.into_iter()
                .map(|text|text.text).filter(|text|!text.is_empty()).collect();
            expected_text.retain(|text|!text.is_empty());
            expected_text.sort();actual.sort();
            if actual!=expected_text {
                return Err(err("This object operation cannot preserve the page text; document unchanged"));
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }

    fn expand_copy_wrappers(
        &self,
        doc: &mut Document,
        page: u16,
        indices: Vec<usize>,
    ) -> Result<()> {
        // Native copies wrap even individual text/images. Expand only those wrappers
        // on the candidate so imported objects stay directly editable. Reverse order
        // keeps earlier object ordinals stable, and the destination receives one undo.
        for index in indices.into_iter().rev() {
            let item = self
                .page_items(doc, page)?
                .into_iter()
                .find(|item| item.index == index)
                .ok_or_else(|| err("Copied object no longer exists"))?;
            self.ungroup_page_item(doc, &item)?;
            doc.undo.clear();
        }
        Ok(())
    }
}

fn copy_native_object<'a>(
    object: &mut PdfPageObject<'a>,
    source: &'a PdfPage<'a>,
    target: &mut PdfPage<'a>,
) -> Result<PdfPageObject<'a>> {
    Ok(if object.object_type() == PdfPageObjectType::XObjectForm {
        let mut group = source.objects().create_empty_group();
        group.push(object)?;
        group.copy_to_page(target)?
    } else {
        object.copy_to_page(target)?
    })
}
