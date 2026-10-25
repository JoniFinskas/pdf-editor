use crate::*;
use lopdf::{Dictionary, Object, Stream, content::Content};
#[path="sanitize_verify.rs"]
mod verification;

#[derive(Clone, Copy, Debug, Default)]
pub struct SanitizeOptions {
    pub remove_comments: bool,
    pub remove_links: bool,
    /// Rebuild only visible pixels; removes editable text, forms and links.
    pub flatten_visible: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SanitizeReport {
    pub metadata: usize,
    pub hidden_text_blocks: usize,
    pub attachments: usize,
    pub actions: usize,
    pub comments: usize,
    pub links: usize,
    pub rasterized_pages: usize,
}
impl std::fmt::Display for SanitizeReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.rasterized_pages > 0 {
            return write!(f, "Rebuilt {} image-only pages. Save As to keep the cleaned copy.", self.rasterized_pages);
        }
        let removed: Vec<_> = [
            (self.metadata, "metadata entries"),
            (self.hidden_text_blocks, "hidden text blocks"),
            (self.attachments, "attachments"),
            (self.actions, "actions"),
            (self.comments, "comments"),
            (self.links, "links"),
        ].into_iter().filter(|(count, _)| *count > 0)
            .map(|(count, label)| format!("{count} {label}")).collect();
        if removed.is_empty() {
            return f.write_str("Sanitized. Save As to keep the cleaned copy.");
        }
        write!(f, "Removed {}. Save As to keep the cleaned copy.", removed.join(", "))
    }
}
impl Engine {
    /// Export and verify a new sanitized copy without changing the editor or its history.
    /// Output is unencrypted; the source and existing destinations are never replaced.
    pub fn export_sanitized(&self, doc: &Document, options: SanitizeOptions, path: &Path) -> Result<SanitizeReport> {
        guarded(|| {
            let mut copy = doc.comparison_snapshot();
            operation::checkpoint("Sanitizing copy",0,1)?;
            let report = self.sanitize(&mut copy, options)?;
            self.verify_sanitized(&copy, options)?;
            operation::checkpoint("Writing sanitized copy",0,1)?;
            save::write_verified_new(path, &copy.bytes, |temporary| {
                operation::checkpoint("Verifying saved copy",0,1).map_err(std::io::Error::other)?;
                let saved = self.open(temporary).map_err(std::io::Error::other)?;
                if saved.bytes != copy.bytes {
                    return Err(std::io::Error::other("Sanitized copy verification failed"));
                }
                self.verify_sanitized(&saved, options).map_err(std::io::Error::other)?;
                operation::checkpoint("Publishing sanitized copy",1,1).map_err(std::io::Error::other)
            })?;
            Ok(report)
        })
    }

    /// Check the selected sanitization guarantees; this does not assess visible content.
    pub fn verify_sanitized(&self, doc: &Document, options: SanitizeOptions) -> Result<()> {
        guarded(|| {
            let pdf = parse(&doc.bytes)?;
            if pdf.trailer.has(b"Prev") || pdf.trailer.has(b"Info") {
                return Err(err("Sanitized copy contains metadata or saved revisions"));
            }
            verification::base(&pdf,&doc.bytes)?;
            let mut check = doc.comparison_snapshot();
            let report = self.sanitize(&mut check, SanitizeOptions { flatten_visible: false, ..options })?;
            if report != SanitizeReport::default() {
                return Err(err("Sanitized copy still contains removable content"));
            }
            if options.flatten_visible {
                verification::raster(&pdf)?;
                fn raster_only(object: &Object, depth: usize) -> Result<()> {
                    if depth > 128 { return Err(err("Sanitized object nesting limit exceeded")); }
                    match object {
                        Object::Dictionary(dict) => {
                            for (key, value) in dict.iter() {
                                if matches!(key.as_slice(), b"Font"|b"FontDescriptor"|b"ToUnicode"|b"Annots"|b"AcroForm"|b"Outlines"|b"StructTreeRoot") {
                                    return Err(err("Rebuilt copy contains non-image document content"));
                                }
                                raster_only(value, depth + 1)?;
                            }
                        }
                        Object::Stream(stream) => raster_only(&Object::Dictionary(stream.dict.clone()), depth + 1)?,
                        Object::Array(values) => for value in values { raster_only(value, depth + 1)?; },
                        _ => (),
                    }
                    Ok(())
                }
                for object in pdf.objects.values() { raster_only(object, 0)?; }
                for (&number, &id) in &pdf.get_pages() {
                    let operators = Content::decode(&pdf.get_page_content_with_limit(id, MAX_STREAM)?)?;
                    if operators.operations.iter().any(|op| !matches!(op.operator.as_str(), "q"|"Q"|"cm"|"Do"))
                        || !self.page_text_characters(doc, (number - 1) as u16)?.is_empty() {
                        return Err(err("Rebuilt copy contains non-image page content"));
                    }
                }
            }
            // Loading every page also verifies the saved page tree and native parser.
            self.pages(doc)?;
            Ok(())
        })
    }

