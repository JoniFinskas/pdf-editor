use crate::*;
use lopdf::{Dictionary, Object, Stream, dictionary};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkupKind {
    Highlight,
    Underline,
    Strikethrough,
    Note,
}
impl MarkupKind {
    fn subtype(self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::Underline => "Underline",
            Self::Strikethrough => "StrikeOut",
            Self::Note => "Text",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Markup {
    pub page: u16,
    pub kind: MarkupKind,
    pub bounds: Bounds,
    pub color: [u8; 3],
    pub opacity: f32,
    pub text: String,
}
#[derive(Clone, Debug)]
pub struct Annotation {
    pub index: usize,
    pub revision: u64,
    pub markup: Markup,
}
fn annots(pdf: &lopdf::Document, page: lopdf::ObjectId) -> Result<Vec<Object>> {
    Ok(match pdf.get_dictionary(page)?.get(b"Annots") {
        Ok(value) => pdf.dereference(value)?.1.as_array()?.clone(),
        Err(_) => vec![],
    })
}
fn number(object: &Object) -> Result<f32> {
    Ok(object.as_float()?)
}
impl Engine {
    pub fn annotations(&self, doc: &Document) -> Result<Vec<Annotation>> {
        guarded(|| {
            let pdf = parse(&doc.bytes)?;
            let mut output = vec![];
            for (page_no, page) in pdf.get_pages() {
                for (index, object) in annots(&pdf, page)?.iter().enumerate() {
                    let d = pdf.dereference(object)?.1.as_dict()?;
                    let kind = match d.get(b"Subtype")?.as_name()? {
                        b"Highlight" => MarkupKind::Highlight,
                        b"Underline" => MarkupKind::Underline,
                        b"StrikeOut" => MarkupKind::Strikethrough,
                        b"Text" => MarkupKind::Note,
                        _ => continue,
                    };
                    let rect = d.get(b"Rect")?.as_array()?;
                    if rect.len() != 4 {
                        return Err(err("Invalid annotation bounds"));
                    }
                    let mut color = [255, 210, 40];
                    if let Ok(c) = d.get(b"C").and_then(Object::as_array)
                        && c.len() == 3
                    {
                        for i in 0..3 {
                            color[i] = (number(&c[i])?.clamp(0.0, 1.0) * 255.0) as u8;
                        }
                    }
                    let text = d
                        .get(b"Contents")
                        .ok()
                        .and_then(|o| o.as_str().ok())
                        .map(decode_text)
                        .unwrap_or_default();
                    let opacity = d
                        .get(b"CA")
                        .ok()
                        .and_then(|o| o.as_float().ok())
                        .unwrap_or(1.0);
                    output.push(Annotation {
                        index,
                        revision: doc.revision,
                        markup: Markup {
                            page: (page_no - 1) as u16,
                            kind,
                            bounds: Bounds {
                                left: number(&rect[0])?,
                                bottom: number(&rect[1])?,
                                right: number(&rect[2])?,
                                top: number(&rect[3])?,
                            },
                            color,
                            opacity,
                            text,
                        },
                    });
                    if output.len() > 10000 {
                        return Err(err("Too many annotations"));
                    }
                }
            }
            Ok(output)
        })
    }
    pub fn add_annotation(&self, doc: &mut Document, markup: &Markup) -> Result<()> {
        self.change_annotation(doc, None, Some(markup))
    }
    pub fn update_annotation(
        &self,
        doc: &mut Document,
        annotation: &Annotation,
        markup: &Markup,
    ) -> Result<()> {
        self.change_annotation(doc, Some(annotation), Some(markup))
    }
    pub fn delete_annotation(&self, doc: &mut Document, annotation: &Annotation) -> Result<()> {
        self.change_annotation(doc, Some(annotation), None)
    }
    fn change_annotation(
        &self,
        doc: &mut Document,
        selected: Option<&Annotation>,
        markup: Option<&Markup>,
    ) -> Result<()> {
        guarded(|| {
            if selected.is_some_and(|a| a.revision != doc.revision) {
                return Err(err("Annotation selection is stale"));
            }
            let page_no = selected
                .map(|a| a.markup.page)
                .or_else(|| markup.map(|m| m.page))
                .ok_or_else(|| err("No annotation"))?;
            if markup.is_some_and(|m| m.page != page_no) {
                return Err(err("Keep an edited annotation on its page"));
            }
            let mut pdf = parse(&doc.bytes)?;
            let page = *pdf
                .get_pages()
                .get(&(u32::from(page_no) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let mut objects = annots(&pdf, page)?;
            if let Some(selected) = selected
                && selected.index >= objects.len()
            {
                return Err(err("Annotation no longer exists"));
            }
            if let Some(markup) = markup {
                let dictionary = make_annotation(&mut pdf, markup, page)?;
                if let Some(selected) = selected {
                    let old = pdf.dereference(&objects[selected.index])?.1.as_dict()?;
                    let mut updated = old.clone();
                    for (k, v) in dictionary.iter() {
                        updated.set(k.clone(), v.clone());
                    }
                    if let Object::Reference(id) = objects[selected.index] {
                        pdf.objects.insert(id, Object::Dictionary(updated));
                    } else {
                        objects[selected.index] = Object::Reference(pdf.add_object(updated));
                    }
                } else {
                    objects.push(Object::Reference(pdf.add_object(dictionary)));
                }
            } else if let Some(selected) = selected {
                objects.remove(selected.index);
            }
            pdf.get_dictionary_mut(page)?.set("Annots", objects);
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        String::from_utf16_lossy(
            &bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
fn make_annotation(
    pdf: &mut lopdf::Document,
    m: &Markup,
    page: lopdf::ObjectId,
) -> Result<Dictionary> {
    let b = m.bounds;
    let width = b.right - b.left;
    let height = b.top - b.bottom;
    if ![b.left, b.bottom, b.right, b.top, m.opacity]
        .iter()
        .all(|v| v.is_finite())
        || width <= 0.0
        || height <= 0.0
        || width > 100000.0
        || height > 100000.0
        || !(0.0..=1.0).contains(&m.opacity)
        || m.text.len() > 16384
    {
        return Err(err("Invalid annotation bounds, opacity or text"));
    }
    let color: Vec<Object> = m
        .color
        .iter()
        .map(|c| Object::Real(f32::from(*c) / 255.0))
        .collect();
    let mut text = vec![0xfe, 0xff];
    for ch in m.text.encode_utf16() {
        text.extend(ch.to_be_bytes());
    }
    let mut d = dictionary! {"Type"=>"Annot","Subtype"=>m.kind.subtype(),"P"=>page,"Rect"=>vec![b.left.into(),b.bottom.into(),b.right.into(),b.top.into()],"C"=>color,"CA"=>m.opacity,"F"=>4,"Contents"=>Object::String(text,lopdf::StringFormat::Hexadecimal)};
    if m.kind != MarkupKind::Note {
        d.set(
            "QuadPoints",
            vec![
                b.left.into(),
                b.top.into(),
                b.right.into(),
                b.top.into(),
                b.left.into(),
                b.bottom.into(),
                b.right.into(),
                b.bottom.into(),
            ],
        );
    } else {
        d.set("Name", "Comment");
        d.set("Open", false);
    }
    let [r, g, blue] = m.color.map(|c| f32::from(c) / 255.0);
    let path = match m.kind {
        MarkupKind::Highlight => format!("0 0 {width} {height} re f"),
        MarkupKind::Underline => format!("1 w 0 1 m {width} 1 l S"),
        MarkupKind::Strikethrough => {
            format!("1 w 0 {} m {width} {} l S", height / 2.0, height / 2.0)
        }
        MarkupKind::Note => {
            format!("0 0 {width} {height} re f 0 0 0 RG 1 w 0 0 {width} {height} re S")
        }
    };
    let resources = dictionary! {"ExtGState"=>dictionary!{"GS"=>dictionary!{"Type"=>"ExtGState","ca"=>m.opacity,"CA"=>m.opacity,"BM"=>"Multiply"}}};
    let stream=pdf.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),width.into(),height.into()],"Resources"=>resources},format!("q /GS gs {r} {g} {blue} rg {r} {g} {blue} RG {path} Q").into_bytes()));
    d.set("AP", dictionary! {"N"=>stream});
    Ok(d)
}
