//! Conservative paragraph detection and atomic font-preserving reflow.
use crate::*;
use lopdf::{
    Dictionary, Object, Stream,
    content::{Content, Operation},
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParagraphAlignment {
    Left,
    Right,
    Center,
    Justified,
}
#[derive(Clone, Debug)]
pub struct Paragraph {
    pub objects: Vec<TextObject>,
    pub bounds: Bounds,
    pub text: String,
    pub alignment: ParagraphAlignment,
    pub line_spacing: f32,
    pub line_count: usize,
    /// Optional writable width in PDF points; changing it reflows without scaling.
    pub wrap_width: Option<f32>,
    /// Some means the selection must use ordinary line editing.
    pub fallback: Option<String>,
}
fn horizontal(o: &TextObject) -> bool {
    [o.bounds.left, o.bounds.right, o.bounds.top, o.bounds.bottom]
        .iter()
        .all(|v| v.is_finite())
        && o.matrix.iter().all(|v| v.is_finite())
        && o.matrix[1].abs() < 0.001
        && o.matrix[2].abs() < 0.001
        && o.matrix[0] > 0.0
        && o.matrix[3] > 0.0
        && o.size.is_finite()
        && o.size > 0.0
        && o.color[3] == 255
}
fn same(a: &TextObject, b: &TextObject) -> bool {
    a.page == b.page
        && a.revision == b.revision
        && fonts::family(&a.font) == fonts::family(&b.font)
        && fonts::base_name(&a.font).eq_ignore_ascii_case(fonts::base_name(&b.font))
        && (a.size - b.size).abs() < 0.05
        && a.matrix[..4].iter().zip(&b.matrix[..4]).all(|(x,y)|(x-y).abs()<0.001)
        && a.color == b.color
}
fn union(a: Bounds, b: Bounds) -> Bounds {
    Bounds {
        left: a.left.min(b.left),
        right: a.right.max(b.right),
        bottom: a.bottom.min(b.bottom),
        top: a.top.max(b.top),
    }
}
fn list_marker(text: &str) -> bool {
    let text=text.trim();
    matches!(text,"•"|"●"|"▪"|"◦"|"-")
        || text.strip_suffix(['.',')']).is_some_and(|n|
            !n.is_empty() && n.len()<=6 && n.chars().all(|c|c.is_ascii_digit()))
}
fn line(objects: Vec<TextObject>, fallback: Option<String>) -> Paragraph {
    let first = &objects[0];
    let mut text = String::new();
    let mut bounds = first.bounds;
    let mut end = first.bounds.left;
    for o in &objects {
        if !text.is_empty() && o.bounds.left - end > o.size * o.matrix[0] * 0.15 && !text.ends_with(' ') {
            text.push(' ')
        }
        text.push_str(&o.text);
        end = o.bounds.right;
        bounds = union(bounds, o.bounds);
    }
    Paragraph {
        line_spacing: first.size * first.matrix[3] * 1.2,
        line_count: 1,
        wrap_width: None,
        objects,
        bounds,
        text,
        alignment: ParagraphAlignment::Left,
        fallback,
    }
}
/// Groups a single page's current text objects; ambiguous layouts stay as lines.
pub fn paragraphs(objects: &[TextObject]) -> Vec<Paragraph> {
    if objects.len() > 4096 {
        return objects
            .iter()
            .cloned()
            .map(|o| {
                line(
                    vec![o],
                    Some("Dense page; editing individual lines only".into()),
                )
            })
            .collect();
    }
    let mut sorted: Vec<_> = objects
        .iter()
        .filter(|o| !o.text.trim().is_empty() && !list_marker(&o.text))
        .cloned()
        .collect();
    sorted.sort_by(|a, b| {
        b.matrix[5]
            .total_cmp(&a.matrix[5])
            .then(a.bounds.left.total_cmp(&b.bounds.left))
    });
    let mut lines: Vec<Paragraph> = vec![];
    for o in sorted {
        if let Some(last) = lines.last_mut() {
            let a = &last.objects[0];
            if a.page == o.page
                && (a.matrix[5] - o.matrix[5]).abs() < a.size * a.matrix[3] * 0.15
                && horizontal(a)
                && horizontal(&o)
                && o.bounds.left - last.bounds.right < a.size * a.matrix[0] * 1.5
            {
                let mixed = !same(a, &o);
                let mut items = last.objects.clone();
                items.push(o);
                *last = line(
                    items,
                    if mixed || last.fallback.is_some() {
                        Some("Mixed fonts within a line; editing this line only".into())
                    } else {
                        None
                    },
                );
                continue;
            }
        }
        let fallback =
            (!horizontal(&o)).then(|| "Rotated or transformed text; editing this line only".into());
        lines.push(line(vec![o], fallback));
    }
    // Side-by-side lines on the same baseline are ambiguous columns.
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            if lines[i].objects[0].page != lines[j].objects[0].page {
                continue;
            }
            let dy = (lines[i].objects[0].matrix[5] - lines[j].objects[0].matrix[5]).abs();
            let em = lines[i].objects[0].size * lines[i].objects[0].matrix[3].abs();
            if dy < em * 0.2 {
                lines[i].fallback = Some("Unclear columns; editing this line only".into());
                lines[j].fallback = lines[i].fallback.clone();
            }
            if dy > em * 4.0 {
                break;
            }
        }
    }
    let mut groups: Vec<Vec<Paragraph>> = vec![];
    for l in lines {
        let starts_item = objects.iter().any(|o| list_marker(&o.text)
            && o.page == l.objects[0].page
            && (o.matrix[5]-l.objects[0].matrix[5]).abs()<l.objects[0].size*l.objects[0].matrix[3]*0.15
            && o.bounds.right<=l.bounds.left);
        let joins = groups.last().is_some_and(|g| {
            let prev = &g[g.len() - 1];
            let first = &g[0];
            let spacing = prev.objects[0].matrix[5] - l.objects[0].matrix[5];
            let size = l.objects[0].size * l.objects[0].matrix[3];
            let tol = l.objects[0].size * l.objects[0].matrix[0] * 0.35;
            same(&first.objects[0], &l.objects[0])
                && !starts_item
                && l.fallback.is_none()
                && prev.fallback.is_none()
                && spacing > size * 0.7
                && spacing < size * 2.0
                && (g.len() < 2
                    || (spacing - (g[0].objects[0].matrix[5] - g[1].objects[0].matrix[5])).abs()
                        < size * 0.15)
                && l.bounds.right > prev.bounds.left
                && l.bounds.left < prev.bounds.right
                && ((l.bounds.left - first.bounds.left).abs() < tol
                    || (l.bounds.right - first.bounds.right).abs() < tol
                    || ((l.bounds.left + l.bounds.right)
                        - (first.bounds.left + first.bounds.right))
                        .abs()
                        < tol * 2.0)
        });
        if joins {
            groups.last_mut().unwrap().push(l)
        } else {
            groups.push(vec![l])
        }
    }
    let mut result: Vec<_> = groups
        .into_iter()
        .map(|g| {
            let mut p = g[0].clone();
            if g.len() == 1 {
                return p;
            }
            p.line_count = g.len();
            p.line_spacing = g[0].objects[0].matrix[5] - g[1].objects[0].matrix[5];
            let tol = p.objects[0].size * p.objects[0].matrix[0] * 0.35;
            let left = g
                .iter()
                .all(|l| (l.bounds.left - p.bounds.left).abs() < tol);
            let right = g
                .iter()
                .all(|l| (l.bounds.right - p.bounds.right).abs() < tol);
            let center = g.iter().all(|l| {
                ((l.bounds.left + l.bounds.right) - (p.bounds.left + p.bounds.right)).abs()
                    < tol * 2.0
            });
            let justified = g.len() >= 3
                && g.last()
                    .is_some_and(|last| last.bounds.right < p.bounds.right - tol)
                && left
                && g[..g.len() - 1]
                    .iter()
                    .all(|l| (l.bounds.right - p.bounds.right).abs() < tol);
            p.alignment = if justified {
                ParagraphAlignment::Justified
            } else if left {
                ParagraphAlignment::Left
            } else if right {
                ParagraphAlignment::Right
            } else if center {
                ParagraphAlignment::Center
            } else {
                p.fallback = Some("Unclear alignment; editing individual lines only".into());
                ParagraphAlignment::Left
            };
            let widest = g.iter().map(|l| l.bounds.right - l.bounds.left).fold(0.0, f32::max);
            for (previous, l) in g.iter().zip(&g[1..]) {
                p.bounds = union(p.bounds, l.bounds);
                // PDF rows have no paragraph markers. Join likely soft wraps;
                // retain short rows and new list items as explicit breaks.
                let list = l.text.trim_start();
                let list_start = list.starts_with(['•', '●', '▪', '-'])
                    || list.split_once(['.', ')']).is_some_and(|(n, rest)|
                        !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && rest.starts_with(' '));
                let short = previous.bounds.right - previous.bounds.left < widest * 0.6;
                p.text.push(if short || list_start { '\n' } else { ' ' });
                p.text.push_str(&l.text);
                p.objects.extend(l.objects.clone());
            }
            p
        })
        .collect();
    // A separate bullet can use a symbol font without splitting its prose block.
    result.extend(objects.iter().filter(|o|list_marker(&o.text)).cloned().map(|o| {
        let fallback=(!horizontal(&o)).then(||"Rotated or transformed text; editing this line only".into());
        line(vec![o],fallback)
    }));
    result.sort_by(|a,b| b.objects[0].matrix[5].total_cmp(&a.objects[0].matrix[5]).then(a.bounds.left.total_cmp(&b.bounds.left)));
    result
}

