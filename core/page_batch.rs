use crate::*;
use lopdf::{Dictionary, Object, ObjectId, dictionary};
use std::collections::{BTreeMap, BTreeSet};

/// Page selections are sorted into document order and must contain no duplicates.
#[derive(Clone, Debug)]
pub enum PageBatch {
    Rotate(Vec<u16>),
    Delete(Vec<u16>),
    /// Insert before an original-document slot, from zero through page count.
    Move {
        pages: Vec<u16>,
        before: u16,
    },
    Duplicate {
        pages: Vec<u16>,
        before: u16,
    },
    Blank {
        before: u16,
        width: f32,
        height: f32,
    },
}

fn selection(pages: &[u16], count: usize) -> Result<BTreeSet<usize>> {
    let selected: BTreeSet<_> = pages.iter().map(|p| usize::from(*p)).collect();
    if pages.is_empty() || selected.len() != pages.len() || selected.iter().any(|p| *p >= count) {
        return Err(err("Select valid pages once each"));
    }
    Ok(selected)
}

fn array(pdf: &lopdf::Document, dict: &Dictionary, key: &[u8]) -> Result<Vec<Object>> {
    dict.get(key)
        .ok()
        .map(|v| Ok(pdf.dereference(v)?.1.as_array()?.clone()))
        .unwrap_or_else(|| Ok(vec![]))
}

// Clone reachable page resources without traversing the document or page trees.
// Separate content streams and annotations keep later edits independent.
struct PageCopy<'a> {
    source: &'a lopdf::Document,
    destination: &'a mut lopdf::Document,
    ids: BTreeMap<ObjectId, ObjectId>,
    pages: BTreeSet<ObjectId>,
}
impl PageCopy<'_> {
    fn object(&mut self, object: &Object, depth: usize) -> Result<Object> {
        if depth > 128 || self.ids.len() > 100_000 {
            return Err(err("Page copy exceeds resource limits"));
        }
        Ok(match object {
            Object::Reference(id) => {
                if let Some(mapped) = self.ids.get(id) {
                    Object::Reference(*mapped)
                } else if self.pages.contains(id) {
                    Object::Reference(*id)
                } else {
                    let mapped = self.destination.new_object_id();
                    self.ids.insert(*id, mapped);
                    let copied = self.object(self.source.get_object(*id)?, depth + 1)?;
                    self.destination.objects.insert(mapped, copied);
                    Object::Reference(mapped)
                }
            }
            Object::Array(values) => Object::Array(
                values
                    .iter()
                    .map(|v| self.object(v, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Object::Dictionary(dict) => {
                if matches!(
                    dict.get(b"Type").and_then(Object::as_name).ok(),
                    Some(b"Pages" | b"Catalog")
                ) {
                    return Err(err(
                        "Page resource refers to a document tree; document unchanged",
                    ));
                }
                let mut copied = Dictionary::new();
                for (key, value) in dict {
                    copied.set(key.clone(), self.object(value, depth + 1)?);
                }
                Object::Dictionary(copied)
            }
            Object::Stream(stream) => {
                let mut copied = stream.clone();
                copied.dict = self
                    .object(&Object::Dictionary(stream.dict.clone()), depth + 1)?
                    .as_dict()?
                    .clone();
                Object::Stream(copied)
            }
            _ => object.clone(),
        })
    }
}

fn duplicate(pdf: &mut lopdf::Document, selected: &[ObjectId]) -> Result<Vec<ObjectId>> {
    let source = pdf.clone();
    let mapping: Vec<_> = selected
        .iter()
        .map(|from| (*from, pdf.new_object_id()))
        .collect();
    let mut copy = PageCopy {
        source: &source,
        destination: pdf,
        ids: mapping.iter().copied().collect(),
        pages: source.get_pages().into_values().collect(),
    };
    let mut annotations = vec![];
    for &(from, to) in &mapping {
        for original in array(&source, source.get_dictionary(from)?, b"Annots")? {
            let id = original.as_reference()?;
            if copy.ids.contains_key(&id) {
                return Err(err(
                    "Shared page annotations cannot be duplicated independently",
                ));
            }
            let mapped = copy.destination.new_object_id();
            copy.ids.insert(id, mapped);
            annotations.push((id, mapped, to));
        }
    }
    for (from, to, page) in annotations {
        let mut annotation = source.get_dictionary(from)?.clone();
        if annotation.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget") {
            annotation.remove(b"Parent");
            annotation.remove(b"Kids");
        }
        annotation.remove(b"P");
        let mut copied = copy
            .object(&Object::Dictionary(annotation), 0)?
            .as_dict()?
            .clone();
        copied.set("P", page);
        copy.destination
            .objects
            .insert(to, Object::Dictionary(copied));
    }
    for &(from, to) in &mapping {
        let mut page = source.get_dictionary(from)?.clone();
        page.remove(b"Parent");
        for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
            if let Some(value) = crate::pages::inherited(&source, from, key)? {
                page.set(key, value);
            }
        }
        let page = copy.object(&Object::Dictionary(page), 0)?;
        copy.destination.objects.insert(to, page);
    }
    crate::page_forms::restore(&source, copy.destination, &mapping)?;
    Ok(mapping.into_iter().map(|(_, to)| to).collect())
}

