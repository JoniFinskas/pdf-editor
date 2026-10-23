use crate::*;
use lopdf::{Dictionary, Object, ObjectId, Stream, content::Content, dictionary};
use std::collections::BTreeSet;

fn intersects(a: Bounds, b: Bounds) -> bool {
    a.left <= b.right && a.right >= b.left && a.bottom <= b.top && a.top >= b.bottom
}

impl Engine {
    /// Remove whole intersecting objects, then paint black rectangles. One undo step.
    pub fn redact(&self, doc: &mut Document, page: u16, bounds: Bounds) -> Result<()> {
        self.redact_regions(doc, &[(page, bounds)])
    }

    /// Atomic batch redaction. Coordinates are unrotated PDF page coordinates.
    pub fn redact_regions(&self, doc: &mut Document, regions: &[(u16, Bounds)]) -> Result<()> {
        guarded(|| {
            if regions.is_empty() || regions.len() > 10000 {
                return Err(err("Choose between 1 and 10000 redaction regions"));
            }
            let pages = self.pages(doc)?;
            let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            for &(page, b) in regions {
                if usize::from(page) >= pages.len()
                    || [b.left, b.bottom, b.right, b.top]
                        .iter()
                        .any(|x| !x.is_finite() || x.abs() > 100000.0)
                    || b.right <= b.left
                    || b.top <= b.bottom
                {
                    return Err(err("Invalid redaction rectangle"));
                }
                let native_page = native.pages().get(i32::from(page))?;
                let config = render_config(&native_page, 1.0)?;
                let points = [
                    (b.left, b.bottom),
                    (b.right, b.top),
                    (b.left, b.top),
                    (b.right, b.bottom),
                ]
                .into_iter()
                .map(|(x, y)| {
                    native_page.points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &config)
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
                let p = &pages[usize::from(page)];
                if points.iter().all(|p| p.0 <= 0)
                    || points.iter().all(|p| p.1 <= 0)
                    || points.iter().all(|q| q.0 as f32 >= p.width)
                    || points.iter().all(|q| q.1 as f32 >= p.height)
                {
                    return Err(err("Redaction rectangle does not intersect the page"));
                }
            }
            let targets: BTreeSet<_> = regions.iter().map(|r| r.0).collect();
            for &index in &targets {
                let mut page = native.pages().get(i32::from(index))?;
                page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
                if page.objects().len() > 20000 {
                    return Err(err("Too many page objects"));
                }
                for i in (0..page.objects().len()).rev() {
                    let obj = page.objects().get(i)?;
                    let b = obj.bounds()?;
                    let bounds = Bounds {
                        left: b.left().value,
                        bottom: b.bottom().value,
                        right: b.right().value,
                        top: b.top().value,
                    };
                    if [bounds.left, bounds.bottom, bounds.right, bounds.top]
                        .iter()
                        .any(|v| !v.is_finite() || v.abs() > 100000.0)
                    {
                        return Err(err("Cannot verify object bounds for redaction"));
                    }
                    let hit = regions
                        .iter()
                        .any(|(p, b)| *p == index && intersects(bounds, *b));
                    drop(obj);
                    if hit {
                        page.objects_mut().remove_object_at_index(i)?;
                    }
                }
                page.regenerate_content()?;
            }
            let bytes = native.save_to_bytes()?;
            drop(native);
            let mut pdf = clean_page_graph(parse(&bytes)?)?;
            let candidate = self.candidate(doc, pdf.clone())?;
            let verification = self
                .pdfium
                .load_pdf_from_byte_slice(&candidate.bytes, None)?;
            // Independent text extraction and native object checks before adding covers.
            for &page in &targets {
                let intersects_region = |bounds| {
                    regions
                        .iter()
                        .any(|(p, b)| *p == page && intersects(bounds, *b))
                };
                let native_page = verification.pages().get(i32::from(page))?;
                let text = native_page.text()?;
                if text.chars().len() > 1_000_000 {
                    return Err(err("Too much text to verify redaction"));
                }
                for character in text.chars().iter() {
                    let b = character.tight_bounds()?;
                    let bounds = Bounds {
                        left: b.left().value,
                        bottom: b.bottom().value,
                        right: b.right().value,
                        top: b.top().value,
                    };
                    if intersects_region(bounds) {
                        return Err(err(
                            "Redaction text verification failed; original document retained",
                        ));
                    }
                }
                if self
                    .page_items(&candidate, page)?
                    .iter()
                    .any(|o| intersects_region(o.bounds))
                    || self
                        .text_objects(&candidate, page)?
                        .iter()
                        .any(|o| intersects_region(o.bounds))
                {
                    return Err(err(
                        "Redaction verification failed; original document retained",
                    ));
                }
            }
            drop(verification);
            for (&number, &id) in &pdf.get_pages() {
                let content = pdf.get_page_content_with_limit(id, MAX_STREAM)?;
                let mut bytes = b"q\n".to_vec();
                bytes.extend_from_slice(&content);
                bytes.extend_from_slice(b"\nQ\n");
                for &(_, b) in regions.iter().filter(|(p, _)| u32::from(*p) + 1 == number) {
                    bytes.extend_from_slice(
                        format!(
                            "\nq 0 g {} {} {} {} re f Q\n",
                            b.left,
                            b.bottom,
                            b.right - b.left,
                            b.top - b.bottom
                        )
                        .as_bytes(),
                    );
                }
                let stream = pdf.add_object(Stream::new(Dictionary::new(), bytes));
                pdf.get_dictionary_mut(id)?.set("Contents", stream);
            }
            pdf.prune_objects();
            let candidate = self.candidate(doc, pdf)?;
            if candidate.bytes.len() > MAX_FILE {
                return Err(err("Redacted document exceeds size limit"));
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}

// A fresh catalog/page tree excludes source history, tags, attachments, annotations,
// actions and metadata that can retain copies of removed content.
fn clean_page_graph(mut pdf: lopdf::Document) -> Result<lopdf::Document> {
    let pages = pdf.get_pages();
    let root = pdf.new_object_id();
    let mut kids = Vec::new();
    let mut visited = BTreeSet::new();
    for id in pages.values() {
        let data = pdf.get_page_content_with_limit(*id, MAX_STREAM)?;
        let resources = content::resources(&pdf, *id)?;
        let resources = clean_resources(&mut pdf, resources, &data, &mut visited, 0)?;
        let mut page = Dictionary::new();
        page.set("Type", "Page");
        page.set("Parent", root);
        for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate", b"UserUnit"] {
            if let Some(value) = inherited(&pdf, *id, key)? {
                page.set(key, value);
            }
        }
        page.set("Resources", resources);
        let stream = pdf.add_object(Stream::new(Dictionary::new(), data));
        page.set("Contents", stream);
        pdf.objects.insert(*id, Object::Dictionary(page));
        kids.push(Object::Reference(*id));
    }
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids}),
    );
    let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    pdf.trailer = dictionary! {"Root"=>catalog};
    pdf.prune_objects();
    let source = pdf.clone();
    for object in pdf.objects.values_mut() {
        clean_resource_roles(object, &source, 0)?;
        strip_auxiliary(object, 0)?;
    }
    pdf.prune_objects();
    Ok(pdf)
}
fn inherited(pdf: &lopdf::Document, mut id: ObjectId, key: &[u8]) -> Result<Option<Object>> {
    for _ in 0..64 {
        let d = pdf.get_dictionary(id)?;
        if let Ok(value) = d.get(key) {
            return Ok(Some(value.clone()));
        }
        match d.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => id = parent,
            Err(_) => return Ok(None),
        }
    }
    Err(err("Page tree nesting exceeds redaction limit"))
}
fn clean_resources(
    pdf: &mut lopdf::Document,
    resources: Dictionary,
    data: &[u8],
    visited: &mut BTreeSet<ObjectId>,
    depth: usize,
) -> Result<Dictionary> {
    if depth > 32 {
        return Err(err("Form nesting exceeds redaction limit"));
    }
    let ops = Content::decode(data)?.operations;
    // Alternate text can conceal copies not represented by geometric objects.
    if ops
        .iter()
        .any(|op| matches!(op.operator.as_str(), "BDC" | "DP"))
    {
        return Err(err(
            "Redaction does not support marked content with properties",
        ));
    }
    let mut result = Dictionary::new();
    for (key, operators) in [
        (b"XObject".as_slice(), &["Do"][..]),
        (b"Font".as_slice(), &["Tf"][..]),
        (b"ExtGState".as_slice(), &["gs"][..]),
        (b"ColorSpace".as_slice(), &["cs", "CS"][..]),
        (b"Pattern".as_slice(), &["scn", "SCN"][..]),
        (b"Shading".as_slice(), &["sh"][..]),
    ] {
        let Some(object) = resources.get(key).ok().cloned() else {
            continue;
        };
        let (_, object) = pdf.dereference(&object)?;
        let original = object.as_dict()?.clone();
        let mut kept = Dictionary::new();
        for (name, value) in original.iter() {
            if !ops.iter().any(|op| {
                operators.contains(&op.operator.as_str())
                    && op
                        .operands
                        .iter()
                        .any(|v| v.as_name().ok() == Some(name.as_slice()))
            }) {
                continue;
            }
            if key == b"Pattern" || key == b"Shading" {
                return Err(err(
                    "Redaction does not support retained patterns or shadings",
                ));
            }
            if key == b"ExtGState" {
                let (_, object) = pdf.dereference(value)?;
                if object.as_dict()?.has(b"Font") {
                    return Err(err(
                        "Redaction does not support graphics-state font selection",
                    ));
                }
                if object
                    .as_dict()?
                    .get(b"SMask")
                    .is_ok_and(|o| o.as_name().ok() != Some(b"None"))
                {
                    return Err(err(
                        "Redaction does not support retained soft-mask graphics states",
                    ));
                }
            }
            if key == b"Font" {
                let (_, object) = pdf.dereference(value)?;
                if object.as_dict()?.get(b"Subtype")?.as_name()? == b"Type3" {
                    return Err(err("Redaction does not support retained Type 3 fonts"));
                }
            }
            if key == b"XObject" {
                let id = value.as_reference()?;
                if !visited.insert(id) {
                    return Err(err("Cyclic form resources cannot be redacted"));
                }
                let mut stream = pdf.get_object(id)?.as_stream()?.clone();
                if stream.dict.get(b"Subtype")?.as_name()? == b"Form" {
                    let data = stream.get_plain_content_with_limit(MAX_STREAM)?;
                    if data.len() > MAX_STREAM {
                        return Err(err("Form content exceeds redaction limit"));
                    }
                    let r = stream
                        .dict
                        .get(b"Resources")
                        .ok()
                        .map(|o| pdf.dereference(o).and_then(|(_, o)| o.as_dict()))
                        .transpose()?
                        .cloned()
                        .ok_or_else(|| {
                            err("Redaction does not support forms with inherited resources")
                        })?;
                    let cleaned = clean_resources(pdf, r, &data, visited, depth + 1)?;
                    stream.dict.set("Resources", cleaned);
                }
                // Remove auxiliary source data without altering image color/mask data.
                for k in [b"Metadata".as_slice(), b"PieceInfo", b"OPI"] {
                    stream.dict.remove(k);
                }
                pdf.objects.insert(id, Object::Stream(stream));
                visited.remove(&id);
            }
            if key == b"Font" || key == b"ExtGState" {
                let (_, object) = pdf.dereference(value)?;
                let mut dictionary = object.as_dict()?.clone();
                dictionary.set("Type", if key == b"Font" { "Font" } else { "ExtGState" });
                let id = pdf.add_object(dictionary);
                kept.set(name.clone(), id);
            } else {
                kept.set(name.clone(), value.clone());
            }
        }
        result.set(key, kept);
    }

    Ok(result)
}

