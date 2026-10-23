use crate::*;
use lopdf::Object;
impl Engine {
    pub fn crop_page(&self, doc: &mut Document, page: u16, bounds: Bounds) -> Result<()> {
        guarded(|| {
            if ![bounds.left, bounds.bottom, bounds.right, bounds.top]
                .iter()
                .all(|v| v.is_finite())
                || bounds.right - bounds.left < 1.0
                || bounds.top - bounds.bottom < 1.0
            {
                return Err(err(
                    "Crop must have a finite width and height of at least one point",
                ));
            }
            let mut pdf = parse(&doc.bytes)?;
            let id = *pdf
                .get_pages()
                .get(&(u32::from(page) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let media = pages::inherited(&pdf, id, b"MediaBox")?
                .ok_or_else(|| err("Missing page media box"))?;
            let rect = pdf
                .dereference(&media)?
                .1
                .as_array()?
                .iter()
                .map(Object::as_float)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if rect.len() != 4
                || bounds.left < rect[0]
                || bounds.bottom < rect[1]
                || bounds.right > rect[2]
                || bounds.top > rect[3]
            {
                return Err(err("Crop must stay inside the page media box"));
            }
            pdf.get_dictionary_mut(id)?.set(
                "CropBox",
                vec![
                    Object::Real(bounds.left),
                    bounds.bottom.into(),
                    bounds.right.into(),
                    bounds.top.into(),
                ],
            );
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