fn wrap(text: &str, width: f32, measure: impl Fn(&str) -> Result<f32>) -> Result<Vec<String>> {
    let mut lines = vec![];
    for hard in text.split('\n') {
        let mut current = String::new();
        for word in hard.split_whitespace() {
            if measure(word)? > width + 0.01 {
                return Err(err(
                    "A word is wider than the text box. Widen the box or add a space.",
                ));
            }
            let next = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if !current.is_empty() && measure(&next)? > width {
                lines.push(current);
                current = word.to_string()
            } else {
                current = next
            }
        }
        lines.push(current);
    }
    if lines.len() > 4096 {
        return Err(err("Paragraph exceeds the line limit"));
    }
    Ok(lines)
}
// Prefer explicit PDF widths, which may differ from the embedded font program.
fn pdf_advance(pdf: &lopdf::Document, font: &Dictionary, encoded: &[u8]) -> Result<Option<f32>> {
    if font.get(b"Subtype")?.as_name()? == b"Type0" {
        if font.get(b"Encoding")?.as_name()? != b"Identity-H" || encoded.len() != 2 {
            return Err(err("Unsupported paragraph CID metrics; use line editing"));
        }
        let child = font
            .get(b"DescendantFonts")?
            .as_array()?
            .first()
            .ok_or_else(|| err("Missing CID metrics"))?;
        let cid = pdf.dereference(child)?.1.as_dict()?;
        let code = u16::from_be_bytes([encoded[0], encoded[1]]) as i64;
        if let Ok(widths) = cid.get(b"W") {
            let a = pdf.dereference(widths)?.1.as_array()?;
            let mut i = 0;
            while i < a.len() {
                let start = a[i].as_i64()?;
                let next = a.get(i + 1).ok_or_else(|| err("Invalid CID widths"))?;
                if let Ok(values) = next.as_array() {
                    if code >= start
                        && let Some(w) = values.get((code - start) as usize)
                    {
                        return Ok(Some(w.as_float()?));
                    }
                    i += 2;
                } else {
                    let end = next.as_i64()?;
                    let w = a
                        .get(i + 2)
                        .ok_or_else(|| err("Invalid CID widths"))?
                        .as_float()?;
                    if code >= start && code <= end {
                        return Ok(Some(w));
                    }
                    i += 3;
                }
            }
        }
        return Ok(Some(
            cid.get(b"DW").and_then(Object::as_float).unwrap_or(1000.0),
        ));
    }
    if encoded.len() != 1 {
        return Err(err("Unsupported paragraph character metrics"));
    }
    if let (Ok(widths), Ok(start)) = (
        font.get(b"Widths"),
        font.get(b"FirstChar").and_then(Object::as_i64),
    ) {
        let index = i64::from(encoded[0]) - start;
        if index < 0 {
            return Err(err("Invalid paragraph glyph width"));
        }
        return Ok(Some(
            pdf.dereference(widths)?
                .1
                .as_array()?
                .get(index as usize)
                .ok_or_else(|| err("Missing paragraph glyph width"))?
                .as_float()?,
        ));
    }
    Ok(None)
}
impl Engine {
    pub fn edit_paragraph(
        &self,
        doc: &mut Document,
        selected: &Paragraph,
        text: &str,
    ) -> Result<EditResult> {
        guarded(|| {
            let first = selected
                .objects
                .first()
                .ok_or_else(|| err("Nothing selected"))?;
            if first.revision != doc.revision {
                return Err(err("Selection is stale; select the paragraph again"));
            }
            let all = self.text_objects(doc, first.page)?;
            let current: std::collections::HashMap<_, _> = all.iter().map(|object| (object.index, object)).collect();
            for object in &selected.objects {
                let actual = current.get(&object.index).ok_or_else(|| err("Paragraph selection changed"))?;
                validate_text_selection(object, actual)?;
            }
            let p = paragraphs(&all)
                .into_iter()
                .find(|p| {
                    p.objects
                        .iter()
                        .map(|o| o.index)
                        .eq(selected.objects.iter().map(|o| o.index))
                })
                .ok_or_else(|| err("Paragraph selection changed"))?;
            let first = &p.objects[0];
            if p.fallback.is_some() {
                return Err(err("This layout requires line editing"));
            }
            if text.chars().count() > 16384 || text.chars().any(|c| c.is_control() && c != '\n') {
                return Err(err("Paragraph requires at most 16384 printable characters"));
            }
            let width = selected.wrap_width.unwrap_or(p.bounds.right - p.bounds.left);
            if !width.is_finite() || width <= 0.0 {
                return Err(err("Invalid text box width"));
            }
            if text == p.text && selected.wrap_width.is_none() {
                return Ok(EditResult {
                    warning: None,
                    objects_changed: 0,
                });
            }
            let plain = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut candidate = self.candidate(doc, parse(&doc.bytes)?)?;
            candidate.substitution = Substitution::Line;
            // Prepare one encoded run through the same fallback tiers as line edits.
            let mut result = self.edit_group_for_reflow(&mut candidate, &p.objects, &plain)?;
            if plain.is_empty() {
                operation::checkpoint("Applying paragraph", 1, 1)?;
                doc.accept(candidate.bytes);
                return Ok(result);
            }
            let index = all
                .iter()
                .position(|o| p.objects.iter().any(|s| s.index == o.index))
                .ok_or_else(|| err("Missing paragraph"))?;
            let current = self.text_objects(&candidate, first.page)?;
            let template = current
                .get(index)
                .ok_or_else(|| err("Missing paragraph template"))?
                .clone();
            let pdf = parse(&candidate.bytes)?;
            let page = *pdf
                .get_pages()
                .get(&(u32::from(first.page) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let ops = Content::decode(&pdf.get_page_content_with_limit(page, MAX_STREAM)?)?;
            let runs = content::runs(&pdf, page, &ops)?;
            let run = runs
                .get(index)
                .ok_or_else(|| err("Missing paragraph encoding"))?;
            let font = fonts::embedded_font(&pdf, &run.font, &plain)
                .ok_or_else(|| err("Paragraph metrics unavailable; use line editing"))?;
            let face = FontRef::new(&font.bytes).map_err(|_| err("Invalid paragraph font"))?;
            let metrics = face.glyph_metrics(Size::new(template.size), LocationRef::default());
            let vertical = face.metrics(Size::new(template.size), LocationRef::default());
            let spacing = run.spacing;
            if spacing.iter().any(|v| !v.is_finite()) || spacing[2] <= 0.0 || spacing[3] != 0.0 {
                return Err(err("Unsupported text state; use line editing"));
            }
            let encoding = content::decoding(&pdf, &run.font)?;
            let mut advances = std::collections::BTreeMap::new();
            for c in plain.chars() {
                if advances.contains_key(&c) {
                    continue;
                }
                let encoded = content::encode_with(&encoding, &c.to_string())?;
                let fallback = face
                    .charmap()
                    .map(c)
                    .and_then(|g| metrics.advance_width(g))
                    .ok_or_else(|| err("Missing paragraph glyph metrics"))?;
                let advance = pdf_advance(&pdf, &run.font, &encoded)?
                    .map_or(fallback, |w| w * template.size / 1000.0);
                if !advance.is_finite() || advance < 0.0 || advance > 100_000.0 {
                    return Err(err("Invalid paragraph glyph metrics"));
                }
                advances.insert(
                    c,
                    advance + spacing[0] + if encoded == [32] { spacing[1] } else { 0.0 },
                );
            }
            let measure = |s: &str| -> Result<f32> {
                let advance = s
                    .chars()
                    .map(|c| {
                        advances
                            .get(&c)
                            .copied()
                            .ok_or_else(|| err("Missing paragraph glyph metrics"))
                    })
                    .sum::<Result<f32>>()?;
                Ok((advance - spacing[0]) * spacing[2] / 100.0 * first.matrix[0])
            };
            let lines = wrap(text, width, measure)?;
            let key = run.key.clone();
            // Deletion repairs subsequent text cursors, retaining every other object.
            self.edit(&mut candidate, &template, "")?;
            let mut output = parse(&candidate.bytes)?;
            let existing = output.get_page_content_with_limit(page, MAX_STREAM)?;
            let mut depth = 0i32;
            for op in Content::decode(&existing)?.operations {
                if op.operator == "q" {
                    depth += 1;
                } else if op.operator == "Q" {
                    depth -= 1;
                }
                if !(0..=128).contains(&depth) {
                    return Err(err("Unbalanced graphics state; use line editing"));
                }
            }
            if depth != 0 {
                return Err(err("Unbalanced graphics state; use line editing"));
            }
            let mut data = b"q\n".to_vec();
            data.extend(existing);
            data.extend(b"\nQ\n");
            let mut added = vec![];
            let mut expected = vec![];
            let visible = {
                let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                let page = native.pages().get(i32::from(first.page))?;
                let bounds = page.boundaries().bounding()?.bounds;
                [bounds.left().value, bounds.bottom().value, bounds.right().value, bounds.top().value]
            };
            if visible.iter().any(|n| !n.is_finite()) || visible[0] >= visible[2] || visible[1] >= visible[3] {
                return Err(err("Invalid visible page bounds"));
            }
            if width > visible[2] - visible[0]
                || p.bounds.left < visible[0]
                || p.bounds.left + width > visible[2]
            {
                return Err(err("Paragraph bounds exceed the page"));
            }
            for (i, l) in lines.iter().enumerate() {
                operation::checkpoint("Laying out paragraph", i, lines.len())?;
                if l.is_empty() {
                    continue;
                }
                let w = measure(l)?;
                let x = match p.alignment {
                    ParagraphAlignment::Right => p.bounds.left + width - w,
                    ParagraphAlignment::Center => p.bounds.left + (width - w) / 2.0,
                    _ => p.bounds.left,
                };
                let y = first.matrix[5] - i as f32 * p.line_spacing;
                if y + vertical.descent * first.matrix[3] < visible[1]
                    || y + vertical.ascent * first.matrix[3] > visible[3]
                {
                    return Err(err("Paragraph would extend beyond the page"));
                }
                added.extend([
                    Operation::new("q", vec![]),
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec![Object::Name(key.clone()), template.size.into()]),
                    Operation::new("Tc", vec![spacing[0].into()]),
                    Operation::new("Tw", vec![spacing[1].into()]),
                    Operation::new("Tz", vec![spacing[2].into()]),
                    Operation::new("Ts", vec![0.into()]),
                    Operation::new("Tr", vec![0.into()]),
                    Operation::new(
                        "rg",
                        first.color[..3]
                            .iter()
                            .map(|c| Object::Real(*c as f32 / 255.0))
                            .collect(),
                    ),
                    Operation::new(
                        "Tm",
                        vec![first.matrix[0].into(), 0.into(), 0.into(), first.matrix[3].into(), x.into(), y.into()],
                    ),
                ]);
                let justify = p.alignment == ParagraphAlignment::Justified
                    && i + 1 < lines.len()
                    && l.contains(' ');
                if justify {
                    let gaps = l.matches(' ').count();
                    let extra = (width - w) / gaps as f32;
                    let mut items = vec![];
                    let words: Vec<_> = l.split(' ').collect();
                    for (n, word) in words.iter().enumerate() {
                        let t = if n + 1 < words.len() {
                            format!("{word} ")
                        } else {
                            word.to_string()
                        };
                        items.push(Object::String(
                            content::encode_with(&encoding, &t)?,
                            lopdf::StringFormat::Hexadecimal,
                        ));
                        if n + 1 < words.len() {
                            items.push(Object::Real(
                                -extra / first.matrix[0] / template.size * 1000.0 * 100.0 / spacing[2],
                            ))
                        }
                    }
                    added.push(Operation::new("TJ", vec![Object::Array(items)]));
                } else {
                    added.push(Operation::new(
                        "Tj",
                        vec![Object::String(
                            content::encode_with(&encoding, l)?,
                            lopdf::StringFormat::Hexadecimal,
                        )],
                    ));
                }
                added.extend([Operation::new("ET", vec![]), Operation::new("Q", vec![])]);
                expected.push(l.clone());
            }
            data.extend(Content { operations: added }.encode()?);
            if data.len() > MAX_STREAM {
                return Err(err("Paragraph content exceeds the stream limit"));
            }
            let stream = output.add_object(Stream::new(Dictionary::new(), data));
            output
                .get_object_mut(page)?
                .as_dict_mut()?
                .set("Contents", stream);
            let final_doc = self.candidate(doc, output)?;
            let after = self.text_objects(&final_doc, first.page)?;
            let unaffected: Vec<_> = all
                .iter()
                .filter(|o| !p.objects.iter().any(|s| s.index == o.index))
                .collect();
            if after.len() != unaffected.len() + expected.len() {
                return Err(err("Paragraph failed object verification"));
            }
            for (a, b) in unaffected.iter().zip(&after) {
                if a.text != b.text
                    || a.font != b.font
                    || a.matrix != b.matrix
                    || a.color != b.color
                    || a.size != b.size
                {
                    return Err(err("Paragraph altered neighboring content"));
                }
            }
            for (a, b) in expected.iter().zip(&after[unaffected.len()..]) {
                if *a != b.text {
                    return Err(err("Paragraph failed text verification"));
                }
            }
            let intersects = |a: Bounds, b: Bounds| {
                a.left < b.right - 0.1 && a.right > b.left + 0.1
                    && a.bottom < b.top - 0.1 && a.top > b.bottom + 0.1
            };
            for neighbor in &unaffected {
                if !p.objects.iter().any(|old| intersects(old.bounds, neighbor.bounds))
                    && after[unaffected.len()..].iter().any(|new| intersects(new.bounds, neighbor.bounds))
                {
                    return Err(err("Not enough room for this text. Widen the box or shorten it."));
                }
            }
            if lines.len() > p.line_count {
                let warning = "Text uses more lines";
                result.warning = Some(
                    result
                        .warning
                        .map_or_else(|| warning.into(), |s| format!("{s}. {warning}")),
                );
            }
            result.objects_changed = p.objects.len();
            operation::checkpoint("Applying paragraph", 1, 1)?;
            doc.accept(final_doc.bytes);
            Ok(result)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    use std::path::PathBuf;
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }
    fn fixture(alignment: ParagraphAlignment) -> Vec<u8> {
        let mut pdf = lopdf::Document::with_version("1.7");
        let font = fonts::Font {
            bytes: fs::read(root().join("tests/fonts/Inter-Regular.ttf")).unwrap(),
            name: "Inter-Regular".into(),
            exact: true,
        };
        let strings = if alignment == ParagraphAlignment::Justified {
            vec![
                "Finnish ääkköset täällä",
                "Finnish ääkköset täällä",
                "Lyhyt loppu",
            ]
        } else {
            vec!["Finnish ääkköset täällä", "Tämä rivi", "Viimeinen rivi"]
        };
        let all = format!(
            "{} Muutos lyhyt pidempi teksti å ö ä € sivu untouched",
            strings.join(" ")
        );
        let font_ref = FontRef::new(&font.bytes).unwrap();
        let gm = font_ref.glyph_metrics(Size::new(12.0), LocationRef::default());
        let width = |s: &str| {
            s.chars()
                .map(|c| {
                    gm.advance_width(font_ref.charmap().map(c).unwrap())
                        .unwrap()
                })
                .sum::<f32>()
        };
        let widest = width(strings[0]);
        let f = fonts::embed(&mut pdf, &font, &all).unwrap();
        let pages = pdf.new_object_id();
        let mut ops = vec![];
        for (i, text) in strings.iter().enumerate() {
            let x = match alignment {
                ParagraphAlignment::Right => 60.0 + widest - width(text),
                ParagraphAlignment::Center => 60.0 + (widest - width(text)) / 2.0,
                _ => 60.0,
            };
            ops.extend([
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F".to_vec()), 12.into()]),
                Operation::new(
                    "Tm",
                    vec![
                        1.into(),
                        0.into(),
                        0.into(),
                        1.into(),
                        x.into(),
                        (250.0 - i as f32 * 16.0).into(),
                    ],
                ),
                Operation::new(
                    "Tj",
                    vec![Object::String(
                        f.encode(text).unwrap(),
                        lopdf::StringFormat::Hexadecimal,
                    )],
                ),
                Operation::new("ET", vec![]),
            ]);
        }
        ops.extend([
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(b"F".to_vec()), 12.into()]),
            Operation::new(
                "Tm",
                vec![
                    1.into(),
                    0.into(),
                    0.into(),
                    1.into(),
                    300.into(),
                    70.into(),
                ],
            ),
            Operation::new(
                "Tj",
                vec![Object::String(
                    f.encode("untouched").unwrap(),
                    lopdf::StringFormat::Hexadecimal,
                )],
            ),
            Operation::new("ET", vec![]),
        ]);
        let stream = pdf.add_object(Stream::new(
            Dictionary::new(),
            Content { operations: ops }.encode().unwrap(),
        ));
        let page=pdf.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),500.into(),300.into()],"Resources"=>dictionary!{"Font"=>dictionary!{"F"=>f.id}},"Contents"=>stream});
        pdf.objects.insert(
            pages,
            dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}.into(),
        );
        let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        pdf.trailer.set("Root", catalog);
        let mut bytes = vec![];
        pdf.save_to(&mut bytes).unwrap();
        bytes
    }
    fn engine() -> Engine {
        Engine::new(&root().join("target/debug/pdfium.dll")).unwrap()
    }
    #[test]
    fn writable_width_reflows_scaled_text_and_rejects_new_heading_overlap_atomically() {
        let e = engine();
        let original = e.open(&root().join("tests/fixtures/generated/paragraph-Left.pdf")).unwrap();
        for [sx, sy] in [[1., 1.], [0.75, 0.75], [1.3, 1.1]] {
            let mut pdf = parse(&original.bytes).unwrap();
            let page = pdf.get_pages()[&1];
            let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
            let mut index = 0;
            for op in &mut ops.operations {
                if op.operator == "Tm" {
                    op.operands[0] = Object::Real(sx);
                    op.operands[3] = Object::Real(sy);
                    if index == 3 {
                        op.operands[4] = 60.into();
                        op.operands[5] = 195.into();
                    }
                    index += 1;
                }
            }
            pdf.change_page_content(page, ops.encode().unwrap()).unwrap();
            let mut doc = e.candidate(&original, pdf).unwrap();
            let before = doc.bytes.clone();
            let image = e.render(&doc, 0, 1.).unwrap();
            let objects = e.text_objects(&doc, 0).unwrap();
            let mut selected = paragraphs(&objects).remove(0);
            assert_eq!(selected.line_count, 3);
            assert!(selected.fallback.is_none());
            assert!(selected.text.starts_with("Finnish ääkköset täällä Tämä rivi"));
            let text = "Muutos lyhyt pidempi teksti ".repeat(4).trim().to_owned();
            let history = doc.recovery_bytes().unwrap();
            let error = e.edit_paragraph(&mut doc, &selected, &text).unwrap_err().to_string();
            assert!(error.contains("Widen the box"), "{sx},{sy}: {error}");
            assert!(doc.bytes == before);
            assert!(doc.recovery_bytes().unwrap() == history);
            for invalid in [f32::NAN, f32::INFINITY, -1., 0., 500.] {
                selected.wrap_width = Some(invalid);
                assert!(e.edit_paragraph(&mut doc, &selected, &text).is_err());
                assert!(doc.bytes == before);
            }
            selected.wrap_width = Some(410.);
            e.edit_paragraph(&mut doc, &selected, &text).unwrap();
            let after = e.text_objects(&doc, 0).unwrap();
            assert_eq!(after.iter().filter(|o| o.text != "untouched").map(|o| o.text.as_str()).collect::<Vec<_>>().join(" "), text);
            for o in after.iter().filter(|o| o.text != "untouched") {
                assert!((o.size - 12.).abs() < 0.001);
                assert!((o.matrix[0] - sx).abs() < 0.001);
                assert!((o.matrix[3] - sy).abs() < 0.001);
            }
            let old = objects.iter().find(|o| o.text == "untouched").unwrap();
            let new = after.iter().find(|o| o.text == "untouched").unwrap();
            assert_eq!(old.matrix, new.matrix);
            assert_eq!(old.font, new.font);
            let rendered = e.render(&doc, 0, 1.).unwrap();
            let region = after.iter().filter(|o| o.text != "untouched").fold(selected.bounds, |b,o| union(b,o.bounds));
            for y in 0..image.height {
                for x in 0..image.width {
                    let py = image.height as f32 - y as f32;
                    if (x as f32) < region.left-4. || x as f32 > region.right+4. || py < region.bottom-4. || py > region.top+4. {
                        let i = ((y*image.width+x)*4) as usize;
                        assert_eq!(&image.rgba[i..i+4], &rendered.rgba[i..i+4]);
                    }
                }
            }
            let saved = root().join(format!("target/text-width-{sx}-{sy}.pdf"));
            if saved.exists() { fs::remove_file(&saved).unwrap(); }
            doc.save_as(&saved).unwrap();
            assert_eq!(e.render(&e.open(&saved).unwrap(),0,1.).unwrap().rgba,rendered.rgba);
            let edited = doc.bytes.clone();
            assert!(doc.undo());
            assert!(doc.bytes == before);
            assert!(doc.redo());
            assert!(doc.bytes == edited);
            fs::remove_file(saved).unwrap();
        }
        let mut doc = e.candidate(&original, parse(&original.bytes).unwrap()).unwrap();
        let mut p = paragraphs(&e.text_objects(&doc,0).unwrap()).remove(0);
        p.wrap_width = Some(410.);
        assert!(e.edit_paragraph(&mut doc,&p,&p.text).unwrap().objects_changed > 0);
        let p = paragraphs(&e.text_objects(&doc,0).unwrap()).remove(0);
        assert!(p.line_count < 3);
        e.edit_paragraph(&mut doc,&p,"Muutos\nlyhyt\npidempi").unwrap();
        assert_eq!(e.text_objects(&doc,0).unwrap().iter().filter(|o|o.text!="untouched").count(),3);
        assert_eq!(fs::read(root().join("tests/fixtures/generated/paragraph-Left.pdf")).unwrap(),original.bytes);
    }
    #[test]
    fn paragraph_edits_use_unrotated_visible_bounds_with_crop_and_origin_offsets() {
        let e = engine();
        let path = root().join("tests/fixtures/generated/paragraph-Left.pdf");
        let original = e.open(&path).unwrap();
        for [ox, oy] in [[0., 0.], [120., 180.]] {
            for crop in [false, true] {
                for rotation in [0, 90, 180, 270] {
                    let mut pdf = parse(&original.bytes).unwrap();
                    let page = pdf.get_pages()[&1];
                    let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
                    pdf.get_dictionary_mut(page).unwrap().remove(b"MediaBox");
                    pdf.get_dictionary_mut(page).unwrap().set("Rotate", rotation);
                    pdf.get_dictionary_mut(parent).unwrap().set("MediaBox", vec![Object::Real(ox), oy.into(), (500. + ox).into(), (300. + oy).into()]);
                    if crop {
                        pdf.get_dictionary_mut(parent).unwrap().set("CropBox", vec![Object::Real(230. + ox), (30. + oy).into(), (490. + ox).into(), (285. + oy).into()]);
                    }
                    let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
                    let mut line = 0;
                    for op in &mut ops.operations {
                        if op.operator == "Tm" {
                            op.operands[4] = Object::Real(op.operands[4].as_float().unwrap() + ox + if line < 3 { 220. } else { 0. });
                            op.operands[5] = Object::Real(op.operands[5].as_float().unwrap() + oy);
                            line += 1;
                        }
                    }
                    pdf.change_page_content(page, ops.encode().unwrap()).unwrap();
                    let mut doc = e.candidate(&original, pdf).unwrap();
                    let before = doc.bytes.clone();
                    let objects = e.text_objects(&doc, 0).unwrap();
                    let paragraph = paragraphs(&objects).remove(0);
                    let old = objects.iter().find(|o| o.text == "untouched").unwrap();
                    assert_eq!(paragraph.line_count, 3);
                    let history = doc.recovery_bytes().unwrap();
                    let overflow = "Muutos ä ö å €\n".repeat(30);
                    let error = e.edit_paragraph(&mut doc, &paragraph, &overflow).unwrap_err().to_string();
                    assert!(error.contains("beyond the page"), "{error}");
                    assert!(doc.bytes == before);
                    assert!(doc.recovery_bytes().unwrap() == history);
                    for edge in ["left", "top"] {
                        let mut invalid = parse(&before).unwrap();
                        let visible = if crop { [230. + ox, 30. + oy, 490. + ox, 285. + oy] } else { [ox, oy, 500. + ox, 300. + oy] };
                        let mut bounds = visible;
                        if edge == "left" { bounds[0] = paragraph.bounds.left + 4.; }
                        else { bounds[3] = paragraph.objects[0].matrix[5] + 2.; }
                        invalid.get_dictionary_mut(page).unwrap().set("CropBox", bounds.into_iter().map(Object::Real).collect::<Vec<_>>());
                        let mut invalid = e.candidate(&doc, invalid).unwrap();
                        let selected = paragraphs(&e.text_objects(&invalid, 0).unwrap()).remove(0);
                        let bytes = invalid.bytes.clone();
                        let history = invalid.recovery_bytes().unwrap();
                        assert!(e.edit_paragraph(&mut invalid, &selected, "Muutos ä ö å €").is_err(), "{edge}");
                        assert!(invalid.bytes == bytes);
                        assert!(invalid.recovery_bytes().unwrap() == history);
                    }
                    let before_image = e.render(&doc, 0, 1.).unwrap();
                    e.edit_paragraph(&mut doc, &paragraph, "Muutos ä ö å €").unwrap_or_else(|error|
                        panic!("rotation={rotation}, crop={crop}, origin={ox},{oy}: {error}"));
                    let after = e.text_objects(&doc, 0).unwrap();
                    let new = after.iter().find(|o| o.text == "untouched").unwrap();
                    assert_eq!(old.matrix, new.matrix);
                    assert_eq!(old.font, new.font);
                    let changed = after.iter().find(|o| o.text == "Muutos ä ö å €").unwrap();
                    assert!((changed.matrix[5] - paragraph.objects[0].matrix[5]).abs() < 0.001);
                    let image = e.render(&doc, 0, 1.).unwrap();
                    let region = after.iter().filter(|o| o.text != "untouched").fold(paragraph.bounds, |b,o| union(b,o.bounds));
                    let corners = [(region.left, region.bottom), (region.right, region.top)].map(|(x,y)| e.page_to_pixel(&doc, 0, 1., x, y).unwrap());
                    for y in 0..image.height as i32 {
                        for x in 0..image.width as i32 {
                            if x < corners[0].0.min(corners[1].0) - 4 || x > corners[0].0.max(corners[1].0) + 4
                                || y < corners[0].1.min(corners[1].1) - 4 || y > corners[0].1.max(corners[1].1) + 4 {
                                let i = (y as usize * image.width as usize + x as usize) * 4;
                                assert_eq!(&before_image.rgba[i..i+4], &image.rgba[i..i+4]);
                            }
                        }
                    }
                    let saved = root().join(format!("target/paragraph-bounds-{rotation}-{crop}-{ox}.pdf"));
                    if saved.exists() { fs::remove_file(&saved).unwrap(); }
                    doc.save_as(&saved).unwrap();
                    assert_eq!(e.render(&e.open(&saved).unwrap(), 0, 1.).unwrap().rgba, image.rgba);
                    let changed = doc.bytes.clone();
                    assert!(doc.undo());
                    assert!(doc.bytes == before);
                    assert!(doc.redo());
                    assert!(doc.bytes == changed);
                    fs::remove_file(saved).unwrap();
                }
            }
        }
        assert_eq!(fs::read(path).unwrap(), original.bytes);
    }
    #[test]
    fn paragraph_reflow_reuses_encoding_for_distinct_characters_and_lines() {
        let e = engine();
        let original = e.open(&root().join("tests/fixtures/generated/paragraph-Left.pdf")).unwrap();
        let before = e.text_objects(&original, 0).unwrap();
        let alphabet: String = (33u8..=126).map(char::from).collect();
        let text = alphabet.as_bytes().chunks(8).map(|word| std::str::from_utf8(word).unwrap()).collect::<Vec<_>>().join(" ");
        let mut pdf = parse(&original.bytes).unwrap();
        let page = pdf.get_pages()[&1];
        let font = fonts::Font {
            bytes: fs::read(root().join("tests/fonts/Inter-Regular.ttf")).unwrap(),
            name: "Inter-Regular".into(), exact: true,
        };
        let font = fonts::embed(&mut pdf, &font, &format!("{alphabet} {}", before.iter().map(|o| o.text.as_str()).collect::<Vec<_>>().join(" "))).unwrap();
        let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
        let mut index = 0;
        for op in &mut ops.operations {
            if op.operator == "Tj" {
                op.operands = vec![Object::String(font.encode(&before[index].text).unwrap(), lopdf::StringFormat::Hexadecimal)];
                index += 1;
            }
        }
        pdf.get_dictionary_mut(page).unwrap().set("Resources", dictionary! {"Font"=>dictionary! {"F"=>font.id}});
        pdf.change_page_content(page, ops.encode().unwrap()).unwrap();
        pdf.prune_objects();
        let mut doc = e.candidate(&original, pdf).unwrap();
        let source = doc.bytes.clone();
        let paragraph = paragraphs(&e.text_objects(&doc, 0).unwrap()).remove(0);
        content::ENCODING_BUILDS.with(|count| count.set(0));
        let started = std::time::Instant::now();
        e.edit_paragraph(&mut doc, &paragraph, &text).unwrap();
        let builds = content::ENCODING_BUILDS.with(|count| count.get());
        eprintln!("paragraph_encoder_builds={builds} elapsed_us={}", started.elapsed().as_micros());
        assert!(builds <= 24, "Repeated paragraph font decoding: {builds}");
        let after = e.text_objects(&doc, 0).unwrap();
        assert_eq!(after.iter().filter(|o| o.text != "untouched").map(|o| o.text.as_str()).collect::<Vec<_>>().join(" "), text);
        let old = before.iter().find(|o| o.text == "untouched").unwrap();
        let new = after.iter().find(|o| o.text == "untouched").unwrap();
        assert_eq!(old.font, new.font);
        assert_eq!(old.matrix, new.matrix);
        let saved = root().join("target/paragraph-encoding-saved.pdf");
        if saved.exists() { fs::remove_file(&saved).unwrap(); }
        doc.save_as(&saved).unwrap();
        assert_eq!(e.render(&e.open(&saved).unwrap(), 0, 1.).unwrap().rgba, e.render(&doc, 0, 1.).unwrap().rgba);
        let changed = doc.bytes.clone();
        assert!(doc.undo());
        assert!(doc.bytes == source);
        assert!(doc.redo());
        assert!(doc.bytes == changed);
        fs::remove_file(saved).unwrap();
        assert_eq!(fs::read(root().join("tests/fixtures/generated/paragraph-Left.pdf")).unwrap(), original.bytes);
    }
    #[test]
    fn standard_font_paragraphs_reflow_without_reformatting_neighbors() {
        let e = engine();
        let original = e.open(&root().join("tests/fixtures/generated/paragraph-Left.pdf")).unwrap();
        let source_text = e.text_objects(&original, 0).unwrap();
        for name in ["Helvetica", "Helvetica-Bold", "Times-Roman", "Times-Italic", "Courier", "Courier-Oblique"] {
          for single in [false, true] {
            let mut pdf = parse(&original.bytes).unwrap();
            let page = pdf.get_pages()[&1];
            let font = dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>name, "Encoding"=>"WinAnsiEncoding"};
            let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
            let mut index = 0;
            for op in &mut ops.operations {
                if op.operator == "Tj" {
                    let text = if single && (1..=2).contains(&index) { "" } else { &source_text[index].text };
                    op.operands = vec![Object::String(content::encode(&pdf, &font, text).unwrap(), lopdf::StringFormat::Hexadecimal)];
                    index += 1;
                }
            }
            let font_id = pdf.add_object(font);
            pdf.get_dictionary_mut(page).unwrap().set("Resources", dictionary! {"Font"=>dictionary! {"F"=>font_id}});
            pdf.change_page_content(page, ops.encode().unwrap()).unwrap();
            pdf.prune_objects();
            let mut doc = e.candidate(&original, pdf).unwrap();
            doc.substitution = Substitution::Document;
            let source = doc.bytes.clone();
            let before = e.text_objects(&doc, 0).unwrap();
            let paragraph = paragraphs(&before).remove(0);
            assert_eq!(paragraph.line_count, if single { 1 } else { 3 });
            assert!(paragraph.fallback.is_none());
            let before_image = e.render(&doc, 0, 1.).unwrap();
            let text = if single { paragraph.text.replacen(' ', "\n", 1) } else { "Muutos ä ö å €".into() };
            let result = e.edit_paragraph(&mut doc, &paragraph, &text).unwrap_or_else(|error| panic!("{name} single={single}: {error}"));
            assert!(result.objects_changed > 0);
            let after = e.text_objects(&doc, 0).unwrap();
            assert_eq!(after.iter().filter(|o| o.text != "untouched").map(|o| o.text.as_str()).collect::<Vec<_>>().join(" "), text.replace('\n', " "));
            if single { assert!(after.len() > 2); }
            let old = before.iter().find(|o| o.text == "untouched").unwrap();
            let new = after.iter().find(|o| o.text == "untouched").unwrap();
            assert_eq!(old.font, new.font);
            assert_eq!(old.matrix, new.matrix);
            assert_eq!(old.color, new.color);
            let region = after.iter().filter(|o| o.text != "untouched").fold(paragraph.bounds, |b,o| union(b,o.bounds));
            let image = e.render(&doc, 0, 1.).unwrap();
            for y in 0..image.height {
                for x in 0..image.width {
                    let (px,py) = (x as f32,image.height as f32-y as f32);
                    if px < region.left-4. || px > region.right+4. || py < region.bottom-4. || py > region.top+4. {
                        let i=((y*image.width+x)*4) as usize;
                        assert_eq!(&before_image.rgba[i..i+4], &image.rgba[i..i+4]);
                    }
                }
            }
            let saved = root().join(format!("target/standard-paragraph-{name}-{single}.pdf"));
            if saved.exists() { fs::remove_file(&saved).unwrap(); }
            doc.save_as(&saved).unwrap();
            assert_eq!(e.render(&e.open(&saved).unwrap(), 0, 1.).unwrap().rgba, e.render(&doc, 0, 1.).unwrap().rgba);
            let changed = doc.bytes.clone();
            assert!(doc.undo());
            assert!(doc.bytes == source);
            assert!(doc.redo());
            assert!(doc.bytes == changed);
            fs::remove_file(saved).unwrap();
          }
        }
        assert_eq!(fs::read(root().join("tests/fixtures/generated/paragraph-Left.pdf")).unwrap(), original.bytes);
    }
    #[test]
    #[ignore = "Regenerates committed paragraph fixtures"]
    fn generate_paragraph_fixtures() {
        for a in [
            ParagraphAlignment::Left,
            ParagraphAlignment::Right,
            ParagraphAlignment::Center,
            ParagraphAlignment::Justified,
        ] {
            fs::write(
                root().join(format!("tests/fixtures/generated/paragraph-{a:?}.pdf")),
                fixture(a),
            )
            .unwrap();
        }
    }
    #[test]
    fn paragraph_alignments_reflow_roundtrip_and_outside_diff() {
        let e = engine();
        for a in [
            ParagraphAlignment::Left,
            ParagraphAlignment::Right,
            ParagraphAlignment::Center,
            ParagraphAlignment::Justified,
        ] {
            let path = root().join(format!("tests/fixtures/generated/paragraph-{a:?}.pdf"));
            let mut doc = e.open(&path).unwrap();
            let objects = e.text_objects(&doc, 0).unwrap();
            let p = paragraphs(&objects).remove(0);
            assert_eq!(p.line_count, 3, "{a:?}: {p:?}");
            assert_eq!(p.alignment, a);
            let before = e.render(&doc, 0, 1.0).unwrap();
            let original = doc.bytes.clone();
            let result = e.edit_paragraph(&mut doc, &p, "Muutos ä ö å €").unwrap();
            assert!(result.warning.is_none());
            let after = e.render(&doc, 0, 1.0).unwrap();
            let padding = 4.0;
            for y in 0..before.height {
                for x in 0..before.width {
                    let (px, py) = (x as f32, before.height as f32 - y as f32);
                    if px < p.bounds.left - padding
                        || px > p.bounds.right + padding
                        || py < p.bounds.bottom - padding
                        || py > p.bounds.top + padding
                    {
                        let i = ((y * before.width + x) * 4) as usize;
                        assert_eq!(
                            &before.rgba[i..i + 4],
                            &after.rgba[i..i + 4],
                            "outside {a:?} {x} {y}"
                        );
                    }
                }
            }
            assert!(
                e.text_objects(&doc, 0)
                    .unwrap()
                    .iter()
                    .any(|o| o.text == "Muutos ä ö å €")
            );
            assert!(doc.undo());
            assert_eq!(doc.bytes, original);
            assert!(doc.redo());
            assert!(e.edit_paragraph(&mut doc, &p, "stale").is_err());
            doc.undo();
            let p = paragraphs(&e.text_objects(&doc, 0).unwrap()).remove(0);
            let long = "pidempi teksti ä ö å € ".repeat(12);
            let result = e.edit_paragraph(&mut doc, &p, long.trim()).unwrap();
            assert!(result.warning.as_deref().unwrap().contains("more lines"));
            let longer = e.render(&doc, 0, 1.0).unwrap();
            let after_objects = e.text_objects(&doc, 0).unwrap();
            let extended = after_objects
                .iter()
                .filter(|o| o.text != "untouched")
                .fold(p.bounds, |b, o| union(b, o.bounds));
            for y in 0..before.height {
                for x in 0..before.width {
                    let (px, py) = (x as f32, before.height as f32 - y as f32);
                    if px < extended.left - 4.0
                        || px > extended.right + 4.0
                        || py < extended.bottom - 4.0
                        || py > extended.top + 4.0
                    {
                        let i = ((y * before.width + x) * 4) as usize;
                        assert_eq!(
                            &before.rgba[i..i + 4],
                            &longer.rgba[i..i + 4],
                            "expanded outside {a:?} {x} {y}"
                        );
                    }
                }
            }
            assert!(after_objects.iter().all(|o| o.font == objects[0].font));
            let saved = root().join(format!(
                "target/tmp/paragraph-{a:?}-{}.pdf",
                std::process::id()
            ));
            if saved.exists() {
                fs::remove_file(&saved).unwrap();
            }
            fs::create_dir_all(saved.parent().unwrap()).unwrap();
            doc.save_as(&saved).unwrap();
            let reopened = e.open(&saved).unwrap();
            fs::remove_file(saved).unwrap();
            assert!(
                e.text_objects(&reopened, 0)
                    .unwrap()
                    .iter()
                    .any(|o| o.text == "untouched")
            );
        }
    }
    #[test]
    fn wrapping_bounds_and_breaks() {
        assert_eq!(
            wrap("aa bb cc", 5.0, |s| Ok(s.len() as f32)).unwrap(),
            ["aa bb", "cc"]
        );
        assert_eq!(
            wrap("aa\nbb", 5.0, |s| Ok(s.len() as f32)).unwrap(),
            ["aa", "bb"]
        );
        assert!(wrap("toolong", 2.0, |s| Ok(s.len() as f32)).is_err());
    }
    #[test]
    fn ambiguous_lines_do_not_become_paragraphs() {
        let e = engine();
        let path = root().join("tests/fixtures/generated/paragraph-Left.pdf");
        let doc = e.open(&path).unwrap();
        let mut objects = e.text_objects(&doc, 0).unwrap();
        objects[0].matrix[1] = 0.5;
        assert!(paragraphs(&objects)[0].fallback.is_some());
        objects[0].matrix[1] = 0.0;
        let mut other = objects[0].clone();
        other.font = "OtherFont".into();
        other.bounds.left = objects[0].bounds.right + 1.0;
        other.bounds.right = other.bounds.left + 20.0;
        other.index = 100;
        objects.push(other);
        assert!(
            paragraphs(&objects)[0]
                .fallback
                .as_deref()
                .unwrap()
                .contains("Mixed")
        );
    }
    #[test]
    fn separate_symbol_bullets_do_not_split_wrapped_prose_or_merge_list_items() {
        let e=engine();
        let doc=e.open(&root().join("tests/fixtures/generated/paragraph-Left.pdf")).unwrap();
        let mut objects=e.text_objects(&doc,0).unwrap();
        let mut bullet=objects[0].clone();
        bullet.text="•".into();bullet.font="Symbol".into();bullet.index=999;
        bullet.bounds.left=40.;bullet.bounds.right=45.;bullet.matrix[4]=40.;
        objects.push(bullet.clone());
        let p=paragraphs(&objects);
        let prose=p.iter().find(|p|p.line_count==3).unwrap();
        assert!(prose.fallback.is_none());
        assert!(!prose.text.contains('•'));
        bullet.index=1000;bullet.matrix[5]=objects[2].matrix[5];
        objects.push(bullet);
        let p=paragraphs(&objects);
        assert_eq!(p.iter().filter(|p|p.line_count==2).count(),1);
        assert!(!p.iter().any(|p|p.line_count==3));
        objects.iter_mut().find(|o|o.index==999).unwrap().matrix[1]=0.2;
        assert!(paragraphs(&objects).iter().find(|p|p.objects[0].index==999).unwrap().fallback.is_some());
    }
}
