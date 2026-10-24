//! Bounded page-by-page comparison; input documents are never modified.
use crate::{Bounds, Document, Engine, Image, Result, TextObject, err};
use pdfium_render::prelude::PdfPoints;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PagePair {
    pub left: Option<u16>,
    pub right: Option<u16>,
}

/// Align unchanged-page fingerprints, then pair modified gaps by position.
/// LCS storage is capped at four million cells; larger inputs use 64-page lookahead.
pub fn align_comparison_pages(left: &[u64], right: &[u64]) -> Result<Vec<PagePair>> {
    if left.len().max(right.len()) > 32_767 {
        return Err(err("Comparison supports at most 32,767 pages per file"));
    }
    let (n, m) = (left.len(), right.len());
    let mut anchors = Vec::new();
    if (n + 1).saturating_mul(m + 1) <= 4_000_000 {
        let stride = m + 1;
        let mut lengths = vec![0u16; (n + 1) * stride];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lengths[i * stride + j] = if left[i] == right[j] {
                    1 + lengths[(i + 1) * stride + j + 1]
                } else {
                    lengths[(i + 1) * stride + j].max(lengths[i * stride + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if left[i] == right[j] {
                anchors.push((i, j));
                i += 1;
                j += 1;
            } else if lengths[(i + 1) * stride + j] > lengths[i * stride + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    } else {
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if left[i] == right[j] {
                anchors.push((i, j));
                i += 1;
                j += 1;
                continue;
            }
            let a = (i + 1..n.min(i + 65)).find(|&k| left[k] == right[j]);
            let b = (j + 1..m.min(j + 65)).find(|&k| right[k] == left[i]);
            match (a, b) {
                (Some(a), Some(b)) if a - i <= b - j => i = a,
                (_, Some(b)) => j = b,
                (Some(a), None) => i = a,
                _ => {
                    i += 1;
                    j += 1;
                }
            }
        }
    }
    let mut pairs = Vec::with_capacity(n.max(m));
    let (mut i, mut j) = (0, 0);
    for (a, b) in anchors.into_iter().chain(std::iter::once((n, m))) {
        while i < a || j < b {
            let l = (i < a).then_some(i as u16);
            let r = (j < b).then_some(j as u16);
            pairs.push(PagePair { left: l, right: r });
            i += usize::from(l.is_some());
            j += usize::from(r.is_some());
        }
        if a < n && b < m {
            pairs.push(PagePair {
                left: Some(a as u16),
                right: Some(b as u16),
            });
            i = a + 1;
            j = b + 1;
        }
    }
    Ok(pairs)
}
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
    /// Session-only fingerprint from text and a small raster; never a security hash.
    pub fn comparison_fingerprint(&self, doc: &Document, page: u16) -> Result<u64> {
        use std::hash::{Hash, Hasher};
        crate::guarded(|| {
            let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let native = pdf.pages().get(i32::from(page))?;
            let width = native.width().value;
            let height = native.height().value;
            if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                return Err(err("Invalid page size"));
            }
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            width.to_bits().hash(&mut hasher);
            height.to_bits().hash(&mut hasher);
            let image = self.render(doc, page, 64.0 / width.max(height))?;
            image.rgba.hash(&mut hasher);
            let objects = self.text_objects(doc, page)?;
            let mut bytes = 0usize;
            for object in objects {
                bytes = bytes.saturating_add(object.text.len());
                if bytes > 8 * 1024 * 1024 {
                    return Err(err("Comparison text exceeds limit"));
                }
                object.text.hash(&mut hasher);
            }
            Ok(hasher.finish())
        })
    }
    /// Compare corresponding page indices at a shared, automatically bounded scale.
    pub fn compare_page(
        &self,
        left: &Document,
        right: &Document,
        page: u16,
        overlay: bool,
    ) -> Result<ComparePage> {
        let left_page = ((page as usize) < self.pages(left)?.len()).then_some(page);
        let right_page = ((page as usize) < self.pages(right)?.len()).then_some(page);
        self.compare_page_pair(
            left,
            right,
            PagePair {
                left: left_page,
                right: right_page,
            },
            overlay,
        )
    }

    /// Compare an aligned pair. A missing side represents an inserted/deleted page.
    pub fn compare_page_pair(
        &self,
        left: &Document,
        right: &Document,
        pair: PagePair,
        overlay: bool,
    ) -> Result<ComparePage> {
        crate::guarded(|| {
            let dimensions =
                |doc: &Document, page: Option<u16>| -> Result<Option<crate::PageInfo>> {
                    let Some(page) = page else {
                        return Ok(None);
                    };
                    let pdf = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                    if i32::from(page) >= pdf.pages().len() {
                        return Err(err("Comparison page is out of range"));
                    }
                    let p = pdf.pages().get(i32::from(page))?;
                    Ok(Some(crate::PageInfo {
                        width: p.width().value,
                        height: p.height().value,
                    }))
                };
            let a = dimensions(left, pair.left)?;
            let b = dimensions(right, pair.right)?;
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
            let l = if let Some(page) = pair.left {
                self.text_objects(left, page)?
            } else {
                vec![]
            };
            let r = if let Some(page) = pair.right {
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
                let page = (if is_left { pair.left } else { pair.right })
                    .ok_or_else(|| err("Missing comparison side"))?;
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
            let mut li = if let Some(page) = pair.left {
                Some(self.render(left, page, scale)?)
            } else {
                None
            };
            let mut ri = if let Some(page) = pair.right {
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
    #[test]
    fn page_alignment_handles_insert_delete_changes_repeats_and_limits() {
        let pairs = align_comparison_pages(&[1, 2, 3], &[1, 9, 2, 3]).unwrap();
        assert_eq!(
            pairs,
            vec![
                PagePair {
                    left: Some(0),
                    right: Some(0)
                },
                PagePair {
                    left: None,
                    right: Some(1)
                },
                PagePair {
                    left: Some(1),
                    right: Some(2)
                },
                PagePair {
                    left: Some(2),
                    right: Some(3)
                },
            ]
        );
        let deleted = align_comparison_pages(&[1, 9, 2, 3], &[1, 2, 3]).unwrap();
        assert_eq!(
            deleted[1],
            PagePair {
                left: Some(1),
                right: None
            }
        );
        assert_eq!(
            align_comparison_pages(&[1, 2, 3], &[1, 8, 3])
                .unwrap()
                .len(),
            3
        );
        assert!(align_comparison_pages(&[], &[]).unwrap().is_empty());
        for seed in 0..80 {
            let left: Vec<_> = (0..seed % 13)
                .map(|i| ((i * 3 + seed) % 7) as u64)
                .collect();
            let right: Vec<_> = (0..seed % 17)
                .map(|i| ((i * 5 + seed) % 7) as u64)
                .collect();
            let pairs = align_comparison_pages(&left, &right).unwrap();
            assert_eq!(
                pairs.iter().filter_map(|p| p.left).collect::<Vec<_>>(),
                (0..left.len() as u16).collect::<Vec<_>>()
            );
            assert_eq!(
                pairs.iter().filter_map(|p| p.right).collect::<Vec<_>>(),
                (0..right.len() as u16).collect::<Vec<_>>()
            );
            assert!(pairs.iter().all(|p| p.left.is_some() || p.right.is_some()));
        }
        let left: Vec<u64> = (0..2200).collect();
        let mut right = left.clone();
        right.insert(10, 9999);
        let pairs = align_comparison_pages(&left, &right).unwrap();
        assert_eq!(
            pairs[10],
            PagePair {
                left: None,
                right: Some(10)
            }
        );
        assert_eq!(
            pairs.last().unwrap(),
            &PagePair {
                left: Some(2199),
                right: Some(2200)
            }
        );
        assert!(align_comparison_pages(&vec![0; 32768], &[]).is_err());
    }
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