fn remove_fields(pdf: &mut lopdf::Document, removed: &[ObjectId]) -> Result<()> {
    let Some(acro) = pdf.catalog()?.get(b"AcroForm").ok().cloned() else {
        return Ok(());
    };
    let mut acro = pdf.dereference(&acro)?.1.as_dict()?.clone();
    let mut widgets = BTreeSet::new();
    for page in removed {
        for annotation in array(pdf, pdf.get_dictionary(*page)?, b"Annots")? {
            if pdf
                .dereference(&annotation)?
                .1
                .as_dict()?
                .get(b"Subtype")
                .and_then(Object::as_name)
                .ok()
                == Some(b"Widget")
            {
                widgets.insert(annotation.as_reference()?);
            }
        }
    }
    if widgets.is_empty() {
        return Ok(());
    }
    if acro.has(b"XFA") {
        return Err(err(
            "Page deletion cannot preserve XFA forms; document unchanged",
        ));
    }
    fn retain(
        pdf: &mut lopdf::Document,
        id: ObjectId,
        removed: &BTreeSet<ObjectId>,
        seen: &mut BTreeSet<ObjectId>,
        depth: usize,
    ) -> Result<bool> {
        if depth > 64 || seen.len() > 10_000 || !seen.insert(id) {
            return Err(err("Invalid form field tree"));
        }
        if removed.contains(&id) {
            return Ok(false);
        }
        let mut field = pdf.get_dictionary(id)?.clone();
        if field.has(b"Kids") {
            let mut kept = vec![];
            for kid in array(pdf, &field, b"Kids")? {
                if retain(pdf, kid.as_reference()?, removed, seen, depth + 1)? {
                    kept.push(kid);
                }
            }
            if kept.is_empty() {
                return Ok(false);
            }
            field.set("Kids", kept);
            pdf.objects.insert(id, Object::Dictionary(field));
        }
        Ok(true)
    }
    let mut kept = vec![];
    let mut seen = BTreeSet::new();
    for root in array(pdf, &acro, b"Fields")? {
        if retain(pdf, root.as_reference()?, &widgets, &mut seen, 0)? {
            kept.push(root);
        }
    }
    acro.set("Fields", kept);
    pdf.catalog_mut()?.set("AcroForm", acro);
    Ok(())
}

