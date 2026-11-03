use crate::*;
use std::collections::HashSet;

/// Temporary drag layers. The foreground rectangle is in background raster pixels.
pub struct ObjectPreview {
    pub background: Image,
    pub foreground: Image,
    pub rect: [u32; 4],
}

impl Engine {
    /// Render a selection and the page behind it without changing document bytes.
    pub fn object_preview(
        &self,
        doc: &Document,
        items: &[PageItem],
        scale: f32,
    ) -> Result<ObjectPreview> {
        self.object_preview_layers(doc, items, scale, None)
    }

    /// Render a bounded, higher-density foreground for interactive enlargement.
    pub fn object_preview_sharp(&self, doc: &Document, items: &[PageItem], scale: f32, max_side: u32) -> Result<ObjectPreview> {
        self.object_preview_layers(doc, items, scale, Some(max_side))
    }

    fn object_preview_layers(&self, doc: &Document, items: &[PageItem], scale: f32, max_side: Option<u32>) -> Result<ObjectPreview> {
        guarded(|| {
            if max_side.is_some_and(|side| side==0 || side>16_384) {return Err(err("Invalid preview texture limit"));}
            let first = items.first().ok_or_else(|| err("Select an object first"))?;
            let selected: HashSet<_> = items.iter().map(|item| item.index).collect();
            if items.len() > 20000
                || selected.len() != items.len()
                || items
                    .iter()
                    .any(|item| item.page != first.page || item.revision != doc.revision)
            {
                return Err(err("Selection is stale or spans pages"));
            }
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let mut page = pdf.pages().get(i32::from(first.page))?;
            page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let config = render_config(&page, scale)?;
            let width=(page.width().value * scale).ceil();let height=(page.height().value * scale).ceil();
            if width * height > 2_000_000. || max_side.is_some_and(|side|width>side as f32 || height>side as f32) {
                return Err(err("Drag preview exceeds limits"));
            }
            let count = page.objects().len();
            if count > 20000 || selected.iter().any(|&index| index >= count) {
                return Err(err("Object no longer exists or page exceeds limits"));
            }
            let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for index in 0..count {
                let mut object = page.objects().get(index)?;
                if selected.contains(&index) {
                    let b = object.bounds()?;
                    for (x, y) in [(b.left(), b.bottom()), (b.right(), b.top())] {
                        let (x, y) = page.points_to_pixels(x, y, &config)?;
                        bounds[0] = bounds[0].min(x);
                        bounds[1] = bounds[1].min(y);
                        bounds[2] = bounds[2].max(x);
                        bounds[3] = bounds[3].max(y);
                    }
                    object.set_inactive()?;
                }
            }
            let bitmap = page.render_with_config(&config)?;
            let background = Image {
                width: bitmap.width() as u32,
                height: bitmap.height() as u32,
                rgba: bitmap.as_rgba_bytes(),
            };
            let x = bounds[0]
                .saturating_sub(2)
                .max(0)
                .min(background.width as i32);
            let y = bounds[1]
                .saturating_sub(2)
                .max(0)
                .min(background.height as i32);
            let right = bounds[2]
                .saturating_add(2)
                .max(0)
                .min(background.width as i32);
            let bottom = bounds[3]
                .saturating_add(2)
                .max(0)
                .min(background.height as i32);
            if right <= x || bottom <= y {
                return Err(err("Selection is outside the page"));
            }
            drop(bitmap);
            drop(page);
            drop(pdf);
            let native = forms::NativeForm::open(self.raw, &doc.bytes, first.page)?;
            let density = max_side.map_or(1,|side| ((2_000_000. / ((right-x) as f64 * (bottom-y) as f64)).sqrt().floor() as i32).min(side as i32/(right-x)).min(side as i32/(bottom-y)).clamp(1,8));
            let width = (right - x) * density;
            let height = (bottom - y) * density;
            let mut rgba = vec![0; width as usize * height as usize * 4];
            // Render the native display matrix without a form overlay. The wrapper's
            // matrix path ignores bitmap origins when form rendering is disabled.
            unsafe {
                for index in 0..count {
                    if !selected.contains(&index) {
                        let object = self.raw.FPDFPage_GetObject(native.page, index as i32);
                        if object.is_null() || self.raw.FPDFPageObj_SetIsActive(object, 0) == 0 {
                            return Err(err("Selection preview could not be prepared"));
                        }
                    }
                }
                let bitmap = self.raw.FPDFBitmap_CreateEx(
                    width,
                    height,
                    PdfBitmapFormat::BGRA as i32,
                    rgba.as_mut_ptr().cast(),
                    width * 4,
                );
                if bitmap.is_null() {
                    return Err(err("Selection preview could not be allocated"));
                }
                self.raw.FPDF_RenderPageBitmap(
                    bitmap,
                    native.page,
                    -x * density,
                    -y * density,
                    background.width as i32 * density,
                    background.height as i32 * density,
                    0,
                    0,
                );
                self.raw.FPDFBitmap_Destroy(bitmap);
            }
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            let foreground = Image {
                width: width as u32,
                height: height as u32,
                rgba,
            };
            Ok(ObjectPreview {
                background,
                foreground,
                rect: [x as u32, y as u32, (right - x) as u32, (bottom - y) as u32],
            })
        })
    }
}
