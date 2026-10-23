use crate::*;
use lopdf::{
    Dictionary, Object, Stream,
    content::{Content, Operation},
};
use std::collections::BTreeMap;

fn form<'a>(
    pdf: &'a lopdf::Document,
    resources: &'a Dictionary,
    op: &Operation,
) -> Result<Option<&'a Stream>> {
    if op.operator != "Do" {
        return Ok(None);
    }
    let name = op
        .operands
        .first()
        .ok_or_else(|| err("Missing object name"))?
        .as_name()?;
    let objects = resources.get_deref(b"XObject", pdf)?.as_dict()?;
    let stream = objects.get_deref(name, pdf)?.as_stream()?;
    Ok((stream.dict.get(b"Subtype")?.as_name()? == b"Form").then_some(stream))
}

struct Expander<'a> {
    pdf: &'a lopdf::Document,
    resources: Dictionary,
    serial: usize,
    operations: usize,
    expanded_bytes: usize,
    transparency: bool,
    allow_transparency: bool,
}
impl Expander<'_> {
    fn expand(
        &mut self,
        stream: &Stream,
        inherited: &Dictionary,
        depth: usize,
    ) -> Result<Vec<Operation>> {
        if depth > 32 || stream.dict.has(b"OC") || stream.dict.has(b"Ref") {
            return Err(err(
                "Cannot ungroup optional content, reference forms or excessive nesting",
            ));
        }
        if stream.dict.has(b"Group") {
            if !self.allow_transparency {
                return Err(err("Cannot ungroup transparency groups"));
            }
            let group = stream.dict.get_deref(b"Group", self.pdf)?.as_dict()?;
            if group.get(b"S")?.as_name()? != b"Transparency"
                || group.get(b"K").and_then(Object::as_bool).unwrap_or(false)
            {
                return Err(err("Unsupported group compositing"));
            }
            check_opaque_compositing(self.pdf)?;
            self.transparency = true;
        }
        let local = if stream.dict.has(b"Resources") {
            stream
                .dict
                .get_deref(b"Resources", self.pdf)?
                .as_dict()?
                .clone()
        } else {
            inherited.clone()
        };
        self.serial += 1;
        let mut names = BTreeMap::new();
        for category in [
            "Font",
            "XObject",
            "ExtGState",
            "ColorSpace",
            "Pattern",
            "Shading",
            "Properties",
        ] {
            if local.has(category.as_bytes()) {
                let value = local.get_deref(category.as_bytes(), self.pdf)?;
                let mut destination = if self.resources.has(category.as_bytes()) {
                    self.resources
                        .get_deref(category.as_bytes(), self.pdf)?
                        .as_dict()?
                        .clone()
                } else {
                    Dictionary::new()
                };
                for (old, value) in value.as_dict()?.iter() {
                    let mut fresh = format!("Ungroup{}_", self.serial).into_bytes();
                    fresh.extend(old);
                    while destination.has(&fresh) {
                        fresh.push(b'_');
                    }
                    destination.set(fresh.clone(), value.clone());
                    names.insert((category, old.clone()), fresh);
                }
                self.resources.set(category, destination);
            }
        }
        let mut out = vec![Operation::new("q", vec![])];
        if let Ok(matrix) = stream.dict.get(b"Matrix") {
            let matrix = matrix.as_array()?;
            if matrix.len() != 6
                || matrix.iter().any(|v| {
                    v.as_float()
                        .map_or(true, |f| !f.is_finite() || f.abs() > 100000.)
                })
            {
                return Err(err("Invalid form matrix"));
            }
            out.push(Operation::new("cm", matrix.clone()));
        }
        let bounds = stream.dict.get(b"BBox")?.as_array()?;
        if bounds.len() != 4 {
            return Err(err("Invalid form bounds"));
        }
        let b = bounds
            .iter()
            .map(Object::as_float)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if b.iter().any(|f| !f.is_finite() || f.abs() > 100000.) || b[2] < b[0] || b[3] < b[1] {
            return Err(err("Invalid form bounds"));
        }
        out.push(Operation::new(
            "re",
            vec![
                b[0].into(),
                b[1].into(),
                (b[2] - b[0]).into(),
                (b[3] - b[1]).into(),
            ],
        ));
        out.push(Operation::new("W", vec![]));
        out.push(Operation::new("n", vec![]));
        let data = stream.get_plain_content_with_limit(MAX_STREAM)?;
        self.expanded_bytes += data.len();
        if self.expanded_bytes > MAX_STREAM {
            return Err(err("Expanded forms exceed content limit"));
        }
        let mut graphics_depth = 0usize;
        let mut in_text = false;
        for mut op in Content::decode(&data)?.operations {
            match op.operator.as_str() {
                "BT" => {
                    if in_text {
                        return Err(err("Nested form text state"));
                    }
                    in_text = true;
                }
                "ET" => in_text = false,
                "q" => {
                    graphics_depth += 1;
                    if graphics_depth > 128 {
                        return Err(err("Excessive graphics nesting"));
                    }
                }
                "Q" => {
                    graphics_depth = graphics_depth
                        .checked_sub(1)
                        .ok_or_else(|| err("Unbalanced form graphics state"))?;
                }
                _ => {}
            }
            if op.operator == "BI" {
                return Err(err("Ungrouping inline images is not supported"));
            }
            self.operations += 1;
            if self.operations > 100000 {
                return Err(err("Too many ungrouped operations"));
            }
            if let Some(child) = form(self.pdf, &local, &op)? {
                if in_text {
                    return Err(err("Cannot ungroup forms invoked inside text objects"));
                }
                out.extend(self.expand(child, &local, depth + 1)?);
                continue;
            }
            let category = match op.operator.as_str() {
                "Tf" => Some("Font"),
                "Do" => Some("XObject"),
                "gs" => Some("ExtGState"),
                "CS" | "cs" => Some("ColorSpace"),
                "SCN" | "scn" => Some("Pattern"),
                "sh" => Some("Shading"),
                "BDC" | "DP" => Some("Properties"),
                _ => None,
            };
            if let Some(category) = category {
                let value = if matches!(category, "Properties" | "Pattern") {
                    op.operands.last_mut()
                } else {
                    op.operands.first_mut()
                };
                if let Some(Object::Name(name)) = value
                    && let Some(fresh) = names.get(&(category, name.clone()))
                {
                    *name = fresh.clone();
                }
            }
            out.push(op);
        }
        if graphics_depth != 0 || in_text {
            return Err(err("Unbalanced form graphics state"));
        }
        out.push(Operation::new("Q", vec![]));
        Ok(out)
    }
}

