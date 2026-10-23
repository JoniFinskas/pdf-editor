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
}
impl Expander<'_> {
    fn expand(
        &mut self,
        stream: &Stream,
        inherited: &Dictionary,
        depth: usize,
    ) -> Result<Vec<Operation>> {
        if depth > 32
            || stream.dict.has(b"Group")
            || stream.dict.has(b"OC")
            || stream.dict.has(b"Ref")
        {
            return Err(err(
                "Cannot ungroup transparency groups, optional content or recursive forms",
            ));
        }
        let local = match stream.dict.get_deref(b"Resources", self.pdf) {
            Ok(value) => value.as_dict()?.clone(),
            Err(_) => inherited.clone(),
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
            if let Ok(value) = local.get_deref(category.as_bytes(), self.pdf) {
                let mut destination = self
                    .resources
                    .get_deref(category.as_bytes(), self.pdf)
                    .and_then(Object::as_dict)
                    .cloned()
                    .unwrap_or_default();
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
            };
            let mut out = vec![];
            let mut count = 0;
            let mut in_text = false;
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
            let resources = expander.resources;
            let data = Content { operations: out }.encode()?;
            if data.len() > MAX_STREAM {
                return Err(err("Ungrouped content exceeds limit"));
            }
            let id = pdf.add_object(Stream::new(Dictionary::new(), data));
            let dictionary = pdf.get_object_mut(page)?.as_dict_mut()?;
            dictionary.set("Contents", id);
            dictionary.set("Resources", resources);
            let candidate = self.candidate(doc, pdf)?;
            if candidate.bytes.len() > MAX_FILE {
                return Err(err("Edited PDF exceeds size limit"));
            }
            self.page_items(&candidate, item.page)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
