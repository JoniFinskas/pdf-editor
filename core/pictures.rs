use crate::*;
use lopdf::{Dictionary, Object, Stream, content::Content};
#[derive(Clone, Debug)]
pub struct PlacedPicture {
    pub page: u16,
    pub name: String,
    pub revision: u64,
    pub placement: Placement,
}
impl Engine {
    pub fn placed_pictures(&self, doc: &Document) -> Result<Vec<PlacedPicture>> {
        guarded(|| {
            let pdf = parse(&doc.bytes)?;
            let mut pictures = vec![];
            for (page, id) in pdf.get_pages() {
                let ops =
                    Content::decode(&pdf.get_page_content_with_limit(id, MAX_STREAM)?)?.operations;
                for group in ops.windows(3) {
                    if group[0].operator != "BMC"
                        || group[1].operator != "cm"
                        || group[2].operator != "Do"
                    {
                        continue;
                    }
                    let Some(name) = group[0].operands.first().and_then(|v| v.as_name().ok())
                    else {
                        continue;
                    };
                    if !name.starts_with(b"EditorImage")
                        || group[2].operands.first().and_then(|v| v.as_name().ok()) != Some(name)
                    {
                        continue;
                    }
                    let matrix = group[1]
                        .operands
                        .iter()
                        .map(Object::as_float)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    if matrix.len() != 6 || matrix[1] != 0.0 || matrix[2] != 0.0 {
                        continue;
                    }
                    pictures.push(PlacedPicture {
                        page: (page - 1) as u16,
                        name: String::from_utf8_lossy(name).into_owned(),
                        revision: doc.revision,
                        placement: Placement {
                            page: (page - 1) as u16,
                            left: matrix[4],
                            bottom: matrix[5],
                            width: matrix[0],
                            height: matrix[3],
                        },
                    });
                }
            }
            Ok(pictures)
        })
    }
    pub fn move_picture(
        &self,
        doc: &mut Document,
        selected: &PlacedPicture,
        at: Placement,
    ) -> Result<()> {
        guarded(|| {
            if selected.revision != doc.revision {
                return Err(err("Image selection is stale; refresh images"));
            }
            if at.page != selected.page
                || ![at.left, at.bottom, at.width, at.height]
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 100000.0)
                || at.width <= 0.0
                || at.height <= 0.0
            {
                return Err(err("Invalid image position or size"));
            }
            let mut pdf = parse(&doc.bytes)?;
            let page = *pdf
                .get_pages()
                .get(&(u32::from(at.page) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let mut content = Content::decode(&pdf.get_page_content_with_limit(page, MAX_STREAM)?)?;
            let mut found = 0;
            for index in 0..content.operations.len().saturating_sub(2) {
                let ops = &mut content.operations;
                if ops[index].operator == "BMC"
                    && ops[index].operands.first().and_then(|v| v.as_name().ok())
                        == Some(selected.name.as_bytes())
                    && ops[index + 1].operator == "cm"
                    && ops[index + 2].operator == "Do"
                {
                    ops[index + 1].operands = vec![
                        at.width.into(),
                        0.into(),
                        0.into(),
                        at.height.into(),
                        at.left.into(),
                        at.bottom.into(),
                    ];
                    found += 1;
                }
            }
            if found != 1 {
                return Err(err("Image marker is missing or ambiguous"));
            }
            let stream = pdf.add_object(Stream::new(Dictionary::new(), content.encode()?));
            pdf.get_dictionary_mut(page)?.set("Contents", stream);
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