impl Engine {
    /// Expand one selected Form XObject, recursively, without altering shared forms.
    pub fn ungroup_page_item(&self, doc: &mut Document, item: &PageItem) -> Result<()> {
        self.expand_page_item(doc, item, false)
    }

    /// Expose text in a page-wrapping group while retaining page compositing.
    pub fn prepare_grouped_text(&self, doc: &mut Document, item: &PageItem) -> Result<()> {
        self.expand_page_item(doc, item, true)
    }

    fn expand_page_item(
        &self,
        doc: &mut Document,
        item: &PageItem,
        allow_transparency: bool,
    ) -> Result<()> {
        guarded(|| {
            if item.revision != doc.revision {
                return Err(err("Selection is stale"));
            }
            let items = self.page_items(doc, item.page)?;
            let forms: Vec<_> = items.iter().filter(|i| i.kind == "XObjectForm").collect();
            let ordinal = forms
                .iter()
                .position(|i| i.index == item.index)
                .ok_or_else(|| err("Select a grouped object"))?;
            let mut pdf = parse(&doc.bytes)?;
            let page = *pdf
                .get_pages()
                .get(&(u32::from(item.page) + 1))
                .ok_or_else(|| err("Page does not exist"))?;
            let resources = content::resources(&pdf, page)?;
            let content = Content::decode(&pdf.get_page_content_with_limit(page, MAX_STREAM)?)?;
            let mut expander = Expander {
                pdf: &pdf,
                resources: resources.clone(),
                serial: 0,
                operations: 0,
                expanded_bytes: 0,
                transparency: false,
                allow_transparency,
            };
            let mut out = vec![];
            let mut count = 0;
            let mut in_text = false;
            let mut page_group = None;
            for op in content.operations {
                if op.operator == "BT" {
                    in_text = true;
                }
                if op.operator == "ET" {
                    in_text = false;
                }
                if let Some(stream) = form(&pdf, &resources, &op)? {
                    if count == ordinal {
                        if in_text {
                            return Err(err("Cannot ungroup forms invoked inside text objects"));
                        }
                        if allow_transparency && stream.dict.has(b"Group") {
                            if !pdf.get_dictionary(page)?.has(b"Group") {
                                page_group = Some(stream.dict.get_deref(b"Group", &pdf)?.clone());
                            }
                        }
                        out.extend(expander.expand(stream, &resources, 0)?);
                    } else {
                        out.push(op);
                    }
                    count += 1;
                } else {
                    out.push(op);
                }
            }
            if count != forms.len() {
                return Err(err("Ambiguous grouped object selection"));
            }
            let verify_render = expander.transparency;
            let resources = expander.resources;
            let data = Content { operations: out }.encode()?;
            if data.len() > MAX_STREAM {
                return Err(err("Ungrouped content exceeds limit"));
            }
            let id = pdf.add_object(Stream::new(Dictionary::new(), data));
            let dictionary = pdf.get_object_mut(page)?.as_dict_mut()?;
            dictionary.set("Contents", id);
            dictionary.set("Resources", resources);
            if let Some(group) = page_group {
                dictionary.set("Group", group);
            }
            let candidate = self.candidate(doc, pdf)?;
            if candidate.bytes.len() > MAX_FILE {
                return Err(err("Edited PDF exceeds size limit"));
            }
            self.page_items(&candidate, item.page)?;
            if verify_render {
                for scale in [1.0, 2.0] {
                    let before = self.render(doc, item.page, scale)?;
                    let after = self.render(&candidate, item.page, scale)?;
                    if before.width != after.width
                        || before.height != after.height
                        || before
                            .rgba
                            .iter()
                            .zip(&after.rgba)
                            .any(|(a, b)| a.abs_diff(*b) > 1)
                        || before
                            .rgba
                            .iter()
                            .zip(&after.rgba)
                            .filter(|(a, b)| a != b)
                            .count()
                            > before.rgba.len() / 100
                    {
                        return Err(err(
                            "This group's transparency cannot be preserved when ungrouping",
                        ));
                    }
                }
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}

// Opaque (or fully invisible) normal painting does not depend on group alpha.
// Reject fractional alpha and special compositing before removing a group;
// rendered equality is checked separately before accepting the candidate.
fn check_opaque_compositing(pdf: &lopdf::Document) -> Result<()> {
    fn visit(object: &Object, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(err("Compositing resources exceed nesting limit"));
        }
        let dictionary = match object {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            Object::Array(values) => {
                for value in values {
                    visit(value, depth + 1)?;
                }
                None
            }
            _ => None,
        };
        if let Some(d) = dictionary {
            for (key, value) in d.iter() {
                match key.as_slice() {
                    b"ca" | b"CA" => {
                        let alpha = value.as_float()?;
                        if alpha != 0.0 && alpha != 1.0 {
                            return Err(err("Cannot ungroup partial transparency"));
                        }
                    }
                    b"BM" if !matches!(value.as_name(), Ok(b"Normal" | b"Compatible")) => {
                        return Err(err("Cannot ungroup non-normal blending"));
                    }
                    b"SMask" if value.as_name().ok() != Some(b"None") => {
                        return Err(err("Cannot ungroup soft masks"));
                    }
                    _ => {}
                }
                visit(value, depth + 1)?;
            }
        }
        Ok(())
    }
    for object in pdf.objects.values() {
        visit(object, 0)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn transparency_guard_rejects_fractional_alpha_blending_and_masks() {
        for alpha in [0.0, 1.0] {
            let mut pdf = lopdf::Document::with_version("1.7");
            pdf.add_object(dictionary! { "ca" => alpha, "CA" => 1, "BM" => "Normal" });
            assert!(check_opaque_compositing(&pdf).is_ok());
        }
        for dictionary in [
            dictionary! { "ca" => 0.5 },
            dictionary! { "BM" => "Multiply" },
            dictionary! { "SMask" => Object::Reference((12, 0)) },
        ] {
            let mut pdf = lopdf::Document::with_version("1.7");
            pdf.add_object(dictionary);
            assert!(check_opaque_compositing(&pdf).is_err());
        }
    }

    #[test]
    fn malformed_form_resources_are_not_inherited_or_discarded() {
        let pdf = lopdf::Document::with_version("1.7");
        let inherited = Dictionary::new();
        let base = Stream::new(
            dictionary! {
                "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            },
            Vec::new(),
        );
        let expand = |stream: &Stream, resources: Dictionary| {
            Expander {
                pdf: &pdf,
                resources,
                serial: 0,
                operations: 0,
                expanded_bytes: 0,
                transparency: false,
                allow_transparency: false,
            }
            .expand(stream, &inherited, 0)
        };
        assert!(expand(&base, Dictionary::new()).is_ok());
        for invalid in [Object::Reference((999, 0)), Object::Integer(42)] {
            let mut stream = base.clone();
            stream.dict.set("Resources", invalid.clone());
            assert!(expand(&stream, Dictionary::new()).is_err());
            for category in [
                "Font",
                "XObject",
                "ExtGState",
                "ColorSpace",
                "Pattern",
                "Shading",
                "Properties",
            ] {
                stream
                    .dict
                    .set("Resources", dictionary! { category => invalid.clone() });
                assert!(expand(&stream, Dictionary::new()).is_err());
                stream
                    .dict
                    .set("Resources", dictionary! { category => Dictionary::new() });
                assert!(expand(&stream, dictionary! { category => invalid.clone() }).is_err());
            }
        }
    }
}