    fn sanitize_visible(&self, doc: &mut Document, options: SanitizeOptions) -> Result<SanitizeReport> {
        let mut source = parse(&doc.bytes)?;
        // Remove requested annotation appearances before rendering. Widgets remain
        // visible, but their interactive payload is never copied to the new PDF.
        for id in source.get_pages().values() {
            let annots=source.get_dictionary(*id)?.get(b"Annots").ok().cloned();
            if let Some(annots)=annots {
                let mut keep=Vec::new();
                for value in resolve(&source,&annots)?.as_array()? {
                    let annotation=resolve(&source,value)?.as_dict()?;
                    let subtype=annotation.get(b"Subtype").and_then(Object::as_name).unwrap_or_default();
                    let remove=match subtype {
                        b"Widget"=>false,
                        b"Link"=>options.remove_links,
                        _=>options.remove_comments,
                    };
                    if !remove {keep.push(value.clone());}
                }
                source.get_dictionary_mut(*id)?.set("Annots",keep);
            }
        }
        let source=self.candidate(doc,source)?;
        let sizes=self.pages(&source)?;
        if sizes.len()>100 {return Err(err("Visible-page sanitization supports up to 100 pages per document"));}
        let pictures=sizes.iter().enumerate().map(|(index,size)| {
            operation::checkpoint("Rebuilding pages",index,sizes.len())?;
            let scale=2.0_f32.min(2048.0/size.width.max(size.height));
            self.render(&source,index as u16,scale).map(|image| Picture {
                width:image.width,height:image.height,rgba:image.rgba
            })
        });
        let rebuilt=self.pictures_to_pdf(pictures,ImagePageSize::Image)?;
        let rebuilt_sizes=self.pages(&rebuilt)?;
        let mut pdf=parse(&rebuilt.bytes)?;
        for (index,id) in pdf.get_pages().values().enumerate() {
            let size=&sizes[index];
            let previous=&rebuilt_sizes[index];
            let content=pdf.get_page_content_with_limit(*id,MAX_STREAM)?;
            let mut scaled=format!("q {} 0 0 {} 0 0 cm\n",size.width/previous.width,size.height/previous.height).into_bytes();
            scaled.extend(content);
            scaled.extend_from_slice(b"\nQ");
            pdf.change_page_content(*id,scaled)?;
            pdf.get_dictionary_mut(*id)?.set("MediaBox",vec![Object::Integer(0),Object::Integer(0),
                Object::Real(size.width),Object::Real(size.height)]);
        }
        verification::canonicalize(&mut pdf);
        let candidate=self.candidate(doc,pdf)?;
        for index in 0..sizes.len() {
            if !self.text_objects(&candidate,index as u16)?.is_empty() {
                return Err(err("Visible-page sanitization verification failed; document unchanged"));
            }
        }
        operation::checkpoint("Applying sanitized pages",sizes.len(),sizes.len())?;
        doc.accept(candidate.bytes);
        Ok(SanitizeReport {rasterized_pages:sizes.len(),..Default::default()})
    }
    pub fn sanitize(&self, doc: &mut Document, options: SanitizeOptions) -> Result<SanitizeReport> {
        guarded(|| {
            if options.flatten_visible { return self.sanitize_visible(doc,options); }
            let mut pdf = parse(&doc.bytes)?;
            pdf.prune_objects();
            let mut report = SanitizeReport::default();
            reject_payload_aliases(&pdf)?;
            // Optional-content visibility and soft masks require a visibility evaluator.
            // Reject these rather than claiming that their hidden content was removed.
            for object in pdf.objects.values() {
                if unsupported_visibility(object, 0)? {
                    return Err(err(
                        "Sanitize cannot verify optional layers, soft masks or XFA forms in this document; no changes were made",
                    ));
                }
            }
            let pages = pdf.get_pages();
            let mut walk = Walk::default();
            for (index,id) in pages.values().enumerate() {
                operation::checkpoint("Sanitizing pages",index,pages.len())?;
                let resources = page_resources(&pdf, *id)?;
                let content = pdf.get_page_content_with_limit(*id, MAX_STREAM)?;
                let cleaned =
                    clean_content(&mut pdf, &content, &resources, &mut report, &mut walk)?;
                let stream = pdf.add_object(Stream::new(Dictionary::new(), cleaned));
                pdf.get_object_mut(*id)?
                    .as_dict_mut()?
                    .set("Contents", stream);
            }
            // Remove attachment/comment/link annotations from page arrays, including indirect arrays.
            for id in pages.values() {
                let value = pdf.get_object(*id)?.as_dict()?.get(b"Annots").ok().cloned();
                if let Some(value) = value {
                    let values = resolve(&pdf, &value)?.as_array()?.clone();
                    let mut keep = vec![];
                    for mut value in values {
                        let subtype = resolve(&pdf, &value)?
                            .as_dict()?
                            .get(b"Subtype")
                            .and_then(Object::as_name)
                            .unwrap_or_default();
                        let remove = match subtype {
                            b"FileAttachment" | b"RichMedia" | b"Movie" | b"Sound" | b"3D" => {
                                report.attachments += 1;
                                true
                            }
                            b"Link" if options.remove_links => {
                                report.links += 1;
                                true
                            }
                            b"Widget" | b"Link" => false,
                            _ if options.remove_comments => {
                                report.comments += 1;
                                true
                            }
                            _ => false,
                        };
                        if !remove {
                            if resolve(&pdf, &value)?.as_dict()?.has(b"AP") {
                                return Err(err(
                                    "Sanitize cannot verify retained annotation appearances; no changes were made",
                                ));
                            }
                            if subtype == b"Link" {
                                let mut annotation = resolve(&pdf, &value)?.as_dict()?.clone();
                                if let Ok(action) = annotation.get(b"A") {
                                    let original = resolve(&pdf, action)?.as_dict()?;
                                    let safe = safe_link_action(&pdf, action)?;
                                    if safe.as_ref().is_none_or(|safe| original != safe) {
                                        report.actions += 1;
                                    }
                                    annotation.remove(b"A");
                                    if let Some(action) = safe {
                                        annotation.set("A", action);
                                    }
                                }
                                value = Object::Dictionary(annotation);
                            }
                            keep.push(value);
                        }
                    }
                    pdf.get_object_mut(*id)?.as_dict_mut()?.set("Annots", keep);
                }
            }
            pdf.prune_objects();
            for (id, object) in &pdf.objects {
                if let Object::Stream(stream) = object
                    && stream
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|s| s == b"Form")
                        && !walk.visited.contains(id)
                {
                    return Err(err(
                        "Sanitize cannot verify unused retained grouped content; no changes were made",
                    ));
                }
            }
            clean_dict(&mut pdf.trailer, &mut report, 0)?;
            for object in pdf.objects.values_mut() {
                clean_object(object, &mut report, 0)?;
            }
            verification::canonicalize(&mut pdf);
            let candidate = self.candidate(doc, pdf)?;
            operation::checkpoint("Applying sanitized pages",pages.len(),pages.len())?;
            doc.accept(candidate.bytes);
            Ok(report)
        })
    }
}
// Payloads reached through unrelated references cannot safely be discarded by pruning.
fn reject_payload_aliases(pdf: &lopdf::Document) -> Result<()> {
    fn scan(
        pdf: &lopdf::Document,
        object: &Object,
        depth: usize,
        counts: &mut std::collections::BTreeMap<lopdf::ObjectId, usize>,
        payloads: &mut Vec<Object>,
    ) -> Result<()> {
        if depth > 128 {
            return Err(err("Sanitize object nesting exceeds the supported depth"));
        }
        match object {
            Object::Reference(id) => {
                *counts.entry(*id).or_default() += 1;
            }
            Object::Array(items) => {
                for item in items {
                    scan(pdf, item, depth + 1, counts, payloads)?;
                }
            }
            Object::Dictionary(dict) => {
                if let Ok(ef) = dict.get(b"EF") {
                    payloads.push(ef.clone());
                    for (_, payload) in resolve(pdf, ef)?.as_dict()?.iter() {
                        payloads.push(payload.clone());
                    }
                }
                for key in [b"JS".as_slice(), b"Info", b"Metadata"] {
                    if let Ok(value) = dict.get(key) {
                        payloads.push(value.clone());
                    }
                }
                for (_, value) in dict.iter() {
                    scan(pdf, value, depth + 1, counts, payloads)?;
                }
            }
            Object::Stream(stream) => scan(
                pdf,
                &Object::Dictionary(stream.dict.clone()),
                depth + 1,
                counts,
                payloads,
            )?,
            _ => (),
        }
        Ok(())
    }
    let mut counts = std::collections::BTreeMap::new();
    let mut payloads = vec![];
    scan(
        pdf,
        &Object::Dictionary(pdf.trailer.clone()),
        0,
        &mut counts,
        &mut payloads,
    )?;
    for object in pdf.objects.values() {
        scan(pdf, object, 0, &mut counts, &mut payloads)?;
    }
    // Count only actual page-content edges. Shared streams used by several
    // pages are safe once every Contents entry is replaced; aliases from any
    // other dictionary retain the old bytes and must fail closed.
    fn content_edges(pdf:&lopdf::Document,value:&Object,expected:&mut std::collections::BTreeMap<lopdf::ObjectId,usize>,seen:&mut std::collections::BTreeSet<lopdf::ObjectId>,active:&mut std::collections::BTreeSet<lopdf::ObjectId>,depth:usize)->Result<()> {
        if depth>128{return Err(err("Sanitize content nesting limit exceeded"));}
        match value {
            Object::Reference(id)=>{
                *expected.entry(*id).or_default()+=1;
                if active.contains(id) {return Err(err("Sanitize content reference cycle"));}
                if seen.insert(*id) {
                    active.insert(*id);
                    content_edges(pdf,pdf.get_object(*id)?,expected,seen,active,depth+1)?;
                    active.remove(id);
                }
            }
            Object::Array(values)=>for value in values {content_edges(pdf,value,expected,seen,active,depth+1)?;},
            Object::Stream(_)=>(),
            _=>return Err(err("Unsupported page content structure")),
        }Ok(())
    }
    let mut expected=std::collections::BTreeMap::new();let mut seen=std::collections::BTreeSet::new();
    let pages=pdf.get_pages().values().copied().collect::<std::collections::BTreeSet<_>>();
    for page in pages {
        if let Ok(contents)=pdf.get_dictionary(page)?.get(b"Contents") {
            content_edges(pdf,contents,&mut expected,&mut seen,&mut Default::default(),0)?;
        }
    }
    if expected.iter().any(|(id,count)|counts.get(id).copied().unwrap_or_default()>*count) {
        return Err(err("Sanitize cannot safely remove aliased page content. Use Rebuild visible pages; no changes were made"));
    }
    for payload in &payloads {
        let mut object = payload;
        let mut seen = std::collections::BTreeSet::new();
        while let Object::Reference(id) = object {
            if counts.get(id).copied().unwrap_or_default() > 1
                || !seen.insert(*id)
                || seen.len() > 128
            {
                return Err(err(
                    "Sanitize cannot safely remove aliased content or metadata. Use Rebuild visible pages; no changes were made",
                ));
            }
            object = pdf
                .objects
                .get(id)
                .ok_or_else(|| err("Missing attachment or script payload"))?;
        }
    }
    Ok(())
}
fn safe_inline_link(action: &Dictionary) -> bool {
    if action.len() != 2 {
        return false;
    }
    match action
        .get(b"S")
        .and_then(Object::as_name)
        .unwrap_or_default()
    {
        b"URI" => action
            .get(b"URI")
            .and_then(Object::as_str)
            .is_ok_and(|uri| {
                !uri.iter().any(|byte| *byte < 32 || *byte == 127)
                    && [b"http:".as_slice(), b"https:", b"mailto:"]
                        .iter()
                        .any(|prefix| uri.starts_with(prefix))
            }),
        b"GoTo" => action.get(b"D").is_ok_and(|d| match d {
            Object::Name(_) | Object::String(_, _) => true,
            Object::Array(a) => {
                (2..=6).contains(&a.len())
                    && matches!(a[0], Object::Reference(_) | Object::Integer(_))
                    && matches!(a[1], Object::Name(_))
                    && a[2..]
                        .iter()
                        .all(|v| matches!(v, Object::Null | Object::Integer(_) | Object::Real(_)))
            }
            _ => false,
        }),
        _ => false,
    }
}
// Only ordinary web/mail links and internal destinations survive when links are retained.
fn safe_link_action(pdf: &lopdf::Document, action: &Object) -> Result<Option<Dictionary>> {
    let action = resolve(pdf, action)?.as_dict()?;
    let mut safe = Dictionary::new();
    match action
        .get(b"S")
        .and_then(Object::as_name)
        .unwrap_or_default()
    {
        b"URI" => {
            let uri = action
                .get(b"URI")
                .and_then(Object::as_str)
                .unwrap_or_default();
            if ![b"https:".as_slice(), b"http:", b"mailto:"]
                .iter()
                .any(|prefix| uri.starts_with(prefix))
            {
                return Ok(None);
            }
            safe.set("S", "URI");
            safe.set("URI", Object::string_literal(uri));
        }
        b"GoTo" => {
            safe.set("S", "GoTo");
            safe.set("D", action.get(b"D")?.clone());
        }
        _ => return Ok(None),
    }
    Ok(safe_inline_link(&safe).then_some(safe))
}
fn resolve<'a>(pdf: &'a lopdf::Document, object: &'a Object) -> Result<&'a Object> {
    Ok(pdf.dereference(object)?.1)
}
fn page_resources(pdf: &lopdf::Document, id: lopdf::ObjectId) -> Result<Dictionary> {
    let (local, parents) = pdf.get_page_resources(id)?;
    let mut resources = Dictionary::new();
    for parent in parents.into_iter().rev() {
        for (key, value) in pdf.get_object(parent)?.as_dict()?.iter() {
            resources.set(key.clone(), value.clone());
        }
    }
    if let Some(local) = local {
        for (key, value) in local.iter() {
            resources.set(key.clone(), value.clone());
        }
    }
    Ok(resources)
}
fn unsupported_visibility(object: &Object, depth: usize) -> Result<bool> {
    if depth > 128 {
        return Err(err("Sanitize object nesting exceeds the supported depth"));
    }
    let dict = match object {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    };
    if let Some(d) = dict {
        for (key, value) in d.iter() {
            if matches!(key.as_slice(), b"OCProperties" | b"OC" | b"SMask" | b"XFA")
                && !matches!(value, Object::Name(n) if n == b"None")
            {
                return Ok(true);
            }
            if unsupported_visibility(value, depth + 1)? {
                return Ok(true);
            }
        }
    } else if let Object::Array(a) = object {
        for item in a {
            if unsupported_visibility(item, depth + 1)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn clean_object(object: &mut Object, report: &mut SanitizeReport, depth: usize) -> Result<()> {
    if depth > 128 {
        return Err(err("Sanitize object nesting exceeds the supported depth"));
    }
    match object {
        Object::Dictionary(d) => clean_dict(d, report, depth + 1)?,
        Object::Stream(s) => {
            match s
                .dict
                .get(b"Type")
                .and_then(Object::as_name)
                .unwrap_or_default()
            {
                b"EmbeddedFile" => {
                    s.set_plain_content(vec![]);
                    report.attachments += 1;
                }
                b"Metadata" => {
                    s.set_plain_content(vec![]);
                    report.metadata += 1;
                }
                _ => (),
            }
            clean_dict(&mut s.dict, report, depth + 1)?;
        }
        Object::Array(a) => {
            for v in a {
                clean_object(v, report, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn clean_dict(d: &mut Dictionary, report: &mut SanitizeReport, depth: usize) -> Result<()> {
    if depth > 128 {
        return Err(err("Sanitize object nesting exceeds the supported depth"));
    }
    for key in [
        b"Info".as_slice(),
        b"Metadata",
        b"PieceInfo",
        b"LastModified",
        b"CreationDate",
        b"ModDate",
        b"ActualText",
        b"Alt",
        b"ID",
    ] {
        report.metadata += usize::from(d.remove(key).is_some());
    }
    for key in [b"EmbeddedFiles".as_slice(), b"AF", b"EF", b"Collection"] {
        report.attachments += usize::from(d.remove(key).is_some());
    }
    for key in [b"JavaScript".as_slice(), b"JS", b"OpenAction", b"AA"] {
        report.actions += usize::from(d.remove(key).is_some());
    }
    // Action dictionaries can be reached through extension keys too. Clear their payload.
    let action = d.get(b"S").and_then(Object::as_name).is_ok_and(|name| {
        matches!(
            name,
            b"JavaScript"
                | b"Launch"
                | b"URI"
                | b"GoTo"
                | b"GoToR"
                | b"GoToE"
                | b"SubmitForm"
                | b"ResetForm"
                | b"ImportData"
                | b"Named"
                | b"Rendition"
                | b"Movie"
                | b"Sound"
                | b"Hide"
                | b"SetOCGState"
                | b"Trans"
                | b"GoTo3DView"
                | b"Thread"
                | b"GoToDp"
                | b"RichMediaExecute"
        )
    });
    if action
        || d.get(b"Type")
            .and_then(Object::as_name)
            .is_ok_and(|name| name == b"Action")
    {
        *d = Dictionary::new();
        report.actions += 1;
    }
    let link = d
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_ok_and(|v| v == b"Link");
    for (key, value) in d.iter_mut() {
        if link && key == b"A" && matches!(value, Object::Dictionary(a) if safe_inline_link(a)) {
            continue;
        }
        clean_object(value, report, depth + 1)?;
    }
    Ok(())
}
#[derive(Clone, Copy, Default, PartialEq)]
struct State {
    mode: i64,
    fill: bool,
    stroke: bool,
}
fn has_text(object: &Object) -> bool {
    match object {
        Object::String(bytes, _) => !bytes.is_empty(),
        Object::Array(items) => items.iter().any(has_text),
        _ => false,
    }
}
#[derive(Default)]
struct Walk {
    active: Vec<lopdf::ObjectId>,
    operations: usize,
    visited: std::collections::BTreeSet<lopdf::ObjectId>,
}
fn clean_content(
    pdf: &mut lopdf::Document,
    bytes: &[u8],
    resources: &Dictionary,
    report: &mut SanitizeReport,
    walk: &mut Walk,
) -> Result<Vec<u8>> {
    let mut content = Content::decode(bytes)?;
    walk.operations = walk.operations.saturating_add(content.operations.len());
    if walk.operations > 1_000_000 {
        return Err(err("Sanitize content exceeds the operation limit"));
    }
    for op in &mut content.operations {
        for operand in &mut op.operands {
            clean_object(operand, report, 0)?;
        }
    }
    let mut state = State::default();
    let mut stack = vec![];
    let mut block = None;
    let mut hidden = false;
    let mut visible = false;
    let mut remove = vec![];
    for (index, op) in content.operations.iter().enumerate() {
        match op.operator.as_str() {
            "Tf" | "Tz"
                if op
                    .operands
                    .last()
                    .is_some_and(|v| v.as_float().is_ok_and(|n| n == 0.0)) =>
            {
                return Err(err(
                    "Sanitize cannot verify text with zero size or horizontal scale; no changes were made",
                ));
            }
            "Tm" | "cm" if op.operands.len() == 6 => {
                let a = op.operands[0].as_float()?;
                let b = op.operands[1].as_float()?;
                let c = op.operands[2].as_float()?;
                let d = op.operands[3].as_float()?;
                if ![a, b, c, d].iter().all(|n| n.is_finite()) || (a * d - b * c).abs() < 0.000001 {
                    return Err(err(
                        "Sanitize cannot verify degenerate text or page transforms; no changes were made",
                    ));
                }
            }
            "W" | "W*" => {
                return Err(err(
                    "Sanitize cannot verify hidden text behind clipping paths; no changes were made",
                ));
            }
            "q" => {
                if stack.len() >= 128 {
                    return Err(err("Graphics state is too deeply nested"));
                }
                stack.push(state);
            }
            "Q" => {
                state = stack
                    .pop()
                    .ok_or_else(|| err("Unbalanced graphics state"))?
            }
            "Tr" => {
                state.mode = op
                    .operands
                    .first()
                    .ok_or_else(|| err("Invalid text mode"))?
                    .as_i64()?;
                if !(0..=3).contains(&state.mode) {
                    return Err(err(
                        "Sanitize does not support clipping text; no changes were made",
                    ));
                }
            }
            "gs" => {
                let name = op
                    .operands
                    .first()
                    .ok_or_else(|| err("Invalid graphics state"))?
                    .as_name()?;
                let ext = resolve(pdf, resources.get(b"ExtGState")?)?.as_dict()?;
                let dict = resolve(pdf, ext.get(name)?)?.as_dict()?;
                if let Ok(alpha) = dict.get(b"ca") {
                    state.fill = alpha.as_float()? == 0.0;
                }
                if let Ok(alpha) = dict.get(b"CA") {
                    state.stroke = alpha.as_float()? == 0.0;
                }
            }
            "BT" => {
                if block.is_some() {
                    return Err(err("Nested text blocks"));
                }
                block = Some(index);
                hidden = false;
                visible = false;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                if !op.operands.iter().any(has_text) {
                    continue;
                }
                if block.is_none() {
                    return Err(err("Text outside a text block"));
                }
                if state.mode == 3
                    || match state.mode {
                        0 => state.fill,
                        1 => state.stroke,
                        2 => state.fill && state.stroke,
                        _ => false,
                    }
                {
                    hidden = true;
                } else {
                    visible = true;
                }
            }
            "ET" => {
                let start = block.take().ok_or_else(|| err("Unbalanced text block"))?;
                if hidden && visible {
                    return Err(err(
                        "Sanitize cannot safely separate visible and hidden text in one text block; no changes were made",
                    ));
                }
                if hidden {
                    remove.push(start..=index);
                    report.hidden_text_blocks += 1;
                }
            }
            "Do" => {
                let name = op
                    .operands
                    .first()
                    .ok_or_else(|| err("Invalid XObject"))?
                    .as_name()?;
                let objects = resolve(pdf, resources.get(b"XObject")?)?.as_dict()?;
                let reference = objects.get(name)?;
                let stream = resolve(pdf, reference)?.as_stream()?;
                if stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|s| s == b"Form")
                {
                    if state != State::default() {
                        return Err(err(
                            "Sanitize cannot verify inherited text visibility in grouped content; no changes were made",
                        ));
                    }
                    let id = reference.as_reference()?;
                    if walk.active.contains(&id) || walk.active.len() >= 32 {
                        return Err(err("Cyclic or deeply nested grouped content"));
                    }
                    let local = resolve(pdf, stream.dict.get(b"Resources").map_err(|_| err(
                        "Sanitize cannot verify grouped content with inherited resources; no changes were made"
                    ))?)?.as_dict()?.clone();
                    let data = stream.get_plain_content_with_limit(MAX_STREAM)?;
                    if data.len() > MAX_STREAM {
                        return Err(err("Grouped content exceeds the supported size"));
                    }
                    walk.active.push(id);
                    let cleaned = clean_content(pdf, &data, &local, report, walk)?;
                    walk.active.pop();
                    walk.visited.insert(id);
                    pdf.get_object_mut(id)?
                        .as_stream_mut()?
                        .set_plain_content(cleaned);
                }
            }
            _ => (),
        }
    }
    if block.is_some() || !stack.is_empty() {
        return Err(err("Unbalanced content stream"));
    }
    // Keep state-setting operations; remove only text-show operations so later blocks
    // inherit exactly the same font, matrices, spacing and rendering mode.
    for range in remove {
        for op in &mut content.operations[range] {
            match op.operator.as_str() {
                "TJ" => op.operands = vec![Object::Array(vec![])],
                "Tj" | "'" | "\"" => {
                    let operand = op
                        .operands
                        .last_mut()
                        .ok_or_else(|| err("Invalid text show operation"))?;
                    *operand = Object::string_literal("");
                }
                _ => (),
            }
        }
    }
    Ok(content.encode()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    #[test]
    fn cyclic_forms_are_rejected() {
        let mut pdf = lopdf::Document::with_version("1.7");
        let form = pdf.new_object_id();
        let resources = dictionary! {"XObject"=>dictionary!{"Self"=>form}};
        pdf.objects.insert(
            form,
            Object::Stream(Stream::new(
                dictionary! {"Subtype"=>"Form", "Resources"=>resources.clone()},
                b"/Self Do".to_vec(),
            )),
        );
        let error = clean_content(
            &mut pdf,
            b"/Self Do",
            &resources,
            &mut SanitizeReport::default(),
            &mut Walk::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Cyclic"));
    }
    #[test]
    fn excessive_object_nesting_is_rejected() {
        let mut object = Object::Null;
        for _ in 0..140 {
            object = Object::Array(vec![object]);
        }
        assert!(unsupported_visibility(&object, 0).is_err());
        assert!(clean_object(&mut object, &mut SanitizeReport::default(), 0).is_err());
    }
    #[test]
    fn hidden_quote_preserves_spacing_and_safe_links_keep_only_navigation() {
        let mut pdf = lopdf::Document::with_version("1.7");
        let mut report = SanitizeReport::default();
        let cleaned = clean_content(
            &mut pdf,
            b"BT /F 12 Tf 3 Tr 5 2 (hidden) \" ET BT 0 Tr (visible) Tj ET",
            &Dictionary::new(),
            &mut report,
            &mut Walk::default(),
        )
        .unwrap();
        let ops = Content::decode(&cleaned).unwrap().operations;
        let quote = ops.iter().find(|o| o.operator == "\"").unwrap();
        assert_eq!(quote.operands[0].as_i64().unwrap(), 5);
        assert_eq!(quote.operands[1].as_i64().unwrap(), 2);
        assert_eq!(quote.operands[2].as_str().unwrap(), b"");
        assert_eq!(report.hidden_text_blocks, 1);
        let action = Object::Dictionary(
            dictionary! {"S"=>"URI", "URI"=>Object::string_literal("https://example.invalid"), "Next"=>dictionary!{"S"=>"JavaScript", "JS"=>Object::string_literal("never")}},
        );
        let safe = safe_link_action(&pdf, &action).unwrap().unwrap();
        assert_eq!(safe.len(), 2);
        assert!(!safe.has(b"Next"));
        let unsafe_action = Object::Dictionary(
            dictionary! {"S"=>"URI", "URI"=>Object::string_literal("javascript:never")},
        );
        assert!(safe_link_action(&pdf, &unsafe_action).unwrap().is_none());
    }
    #[test]
    fn hidden_alpha_in_forms_is_removed_and_complex_visibility_rejected() {
        let mut pdf = lopdf::Document::with_version("1.7");
        let resources = dictionary! {"ExtGState"=>dictionary!{"Invisible"=>dictionary!{"ca"=>0}}};
        let form = pdf.add_object(Stream::new(
            dictionary! {"Subtype"=>"Form","Resources"=>resources.clone()},
            b"q /Invisible gs BT (hidden) Tj ET Q".to_vec(),
        ));
        let resources = dictionary! {"XObject"=>dictionary!{"Form1"=>form}};
        let mut report = SanitizeReport::default();
        clean_content(
            &mut pdf,
            b"/Form1 Do",
            &resources,
            &mut report,
            &mut Walk::default(),
        )
        .unwrap();
        assert_eq!(report.hidden_text_blocks, 1);
        let data = pdf
            .get_object(form)
            .unwrap()
            .as_stream()
            .unwrap()
            .get_plain_content_with_limit(MAX_STREAM)
            .unwrap();
        assert!(!data.windows(6).any(|s| s == b"hidden"));
        for input in [
            b"q 0 0 1 1 re W n Q".as_slice(),
            b"BT 7 Tr (hidden) Tj ET",
            b"BT (visible) Tj 3 Tr (hidden) Tj ET",
        ] {
            assert!(
                clean_content(
                    &mut pdf,
                    input,
                    &Dictionary::new(),
                    &mut report,
                    &mut Walk::default()
                )
                .is_err()
            );
        }
        assert!(
            unsupported_visibility(
                &Object::Dictionary(dictionary! {"OCProperties"=>dictionary!{}}),
                0
            )
            .unwrap()
        );
        assert!(
            unsupported_visibility(&Object::Dictionary(dictionary! {"SMask"=>dictionary!{}}), 0)
                .unwrap()
        );
    }
}
