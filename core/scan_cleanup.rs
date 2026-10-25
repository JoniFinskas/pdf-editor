use crate::images_to_pdf::validate_picture;
use crate::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScanColor {
    #[default]
    Color,
    Grayscale,
    BlackWhite,
}
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Clockwise: top left, top right, bottom right, bottom left; normalized image coordinates.
    pub corners: [[f32; 2]; 4],
    pub deskew_degrees: f32,
    pub color: ScanColor,
    pub ocr: bool,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            corners: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            deskew_degrees: 0.0,
            color: ScanColor::Color,
            ocr: false,
        }
    }
}
fn gray(p: &[u8]) -> u8 {
    ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8
}

/// Estimate the paper quadrilateral from the largest bright connected region.
pub fn detect_scan_corners(picture: &Picture) -> Result<[[f32; 2]; 4]> {
    validate_picture(picture)?;
    let step = picture.width.max(picture.height).div_ceil(512).max(1);
    let (w, h) = (
        picture.width.div_ceil(step) as usize,
        picture.height.div_ceil(step) as usize,
    );
    let mut mask = vec![false; w * h];
    for y in 0..h {
        if y % 32 == 0 { operation::checkpoint("Processing scan rows", y, h)?; }
        for x in 0..w {
            let i = ((y as u32 * step * picture.width + x as u32 * step) * 4) as usize;
            mask[y * w + x] = gray(&picture.rgba[i..i + 4]) > 180;
        }
    }
    let mut largest = Vec::new();
    for index in 0..mask.len() {
        if index % 4096 == 0 { operation::checkpoint("Detecting scan edges", index, mask.len())?; }
        if !mask[index] {
            continue;
        }
        mask[index] = false;
        let mut component = vec![index];
        let mut next = 0;
        while next < component.len() {
            let at = component[next];
            next += 1;
            let (x, y) = (at % w, at / w);
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx < w && ny < h && mask[ny * w + nx] {
                    mask[ny * w + nx] = false;
                    component.push(ny * w + nx);
                }
            }
        }
        if component.len() > largest.len() {
            largest = component;
        }
    }
    if largest.len() < w * h / 10 {
        return Ok(ScanOptions::default().corners);
    }
    let points: Vec<_> = largest
        .iter()
        .map(|i| {
            [
                (i % w) as f32 / (w.saturating_sub(1).max(1)) as f32,
                (i / w) as f32 / (h.saturating_sub(1).max(1)) as f32,
            ]
        })
        .collect();
    let extreme = |a: f32, b: f32| {
        *points
            .iter()
            .min_by(|p, q| (p[0] * a + p[1] * b).total_cmp(&(q[0] * a + q[1] * b)))
            .unwrap_or(&[0.0, 0.0])
    };
    let corners = [
        extreme(1.0, 1.0),
        extreme(-1.0, 1.0),
        extreme(-1.0, -1.0),
        extreme(1.0, -1.0),
    ];
    if validate_corners(corners).is_err() {
        Ok(ScanOptions::default().corners)
    } else {
        Ok(corners)
    }
}
fn validate_corners(c: [[f32; 2]; 4]) -> Result<()> {
    if c.iter()
        .flatten()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(err("Corners must stay inside the image"));
    }
    for i in 0..4 {
        let a = c[i];
        let b = c[(i + 1) % 4];
        let d = c[(i + 2) % 4];
        if (b[0] - a[0]) * (d[1] - b[1]) - (b[1] - a[1]) * (d[0] - b[0]) < 0.0001 {
            return Err(err(
                "Keep corners clockwise without crossing, with a nonzero page area",
            ));
        }
    }
    Ok(())
}
/// Rectify a quadrilateral with a projective transform, then deskew and convert color.
pub fn clean_scan(picture: &Picture, options: &ScanOptions) -> Result<Picture> {
    validate_picture(picture)?;
    validate_corners(options.corners)?;
    if !options.deskew_degrees.is_finite() || options.deskew_degrees.abs() > 20.0 {
        return Err(err("Deskew angle must be between -20 and 20 degrees"));
    }
    let c = options.corners.map(|p| {
        [
            p[0] * (picture.width - 1) as f32,
            p[1] * (picture.height - 1) as f32,
        ]
    });
    let distance =
        |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    let w = (distance(c[0], c[1]).max(distance(c[3], c[2])).round() as u32 + 1).max(2);
    let h = (distance(c[0], c[3]).max(distance(c[1], c[2])).round() as u32 + 1).max(2);
    if u64::from(w) * u64::from(h) > 16_000_000 {
        return Err(err("Corrected page exceeds 16 million pixels"));
    }
    let [p0, p1, p2, p3] = c;
    let (dx1, dx2, dx3) = (p1[0] - p2[0], p3[0] - p2[0], p0[0] - p1[0] + p2[0] - p3[0]);
    let (dy1, dy2, dy3) = (p1[1] - p2[1], p3[1] - p2[1], p0[1] - p1[1] + p2[1] - p3[1]);
    let determinant = dx1 * dy2 - dx2 * dy1;
    if determinant.abs() < 0.0001 {
        return Err(err("Page corners form a degenerate quadrilateral"));
    }
    let (g, hp) = (
        (dx3 * dy2 - dx2 * dy3) / determinant,
        (dx1 * dy3 - dx3 * dy1) / determinant,
    );
    let (a, b, d, e) = (
        p1[0] - p0[0] + g * p1[0],
        p3[0] - p0[0] + hp * p3[0],
        p1[1] - p0[1] + g * p1[1],
        p3[1] - p0[1] + hp * p3[1],
    );
    let angle = options.deskew_degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    let mut rgba = vec![255; (w * h * 4) as usize];
    for y in 0..h {
        if y % 32 == 0 { operation::checkpoint("Processing scan rows", y as usize, h as usize)?; }
        for x in 0..w {
            let (cx, cy) = (
                x as f32 - (w - 1) as f32 / 2.0,
                y as f32 - (h - 1) as f32 / 2.0,
            );
            let u = (cx * cos + cy * sin + (w - 1) as f32 / 2.0) / (w - 1) as f32;
            let v = (-cx * sin + cy * cos + (h - 1) as f32 / 2.0) / (h - 1) as f32;
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let denominator = g * u + hp * v + 1.0;
            if denominator.abs() < 0.00001 {
                return Err(err("Perspective transform is singular"));
            }
            let sx = (a * u + b * v + p0[0]) / denominator;
            let sy = (d * u + e * v + p0[1]) / denominator;
            let out = ((y * w + x) * 4) as usize;
            let x0 = sx.floor().clamp(0.0, (picture.width - 1) as f32) as u32;
            let y0 = sy.floor().clamp(0.0, (picture.height - 1) as f32) as u32;
            let x1 = (x0 + 1).min(picture.width - 1);
            let y1 = (y0 + 1).min(picture.height - 1);
            let (fx, fy) = (
                (sx - x0 as f32).clamp(0.0, 1.0),
                (sy - y0 as f32).clamp(0.0, 1.0),
            );
            for channel in 0..3 {
                let value = |xx, yy| {
                    picture.rgba[((yy * picture.width + xx) * 4) as usize + channel] as f32
                };
                rgba[out + channel] = (value(x0, y0) * (1.0 - fx) * (1.0 - fy)
                    + value(x1, y0) * fx * (1.0 - fy)
                    + value(x0, y1) * (1.0 - fx) * fy
                    + value(x1, y1) * fx * fy)
                    .round() as u8;
            }
            if options.color != ScanColor::Color {
                let luminance = gray(&rgba[out..out + 4]);
                let value = if options.color == ScanColor::BlackWhite {
                    if luminance >= 160 { 255 } else { 0 }
                } else {
                    luminance
                };
                rgba[out..out + 3].fill(value);
            }
        }
    }
    Ok(Picture {
        width: w,
        height: h,
        rgba,
    })
}

