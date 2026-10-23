use crate::*;
use lopdf::{Object, Stream, dictionary};
impl Engine {
    /// Rebuild a PDF from sanitized page rasters, retaining no source object graph.
    pub fn redact(&self, doc: &mut Document, page: u16, bounds: Bounds) -> Result<()> {
        guarded(|| {
            let pages = self.pages(doc)?;
            if page as usize >= pages.len()
                || ![bounds.left, bounds.bottom, bounds.right, bounds.top]
                    .iter()
                    .all(|v| v.is_finite())
                || bounds.right <= bounds.left
                || bounds.top <= bounds.bottom
            {
                return Err(err("Invalid redaction rectangle"));
            }
            let mut pdf = lopdf::Document::with_version("1.7");
            let root = pdf.new_object_id();
            let mut kids = vec![];
            let mut total = 0;
            for (index, info) in pages.iter().enumerate() {
                let scale =
                    (200.0f32 / 72.0).min((12_000_000.0 / (info.width * info.height)).sqrt());
                let mut image = self.render(doc, index as u16, scale)?;
                if index == page as usize {
                    let points = [
                        (bounds.left, bounds.bottom),
                        (bounds.right, bounds.top),
                        (bounds.left, bounds.top),
                        (bounds.right, bounds.bottom),
                    ]
                    .into_iter()
                    .map(|(x, y)| self.page_to_pixel(doc, page, scale, x, y))
                    .collect::<Result<Vec<_>>>()?;
                    let left = points
                        .iter()
                        .map(|p| p.0)
                        .min()
                        .unwrap_or(0)
                        .saturating_sub(1)
                        .clamp(0, image.width as i32);
                    let right = points
                        .iter()
                        .map(|p| p.0)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(1)
                        .clamp(0, image.width as i32);
                    let top = points
                        .iter()
                        .map(|p| p.1)
                        .min()
                        .unwrap_or(0)
                        .saturating_sub(1)
                        .clamp(0, image.height as i32);
                    let bottom = points
                        .iter()
                        .map(|p| p.1)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(1)
                        .clamp(0, image.height as i32);
                    if left >= right || top >= bottom {
                        return Err(err("Redaction rectangle does not intersect the page"));
                    }
                    for y in top..bottom {
                        for x in left..right {
                            let at = (y as usize * image.width as usize + x as usize) * 4;
                            image.rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
                        }
                    }
                }
                let rgb = image
                    .rgba
                    .chunks_exact(4)
                    .flat_map(|p| p[..3].iter().copied())
                    .collect();
                let mut stream = Stream::new(
                    dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>image.width,"Height"=>image.height,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8},
                    rgb,
                );
                stream.compress()?;
                total += stream.content.len();
                if total > MAX_FILE - 1024 * 1024 {
                    return Err(err("Redacted document exceeds the supported output size"));
                }
                let image_id = pdf.add_object(stream);
                let content = pdf.add_object(Stream::new(
                    lopdf::Dictionary::new(),
                    format!(
                        "q {} 0 0 {} 0 0 cm /SanitizedPage Do Q",
                        info.width, info.height
                    )
                    .into_bytes(),
                ));
                let page_id=pdf.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),info.width.into(),info.height.into()],"Resources"=>dictionary!{"XObject"=>dictionary!{"SanitizedPage"=>image_id}},"Contents"=>content});
                kids.push(Object::Reference(page_id));
            }
            pdf.objects.insert(
                root,
                Object::Dictionary(
                    dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>pages.len() as i64},
                ),
            );
            let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
            pdf.trailer.set("Root", catalog);
            let candidate = self.candidate(doc, pdf)?;
            if !self.text_objects(&candidate, page)?.is_empty() {
                return Err(err("Redaction verification failed"));
            }
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
