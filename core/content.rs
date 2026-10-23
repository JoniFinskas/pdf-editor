use crate::{
    Document, EditResult, Engine, MAX_STREAM, Result, Substitution, TextObject, err, fonts, parse,
};
use lopdf::{
    Dictionary, Encoding, Object, ObjectId, Stream,
    content::{Content, Operation},
};

/// Text decoding for a font. A ToUnicode CMap takes precedence over /Encoding,
/// matching PDFium's text extraction; lopdf otherwise ignores ToUnicode when
/// /Encoding is present (common for Type3 fonts with custom glyph names).
fn decoding<'a>(doc: &'a lopdf::Document, font: &'a Dictionary) -> Result<Encoding<'a>> {
    if font.has(b"Encoding")
        && font
            .get_deref(b"ToUnicode", doc)
            .and_then(Object::as_stream)
            .is_ok()
    {
        let mut unicode_only = font.clone();
        unicode_only.remove(b"Encoding");
        if let Encoding::UnicodeMapEncoding(cmap) =
            unicode_only.get_font_encoding_with_limit(doc, MAX_STREAM)?
        {
            return Ok(Encoding::UnicodeMapEncoding(cmap));
        }
    }
    Ok(font.get_font_encoding_with_limit(doc, MAX_STREAM)?)
}
struct Run {
    op: usize,
    key: Vec<u8>,
    size: f32,
    text: String,
    font: Dictionary,
}
fn runs(
    doc: &lopdf::Document,
    page: ObjectId,
    content: &Content<Vec<Operation>>,
) -> Result<Vec<Run>> {
    let fonts = doc.get_page_fonts(page)?;
    let mut key = Vec::new();
    let mut size = 0.0;
    let mut stack = vec![];
    let mut result = vec![];
    for (index, op) in content.operations.iter().enumerate() {
        match op.operator.as_str() {
            "q" => {
                if stack.len() > 128 {
                    return Err(err("Excessive graphics nesting"));
                }
                stack.push((key.clone(), size));
            }
            "Q" => {
                if let Some(state) = stack.pop() {
                    (key, size) = state;
                }
            }
            "Tf" => {
                key = op
                    .operands
                    .first()
                    .ok_or_else(|| err("Invalid Tf"))?
                    .as_name()?
                    .to_vec();
                size = op
                    .operands
                    .get(1)
                    .ok_or_else(|| err("Invalid Tf"))?
                    .as_float()?;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                let font = fonts.get(&key).ok_or_else(|| err("Missing text font"))?;
                let encoding = decoding(doc, font)?;
                let items = if op.operator == "TJ" {
                    op.operands
                        .first()
                        .ok_or_else(|| err("Invalid TJ"))?
                        .as_array()?
                        .clone()
                } else {
                    vec![
                        op.operands
                            .last()
                            .ok_or_else(|| err("Invalid text operation"))?
                            .clone(),
                    ]
                };
                let mut text = String::new();
                let mut glyphs = false;
                for item in items {
                    if let Object::String(bytes, _) = item {
                        glyphs |= !bytes.is_empty();
                        text.push_str(&encoding.bytes_to_string(&bytes)?);
                    }
                }
                // PDFium creates a text object for every operator that shows
                // glyphs, even when they decode to no text (such as spaces).
                if glyphs {
                    result.push(Run {
                        op: index,
                        key: key.clone(),
                        size,
                        text,
                        font: (*font).clone(),
                    });
                }
            }
            _ => (),
        }
        if result.len() > 100_000 {
            return Err(err("Too many text runs"));
        }
    }
    Ok(result)
}
fn encode(doc: &lopdf::Document, font: &Dictionary, text: &str) -> Result<Vec<u8>> {
    let encoding = decoding(doc, font)?;
    let bytes = encoding.string_to_bytes(text);
    if encoding.bytes_to_string(&bytes)? != text {
        return Err(err("Font encoding does not contain the replacement text"));
    }
    Ok(bytes)
}
fn font_name(font: &Dictionary) -> Result<String> {
    // Type3 fonts have no BaseFont; their glyph procedures identify them.
    let name = match font.get(b"BaseFont").or_else(|_| font.get(b"Name")) {
        Ok(name) => String::from_utf8_lossy(name.as_name()?).into_owned(),
        Err(_) => match font.get(b"CharProcs") {
            Ok(procs) => {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                format!("{procs:?}").hash(&mut hasher);
                format!("type3-{:016x}", hasher.finish())
            }
            Err(_) => String::new(),
        },
    };
    if name.is_empty() {
        return Err(err("Missing font identity"));
    }
    Ok(fonts::base_name(&name).to_lowercase())
}
/// True when a Type3 font has a glyph procedure for every code of `text`.
fn type3_covers(doc: &lopdf::Document, font: &Dictionary, text: &str) -> bool {
    let check = || -> Result<bool> {
        if font.get(b"Subtype")?.as_name()? != b"Type3" {
            return Ok(false);
        }
        let procs = doc.dereference(font.get(b"CharProcs")?)?.1.as_dict()?;
        let encoding = doc.dereference(font.get(b"Encoding")?)?.1.as_dict()?;
        let mut names: Vec<Option<&[u8]>> = vec![None; 256];
        let mut code = 0usize;
        for item in encoding.get(b"Differences")?.as_array()? {
            match item {
                Object::Integer(start) => code = usize::try_from(*start).unwrap_or(256),
                Object::Name(name) => {
                    if let Some(slot) = names.get_mut(code) {
                        *slot = Some(name);
                    }
                    code += 1;
                }
                _ => return Ok(false),
            }
        }
        let bytes = encode(doc, font, text)?;
        Ok(bytes
            .iter()
            .all(|b| names[usize::from(*b)].is_some_and(|name| procs.has(name))))
    };
    check().unwrap_or(false)
}
pub(crate) fn resources(doc: &lopdf::Document, page: ObjectId) -> Result<Dictionary> {
    let (local, parents) = doc.get_page_resources(page)?;
    if let Some(local) = local {
        return Ok(local.clone());
    }
    for id in parents {
        if let Ok(value) = doc.get_dictionary(id) {
            return Ok(value.clone());
        }
    }
    Ok(Dictionary::new())
}
/// Replaces the runs at `targets` (ordinal, replacement) on the page of
/// `selected`, the first targeted object, in one transaction.
pub(crate) fn edit(
    engine: &Engine,
    state: &Document,
    selected: &TextObject,
    targets: &[(usize, String)],
) -> Result<(Vec<u8>, EditResult)> {
    let (ordinal, _) = targets.first().ok_or_else(|| err("Nothing to edit"))?;
    let ordinal = *ordinal;
    let text: String = targets.iter().map(|(_, t)| t.as_str()).collect();
    let text = text.as_str();
    let mut pdf = parse(&state.bytes)?;
    let pages = pdf.get_pages();
    let page = *pages
        .get(&(selected.page as u32 + 1))
        .ok_or_else(|| err("Invalid page"))?;
    let data = pdf.get_page_content_with_limit(page, MAX_STREAM)?;
    let content = Content::decode(&data)?;
    let source = runs(&pdf, page, &content)?;
    let native = engine.text_objects(state, selected.page)?;
    verify_mapping(&source, &native)?;
    for (o, _) in targets {
        match (source.get(*o), native.get(*o)) {
            (Some(r), Some(n)) if same_text(&r.text, &n.text) => {}
            _ => return Err(err("This run uses symbol glyphs that cannot be edited")),
        }
    }
    let run = source
        .get(ordinal)
        .ok_or_else(|| err("Unsupported text-object mapping"))?;
    let embedded_original = fonts::embedded_font(&pdf, &run.font, text);
    let reuse = text.is_empty()
        || (embedded_original.is_some() && encode(&pdf, &run.font, text).is_ok())
        || type3_covers(&pdf, &run.font, text);
    let chosen = if reuse {
        None
    } else if let Some(font) = embedded_original {
        Some(font)
    } else {
        Some(fonts::find(&selected.font, text)?)
    };
    let whole =
        chosen.as_ref().is_some_and(|f| !f.exact) && state.substitution == Substitution::Document;
    let missing_name = font_name(&run.font)?;
    let mut plans = vec![];
    let mut combined = text.to_string();
    for (&number, &page_id) in &pages {
        if number != selected.page as u32 + 1 && !whole {
            continue;
        }
        let operations = Content::decode(&pdf.get_page_content_with_limit(page_id, MAX_STREAM)?)?;
        let records = runs(&pdf, page_id, &operations)?;
        let before = engine.text_objects(state, (number - 1) as u16)?;
        verify_mapping(&records, &before)?;
        let mut changes = vec![];
        for (i, r) in records.iter().enumerate() {
            let target = (number == selected.page as u32 + 1)
                .then(|| targets.iter().find(|(o, _)| *o == i))
                .flatten();
            if target.is_some() || (whole && font_name(&r.font)? == missing_name) {
                let replacement = match target {
                    Some((_, t)) => t.clone(),
                    None => r.text.clone(),
                };
                combined.push_str(&replacement);
                changes.push((i, replacement));
            }
        }
        if !changes.is_empty() {
            plans.push((number, page_id, operations, records, before, changes));
        }
    }
    let chosen = if let Some(f) = chosen {
        if whole {
            Some(fonts::find(&selected.font, &combined)?)
        } else {
            Some(f)
        }
    } else {
        None
    };
    let embedded = chosen
        .as_ref()
        .map(|f| fonts::embed(&mut pdf, f, &combined))
        .transpose()?;
    let mut count = 0;
    for (_, page_id, operations, records, _, changes) in &mut plans {
        let mut new_key = None;
        if let Some(font) = &embedded {
            let mut res = resources(&pdf, *page_id)?;
            let mut font_dict = match res.get(b"Font") {
                Ok(v) => pdf.dereference(v)?.1.as_dict()?.clone(),
                Err(_) => Dictionary::new(),
            };
            let mut key = format!("PE{}", font.id.0).into_bytes();
            while font_dict.has(&key) {
                key.push(b'X');
            }
            font_dict.set(key.clone(), font.id);
            res.set("Font", font_dict);
            pdf.get_object_mut(*page_id)?
                .as_dict_mut()?
                .set("Resources", res);
            new_key = Some(key);
        }
        for (i, replacement) in changes.iter().rev() {
            let r = &records[*i];
            let bytes = if let Some(f) = &embedded {
                f.encode(replacement)?
            } else {
                encode(&pdf, &r.font, replacement)?
            };
            let original = &operations.operations[r.op];
            let mut revised = vec![];
            if original.operator == "\"" {
                revised.push(Operation::new(
                    "Tw",
                    vec![
                        original
                            .operands
                            .first()
                            .ok_or_else(|| err("Invalid quote"))?
                            .clone(),
                    ],
                ));
                revised.push(Operation::new(
                    "Tc",
                    vec![
                        original
                            .operands
                            .get(1)
                            .ok_or_else(|| err("Invalid quote"))?
                            .clone(),
                    ],
                ));
            }
            if original.operator == "'" || original.operator == "\"" {
                revised.push(Operation::new("T*", vec![]));
            }
            if let Some(key) = &new_key {
                revised.push(Operation::new(
                    "Tf",
                    vec![Object::Name(key.clone()), r.size.into()],
                ));
            }
            revised.push(Operation::new(
                "Tj",
                vec![Object::String(bytes, lopdf::StringFormat::Hexadecimal)],
            ));
            if new_key.is_some() {
                revised.push(Operation::new(
                    "Tf",
                    vec![Object::Name(r.key.clone()), r.size.into()],
                ));
            }
            operations.operations.splice(r.op..=r.op, revised);
            count += 1;
        }
        let stream = pdf.add_object(Stream::new(Dictionary::new(), operations.encode()?));
        pdf.get_object_mut(*page_id)?
            .as_dict_mut()?
            .set("Contents", stream);
    }
    let mut bytes = vec![];
    pdf.save_to(&mut bytes)?;
    // PDFium regeneration drops Tc/Tw. Keep content streams and repair only the
    // text cursor with numeric TJ adjustments, measured by the independent parser.
    // reparsing per correction is O(runs * corrections); use a fully
    // validated text-state interpreter if large documents require lower latency.
    for (number, page_id, _, _, before, changes) in &plans {
        let expected: Vec<_> = before
            .iter()
            .enumerate()
            .filter(|(i, _)| !changes.iter().any(|(j, t)| j == i && t.is_empty()))
            .collect();
        let mut settled = false;
        for _ in 0..=expected.len().min(1024) {
            let candidate = Document {
                bytes: bytes.clone(),
                original: state.original.clone(),
                undo: vec![],
                redo: vec![],
                revision: state.revision,
                substitution: state.substitution,
            };
            let after = engine.text_objects(&candidate, (number - 1) as u16)?;
            if expected.len() != after.len() {
                return Err(err("Text-object structure changed unexpectedly"));
            }
            let mut correction = None;
            for (position, ((ordinal, old), new)) in expected.iter().zip(&after).enumerate() {
                let wanted = changes
                    .iter()
                    .find(|(i, _)| i == ordinal)
                    .map(|(_, t)| t.as_str())
                    .unwrap_or(&old.text);
                if wanted != new.text {
                    return Err(err("Text encoding verification failed"));
                }
                if old.matrix[..4]
                    .iter()
                    .zip(&new.matrix[..4])
                    .any(|(a, b)| (a - b).abs() > 0.0001)
                {
                    return Err(err("Text transform changed unexpectedly"));
                }
                let dx = old.matrix[4] - new.matrix[4];
                let dy = old.matrix[5] - new.matrix[5];
                if dx.abs() > 0.0001 || dy.abs() > 0.0001 {
                    let a = old.matrix[0];
                    let b = old.matrix[1];
                    let norm = a * a + b * b;
                    if norm < 0.000001
                        || old.size.abs() < 0.000001
                        || (dx * b - dy * a).abs() > 0.001
                    {
                        return Err(err("Cannot safely restore text cursor"));
                    }
                    correction = Some((position, -1000.0 * (dx * a + dy * b) / norm / old.size));
                    break;
                }
            }
            let Some((position, amount)) = correction else {
                settled = true;
                break;
            };
            let mut ops = Content::decode(&pdf.get_page_content_with_limit(*page_id, MAX_STREAM)?)?;
            let records = runs(&pdf, *page_id, &ops)?;
            let op_index = records
                .get(position)
                .ok_or_else(|| err("Text mapping changed"))?
                .op;
            let original = ops.operations[op_index].clone();
            let mut prefix = vec![];
            if original.operator == "\"" {
                prefix.push(Operation::new(
                    "Tw",
                    vec![
                        original
                            .operands
                            .first()
                            .ok_or_else(|| err("Invalid quote"))?
                            .clone(),
                    ],
                ));
                prefix.push(Operation::new(
                    "Tc",
                    vec![
                        original
                            .operands
                            .get(1)
                            .ok_or_else(|| err("Invalid quote"))?
                            .clone(),
                    ],
                ));
            }
            if original.operator == "'" || original.operator == "\"" {
                prefix.push(Operation::new("T*", vec![]));
            }
            prefix.push(Operation::new(
                "TJ",
                vec![Object::Array(vec![amount.into()])],
            ));
            prefix.push(if original.operator == "'" || original.operator == "\"" {
                Operation::new(
                    "Tj",
                    vec![
                        original
                            .operands
                            .last()
                            .ok_or_else(|| err("Invalid text operation"))?
                            .clone(),
                    ],
                )
            } else {
                original
            });
            ops.operations.splice(op_index..=op_index, prefix);
            let stream = pdf.add_object(Stream::new(Dictionary::new(), ops.encode()?));
            pdf.get_object_mut(*page_id)?
                .as_dict_mut()?
                .set("Contents", stream);
            bytes.clear();
            pdf.save_to(&mut bytes)?;
        }
        if !settled {
            return Err(err("Text cursor correction limit exceeded"));
        }
    }
    // Remove superseded unreferenced streams instead of retaining previous text bytes.
    pdf.prune_objects();
    bytes.clear();
    pdf.save_to(&mut bytes)?;
    let warning = chosen.filter(|f| !f.exact).map(|f| {
        let missing = if selected.font.trim().is_empty() {
            "(unnamed embedded font)"
        } else {
            selected.font.as_str()
        };
        format!("Font {missing} not found, used {}", f.name)
    });
    Ok((
        bytes,
        EditResult {
            warning,
            objects_changed: count,
        },
    ))
}
fn same_text(a: &str, b: &str) -> bool {
    a.chars()
        .filter(|c| !c.is_whitespace())
        .eq(b.chars().filter(|c| !c.is_whitespace()))
}
fn private_use(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(
            |c| matches!(u32::from(c), 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD),
        )
}
fn verify_mapping(runs: &[Run], objects: &[TextObject]) -> Result<()> {
    // PDFium reports space-only glyph runs as empty text, so whitespace is not
    // compared; every other character must match in order.
    // Icon fonts map a glyph to a private-use code point while PDFium may
    // report the glyph's name; such runs keep their position in the mapping
    // but cannot be edited.
    if runs.len() != objects.len()
        || runs
            .iter()
            .zip(objects)
            .any(|(r, o)| !same_text(&r.text, &o.text) && !private_use(&r.text))
    {
        return Err(err(
            "This PDF's nested or combined text runs cannot be edited safely",
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn run(text: &str) -> Run {
        Run {
            op: 0,
            key: vec![],
            size: 12.0,
            text: text.into(),
            font: Dictionary::new(),
        }
    }
    fn object(text: &str) -> TextObject {
        TextObject {
            page: 0,
            index: 0,
            revision: 0,
            text: text.into(),
            font: String::new(),
            size: 12.0,
            bounds: crate::Bounds {
                left: 0.0,
                bottom: 0.0,
                right: 1.0,
                top: 1.0,
            },
            color: [0, 0, 0, 255],
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
    #[test]
    fn mapping_tolerates_spaces_and_icon_glyphs_only() {
        let runs = [run("A"), run(" "), run("\u{e32c}")];
        let objects = [object("A"), object(""), object("smartphone")];
        assert!(verify_mapping(&runs, &objects).is_ok());
        assert!(verify_mapping(&runs, &[object("B"), object(""), object("x")]).is_err());
        assert!(verify_mapping(&runs[..2], &objects[..1]).is_err());
    }
}