// Resource dictionary meaning comes from its owning key, not an optional /Type.
// Resolve these small dictionaries before pruning so private references cannot
// survive merely because the containing resource is still in use.
fn clean_resource_roles(object: &mut Object, pdf: &lopdf::Document, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(err("Resource nesting exceeds redaction limit"));
    }
    let dictionary = match object {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&mut s.dict),
        Object::Array(values) => {
            for value in values {
                clean_resource_roles(value, pdf, depth + 1)?;
            }
            None
        }
        _ => None,
    };
    if let Some(dictionary) = dictionary {
        if dictionary.has(b"FunctionType") {
            return Err(err(
                "Redaction does not support retained function resources",
            ));
        }
        for (key, value) in dictionary.iter_mut() {
            let allowed: Option<&[&[u8]]> = match key.as_slice() {
                b"Encoding" => Some(&[b"Type", b"BaseEncoding", b"Differences"]),
                b"CIDSystemInfo" => Some(&[b"Registry", b"Ordering", b"Supplement"]),
                b"DecodeParms" => Some(&[
                    b"Predictor",
                    b"Colors",
                    b"BitsPerComponent",
                    b"Columns",
                    b"EarlyChange",
                    b"K",
                    b"EndOfLine",
                    b"EncodedByteAlign",
                    b"Rows",
                    b"EndOfBlock",
                    b"BlackIs1",
                    b"DamagedRowsBeforeError",
                    b"JBIG2Globals",
                    b"ColorTransform",
                ]),
                b"Group" => Some(&[b"Type", b"S", b"CS", b"I", b"K"]),
                _ => None,
            };
            if let Some(allowed) = allowed {
                let (_, resolved) = pdf.dereference(value)?;
                if let Object::Dictionary(d) = resolved {
                    let mut cleaned = d.clone();
                    let removed: Vec<_> = cleaned
                        .iter()
                        .filter(|(k, _)| !allowed.contains(&k.as_slice()))
                        .map(|(k, _)| k.clone())
                        .collect();
                    for k in removed {
                        cleaned.remove(&k);
                    }
                    if key.as_slice() == b"Encoding" {
                        cleaned.set("Type", "Encoding");
                        if !cleaned.has(b"Differences") {
                            cleaned.set("Differences", Vec::<Object>::new());
                        }
                    }
                    *value = Object::Dictionary(cleaned);
                }
            }
            clean_resource_roles(value, pdf, depth + 1)?;
        }
    }
    Ok(())
}

