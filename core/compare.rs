//! Bounded page-by-page comparison; input documents are never modified.
use crate::{Bounds, Document, Engine, Image, Result, TextObject, err};
use pdfium_render::prelude::PdfPoints;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}
#[derive(Clone, Debug)]
pub struct TextChange {
    pub kind: ChangeKind,
    pub left: Option<Bounds>,
    pub right: Option<Bounds>,
}
pub struct ComparePage {
    pub left: Option<Image>,
    pub right: Option<Image>,
    pub changes: Vec<TextChange>,
    pub changed_pixels: usize,
    pub left_marks: Vec<(ChangeKind, [f32; 4])>,
    pub right_marks: Vec<(ChangeKind, [f32; 4])>,
}
/// Compare ordered text runs with bounded lookahead, avoiding quadratic tables.
pub fn compare_text(left: &[TextObject], right: &[TextObject]) -> Result<Vec<TextChange>> {
    if left.len() > 20_000 || right.len() > 20_000 {
        return Err(err("Comparison supports at most 20,000 text runs per page"));
    }
    let text_bytes = left
        .iter()
        .chain(right)
        .try_fold(0usize, |n, object| n.checked_add(object.text.len()));
    if text_bytes.is_none_or(|n| n > 8 * 1024 * 1024) {
        return Err(err("Comparison text exceeds the 8 MiB page-pair limit"));
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < left.len() || j < right.len() {
        if i < left.len() && j < right.len() && left[i].text == right[j].text {
            i += 1;
            j += 1;
            continue;
        }
        let removed = i < left.len()
            && (j == right.len()
                || left
                    .iter()
                    .skip(i + 1)
                    .take(32)
                    .any(|v| v.text == right[j].text));
        let added = j < right.len()
            && (i == left.len()
                || right
                    .iter()
                    .skip(j + 1)
                    .take(32)
                    .any(|v| v.text == left[i].text));
        let kind = if removed {
            ChangeKind::Removed
        } else if added {
            ChangeKind::Added
        } else {
            ChangeKind::Changed
        };
        let l = if kind != ChangeKind::Added {
            let b = left[i].bounds;
            i += 1;
            Some(b)
        } else {
            None
        };
        let r = if kind != ChangeKind::Removed {
            let b = right[j].bounds;
            j += 1;
            Some(b)
        } else {
            None
        };
        out.push(TextChange {
            kind,
            left: l,
            right: r,
        });
    }
    Ok(out)
}
/// Tint changed pixels magenta in both rasters, comparing missing pixels as absent.
pub fn compare_pixels(
    left: &mut Option<Image>,
    right: &mut Option<Image>,
    overlay: bool,
) -> Result<usize> {
    for image in [left.as_ref(), right.as_ref()].into_iter().flatten() {
        if image.width == 0
            || image.height == 0
            || u64::from(image.width) * u64::from(image.height) > 1_100_000
            || image.rgba.len() as u64 != u64::from(image.width) * u64::from(image.height) * 4
        {
            return Err(err("Invalid or oversized comparison raster"));
        }
    }
    let width = left
        .as_ref()
        .map_or(0, |v| v.width)
        .max(right.as_ref().map_or(0, |v| v.width));
    let height = left
        .as_ref()
        .map_or(0, |v| v.height)
        .max(right.as_ref().map_or(0, |v| v.height));
    if u64::from(width) * u64::from(height) > 2_200_000 {
        return Err(err("Comparison canvas exceeds limit"));
    }
    let mut count = 0;
    for y in 0..height {
        for x in 0..width {
            let index = |im: &Image| {
                if x < im.width && y < im.height {
                    Some(((y * im.width + x) * 4) as usize)
                } else {
                    None
                }
            };
            let a = left
                .as_ref()
                .and_then(|im| index(im).map(|i| &im.rgba[i..i + 4]));
            let b = right
                .as_ref()
                .and_then(|im| index(im).map(|i| &im.rgba[i..i + 4]));
            if a != b {
                count += 1;
                if overlay {
                    for im in [left.as_mut(), right.as_mut()].into_iter().flatten() {
                        if let Some(i) = index(im) {
                            im.rgba[i] = ((u16::from(im.rgba[i]) + 255) / 2) as u8;
                            im.rgba[i + 1] /= 2;
                            im.rgba[i + 2] = ((u16::from(im.rgba[i + 2]) + 180) / 2) as u8;
                            im.rgba[i + 3] = 255;
                        }
                    }
                }
            }
        }
    }
    Ok(count)
}
impl Engine {
    /// Compare corresponding page indices at a shared, automatically bounded scale.
    pub fn compare_page(
        &self,
        left: &Document,
        right: &Document,
        page: u16,
        overlay: bool,
    ) -> Result<ComparePage> {
        crate::guarded(|| {
            let dimensions = |doc: &Document| -> Result<Option<crate::PageInfo>> {
                let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                if i32::from(page) >= pdf.pages().len() {
                    return Ok(None);
                }
                let p = pdf.pages().get(i32::from(page))?;
                Ok(Some(crate::PageInfo {
                    width: p.width().value,
                    height: p.height().value,
                }))
            };
            let a = dimensions(left)?;
            let b = dimensions(right)?;
            if a.is_none() && b.is_none() {
                return Err(err("Comparison page is out of range"));
            }
            let mut scale = 1.0f32;
            for p in [a.as_ref(), b.as_ref()].into_iter().flatten() {
                let area = p.width * p.height;
                if !area.is_finite() || area <= 0.0 {
                    return Err(err("Invalid comparison page dimensions"));
                }
                scale = scale
                    .min((1_000_000.0 / area).sqrt())
                    .min(4096.0 / p.width.max(p.height));
            }
            let l = if a.is_some() {
                self.text_objects(left, page)?
            } else {
                vec![]
            };
            let r = if b.is_some() {
                self.text_objects(right, page)?
            } else {
                vec![]
            };
            let changes = compare_text(&l, &r)?;
            let marks = |doc: &Document, is_left: bool| -> Result<Vec<(ChangeKind, [f32; 4])>> {
                if (is_left && a.is_none()) || (!is_left && b.is_none()) {
                    return Ok(vec![]);
                }
                let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                let native = pdf.pages().get(i32::from(page))?;
                let config = crate::render_config(&native, scale)?;
                changes
                    .iter()
                    .filter_map(|c| (if is_left { c.left } else { c.right }).map(|b| (c.kind, b)))
                    .map(|(k, b)| {
                        let (x1, y1) = native.points_to_pixels(
                            PdfPoints::new(b.left),
                            PdfPoints::new(b.top),
                            &config,
                        )?;
                        let (x2, y2) = native.points_to_pixels(
                            PdfPoints::new(b.right),
                            PdfPoints::new(b.bottom),
                            &config,
                        )?;
                        Ok((
                            k,
                            [
                                x1.min(x2) as f32,
                                y1.min(y2) as f32,
                                x1.max(x2) as f32,
                                y1.max(y2) as f32,
                            ],
                        ))
                    })
                    .collect()
            };
            let left_marks = marks(left, true)?;
            let right_marks = marks(right, false)?;
            let mut li = if a.is_some() {
                Some(self.render(left, page, scale)?)
            } else {
                None
            };
            let mut ri = if b.is_some() {
                Some(self.render(right, page, scale)?)
            } else {
                None
            };
            let changed_pixels = compare_pixels(&mut li, &mut ri, overlay)?;
            Ok(ComparePage {
                left: li,
                right: ri,
                changes,
                changed_pixels,
                left_marks,
                right_marks,
            })
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn run(text: &str) -> TextObject {
        TextObject {
            page: 0,
            index: 0,
            revision: 0,
            text: text.into(),
            font: "Test".into(),
            size: 12.,
            bounds: Bounds {
                left: 0.,
                bottom: 0.,
                right: 10.,
                top: 10.,
            },
            color: [0; 4],
            matrix: [0.; 6],
        }
    }
    #[test]
    fn aligns_insert_delete_and_change() {
        let a = [run("a"), run("b"), run("c")];
        let b = [run("a"), run("new"), run("b"), run("changed")];
        let d = compare_text(&a, &b).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].kind, ChangeKind::Added);
        assert_eq!(d[1].kind, ChangeKind::Changed);
        assert_eq!(compare_text(&a, &[]).unwrap().len(), 3);
        assert!(compare_text(&a, &a).unwrap().is_empty());
    }
    #[test]
    fn unicode_middle_insertion_and_resource_limits() {
        let left = [run("ää ö €"), run("middle"), run("last")];
        let right = [run("ää ö €"), run("新增"), run("middle"), run("last")];
        let changes = compare_text(&left, &right).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        let reverse = compare_text(&right, &left).unwrap();
        assert_eq!(reverse.len(), 1);
        assert_eq!(reverse[0].kind, ChangeKind::Removed);
        assert!(compare_text(&vec![run("x"); 20_001], &[]).is_err());
        assert!(compare_text(&[run(&"x".repeat(8 * 1024 * 1024 + 1))], &[]).is_err());
    }
    #[test]
    fn visual_overlay_and_missing_page() {
        let img = Image {
            width: 2,
            height: 1,
            rgba: vec![255; 8],
        };
        let mut a = Some(img.clone());
        let mut b = Some(img);
        b.as_mut().unwrap().rgba[4] = 0;
        assert_eq!(compare_pixels(&mut a, &mut b, true).unwrap(), 1);
        assert_eq!(&a.as_ref().unwrap().rgba[..4], &[255; 4]);
        assert!(a.as_ref().unwrap().rgba[5] < 255);
        assert_eq!(compare_pixels(&mut a, &mut None, false).unwrap(), 2);
        assert!(
            compare_pixels(
                &mut Some(Image {
                    width: u32::MAX,
                    height: 2,
                    rgba: vec![]
                }),
                &mut None,
                false
            )
            .is_err()
        );
    }
}