impl Engine {
    /// Render a photo-only page for scan preview and corner detection.
    pub fn scan_preview(&self, doc: &Document, page: u16) -> Result<Picture> {
        if !self.text_objects(doc, page)?.is_empty() {
            return Err(err(
                "Scan cleanup requires a photo page without text. Export or keep the original text page instead.",
            ));
        }
        let pdf = parse(&doc.bytes)?;
        let id = *pdf
            .get_pages()
            .get(&(u32::from(page) + 1))
            .ok_or_else(|| err("Invalid page"))?;
        if let Ok(annotations) = pdf.get_dictionary(id)?.get(b"Annots")
            && !pdf.dereference(annotations)?.1.as_array()?.is_empty()
        {
            return Err(err(
                "Remove annotations before changing a photo page's geometry",
            ));
        }
        let items = self.page_items(doc, page)?;
        if items.is_empty() || items.iter().any(|i| i.kind != "Image") {
            return Err(err("Scan cleanup supports pages containing only images"));
        }
        let pages = self.pages(doc)?;
        let info = pages
            .get(page as usize)
            .ok_or_else(|| err("Invalid page"))?;
        let scale = (180.0f32 / 72.0).min(2500.0 / info.width.max(info.height));
        let image = self.render(doc, page, scale)?;
        Ok(Picture {
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        })
    }
    /// Replace one photo page atomically; optional OCR is included in the undo step.
    pub fn cleanup_scan(&self, doc: &mut Document, page: u16, options: &ScanOptions) -> Result<()> {
        guarded(|| {
            let preview = self.scan_preview(doc, page)?;
            let original_size = self
                .pages(doc)?
                .get(page as usize)
                .cloned()
                .ok_or_else(|| err("Invalid page"))?;
            let factor = original_size.width / preview.width as f32 / 0.75;
            let picture = clean_scan(&preview, options)?;
            let new_width = picture.width as f32 * 0.75 * factor;
            let new_height = picture.height as f32 * 0.75 * factor;
            let replacement = self.pictures_to_pdf([Ok(picture)], ImagePageSize::Image)?;
            let mut pdf = parse(&doc.bytes)?;
            let target = *pdf
                .get_pages()
                .get(&(u32::from(page) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let mut source = parse(&replacement.bytes)?;
            source.renumber_objects_with(pdf.max_id + 1);
            let source_page = *source
                .get_pages()
                .values()
                .next()
                .ok_or_else(|| err("Missing corrected page"))?;
            let mut corrected_content = format!("q {factor} 0 0 {factor} 0 0 cm\n").into_bytes();
            corrected_content.extend(source.get_page_content_with_limit(source_page, MAX_STREAM)?);
            corrected_content.extend(b"\nQ");
            let stream = source.add_object(lopdf::Stream::new(
                lopdf::Dictionary::new(),
                corrected_content,
            ));
            source
                .get_dictionary_mut(source_page)?
                .set("Contents", stream);
            source.get_dictionary_mut(source_page)?.set(
                "MediaBox",
                vec![
                    lopdf::Object::from(0),
                    0.into(),
                    new_width.into(),
                    new_height.into(),
                ],
            );
            let replacement = source.get_dictionary(source_page)?.clone();
            pdf.max_id = source.max_id;
            pdf.objects.extend(source.objects);
            let target = pdf.get_dictionary_mut(target)?;
            for key in [b"Contents".as_slice(), b"Resources", b"MediaBox"] {
                target.set(key, replacement.get(key)?.clone());
            }
            target.set("Rotate", 0);
            target.set("CropBox", replacement.get(b"MediaBox")?.clone());
            let mut candidate = self.candidate(doc, pdf)?;
            if options.ocr {
                self.ocr_page(&mut candidate, page)?;
            }
            operation::checkpoint("Applying scan cleanup", 1, 1)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn photo() -> Picture {
        let mut p = Picture {
            width: 100,
            height: 80,
            rgba: vec![20; 100 * 80 * 4],
        };
        for y in 10..70 {
            for x in 15..85 {
                let i = (y * 100 + x) * 4;
                p.rgba[i..i + 4].copy_from_slice(&[240, 240, 240, 255]);
            }
        }
        p
    }
    #[test]
    fn corners_detect_paper_and_rectify_to_grayscale() {
        let p = photo();
        let corners = detect_scan_corners(&p).unwrap();
        assert!((corners[0][0] - 15.0 / 99.0).abs() < 0.01);
        assert!((corners[2][1] - 69.0 / 79.0).abs() < 0.01);
        let clean = clean_scan(
            &p,
            &ScanOptions {
                corners,
                color: ScanColor::Grayscale,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((clean.width, clean.height), (70, 60));
        assert!(
            clean
                .rgba
                .as_chunks::<4>().0.iter()
                .all(|p| p[0] == p[1] && p[1] == p[2] && p[0] >= 239)
        );
    }
    #[test]
    fn perspective_bw_and_invalid_geometry() {
        let mut p = photo();
        for y in 30..50 {
            for x in 30..70 {
                let i = (y * 100 + x) * 4;
                p.rgba[i..i + 3].fill(0);
            }
        }
        let options = ScanOptions {
            corners: [[0.1, 0.1], [0.9, 0.2], [0.8, 0.9], [0.2, 0.8]],
            color: ScanColor::BlackWhite,
            deskew_degrees: 3.0,
            ..Default::default()
        };
        let clean = clean_scan(&p, &options).unwrap();
        assert!(clean.rgba.as_chunks::<4>().0.iter().any(|p| p[0] == 0));
        assert!(clean.rgba.as_chunks::<4>().0.iter().all(|p| p[0] == 0 || p[0] == 255));
        let crossed = ScanOptions {
            corners: [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]],
            ..Default::default()
        };
        assert!(clean_scan(&p, &crossed).is_err());
        assert!(
            clean_scan(
                &p,
                &ScanOptions {
                    deskew_degrees: f32::NAN,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
