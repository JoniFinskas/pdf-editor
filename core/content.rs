use crate::{
    Document, EditResult, Engine, MAX_STREAM, Result, Substitution, TextObject, err, fonts, parse,
};
use lopdf::{
    Dictionary, Object, ObjectId, Stream,
    content::{Content, Operation},
};

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
                let encoding = font.get_font_encoding_with_limit(doc, MAX_STREAM)?;
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
                for item in items {
                    if let Object::String(bytes, _) = item {
                        text.push_str(&encoding.bytes_to_string(&bytes)?);
                    }
                }
                if !text.is_empty() {
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
    let encoding = font.get_font_encoding_with_limit(doc, MAX_STREAM)?;
    let bytes = encoding.string_to_bytes(text);
    if encoding.bytes_to_string(&bytes)? != text {
        return Err(err("Font encoding does not contain the replacement text"));
    }
    Ok(bytes)
}
fn font_name(font: &Dictionary) -> Result<String> {
    let name = String::from_utf8_lossy(font.get(b"BaseFont")?.as_name()?);
    if name.is_empty() {
        return Err(err("Missing font identity"));
    }
    Ok(fonts::base_name(&name).to_lowercase())
}
fn resources(doc: &lopdf::Document, page: ObjectId) -> Result<Dictionary> {
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
pub(crate) fn edit(
    engine: &Engine,
    state: &Document,
    selected: &TextObject,
    ordinal: usize,
    text: &str,
) -> Result<(Vec<u8>, EditResult)> {
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
    let run = source
        .get(ordinal)
        .ok_or_else(|| err("Unsupported text-object mapping"))?;
    let embedded_original = fonts::embedded_font(&pdf, &run.font, text);
    let reuse =
        text.is_empty() || (embedded_original.is_some() && encode(&pdf, &run.font, text).is_ok());
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
            let is_selected = number == selected.page as u32 + 1 && i == ordinal;
            if is_selected || (whole && font_name(&r.font)? == missing_name) {
                let replacement = if is_selected {
                    text.to_string()
                } else {
                    r.text.clone()
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
    let warning = chosen
        .filter(|f| !f.exact)
        .map(|f| format!("Font {} not found, used {}", selected.font, f.name));
    Ok((
        bytes,
        EditResult {
            warning,
            objects_changed: count,
        },
    ))
}
fn verify_mapping(runs: &[Run], objects: &[TextObject]) -> Result<()> {
    if runs.len() != objects.len() || runs.iter().zip(objects).any(|(r, o)| r.text != o.text) {
        return Err(err(
            "This PDF's nested or combined text runs cannot be edited safely",
        ));
    }
    Ok(())
}
