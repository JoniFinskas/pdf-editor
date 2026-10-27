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
pub(crate) fn decoding<'a>(doc: &'a lopdf::Document, font: &'a Dictionary) -> Result<Encoding<'a>> {
    #[cfg(test)]
    ENCODING_BUILDS.with(|count| count.set(count.get() + 1));
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
pub(crate) struct Run {
    op: usize,
    pub(crate) key: Vec<u8>,
    size: f32,
    text: String,
    pub(crate) font: Dictionary,
    pub(crate) spacing: [f32; 4],
}
pub(crate) fn runs(
    doc: &lopdf::Document,
    page: ObjectId,
    content: &Content<Vec<Operation>>,
) -> Result<Vec<Run>> {
    let fonts = doc.get_page_fonts(page)?;
    let mut encodings = std::collections::HashMap::new();
    let mut key = Vec::new();
    let mut size = 0.0;
    let mut spacing = [0.0, 0.0, 100.0, 0.0];
    let mut stack = vec![];
    let mut result = vec![];
    for (index, op) in content.operations.iter().enumerate() {
        match op.operator.as_str() {
            "q" => {
                if stack.len() > 128 {
                    return Err(err("Excessive graphics nesting"));
                }
                stack.push((key.clone(), size, spacing));
            }
            "Q" => {
                if let Some(state) = stack.pop() {
                    (key, size, spacing) = state;
                }
            }
            "Tc" | "Tw" | "Tz" | "Tr" => {
                let index = match op.operator.as_str() {
                    "Tc" => 0,
                    "Tw" => 1,
                    "Tz" => 2,
                    _ => 3,
                };
                spacing[index] = op
                    .operands
                    .first()
                    .ok_or_else(|| err("Invalid text state"))?
                    .as_float()?;
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
                // Decode each font map once per page traversal, not once per run.
                if !encodings.contains_key(&key) {
                    encodings.insert(key.clone(), decoding(doc, font)?);
                }
                let encoding = &encodings[&key];
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
                        spacing,
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
pub(crate) fn encode(doc: &lopdf::Document, font: &Dictionary, text: &str) -> Result<Vec<u8>> {
    let encoding = decoding(doc, font)?;
    encode_with(&encoding, text)
}
pub(crate) fn encode_with(encoding: &Encoding<'_>, text: &str) -> Result<Vec<u8>> {
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

/// The standard Latin Type1 fonts provide their predefined glyph sets without
/// embedding. Custom descriptors, widths or maps require the normal checks.
fn standard_font_covers(doc: &lopdf::Document, font: &Dictionary, text: &str) -> bool {
    matches!(font.get(b"Subtype").and_then(Object::as_name), Ok(b"Type1"))
        && matches!(font.get(b"BaseFont").and_then(Object::as_name),
            Ok(b"Helvetica" | b"Helvetica-Bold" | b"Helvetica-Oblique" | b"Helvetica-BoldOblique"
                | b"Times-Roman" | b"Times-Bold" | b"Times-Italic" | b"Times-BoldItalic"
                | b"Courier" | b"Courier-Bold" | b"Courier-Oblique" | b"Courier-BoldOblique"))
        && !font.has(b"FontDescriptor")
        && !font.has(b"ToUnicode")
        && !font.has(b"Widths")
        && !font.has(b"FirstChar")
        && !font.has(b"LastChar")
        && (!font.has(b"Encoding") || matches!(
            font.get_deref(b"Encoding", doc).and_then(Object::as_name),
            Ok(b"StandardEncoding" | b"WinAnsiEncoding" | b"MacRomanEncoding")))
        && encode(doc, font, text).is_ok()
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
// Test-only work counters for cursor repairs and font-map construction.
#[cfg(test)]
thread_local! {
    pub(crate) static REPAIR_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static ENCODING_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

enum CursorRepair {
    Shift(f32),
    Anchor([f32; 6]),
}

// Absolute anchors avoid accumulated floating-point cursor drift. They are
// equivalent only when no operation later consumes the text line matrix, the
// graphics transform is untouched, and horizontal scale/rise are neutral.
fn can_anchor_cursors(ops: &Content<Vec<Operation>>, records: &[Run]) -> bool {
    let mut depth = 0i32;
    let mut text = false;
    records.iter().all(|r| {
        matches!(
            r.font.get(b"Subtype").and_then(Object::as_name),
            Ok(b"Type0" | b"TrueType")
        )
    }) && ops.operations.iter().all(|op| match op.operator.as_str() {
        "BT" => { text = true; true }
        "ET" => { text = false; true }
        "q" | "Q" => {
            // Saving state outside a text object leaves its cursor unchanged.
            depth += if op.operator == "q" { 1 } else { -1 };
            !text && (0..=128).contains(&depth)
        }
        "Tf" | "Tm" | "Tj" | "Tc" | "Tw" | "Tr" | "TL" | "g" | "G" | "rg" | "RG"
        | "k" | "K" => true,
        "TJ" => {
            op.operands
                .first()
                .and_then(|value| value.as_array().ok())
                .is_some_and(|items| {
                    // A leading TJ adjustment may already be included in the native
                    // object's origin. Do not apply it again after an absolute anchor.
                    match items.iter().position(
                        |item| matches!(item,Object::String(bytes,_) if !bytes.is_empty()),
                    ) {
                        None => true,
                        Some(first) => items[..first].iter().all(|item| match item {
                            Object::String(bytes, _) => bytes.is_empty(),
                            value => value.as_float().ok() == Some(0.0),
                        }),
                    }
                })
        }
        "Tz" => op.operands.first().and_then(|v| v.as_float().ok()) == Some(100.0),
        "Ts" => op.operands.first().and_then(|v| v.as_float().ok()) == Some(0.0),
        _ => false,
    }) && depth == 0
}

/// Propagation of planned TJ translations. Glyph advances need not be modeled:
/// a TJ adjustment translates subsequent text positions until a line reset.
#[derive(Default)]
struct CursorShift {
    offset: [f32; 2],
    uncertain: bool,
}
impl CursorShift {
    fn advance(&mut self, operator: &str) {
        match operator {
            "BT" | "Tm" => *self = Self::default(),
            "Td" | "TD" | "T*" | "'" | "\"" if !self.uncertain => self.offset = [0.0; 2],
            // Keep the sequential fallback across graphics transformations or
            // saved state; a later absolute text reset makes batching safe again.
            "cm" | "q" | "Q" if self.offset != [0.0; 2] => self.uncertain = true,
            _ => {}
        }
    }
}

/// Transform selected text shows without regenerating their font resources.
pub(crate) fn transform_text(engine: &Engine, state: &Document, page_index: u16,
    selected: &[usize], matrix: [f32;6]) -> Result<Vec<u8>> {
    fn multiply(a:[f32;6],b:[f32;6])->[f32;6] {
        [a[0]*b[0]+a[2]*b[1],a[1]*b[0]+a[3]*b[1],
         a[0]*b[2]+a[2]*b[3],a[1]*b[2]+a[3]*b[3],
         a[0]*b[4]+a[2]*b[5]+a[4],a[1]*b[4]+a[3]*b[5]+a[5]]
    }
    fn inverse(a:[f32;6])->Result<[f32;6]> {
        let d=a[0]*a[3]-a[1]*a[2];
        if !d.is_finite() || d.abs()<0.000001 {return Err(err("Text transform is singular"));}
        Ok([a[3]/d,-a[1]/d,-a[2]/d,a[0]/d,
            (a[2]*a[5]-a[3]*a[4])/d,(a[1]*a[4]-a[0]*a[5])/d])
    }
    let mut pdf=parse(&state.bytes)?;
    let page=*pdf.get_pages().get(&(u32::from(page_index)+1)).ok_or_else(||err("Invalid page"))?;
    let content=Content::decode(&pdf.get_page_content_with_limit(page,MAX_STREAM)?)?;
    let records=runs(&pdf,page,&content)?;
    let before=engine.text_objects(state,page_index)?;
    verify_mapping(&records,&before)?;
    let selected_ops:std::collections::HashSet<_>=selected.iter().map(|ordinal|
        records.get(*ordinal).map(|r|r.op).ok_or_else(||err("Text selection is stale"))).collect::<Result<_>>()?;
    let mut ctm=[1.0,0.0,0.0,1.0,0.0,0.0];
    let mut clipped=false;
    let mut move_clip=false;
    let mut stack=vec![];
    let mut output=vec![];
    for (index,mut op) in content.operations.into_iter().enumerate() {
        if selected_ops.contains(&index) {
            move_clip |= clipped;
            // Quote operators also change line position and spacing, which must
            // remain outside the temporary graphics transform.
            if op.operator=="\"" {
                output.push(Operation::new("Tw",vec![op.operands.first().ok_or_else(||err("Invalid quote spacing"))?.clone()]));
                output.push(Operation::new("Tc",vec![op.operands.get(1).ok_or_else(||err("Invalid quote spacing"))?.clone()]));
            }
            if op.operator=="'" || op.operator=="\"" {
                output.push(Operation::new("T*",vec![]));
                op=Operation::new("Tj",vec![op.operands.last().ok_or_else(||err("Invalid text show"))?.clone()]);
            }
            let local=multiply(multiply(inverse(ctm)?,matrix),ctm);
            if local.iter().any(|v|!v.is_finite() || v.abs()>1.0e8) {
                return Err(err("Text transform exceeds limits"));
            }
            output.push(Operation::new("cm",local.into_iter().map(Object::Real).collect()));
            output.push(op);
            output.push(Operation::new("cm",inverse(local)?.into_iter().map(Object::Real).collect()));
        } else {
            match op.operator.as_str() {
                "q"=>{if stack.len()>128{return Err(err("Excessive graphics nesting"));}stack.push((ctm,clipped));},
                "Q"=>(ctm,clipped)=stack.pop().ok_or_else(||err("Unbalanced graphics state"))?,
                "W"|"W*"=>clipped=true,
                "cm"=>{
                    let values=op.operands.iter().map(Object::as_float).collect::<std::result::Result<Vec<_>,_>>()?;
                    let values:[f32;6]=values.try_into().map_err(|_|err("Invalid graphics transform"))?;
                    ctm=multiply(ctm,values);
                },
                _=>{}
            }
            output.push(op);
        }
    }
    if move_clip {
        let indices=selected.iter().map(|&ordinal|before[ordinal].index).collect::<Vec<_>>();
        pdf=parse(&engine.transform_page_objects(state,page_index,&indices,matrix)?)?;
    } else {
        pdf.change_page_content(page,Content {operations:output}.encode()?)?;
    }
    let candidate=engine.candidate(state,pdf)?;
    let check=parse(&candidate.bytes)?;
    let check_page=check.get_pages()[&(u32::from(page_index)+1)];
    let check_content=Content::decode(&check.get_page_content_with_limit(check_page,MAX_STREAM)?)?;
    let after_runs=runs(&check,check_page,&check_content)?;
    if records.len()!=after_runs.len() || records.iter().zip(&after_runs).any(|(a,b)|a.text!=b.text) {
        return Err(err("Text transform changed encoded content"));
    }
    let after=engine.text_objects(&candidate,page_index)?;
    if before.len()!=after.len() {return Err(err("Text transform changed the object structure"));}
    for (ordinal,(old,new)) in before.iter().zip(&after).enumerate() {
        let expected=if selected.contains(&ordinal) {multiply(matrix,old.matrix)} else {old.matrix};
        // Native extraction can suppress overlapping identical glyphs. Verify
        // encoded text above and every native transform here.
        if expected.iter().zip(new.matrix).any(|(a,b)|(a-b).abs()>0.05) {
            return Err(err("Text transform verification failed; document unchanged"));
        }
    }
    Ok(candidate.bytes)
}

/// Replaces the runs at `targets` (ordinal, replacement) on the page of
/// `selected`, the first targeted object, in one transaction.
pub(crate) fn edit(
    engine: &Engine,
    state: &Document,
    selected: &TextObject,
    targets: &[(usize, String)],
) -> Result<(Vec<u8>, EditResult)> {
    edit_styled(engine, state, selected, targets, None)
}
pub(crate) fn edit_styled(
    engine: &Engine,
    state: &Document,
    selected: &TextObject,
    targets: &[(usize, String)],
    style: Option<(f32, [u8; 3])>,
) -> Result<(Vec<u8>, EditResult)> {
    edit_with_font(engine, state, selected, targets, style, None, false)
}

pub(crate) fn edit_for_reflow(
    engine: &Engine, state: &Document, selected: &TextObject, targets: &[(usize, String)],
) -> Result<(Vec<u8>, EditResult)> {
    edit_with_font(engine, state, selected, targets, None, None, true)
}

pub(crate) fn edit_formatted(
    engine: &Engine, state: &Document, selected: &TextObject,
    targets: &[(usize, String)], font: &fonts::Font, size: f32, color: [u8; 3],
) -> Result<(Vec<u8>, EditResult)> {
    edit_with_font(engine, state, selected, targets, Some((size, color)), Some(font), false)
}

fn edit_with_font(
    engine: &Engine, state: &Document, selected: &TextObject,
    targets: &[(usize, String)], style: Option<(f32, [u8; 3])>,
    forced_font: Option<&fonts::Font>,
    require_metrics: bool,
) -> Result<(Vec<u8>, EditResult)> {
    #[cfg(test)]
    REPAIR_PASSES.with(|count| count.set(0));
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
    let reuse = style.is_some()
        || text.is_empty()
        || (embedded_original.is_some() && encode(&pdf, &run.font, text).is_ok())
        || (!require_metrics && (type3_covers(&pdf, &run.font, text)
            || standard_font_covers(&pdf, &run.font, text)));
    let chosen = if let Some(font) = forced_font {
        Some(font.clone())
    } else if reuse {
        None
    } else if let Some(font) = embedded_original {
        Some(font)
    } else {
        Some(fonts::find_for_object(selected, text)?)
    };
    let whole =
        chosen.as_ref().is_some_and(|f| !f.exact) && state.substitution == Substitution::Document;
    let missing_name = font_name(&run.font)?;
    let mut plans = vec![];
    let mut combined = text.to_string();
    let mut selected_data = Some((content, source, native));
    for (&number, &page_id) in &pages {
        if number != selected.page as u32 + 1 && !whole {
            continue;
        }
        let (operations, records, before) = if number == selected.page as u32 + 1 {
            selected_data
                .take()
                .ok_or_else(|| err("Duplicate selected page"))?
        } else {
            let operations =
                Content::decode(&pdf.get_page_content_with_limit(page_id, MAX_STREAM)?)?;
            let records = runs(&pdf, page_id, &operations)?;
            let before = engine.text_objects(state, (number - 1) as u16)?;
            (operations, records, before)
        };
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
            Some(fonts::find_for_object(selected, &combined)?)
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
        let mut encodings = std::collections::HashMap::new();
        for (i, replacement) in changes.iter().rev() {
            let r = &records[*i];
            let bytes = if style.is_some() && forced_font.is_none() {
                vec![]
            } else if let Some(f) = &embedded {
                f.encode(replacement)?
            } else {
                if !encodings.contains_key(&r.key) {
                    encodings.insert(r.key.clone(), decoding(&pdf, &r.font)?);
                }
                encode_with(&encodings[&r.key], replacement)?
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
            if let Some((size, color)) = style {
                revised.push(Operation::new("q", vec![]));
                revised.push(Operation::new(
                    "Tf",
                    vec![Object::Name(new_key.clone().unwrap_or_else(|| r.key.clone())), size.into()],
                ));
                for operator in ["rg", "RG"] {
                    revised.push(Operation::new(
                        operator,
                        color
                            .iter()
                            .map(|c| Object::Real(*c as f32 / 255.0))
                            .collect(),
                    ));
                }
                revised.push(if forced_font.is_some() {
                    Operation::new("Tj", vec![Object::String(bytes, lopdf::StringFormat::Hexadecimal)])
                } else if original.operator == "'" || original.operator == "\"" {
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
                    original.clone()
                });
                revised.push(Operation::new("Q", vec![]));
            } else {
                revised.push(Operation::new(
                    "Tj",
                    vec![Object::String(bytes, lopdf::StringFormat::Hexadecimal)],
                ));
            }
            if new_key.is_some() {
                revised.push(Operation::new(
                    "Tf",
                    vec![Object::Name(r.key.clone()), r.size.into()],
                ));
            }
            operations.operations.splice(r.op..=r.op, revised);
            count += 1;
        }
        drop(encodings);
        let stream = pdf.add_object(Stream::new(Dictionary::new(), operations.encode()?));
        pdf.get_object_mut(*page_id)?
            .as_dict_mut()?
            .set("Contents", stream);
    }
    let mut bytes = vec![];
    pdf.save_to(&mut bytes)?;
    // PDFium regeneration drops Tc/Tw. Keep content streams and repair text
    // positions with TJ shifts or eligible Tm anchors, measured independently.
    // Batch predictable cursor translations, then independently verify the whole
    // page. Unsupported graphics-state propagation retains sequential repairs.
    for (number, page_id, _, _, before, changes) in &plans {
        let expected: Vec<_> = before
            .iter()
            .enumerate()
            .filter(|(i, _)| !changes.iter().any(|(j, t)| j == i && t.is_empty()))
            .collect();
        let mut settled = false;
        for _ in 0..=expected.len().min(1024) {
            #[cfg(test)]
            REPAIR_PASSES.with(|count| count.set(count.get() + 1));
            let candidate = Document {
                bytes: bytes.clone(),
                original: state.original.clone(),
                undo: vec![],
                redo: vec![],
                revision: state.revision,
                protected: state.protected,
                substitution: state.substitution,
            };
            let after = engine.text_objects(&candidate, (number - 1) as u16)?;
            if expected.len() != after.len() {
                return Err(err("Text-object structure changed unexpectedly"));
            }
            // Validate text and transforms before skipping unnecessary repair decoding.
            for ((ordinal, old), new) in expected.iter().zip(&after) {
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
            }
            let needs_repair = expected.iter().zip(&after).any(|((_, old), new)| {
                (old.matrix[4] - new.matrix[4]).abs() > 0.0001
                    || (old.matrix[5] - new.matrix[5]).abs() > 0.0001
            });
            if !needs_repair {
                settled = true;
                break;
            }
            let mut ops = Content::decode(&pdf.get_page_content_with_limit(*page_id, MAX_STREAM)?)?;
            let records = runs(&pdf, *page_id, &ops)?;
            if records.len() != after.len() {
                return Err(err("Text mapping changed"));
            }
            let anchor = can_anchor_cursors(&ops, &records);
            let mut corrections = Vec::new();
            let mut cursor = CursorShift::default();
            let mut operation = 0;
            for (position, ((_, old), new)) in expected.iter().zip(&after).enumerate() {
                if anchor {
                    corrections.push((position, CursorRepair::Anchor(old.matrix)));
                    continue;
                }
                let op_index = records[position].op;
                for op in &ops.operations[operation..=op_index] {
                    cursor.advance(&op.operator);
                }
                operation = op_index + 1;
                if cursor.uncertain {
                    continue;
                }
                let drift = [old.matrix[4] - new.matrix[4], old.matrix[5] - new.matrix[5]];
                let dx = drift[0] - cursor.offset[0];
                let dy = drift[1] - cursor.offset[1];
                if dx.abs() > 0.0001 || dy.abs() > 0.0001 {
                    let a = old.matrix[0];
                    let b = old.matrix[1];
                    let norm = a * a + b * b;
                    if norm < 0.000001
                        || old.size.abs() < 0.000001
                        || (dx * b - dy * a).abs() > 0.001
                    {
                        if !corrections.is_empty() {
                            break;
                        }
                        return Err(err("Cannot safely restore text cursor"));
                    }
                    let amount = -1000.0 * (dx * a + dy * b) / norm / old.size;
                    if !amount.is_finite() {
                        return Err(err("Invalid text cursor correction"));
                    }
                    corrections.push((position, CursorRepair::Shift(amount)));
                    cursor.offset = drift;
                }
            }
            if corrections.is_empty() {
                settled = true;
                break;
            }
            // Reverse insertion preserves operation indices for every correction.
            for (position, repair) in corrections.into_iter().rev() {
                let op_index = records
                    .get(position)
                    .ok_or_else(|| err("Text mapping changed"))?
                    .op;
                let amount = match repair {
                    CursorRepair::Anchor(matrix) => {
                        ops.operations.insert(
                            op_index,
                            Operation::new("Tm", matrix.into_iter().map(Object::Real).collect()),
                        );
                        continue;
                    }
                    CursorRepair::Shift(amount) => amount,
                };
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
            }
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
pub(crate) fn verify_mapping(runs: &[Run], objects: &[TextObject]) -> Result<()> {
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
            spacing: [0.0, 0.0, 100.0, 0.0],
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
            quad: [[0.;2];4],
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

    #[test]
    fn cursor_anchors_allow_only_balanced_outer_graphics_saves() {
        let mut record = run("A");
        record.font.set("Subtype", "TrueType");
        for (data, expected) in [
            ("q BT /F1 12 Tf (A) Tj ET Q", true),
            ("q q BT /F1 12 Tf (A) Tj ET Q Q", true),
            ("Q BT /F1 12 Tf (A) Tj ET", false),
            ("q BT /F1 12 Tf (A) Tj ET", false),
            ("BT q /F1 12 Tf (A) Tj Q ET", false),
            ("q 1 0 0 1 2 3 cm BT /F1 12 Tf (A) Tj ET Q", false),
        ] {
            let ops = Content::decode(data.as_bytes()).unwrap();
            assert_eq!(can_anchor_cursors(&ops, std::slice::from_ref(&record)), expected, "{data}");
        }
    }

    #[test]
    fn standard_font_reuse_rejects_custom_fonts_maps_and_missing_glyphs() {
        let pdf = lopdf::Document::with_version("1.7");
        let mut base = Dictionary::new();
        base.set("Type", "Font");
        base.set("Subtype", "Type1");
        base.set("BaseFont", "Helvetica");
        assert!(standard_font_covers(&pdf, &base, "Added 123"));
        assert!(!standard_font_covers(&pdf, &base, "Ж"));
        for (key, value) in [
            ("BaseFont", Object::Name(b"ABCDEF+Helvetica".to_vec())),
            ("BaseFont", Object::Name(b"Symbol".to_vec())),
            ("Subtype", Object::Name(b"Type3".to_vec())),
            ("FontDescriptor", Object::Dictionary(Dictionary::new())),
            ("Encoding", Object::Dictionary(Dictionary::new())),
            ("ToUnicode", Object::Null),
            ("Widths", Object::Array(vec![667.into()])),
            ("FirstChar", 65.into()),
            ("LastChar", 66.into()),
        ] {
            let mut custom = base.clone();
            custom.set(key, value);
            assert!(!standard_font_covers(&pdf, &custom, "Added 123"), "{key}");
        }
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;
    #[test]
    fn graphics_boundaries_defer_propagation_until_an_absolute_reset() {
        for boundary in ["q", "Q", "cm"] {
            let mut cursor = CursorShift {
                offset: [12.0, 0.0],
                uncertain: false,
            };
            cursor.advance(boundary);
            assert!(cursor.uncertain);
            for relative in ["Td", "TD", "T*", "'", "\""] {
                cursor.advance(relative);
                assert!(cursor.uncertain);
            }
            cursor.advance("Tm");
            assert!(!cursor.uncertain);
            assert_eq!(cursor.offset, [0.0; 2]);
        }
    }
}