fn strip_auxiliary(object: &mut Object, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(err("Resource nesting exceeds redaction limit"));
    }
    match object {
        Object::Stream(stream) => {
            let allowed: &[&[u8]] = &[
                b"Type",
                b"Subtype",
                b"Length",
                b"Filter",
                b"DecodeParms",
                b"DL",
                b"Length1",
                b"Length2",
                b"Length3",
                b"BBox",
                b"Matrix",
                b"Resources",
                b"Group",
                b"FormType",
                b"Width",
                b"Height",
                b"ColorSpace",
                b"BitsPerComponent",
                b"Intent",
                b"ImageMask",
                b"Mask",
                b"Decode",
                b"Interpolate",
                b"SMask",
                b"SMaskInData",
                b"Matte",
                b"N",
                b"Alternate",
                b"Range",
                b"CMapName",
                b"CIDSystemInfo",
                b"WMode",
                b"UseCMap",
            ];
            let keys: Vec<_> = stream
                .dict
                .iter()
                .filter(|(k, _)| !allowed.contains(&k.as_slice()))
                .map(|(k, _)| k.clone())
                .collect();
            for k in keys {
                stream.dict.remove(&k);
            }
            let mut dict = Object::Dictionary(stream.dict.clone());
            strip_auxiliary(&mut dict, depth + 1)?;
            stream.dict = dict.as_dict()?.clone();
        }
        Object::Dictionary(dict) => {
            let kind = dict.get(b"Type").ok().and_then(|v| v.as_name().ok());
            let allowed: Option<&[&[u8]]> = match kind {
                Some(b"Font") => Some(&[
                    b"Type",
                    b"Subtype",
                    b"Name",
                    b"BaseFont",
                    b"FirstChar",
                    b"LastChar",
                    b"Widths",
                    b"FontDescriptor",
                    b"Encoding",
                    b"ToUnicode",
                    b"DescendantFonts",
                    b"CIDSystemInfo",
                    b"DW",
                    b"W",
                    b"DW2",
                    b"W2",
                    b"CIDToGIDMap",
                ]),
                Some(b"FontDescriptor") => Some(&[
                    b"Type",
                    b"FontName",
                    b"FontFamily",
                    b"FontStretch",
                    b"FontWeight",
                    b"Flags",
                    b"FontBBox",
                    b"ItalicAngle",
                    b"Ascent",
                    b"Descent",
                    b"Leading",
                    b"CapHeight",
                    b"XHeight",
                    b"StemV",
                    b"StemH",
                    b"AvgWidth",
                    b"MaxWidth",
                    b"MissingWidth",
                    b"FontFile",
                    b"FontFile2",
                    b"FontFile3",
                    b"CharSet",
                    b"Style",
                    b"Lang",
                    b"FD",
                    b"CIDSet",
                ]),
                Some(b"ExtGState") => Some(&[
                    b"Type", b"LW", b"LC", b"LJ", b"ML", b"D", b"RI", b"OP", b"op", b"OPM",
                    b"Font", b"BG", b"BG2", b"UCR", b"UCR2", b"TR", b"TR2", b"HT", b"FL", b"SM",
                    b"SA", b"BM", b"SMask", b"CA", b"ca", b"AIS", b"TK",
                ]),
                _ => None,
            };
            let keys: Vec<_> = dict
                .iter()
                .filter(|(k, _)| {
                    allowed.is_some_and(|a| !a.contains(&k.as_slice()))
                        || [
                            b"Metadata".as_slice(),
                            b"PieceInfo",
                            b"OPI",
                            b"AF",
                            b"AA",
                            b"A",
                            b"JS",
                            b"JavaScript",
                            b"EmbeddedFiles",
                            b"XFA",
                            b"ActualText",
                            b"Alt",
                        ]
                        .contains(&k.as_slice())
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in keys {
                dict.remove(&k);
            }
            for (_, value) in dict.iter_mut() {
                strip_auxiliary(value, depth + 1)?;
            }
        }
        Object::Array(values) => {
            for value in values {
                strip_auxiliary(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
