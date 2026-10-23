use crate::*;
use lopdf::{
    Dictionary, Object, Stream,
    content::{Content, Operation},
};

#[derive(Clone, Debug)]
pub struct NewText {
    pub page: u16,
    /// Baseline position in PDF points.
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub font: String,
    pub size: f32,
    pub color: [u8; 3],
}

impl Engine {
    pub fn thumbnail(&self, doc: &Document, page: u16, edge: u16) -> Result<Image> {
        if !(16..=512).contains(&edge) {
            return Err(err("Thumbnail edge must be 16 to 512 pixels"));
        }
        let pages = self.pages(doc)?;
        let info = pages
            .get(page as usize)
            .ok_or_else(|| err("Invalid page"))?;
        self.render(doc, page, edge as f32 / info.width.max(info.height))
    }

    pub fn style_text(
        &self,
        doc: &mut Document,
        selected: &TextObject,
        size: f32,
        color: [u8; 3],
    ) -> Result<()> {
        guarded(|| {
            if !size.is_finite() || !(1.0..=1000.0).contains(&size) {
                return Err(err("Font size must be from 1 to 1000 points"));
            }
            if selected.revision != doc.revision {
                return Err(err("Selection is stale; select the object again"));
            }
            let objects = self.text_objects(doc, selected.page)?;
            let ordinal = objects
                .iter()
                .position(|o| o.index == selected.index)
                .ok_or_else(|| err("Text object no longer exists"))?;
            let actual = &objects[ordinal];
            let (bytes, _) = content::edit_styled(
                self,
                doc,
                actual,
                &[(ordinal, actual.text.clone())],
                Some((size, color)),
            )?;
            let candidate = self.candidate(doc, parse(&bytes)?)?;
            let after = self.text_objects(&candidate, selected.page)?;
            let changed = after
                .get(ordinal)
                .ok_or_else(|| err("Styled text disappeared"))?;
            if changed.text != actual.text
                || (changed.size - size).abs() > 0.01
                || changed.color[..3] != color
            {
                return Err(err("Text style failed round-trip verification"));
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }

    pub fn delete_text(&self, doc: &mut Document, selected: &TextObject) -> Result<EditResult> {
        self.edit(doc, selected, "")
    }

    pub fn system_fonts(&self) -> Result<Vec<String>> {
        guarded(fonts::system_names)
    }

    pub fn add_text(&self, doc: &mut Document, text: &NewText) -> Result<()> {
        guarded(|| {
            if !text.x.is_finite()
                || !text.y.is_finite()
                || !text.size.is_finite()
                || !(1.0..=1000.0).contains(&text.size)
                || text.text.is_empty()
                || text.text.chars().count() > 16384
                || text.text.chars().any(char::is_control)
            {
                return Err(err(
                    "Text requires printable content, a position and a size from 1 to 1000 points",
                ));
            }
            let before = self.text_objects(doc, text.page)?;
            let font = fonts::find(&text.font, &text.text)?;
            if !font.exact {
                return Err(err(
                    "Selected font cannot embed every requested character; choose another font",
                ));
            }
            let mut pdf = parse(&doc.bytes)?;
            let page = *pdf
                .get_pages()
                .get(&(u32::from(text.page) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let embedded = fonts::embed(&mut pdf, &font, &text.text)?;
            let mut resources = content::resources(&pdf, page)?;
            let mut font_dict = resources
                .get(b"Font")
                .ok()
                .and_then(|o| pdf.dereference(o).ok())
                .and_then(|(_, o)| o.as_dict().ok())
                .cloned()
                .unwrap_or_default();
            let mut key = format!("EditorFont{}", embedded.id.0);
            while font_dict.has(key.as_bytes()) {
                key.push('X');
            }
            font_dict.set(key.as_bytes(), embedded.id);
            resources.set("Font", font_dict);
            pdf.get_object_mut(page)?
                .as_dict_mut()?
                .set("Resources", resources);
            let mut data = b"q\n".to_vec();
            data.extend(pdf.get_page_content_with_limit(page, MAX_STREAM)?);
            data.extend(b"\nQ\n");
            let ops = Content {
                operations: vec![
                    Operation::new("q", vec![]),
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec![Object::Name(key.into_bytes()), text.size.into()]),
                    Operation::new(
                        "rg",
                        text.color
                            .iter()
                            .map(|c| Object::Real(*c as f32 / 255.0))
                            .collect(),
                    ),
                    Operation::new(
                        "Tm",
                        vec![
                            1.into(),
                            0.into(),
                            0.into(),
                            1.into(),
                            text.x.into(),
                            text.y.into(),
                        ],
                    ),
                    Operation::new(
                        "Tj",
                        vec![Object::String(
                            embedded.encode(&text.text)?,
                            lopdf::StringFormat::Hexadecimal,
                        )],
                    ),
                    Operation::new("ET", vec![]),
                    Operation::new("Q", vec![]),
                ],
            };
            data.extend(ops.encode()?);
            let stream = pdf.add_object(Stream::new(Dictionary::new(), data));
            pdf.get_object_mut(page)?
                .as_dict_mut()?
                .set("Contents", stream);
            let candidate = self.candidate(doc, pdf)?;
            let after = self.text_objects(&candidate, text.page)?;
            if after.len() != before.len() + 1 || after.last().is_none_or(|o| o.text != text.text) {
                return Err(err("New text failed round-trip verification"));
            }
            for (old, new) in before.iter().zip(&after) {
                if old.text != new.text
                    || old.font != new.font
                    || old.size != new.size
                    || old.color != new.color
                    || old.matrix != new.matrix
                {
                    return Err(err(
                        "Existing text changed during insertion; document unchanged",
                    ));
                }
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }

    pub(crate) fn candidate(&self, doc: &Document, mut pdf: lopdf::Document) -> Result<Document> {
        pdf.prune_objects();
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes)?;
        parse(&bytes)?;
        let candidate = Document {
            bytes,
            original: doc.original.clone(),
            undo: vec![],
            redo: vec![],
            revision: doc.revision,
            protected: doc.protected,
            substitution: doc.substitution,
        };
        self.pages(&candidate)?;
        Ok(candidate)
    }
}
impl Document {
    pub(crate) fn accept(&mut self, bytes: Vec<u8>) {
        self.undo.push(std::mem::replace(&mut self.bytes, bytes));
        self.redo.clear();
        while self.undo.iter().map(Vec::len).sum::<usize>() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.revision = self.revision.wrapping_add(1);
    }
}
