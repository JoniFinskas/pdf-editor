use crate::*;
use lopdf::{Dictionary, Object, ObjectId, dictionary};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Uri(String),
    Page(u16),
    Unsupported,
}
impl LinkTarget {
    pub fn uri(value: &str) -> Result<Self> {
        Ok(Self::Uri(normalized_uri(value)?))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct LinkSpec {
    pub page: u16,
    pub bounds: Bounds,
    pub target: LinkTarget,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PdfLink {
    pub index: usize,
    pub revision: u64,
    pub object_id: Option<ObjectId>,
    pub link: LinkSpec,
}
fn host(value: &str) -> bool {
    if value.is_empty() || !value.is_ascii() || value.contains(['@', '%', '\\']) {
        return false;
    }
    let (name, port) = if let Some(rest) = value.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return false;
        };
        if rest[..end].parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        let suffix = &rest[end + 1..];
        if suffix.is_empty() {
            return true;
        }
        let Some(port) = suffix.strip_prefix(':') else {
            return false;
        };
        ("ipv6", Some(port))
    } else {
        let (name, port) = value
            .rsplit_once(':')
            .map_or((value, None), |(a, b)| (a, Some(b)));
        if !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
            || name.trim_end_matches('.').split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
            })
        {
            return false;
        }
        (name, port)
    };
    !name.is_empty() && port.is_none_or(|port| port.parse::<u16>().is_ok_and(|value| value > 0))
}
fn normalized_uri(input: &str) -> Result<String> {
    let value = input.trim();
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_control)
        || value.contains(['\\', '<', '>', '"'])
    {
        return Err(err("Enter a web or email link without control characters"));
    }
    let (scheme, rest) = value
        .split_once(':')
        .ok_or_else(|| err("Start the link with https://, http:// or mailto:"))?;
    let scheme = scheme.to_ascii_lowercase();
    match scheme.as_str() {
        "http" | "https" => {
            let authority = rest
                .strip_prefix("//")
                .ok_or_else(|| err("Web links require // and a hostname"))?
                .split(['/', '?', '#'])
                .next()
                .unwrap_or_default();
            if !host(authority) {
                return Err(err("Use a valid web hostname without embedded credentials"));
            }
        }
        "mailto" => {
            let addresses = rest.split('?').next().unwrap_or_default();
            if addresses.is_empty()
                || addresses.split(',').any(|address| {
                    let Some((name, domain)) = address.rsplit_once('@') else {
                        return true;
                    };
                    name.is_empty()
                        || name.contains(['@', '/', ':', ' '])
                        || !host(domain)
                        || domain.contains(':')
                })
            {
                return Err(err("Enter a valid mailto: email address"));
            }
        }
        _ => return Err(err("Only http, https, mailto and page links are supported")),
    }
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let pair = bytes
                .get(i + 1..i + 3)
                .ok_or_else(|| err("Invalid percent escape in link"))?;
            let text =
                std::str::from_utf8(pair).map_err(|_| err("Invalid percent escape in link"))?;
            let decoded =
                u8::from_str_radix(text, 16).map_err(|_| err("Invalid percent escape in link"))?;
            if decoded < 32 || decoded == 127 {
                return Err(err("Links cannot contain encoded control characters"));
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut output = format!("{scheme}:");
    for byte in rest.bytes() {
        if byte == b' ' || !byte.is_ascii() {
            output.push_str(&format!("%{byte:02X}"));
        } else {
            output.push(char::from(byte));
        }
    }
    if output.len() > 4096 {
        return Err(err("Link is too long"));
    }
    Ok(output)
}
fn valid_bounds(bounds: Bounds) -> bool {
    [bounds.left, bounds.bottom, bounds.right, bounds.top]
        .iter()
        .all(|n| n.is_finite() && n.abs() <= 100000.)
        && bounds.left < bounds.right
        && bounds.bottom < bounds.top
}
fn normalized_spec(spec: &LinkSpec, count: usize) -> Result<LinkSpec> {
    if usize::from(spec.page) >= count || !valid_bounds(spec.bounds) {
        return Err(err("Choose a valid link page and area"));
    }
    let target = match &spec.target {
        LinkTarget::Uri(uri) => LinkTarget::uri(uri)?,
        LinkTarget::Page(page) if usize::from(*page) < count => LinkTarget::Page(*page),
        _ => return Err(err("Choose a web, email or document-page destination")),
    };
    Ok(LinkSpec {
        target,
        ..spec.clone()
    })
}
fn read_link(
    pdf: &lopdf::Document,
    revision: u64,
    page: u16,
    index: usize,
    object: &Object,
    pages: &BTreeMap<ObjectId, u16>,
    names: &BTreeMap<Vec<u8>, Object>,
) -> Result<Option<PdfLink>> {
    let Ok((_, object_dict)) = pdf.dereference(object) else {
        return Ok(None);
    };
    let Ok(d) = object_dict.as_dict() else {
        return Ok(None);
    };
    if d.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Link") {
        return Ok(None);
    }
    let rect = pdf.dereference(d.get(b"Rect")?)?.1.as_array()?;
    if rect.len() != 4 {
        return Err(err("Invalid link area"));
    }
    let values = rect
        .iter()
        .map(Object::as_float)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let bounds = Bounds {
        left: values[0].min(values[2]),
        bottom: values[1].min(values[3]),
        right: values[0].max(values[2]),
        top: values[1].max(values[3]),
    };
    if !valid_bounds(bounds) {
        return Err(err("Invalid link area"));
    }
    let destination = |value: &Object| {
        crate::bookmarks::destination(pdf, value, pages, names, 0)
            .map(LinkTarget::Page)
            .unwrap_or(LinkTarget::Unsupported)
    };
    let target = if let Ok(action) = d
        .get(b"A")
        .and_then(|a| pdf.dereference(a).map(|(_, a)| a))
        .and_then(Object::as_dict)
    {
        match action
            .get(b"S")
            .and_then(Object::as_name)
            .unwrap_or_default()
        {
            b"URI" => action
                .get(b"URI")
                .ok()
                .and_then(|value| {
                    let value = pdf.dereference(value).ok()?.1;
                    let bytes = value.as_str().ok()?;
                    if bytes.len() > 8192 {
                        return None;
                    }
                    let text = std::str::from_utf8(bytes)
                        .map(str::to_owned)
                        .or_else(|_| lopdf::decode_text_string(value))
                        .ok()?;
                    LinkTarget::uri(&text).ok()
                })
                .unwrap_or(LinkTarget::Unsupported),
            b"GoTo" => action
                .get(b"D")
                .map(destination)
                .unwrap_or(LinkTarget::Unsupported),
            _ => LinkTarget::Unsupported,
        }
    } else {
        d.get(b"Dest")
            .map(destination)
            .unwrap_or(LinkTarget::Unsupported)
    };
    Ok(Some(PdfLink {
        index,
        revision,
        object_id: object.as_reference().ok(),
        link: LinkSpec {
            page,
            bounds,
            target,
        },
    }))
}
fn primary_action_only(pdf: &lopdf::Document, old: &Dictionary, target: &LinkTarget) -> bool {
    if old.has(b"AA") || old.has(b"PA") {
        return false;
    }
    if let Ok(action) = old.get(b"A") {
        let Ok((_, action)) = pdf.dereference(action) else {
            return false;
        };
        let Ok(action) = action.as_dict() else {
            return false;
        };
        match action
            .get(b"S")
            .and_then(Object::as_name)
            .unwrap_or_default()
        {
            b"URI" => {
                action
                    .iter()
                    .all(|(key, _)| matches!(key.as_slice(), b"S" | b"URI"))
                    && matches!(target, LinkTarget::Uri(uri) if action.get(b"URI").and_then(Object::as_str).ok()==Some(uri.as_bytes()))
            }
            b"GoTo" => action
                .iter()
                .all(|(key, _)| matches!(key.as_slice(), b"S" | b"D")),
            _ => false,
        }
    } else {
        old.has(b"Dest")
    }
}
impl Engine {
    pub fn links(&self, doc: &Document) -> Result<Vec<PdfLink>> {
        guarded(|| {
            let pdf = parse(&doc.bytes)?;
            let pages = pdf.get_pages();
            let ids = pages
                .iter()
                .map(|(index, id)| (*id, (*index - 1) as u16))
                .collect();
            let names = crate::bookmarks::destinations(&pdf).unwrap_or_default();
            let mut output = vec![];
            let mut visited = 0;
            for (number, page) in pages {
                let entries = crate::annotations::annots(&pdf, page)?;
                visited += entries.len();
                if visited > 20000 {
                    return Err(err("Too many annotation entries"));
                }
                for (index, object) in entries.iter().enumerate() {
                    if let Some(link) = read_link(
                        &pdf,
                        doc.revision,
                        (number - 1) as u16,
                        index,
                        object,
                        &ids,
                        &names,
                    )? {
                        output.push(link);
                    }
                    if output.len() > 10000 {
                        return Err(err("Too many links"));
                    }
                }
            }
            Ok(output)
        })
    }
    pub fn add_link(&self, doc: &mut Document, link: &LinkSpec) -> Result<()> {
        self.change_link(doc, None, Some(link))
    }
    pub fn update_link(
        &self,
        doc: &mut Document,
        selected: &PdfLink,
        link: &LinkSpec,
    ) -> Result<()> {
        self.change_link(doc, Some(selected), Some(link))
    }
    pub fn delete_link(&self, doc: &mut Document, selected: &PdfLink) -> Result<()> {
        self.change_link(doc, Some(selected), None)
    }
    fn change_link(
        &self,
        doc: &mut Document,
        selected: Option<&PdfLink>,
        desired: Option<&LinkSpec>,
    ) -> Result<()> {
        guarded(|| {
            if selected.is_some_and(|s| s.revision != doc.revision) {
                return Err(err("Link selection is stale; refresh links"));
            }
            let mut pdf = parse(&doc.bytes)?;
            let pages = pdf.get_pages();
            let desired = desired
                .map(|spec| normalized_spec(spec, pages.len()))
                .transpose()?;
            let page = selected
                .map(|s| s.link.page)
                .or_else(|| desired.as_ref().map(|s| s.page))
                .ok_or_else(|| err("No link selected"))?;
            if desired.as_ref().is_some_and(|s| s.page != page) {
                return Err(err("Keep an edited link on its page"));
            }
            let id = *pages
                .get(&(u32::from(page) + 1))
                .ok_or_else(|| err("Invalid link page"))?;
            let mut entries = crate::annotations::annots(&pdf, id)?;
            let verify_index = selected.map_or(entries.len(), |s| s.index);
            let ids = pages
                .iter()
                .map(|(index, id)| (*id, (*index - 1) as u16))
                .collect();
            let names = crate::bookmarks::destinations(&pdf).unwrap_or_default();
            let old = if let Some(selected) = selected {
                let object = entries
                    .get(selected.index)
                    .ok_or_else(|| err("Link no longer exists"))?;
                let current = read_link(
                    &pdf,
                    doc.revision,
                    page,
                    selected.index,
                    object,
                    &ids,
                    &names,
                )?
                .ok_or_else(|| err("Link no longer exists"))?;
                if current != *selected {
                    return Err(err("Link selection no longer matches the document"));
                }
                Some(pdf.dereference(object)?.1.as_dict()?.clone())
            } else {
                None
            };
            if let Some(spec) = &desired {
                if selected.is_some_and(|s| s.link == *spec)
                    && old
                        .as_ref()
                        .is_some_and(|old| primary_action_only(&pdf, old, &spec.target))
                {
                    return Ok(());
                }
                let b = spec.bounds;
                let mut updated = dictionary! {"Type"=>"Annot","Subtype"=>"Link","P"=>id,"Rect"=>vec![Object::Real(b.left),b.bottom.into(),b.right.into(),b.top.into()],"Border"=>vec![0.into(),0.into(),0.into()],"F"=>4};
                if let Some(old) = &old {
                    for key in [
                        b"Border".as_slice(),
                        b"BS",
                        b"C",
                        b"H",
                        b"F",
                        b"AP",
                        b"AS",
                        b"QuadPoints",
                        b"CA",
                        b"OC",
                        b"Contents",
                        b"T",
                        b"NM",
                        b"M",
                    ] {
                        if key == b"QuadPoints"
                            && selected.is_some_and(|s| s.link.bounds != spec.bounds)
                        {
                            continue;
                        }
                        if let Ok(value) = old.get(key) {
                            updated.set(key, value.clone());
                        }
                    }
                }
                match &spec.target {
                    LinkTarget::Uri(uri) => updated.set(
                        "A",
                        dictionary! {"S"=>"URI","URI"=>Object::string_literal(uri.as_bytes())},
                    ),
                    LinkTarget::Page(page) => {
                        let preserved = if selected.is_some_and(|s| s.link.target == spec.target) {
                            old.as_ref().and_then(|old| {
                                if let Ok(action) = old
                                    .get(b"A")
                                    .and_then(|a| pdf.dereference(a).map(|(_, a)| a))
                                    .and_then(Object::as_dict)
                                {
                                    if action.get(b"S").and_then(Object::as_name).ok()
                                        == Some(b"GoTo")
                                    {
                                        action.get(b"D").ok().cloned()
                                    } else {
                                        None
                                    }
                                } else {
                                    old.get(b"Dest").ok().cloned()
                                }
                            })
                        } else {
                            None
                        };
                        updated.set(
                            "Dest",
                            preserved.unwrap_or_else(|| {
                                Object::Array(vec![
                                    Object::Reference(pages[&(u32::from(*page) + 1)]),
                                    Object::Name(b"XYZ".to_vec()),
                                    Object::Null,
                                    Object::Null,
                                    Object::Null,
                                ])
                            }),
                        );
                    }
                    LinkTarget::Unsupported => return Err(err("Unsupported link destination")),
                }
                let object = Object::Reference(pdf.add_object(updated));
                if let Some(selected) = selected {
                    entries[selected.index] = object;
                } else {
                    entries.push(object);
                }
            } else if let Some(selected) = selected {
                entries.remove(selected.index);
            }
            pdf.get_dictionary_mut(id)?.set("Annots", entries);
            let candidate = self.candidate(doc, pdf)?;
            if let Some(spec) = desired
                && !self
                    .links(&candidate)?
                    .iter()
                    .any(|item| item.index == verify_index && item.link == spec)
            {
                return Err(err("Link failed verification; document unchanged"));
            }
            operation::checkpoint("Applying link", 1, 1)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
