//! Restore the field tree and default resources omitted by native page import.
use crate::*;
use lopdf::{Dictionary, Object, ObjectId, content::Content};
use std::collections::{BTreeMap, BTreeSet};

fn array(pdf: &lopdf::Document, dict: &Dictionary, key: &[u8]) -> Result<Vec<Object>> {
    dict.get(key)
        .ok()
        .map(|v| Ok(pdf.dereference(v)?.1.as_array()?.clone()))
        .unwrap_or_else(|| Ok(vec![]))
}

struct Import<'a> {
    source: &'a lopdf::Document,
    destination: &'a mut lopdf::Document,
    ids: BTreeMap<ObjectId, ObjectId>,
    pages: BTreeSet<ObjectId>,
}
impl Import<'_> {
    fn object(&mut self, value: &Object, depth: usize) -> Result<Object> {
        if depth > 128 || self.ids.len() > 100_000 {
            return Err(err(
                "Imported form resources exceed nesting or object limits",
            ));
        }
        Ok(match value {
            Object::Reference(id) => {
                if let Some(mapped) = self.ids.get(id) {
                    Object::Reference(*mapped)
                } else {
                    if self.pages.contains(id) {
                        return Err(err(
                            "Form refers to a page outside the selected range; document unchanged",
                        ));
                    }
                    let mapped = self.destination.new_object_id();
                    self.ids.insert(*id, mapped);
                    let object = self.source.get_object(*id)?.clone();
                    let object = self.object(&object, depth + 1)?;
                    self.destination.objects.insert(mapped, object);
                    Object::Reference(mapped)
                }
            }
            Object::Array(values) => Object::Array(
                values
                    .iter()
                    .map(|v| self.object(v, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Object::Dictionary(dict) => Object::Dictionary(self.dictionary(dict, depth + 1)?),
            Object::Stream(stream) => {
                let mut stream = stream.clone();
                stream.dict = self.dictionary(&stream.dict, depth + 1)?;
                Object::Stream(stream)
            }
            _ => value.clone(),
        })
    }
    fn dictionary(&mut self, dict: &Dictionary, depth: usize) -> Result<Dictionary> {
        if matches!(
            dict.get(b"Type").and_then(Object::as_name).ok(),
            Some(b"Pages" | b"Catalog")
        ) {
            return Err(err(
                "Form resource refers to a document tree; document unchanged",
            ));
        }
        let mut result = Dictionary::new();
        for (key, value) in dict {
            result.set(key.clone(), self.object(value, depth + 1)?);
        }
        Ok(result)
    }
}

pub(crate) fn restore(
    source: &lopdf::Document,
    destination: &mut lopdf::Document,
    pages: &[(ObjectId, ObjectId)],
) -> Result<()> {
    let Some(acro) = source.catalog()?.get(b"AcroForm").ok() else {
        return Ok(());
    };
    let acro = source.dereference(acro)?.1.as_dict()?;
    if acro.has(b"XFA") {
        return Err(err(
            "Page transfer cannot preserve XFA forms; document unchanged",
        ));
    }
    if pages
        .iter()
        .map(|(from, _)| from)
        .collect::<BTreeSet<_>>()
        .len()
        != pages.len()
    {
        return Err(err(
            "Select each form page once to preserve independent field groups",
        ));
    }
    let mut import = Import {
        source,
        destination,
        ids: pages.iter().copied().collect(),
        pages: source.get_pages().into_values().collect(),
    };
    let mut fields = BTreeSet::new();
    for &(from, to) in pages {
        let originals = array(source, source.get_dictionary(from)?, b"Annots")?;
        let copied = array(
            import.destination,
            import.destination.get_dictionary(to)?,
            b"Annots",
        )?;
        if originals.len() != copied.len() {
            return Err(err(
                "Page transfer changed form annotations; document unchanged",
            ));
        }
        for (original, copied) in originals.iter().zip(&copied) {
            let widget = source.dereference(original)?.1.as_dict()?;
            if widget.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Widget") {
                continue;
            }
            let mut id = original.as_reference()?;
            import.ids.insert(id, copied.as_reference()?);
            let mut chain = BTreeSet::new();
            loop {
                if !chain.insert(id) || chain.len() > 64 || fields.len() > 10_000 {
                    return Err(err("Invalid imported form field tree"));
                }
                fields.insert(id);
                match source.get_dictionary(id)?.get(b"Parent") {
                    Ok(parent) => id = parent.as_reference()?,
                    Err(_) => break,
                }
            }
        }
    }
    if fields.is_empty() {
        return Ok(());
    }
    // Allocate the entire selected field tree first, including shared parents.
    // Unselected widgets never enter its Kids arrays or the resource copier.
    for id in &fields {
        if !import.ids.contains_key(id) {
            let mapped = import.destination.new_object_id();
            import.ids.insert(*id, mapped);
        }
    }
    let mut target_acro = import
        .destination
        .catalog()?
        .get(b"AcroForm")
        .ok()
        .map(|v| Ok::<_, Error>(import.destination.dereference(v)?.1.as_dict()?.clone()))
        .transpose()?
        .unwrap_or_default();
    let mut roots = array(import.destination, &target_acro, b"Fields")?;
    let mut names = roots
        .iter()
        .filter_map(|id| {
            let dict = import.destination.dereference(id).ok()?.1.as_dict().ok()?;
            lopdf::decode_text_string(dict.get(b"T").ok()?).ok()
        })
        .collect::<BTreeSet<_>>();
    let mut resources = target_acro
        .get(b"DR")
        .ok()
        .map(|v| Ok::<_, Error>(import.destination.dereference(v)?.1.as_dict()?.clone()))
        .transpose()?
        .unwrap_or_default();
    let mut fonts = resources
        .get(b"Font")
        .ok()
        .map(|v| Ok::<_, Error>(import.destination.dereference(v)?.1.as_dict()?.clone()))
        .transpose()?
        .unwrap_or_default();
    let mut font_names = BTreeMap::new();
    if let Ok(dr) = acro.get(b"DR") {
        for (category, values) in source.dereference(dr)?.1.as_dict()? {
            if category == b"Font" {
                for (name, font) in source.dereference(values)?.1.as_dict()? {
                    let mut alias =
                        format!("ImportedFormFont{}", import.destination.max_id).into_bytes();
                    while fonts.has(&alias) {
                        alias.push(b'_');
                    }
                    fonts.set(alias.clone(), import.object(font, 0)?);
                    font_names.insert(name.clone(), alias);
                }
            } else {
                if resources.has(category) {
                    return Err(err(
                        "Imported form has conflicting default resources; document unchanged",
                    ));
                }
                resources.set(category.clone(), import.object(values, 0)?);
            }
        }
    }
    resources.set("Font", fonts);
    target_acro.set("DR", resources);
    for id in &fields {
        let mut field = source.get_dictionary(*id)?.clone();
        let root = !field.has(b"Parent");
        if field.has(b"Kids") {
            let kids = array(source, &field, b"Kids")?
                .into_iter()
                .filter(|kid| kid.as_reference().is_ok_and(|id| fields.contains(&id)))
                .collect::<Vec<_>>();
            field.set("Kids", kids);
        }
        if root {
            for key in [b"DA".as_slice(), b"Q"] {
                if !field.has(key)
                    && let Ok(default) = acro.get(key)
                {
                    field.set(key, default.clone());
                }
            }
            let original_name = field
                .get(b"T")
                .ok()
                .map(lopdf::decode_text_string)
                .transpose()?;
            if let Some(name) = original_name {
                let mut unique = name.clone();
                let mut suffix = 2;
                while names.contains(&unique) {
                    unique = format!("{name} ({suffix})");
                    suffix += 1;
                }
                names.insert(unique.clone());
                field.set("T", lopdf::text_string(&unique));
            }
            roots.push(Object::Reference(import.ids[id]));
        }
        if let Ok(appearance) = field.get(b"DA") {
            let appearance = source.dereference(appearance)?.1.as_str()?;
            let mut content = Content::decode(appearance)?;
            for operation in &mut content.operations {
                if operation.operator == "Tf"
                    && let Some(Object::Name(name)) = operation.operands.first_mut()
                    && let Some(alias) = font_names.get(name)
                {
                    *name = alias.clone();
                }
            }
            field.set("DA", Object::string_literal(content.encode()?));
        }
        let field = import.dictionary(&field, 0)?;
        import
            .destination
            .objects
            .insert(import.ids[id], Object::Dictionary(field));
    }
    target_acro.set("Fields", roots);
    import
        .destination
        .catalog_mut()?
        .set("AcroForm", target_acro);
    Ok(())
}
