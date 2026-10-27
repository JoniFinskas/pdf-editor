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
        && (o.matrix[0] - 1.0).abs() < 0.001
        && (o.matrix[3] - 1.0).abs() < 0.001
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
fn line(objects: Vec<TextObject>, fallback: Option<String>) -> Paragraph {
    let first = &objects[0];
    let mut text = String::new();
    let mut bounds = first.bounds;
    let mut end = first.bounds.left;
    for o in &objects {
        if !text.is_empty() && o.bounds.left - end > o.size * 0.15 && !text.ends_with(' ') {
            text.push(' ')
        }
        text.push_str(&o.text);
        end = o.bounds.right;
        bounds = union(bounds, o.bounds);
    }
    Paragraph {
        line_spacing: first.size * 1.2,
        line_count: 1,
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
        .filter(|o| !o.text.trim().is_empty())
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
                && (a.matrix[5] - o.matrix[5]).abs() < a.size * 0.15
                && horizontal(a)
                && horizontal(&o)
                && o.bounds.left - last.bounds.right < a.size * 1.5
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
            if dy < lines[i].objects[0].size * 0.2 {
                lines[i].fallback = Some("Unclear columns; editing this line only".into());
                lines[j].fallback = lines[i].fallback.clone();
            }
            if dy > lines[i].objects[0].size * 4.0 {
                break;
            }
        }
    }
    let mut groups: Vec<Vec<Paragraph>> = vec![];
    for l in lines {
        let joins = groups.last().is_some_and(|g| {
            let prev = &g[g.len() - 1];
            let first = &g[0];
            let spacing = prev.objects[0].matrix[5] - l.objects[0].matrix[5];
            let size = l.objects[0].size;
            let tol = size * 0.35;
            same(&first.objects[0], &l.objects[0])
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
    groups
        .into_iter()
        .map(|g| {
            let mut p = g[0].clone();
            if g.len() == 1 {
                return p;
            }
            p.line_count = g.len();
            p.line_spacing = g[0].objects[0].matrix[5] - g[1].objects[0].matrix[5];
            let tol = p.objects[0].size * 0.35;
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
            for l in &g[1..] {
                p.bounds = union(p.bounds, l.bounds);
                p.text.push('\n');
                p.text.push_str(&l.text);
                p.objects.extend(l.objects.clone());
            }
            p
        })
        .collect()
}

fn wrap(text: &str, width: f32, measure: impl Fn(&str) -> Result<f32>) -> Result<Vec<String>> {
    let mut lines = vec![];
    for hard in text.split('\n') {
        let mut current = String::new();
        for word in hard.split_whitespace() {
            if measure(word)? > width + 0.01 {
                return Err(err(
                    "A word exceeds the paragraph width; insert a space or use line editing",
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
            if text == p.text {
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
            let mut advances = std::collections::BTreeMap::new();
            for c in plain.chars() {
                if advances.contains_key(&c) {
                    continue;
                }
                let encoded = content::encode(&pdf, &run.font, &c.to_string())?;
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
                Ok((advance - spacing[0]) * spacing[2] / 100.0)
            };
            let width = p.bounds.right - p.bounds.left;
            if !width.is_finite() || width <= 0.0 {
                return Err(err("Invalid paragraph width"));
            }
            let lines = wrap(text, width, measure)?;
            let key = run.key.clone();
            let font_dict = run.font.clone();
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
            let info = self.pages(doc)?;
            if width > info[first.page as usize].width
                || p.bounds.left < 0.0
                || p.bounds.right > info[first.page as usize].width
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
                    ParagraphAlignment::Right => p.bounds.right - w,
                    ParagraphAlignment::Center => (p.bounds.left + p.bounds.right - w) / 2.0,
                    _ => p.bounds.left,
                };
                let y = first.matrix[5] - i as f32 * p.line_spacing;
                if y + vertical.descent < 0.0
                    || y + vertical.ascent > info[first.page as usize].height
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
                        vec![1.into(), 0.into(), 0.into(), 1.into(), x.into(), y.into()],
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
                            content::encode(&pdf, &font_dict, &t)?,
                            lopdf::StringFormat::Hexadecimal,
                        ));
                        if n + 1 < words.len() {
                            items.push(Object::Real(
                                -extra / template.size * 1000.0 * 100.0 / spacing[2],
                            ))
                        }
                    }
                    added.push(Operation::new("TJ", vec![Object::Array(items)]));
                } else {
                    added.push(Operation::new(
                        "Tj",
                        vec![Object::String(
                            content::encode(&pdf, &font_dict, l)?,
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
            if lines.len() > p.line_count {
                let warning = "Paragraph extended downward; content below may overlap";
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
            assert!(result.warning.as_deref().unwrap().contains("overlap"));
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
}
