use crate::*;
use lopdf::{
    Object, Stream,
    content::{Content, Operation},
    dictionary,
};
#[derive(Clone, Debug)]
pub struct TextStamp {
    pub template: String,
    pub font: String,
    pub size: f32,
    pub color: [u8; 3],
    pub opacity: f32,
    pub x: f32,
    pub y: f32,
}
impl Engine {
    pub fn stamp_pages(&self, doc: &mut Document, stamp: &TextStamp) -> Result<()> {
        guarded(|| {
            if stamp.template.is_empty()
                || stamp.template.len() > 4096
                || stamp.template.chars().any(char::is_control)
                || ![stamp.size, stamp.opacity, stamp.x, stamp.y]
                    .iter()
                    .all(|v| v.is_finite())
                || !(1.0..=1000.0).contains(&stamp.size)
                || !(0.0..=1.0).contains(&stamp.opacity)
            {
                return Err(err("Invalid stamp text, size, position or opacity"));
            }
            let mut pdf = parse(&doc.bytes)?;
            let pages = pdf.get_pages();
            let texts: Vec<_> = pages
                .keys()
                .map(|n| {
                    stamp
                        .template
                        .replace("{page}", &n.to_string())
                        .replace("{pages}", &pages.len().to_string())
                })
                .collect();
            let alphabet = texts.join(" ");
            if alphabet.len() > 1_000_000 {
                return Err(err("Stamp text exceeds document limits"));
            }
            let font = fonts::find(&stamp.font, &alphabet)?;
            let embedded = fonts::embed(&mut pdf, &font, &alphabet)?;
            let gs = pdf.add_object(
                dictionary! {"Type"=>"ExtGState","ca"=>stamp.opacity,"CA"=>stamp.opacity},
            );
            for ((_, page), text) in pages.into_iter().zip(texts) {
                let mut resources = content::resources(&pdf, page)?;
                let mut names = vec![];
                for (category, id, prefix) in [
                    (b"Font".as_slice(), embedded.id, "StampFont"),
                    (b"ExtGState".as_slice(), gs, "StampOpacity"),
                ] {
                    let mut entries = resources
                        .get(category)
                        .ok()
                        .and_then(|o| pdf.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .cloned()
                        .unwrap_or_default();
                    let mut name = format!("{prefix}{}", id.0);
                    while entries.has(name.as_bytes()) {
                        name.push('X');
                    }
                    entries.set(name.as_bytes(), id);
                    resources.set(category, entries);
                    names.push(name);
                }
                pdf.get_dictionary_mut(page)?.set("Resources", resources);
                let ops = Content {
                    operations: vec![
                        Operation::new("q", vec![]),
                        Operation::new("gs", vec![Object::Name(names[1].as_bytes().to_vec())]),
                        Operation::new("BT", vec![]),
                        Operation::new("Tr", vec![0.into()]),
                        Operation::new(
                            "Tf",
                            vec![
                                Object::Name(names[0].as_bytes().to_vec()),
                                stamp.size.into(),
                            ],
                        ),
                        Operation::new(
                            "rg",
                            stamp
                                .color
                                .iter()
                                .map(|c| Object::Real(f32::from(*c) / 255.0))
                                .collect(),
                        ),
                        Operation::new(
                            "Tm",
                            vec![
                                1.into(),
                                0.into(),
                                0.into(),
                                1.into(),
                                stamp.x.into(),
                                stamp.y.into(),
                            ],
                        ),
                        Operation::new(
                            "Tj",
                            vec![Object::String(
                                embedded.encode(&text)?,
                                lopdf::StringFormat::Hexadecimal,
                            )],
                        ),
                        Operation::new("ET", vec![]),
                        Operation::new("Q", vec![]),
                    ],
                };
                let mut data = b"q\n".to_vec();
                data.extend(pdf.get_page_content_with_limit(page, MAX_STREAM)?);
                data.extend(b"\nQ\n");
                data.extend(ops.encode()?);
                let stream = pdf.add_object(Stream::new(lopdf::Dictionary::new(), data));
                pdf.get_dictionary_mut(page)?.set("Contents", stream);
            }
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
