use crate::*;

impl Engine {
    /// Render a bounded rectangle in full-page raster coordinates, including forms.
    /// Only the tile bitmap is allocated; full-page dimensions can exceed normal renders.
    pub fn render_tile(
        &self,
        doc: &Document,
        page: u16,
        scale: f32,
        rect: [u32; 4],
    ) -> Result<Image> {
        self.render_tiles(doc, page, scale, &[rect])
            .map(|mut images| images.remove(0))
    }
    /// Render up to 16 tiles while opening the document and page only once.
    pub fn render_tiles(
        &self,
        doc: &Document,
        page: u16,
        scale: f32,
        rects: &[[u32; 4]],
    ) -> Result<Vec<Image>> {
        guarded(|| {
            let pixels = rects.iter().try_fold(0u64, |total, rect| {
                total.checked_add(u64::from(rect[2]) * u64::from(rect[3]))
            });
            if rects.is_empty()
                || rects.len() > 16
                || pixels.is_none_or(|pixels| pixels > 4_194_304)
            {
                return Err(err("Invalid render tile batch"));
            }
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let native = pdf.pages().get(i32::from(page))?;
            rects
                .iter()
                .map(|&rect| render(&native, scale, rect))
                .collect()
        })
    }
}
fn render(native: &PdfPage<'_>, scale: f32, rect: [u32; 4]) -> Result<Image> {
    let [x, y, width, height] = rect;
    if !scale.is_finite()
        || scale <= 0.0
        || width == 0
        || height == 0
        || width > 2048
        || height > 2048
    {
        return Err(err("Invalid render tile dimensions"));
    }
    let full_width = (native.width().value * scale).ceil();
    let full_height = (native.height().value * scale).ceil();
    if !full_width.is_finite()
        || !full_height.is_finite()
        || !(1.0..=1_000_000.0).contains(&full_width)
        || !(1.0..=1_000_000.0).contains(&full_height)
        || x.checked_add(width)
            .is_none_or(|end| end > full_width as u32)
        || y.checked_add(height)
            .is_none_or(|end| end > full_height as u32)
    {
        return Err(err("Render tile lies outside the page"));
    }
    // Keep native edge clipping outside the requested tile boundary with
    // a small raster gutter. Only the requested center is returned.
    let left = x.saturating_sub(4);
    let top = y.saturating_sub(4);
    let right = x
        .saturating_add(width)
        .saturating_add(4)
        .min(full_width as u32);
    let bottom = y
        .saturating_add(height)
        .saturating_add(4)
        .min(full_height as u32);
    let config = PdfRenderConfig::new()
        .set_target_size(full_width as i32, full_height as i32)
        .set_origin(-(left as i32), -(top as i32))
        .render_form_data(true)
        .render_annotations(true);
    let mut bitmap = PdfBitmap::empty(
        (right - left) as i32,
        (bottom - top) as i32,
        PdfBitmapFormat::default(),
    )?;
    native.render_into_bitmap_with_config(&mut bitmap, &config)?;
    let raw = bitmap.as_rgba_bytes();
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in 0..height {
        let start = ((row + y - top) as usize * (right - left) as usize + (x - left) as usize) * 4;
        rgba.extend_from_slice(&raw[start..start + width as usize * 4]);
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}
