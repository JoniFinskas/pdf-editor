use crate::office_package::{package, write_new, xml};
use crate::*;
#[derive(Clone, Debug, PartialEq)]
pub struct TableGrid {
    pub cells: Vec<Vec<String>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableFormat {
    Csv,
    Xlsx,
}
impl TableGrid {
    pub fn export(&self, path: &Path, format: TableFormat) -> Result<()> {
        if self
            .cells
            .iter()
            .flatten()
            .try_fold(0usize, |n, s| n.checked_add(s.len()))
            .is_none_or(|n| n > MAX_FILE / 8)
        {
            return Err(err("Table text exceeds export limit"));
        }
        if self.cells.is_empty()
            || self.cells.len() > 10000
            || self.cells.iter().any(|r| r.len() > 256)
            || self.cells.iter().map(Vec::len).sum::<usize>() > 100000
        {
            return Err(err("Invalid table dimensions"));
        }
        if format == TableFormat::Xlsx
            && self
                .cells
                .iter()
                .flatten()
                .any(|v| v.encode_utf16().count() > 32767)
        {
            return Err(err("Excel cells cannot exceed 32767 UTF-16 code units"));
        }
        let bytes = match format {
            TableFormat::Csv => self
                .cells
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|v| {
                            // Spreadsheet applications interpret these prefixes as formulas even in CSV.
                            let safe = if v.trim_start().starts_with(['=', '+', '-', '@'])
                                || v.starts_with(['\t', '\r'])
                            {
                                format!("'{v}")
                            } else {
                                v.clone()
                            };
                            format!("\"{}\"", safe.replace('\"', "\"\""))
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect::<Vec<_>>()
                .join("\r\n")
                .into_bytes(),
            TableFormat::Xlsx => {
                let mut sheet = String::from(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
                );
                for (r, row) in self.cells.iter().enumerate() {
                    sheet.push_str(&format!("<row r=\"{}\">", r + 1));
                    for (c, v) in row.iter().enumerate() {
                        sheet.push_str(&format!("<c r=\"{}{}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",column(c),r+1,xml(v)));
                    }
                    sheet.push_str("</row>");
                }
                sheet.push_str("</sheetData></worksheet>");
                package(&[
                    ("[Content_Types].xml".into(),br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec()),
                    ("_rels/.rels".into(),br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec()),
                    ("xl/workbook.xml".into(),br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Table" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_vec()),
                    ("xl/_rels/workbook.xml.rels".into(),br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_vec()),
                    ("xl/worksheets/sheet1.xml".into(),sheet.into_bytes())])?
            }
        };
        if bytes.len() > MAX_FILE {
            return Err(err("Table export exceeds 64 MiB"));
        }
        write_new(path, &bytes)
    }
}
fn column(mut n: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}
fn clusters(mut values: Vec<f32>, tolerance: f32) -> Vec<f32> {
    values.sort_by(f32::total_cmp);
    let mut out = vec![];
    for v in values {
        if out.last().is_none_or(|last: &f32| v - *last > tolerance) {
            out.push(v);
        }
    }
    out
}
impl Engine {
    pub fn detect_table(&self, doc: &Document, page: u16, area: Bounds) -> Result<TableGrid> {
        guarded(|| {
            if ![area.left, area.bottom, area.right, area.top]
                .iter()
                .all(|x| x.is_finite())
                || area.right <= area.left
                || area.top <= area.bottom
            {
                return Err(err("Select a valid table area"));
            }
            let text = self
                .text_objects(doc, page)?
                .into_iter()
                .filter(|t| {
                    area.contains(
                        (t.bounds.left + t.bounds.right) / 2.,
                        (t.bounds.top + t.bounds.bottom) / 2.,
                    ) && !t.text.trim().is_empty()
                })
                .collect::<Vec<_>>();
            if text.len() > 20000 {
                return Err(err("Table area contains too many text objects"));
            }
            let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let pg = native.pages().get(i32::from(page))?;
            if pg.objects().len() > 20000 {
                return Err(err("Table page has too many objects"));
            }
            let mut segment_count = 0u32;
            let mut xs = vec![];
            let mut ys = vec![];
            for o in pg.objects().iter() {
                if let Some(path) = o.as_path_object() {
                    let segments = path.segments().transform(path.matrix()?);
                    segment_count = segment_count.saturating_add(segments.len());
                    if segment_count > 100000 {
                        return Err(err("Table page has too many path segments"));
                    }
                    let mut prev = None;
                    for s in segments.iter() {
                        let p = (s.x().value, s.y().value);
                        if let Some((x, y)) = prev
                            && s.segment_type() == PdfPathSegmentType::LineTo
                            && area.contains(p.0, p.1)
                            && area.contains(x, y)
                        {
                            if (p.0 - x).abs() < 1. && (p.1 - y).abs() > 5. {
                                xs.push(x);
                            }
                            if (p.1 - y).abs() < 1. && (p.0 - x).abs() > 5. {
                                ys.push(y);
                            }
                        }
                        prev = Some(p);
                    }
                }
            }
            infer(&text, xs, ys)
        })
    }
}
fn infer(text: &[TextObject], xs: Vec<f32>, ys: Vec<f32>) -> Result<TableGrid> {
    if text.is_empty() {
        return Err(err("No text found in the selected area"));
    }
    let xs = clusters(xs, 2.);
    let ys = clusters(ys, 2.);
    let ruled = xs.len() >= 2 && ys.len() >= 2;
    let cols = if ruled {
        xs[..xs.len() - 1].to_vec()
    } else {
        clusters(text.iter().map(|t| t.bounds.left).collect(), 8.)
    };
    let rows = if ruled {
        ys[1..].iter().rev().copied().collect()
    } else {
        let mut y = clusters(text.iter().map(|t| t.bounds.top).collect(), 4.);
        y.reverse();
        y
    };
    if cols.len() > 256 || rows.len() > 10000 || cols.len() * rows.len() > 100000 {
        return Err(err("Detected table is too large"));
    }
    let mut cells = vec![vec![String::new(); cols.len()]; rows.len()];
    let mut sorted = text.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        b.bounds
            .top
            .total_cmp(&a.bounds.top)
            .then(a.bounds.left.total_cmp(&b.bounds.left))
    });
    for t in sorted {
        let x = t.bounds.left;
        let y = (t.bounds.top + t.bounds.bottom) / 2.;
        let c = if ruled {
            cols.iter().rposition(|v| x >= *v - 1.).unwrap_or(0)
        } else {
            cols.iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| (**a - x).abs().total_cmp(&(**b - x).abs()))
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        let r = if ruled {
            rows.iter()
                .rposition(|v| y <= *v + 1.)
                .unwrap_or(rows.len() - 1)
        } else {
            rows.iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (**a - t.bounds.top)
                        .abs()
                        .total_cmp(&(**b - t.bounds.top).abs())
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        if !cells[r][c].is_empty() {
            cells[r][c].push(' ');
        }
        cells[r][c].push_str(&t.text);
    }
    Ok(TableGrid { cells })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn columns_and_xml_escape() {
        assert_eq!(column(0), "A");
        assert_eq!(column(26), "AA");
        assert_eq!(xml("<&\""), "&lt;&amp;&quot;");
    }
}