impl Engine {
    /// Apply a validated batch as one undo step; unsupported preservation fails atomically.
    pub fn edit_page_batch(&self, doc: &mut Document, edit: &PageBatch) -> Result<()> {
        guarded(|| {
            operation::checkpoint("Organizing pages", 0, 1)?;
            let mut pdf = parse(&doc.bytes)?;
            let original: Vec<_> = pdf.get_pages().into_values().collect();
            let count = original.len();
            let selected = match edit {
                PageBatch::Rotate(pages)
                | PageBatch::Delete(pages)
                | PageBatch::Move { pages, .. }
                | PageBatch::Duplicate { pages, .. } => selection(pages, count)?,
                PageBatch::Blank { .. } => BTreeSet::new(),
            };
            let before = match edit {
                PageBatch::Move { before, .. }
                | PageBatch::Duplicate { before, .. }
                | PageBatch::Blank { before, .. } => usize::from(*before),
                _ => 0,
            };
            if before > count {
                return Err(err("Invalid insertion position"));
            }
            let old_bookmarks = self.bookmarks(doc)?;
            let old_fields = self.form_fields(doc)?;
            let mut pages = original.clone();
            let mut imported = vec![];
            match edit {
                PageBatch::Rotate(_) => {
                    for (step, index) in selected.iter().enumerate() {
                        operation::checkpoint("Rotating pages", step, selected.len())?;
                        let id = original[*index];
                        let rotation = crate::pages::inherited(&pdf, id, b"Rotate")?
                            .map(|v| v.as_i64())
                            .transpose()?
                            .unwrap_or(0);
                        pdf.get_dictionary_mut(id)?
                            .set("Rotate", (rotation.rem_euclid(360) + 90) % 360);
                    }
                }
                PageBatch::Delete(_) => {
                    if selected.len() == count {
                        return Err(err("A PDF must retain at least one page"));
                    }
                    let removed: Vec<_> = selected.iter().map(|i| original[*i]).collect();
                    remove_fields(&mut pdf, &removed)?;
                    pages = original
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !selected.contains(i))
                        .map(|(_, id)| *id)
                        .collect();
                }
                PageBatch::Move { .. } => {
                    pages = original
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !selected.contains(i))
                        .map(|(_, id)| *id)
                        .collect();
                    let slot = before - selected.iter().filter(|i| **i < before).count();
                    pages.splice(slot..slot, selected.iter().map(|i| original[*i]));
                    if pages == original {
                        return Ok(());
                    }
                }
                PageBatch::Duplicate { .. } => {
                    if count + selected.len() > 10_000 {
                        return Err(err("Too many pages"));
                    }
                    let from: Vec<_> = selected.iter().map(|i| original[*i]).collect();
                    let added = duplicate(&mut pdf, &from)?;
                    imported = selected
                        .iter()
                        .copied()
                        .zip(added.iter().copied())
                        .collect();
                    pages.splice(before..before, added);
                }
                PageBatch::Blank { width, height, .. } => {
                    if count >= 10_000 {
                        return Err(err("Too many pages"));
                    }
                    if [*width, *height]
                        .iter()
                        .any(|v| !v.is_finite() || !(1.0..=14_400.0).contains(v))
                    {
                        return Err(err(
                            "Blank page dimensions must be between 1 and 14400 points",
                        ));
                    }
                    let id=pdf.add_object(dictionary! {"Type"=>"Page","MediaBox"=>vec![0.into(),0.into(),(*width).into(),(*height).into()],"Resources"=>Dictionary::new()});
                    pages.insert(before, id);
                }
            }
            crate::pages::flatten(&mut pdf, &pages)?;
            let candidate = self.candidate(doc, pdf)?;
            let new_bookmarks = self.bookmarks(&candidate)?;
            if old_bookmarks.items.len() != new_bookmarks.items.len()
                || old_bookmarks
                    .items
                    .iter()
                    .zip(&new_bookmarks.items)
                    .any(|(old, new)| {
                        let expected = old.page.and_then(|p| {
                            pages
                                .iter()
                                .position(|id| *id == original[usize::from(p)])
                                .map(|p| p as u16)
                        });
                        old.title != new.title
                            || old.depth != new.depth
                            || old.page.is_some() && (expected.is_none() || new.page != expected)
                    })
            {
                return Err(err(
                    "Page operation cannot preserve bookmark destinations; document unchanged",
                ));
            }
            let new_fields = self.form_fields(&candidate)?;
            let mut expected = 0;
            for field in &old_fields {
                if let Some(page) = pages
                    .iter()
                    .position(|id| *id == original[usize::from(field.page)])
                {
                    expected += 1;
                    if !new_fields.iter().any(|f| {
                        usize::from(f.page) == page
                            && f.name == field.name
                            && f.value == field.value
                            && f.read_only == field.read_only
                    }) {
                        return Err(err(
                            "Page operation cannot preserve form fields; document unchanged",
                        ));
                    }
                }
            }
            for (source, id) in imported {
                let page = pages
                    .iter()
                    .position(|p| *p == id)
                    .ok_or_else(|| err("Copied page is missing"))?;
                let source_fields: Vec<_> = old_fields
                    .iter()
                    .filter(|f| usize::from(f.page) == source)
                    .collect();
                let copied: Vec<_> = new_fields
                    .iter()
                    .filter(|f| usize::from(f.page) == page)
                    .collect();
                expected += source_fields.len();
                if copied.len() != source_fields.len()
                    || source_fields.iter().zip(copied).any(|(a, b)| {
                        a.value != b.value || old_fields.iter().any(|f| f.name == b.name)
                    })
                {
                    return Err(err(
                        "Page duplication cannot preserve independent form fields; document unchanged",
                    ));
                }
            }
            if new_fields.len() != expected {
                return Err(err(
                    "Page operation changed form fields; document unchanged",
                ));
            }
            operation::checkpoint("Applying page changes", 1, 1)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
