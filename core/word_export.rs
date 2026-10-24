//! Best-effort semantic Word export; PDF coordinates are not Word flow layout.
use crate::office_package::{package, write_new, xml};
use crate::*;
fn run(text: &str, font: &str, size: f32) -> String {
    let name = fonts::base_name(font);
    let lower = name.to_lowercase();
    let style = format!(
        "{}{}",
        if lower.contains("bold") { "<w:b/>" } else { "" },
        if lower.contains("italic") || lower.contains("oblique") {
            "<w:i/>"
        } else {
            ""
        }
    );
    format!(
        "<w:r><w:rPr><w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/>{style}<w:sz w:val=\"{}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        xml(name),
        xml(name),
        (size.clamp(1., 400.) * 2.).round() as u32,
        xml(text)
    )
}
fn paragraph_xml(p: &Paragraph) -> String {
    let align = match p.alignment {
        ParagraphAlignment::Left => "left",
        ParagraphAlignment::Right => "right",
        ParagraphAlignment::Center => "center",
        ParagraphAlignment::Justified => "both",
    };
    let mut out = format!("<w:p><w:pPr><w:jc w:val=\"{align}\"/></w:pPr>");
    let mut previous: Option<&TextObject> = None;
    for o in &p.objects {
        if let Some(prev) = previous
            && !prev.text.ends_with(char::is_whitespace)
            && !o.text.starts_with(char::is_whitespace)
            && ((o.bounds.left - prev.bounds.right) > o.size * 0.15
                || (o.bounds.top - prev.bounds.top).abs() > o.size * 0.5)
        {
            out.push_str(&run(" ", &o.font, o.size));
        }
        out.push_str(&run(&o.text, &o.font, o.size));
        previous = Some(o);
    }
    out.push_str("</w:p>");
    out
}
fn table_xml(grid: &TableGrid, text: &[TextObject]) -> String {
    let mut out = String::from(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\"/><w:left w:val=\"single\" w:sz=\"4\"/><w:bottom w:val=\"single\" w:sz=\"4\"/><w:right w:val=\"single\" w:sz=\"4\"/><w:insideH w:val=\"single\" w:sz=\"4\"/><w:insideV w:val=\"single\" w:sz=\"4\"/></w:tblBorders></w:tblPr>",
    );
    out.push_str("<w:tblGrid>");
    for _ in &grid.cells[0] {
        out.push_str("<w:gridCol w:w=\"2400\"/>");
    }
    out.push_str("</w:tblGrid>");
    for row in &grid.cells {
        out.push_str("<w:tr>");
        for cell in row {
            out.push_str("<w:tc><w:p>");
            let source = text.iter().find(|t| cell.contains(&t.text));
            out.push_str(&run(
                cell,
                source.map_or("Calibri", |t| t.font.as_str()),
                source.map_or(11., |t| t.size),
            ));
            out.push_str("</w:p></w:tc>");
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    out
}
impl Engine {
    pub fn export_word(&self, doc: &Document, path: &Path) -> Result<()> {
        guarded(|| {
            // Expand ordinary groups in isolation so export never changes editor history.
            let original = doc;
            let mut raster_pages = std::collections::BTreeSet::new();
            let mut expanded = self.candidate(doc, parse(&doc.bytes)?)?;
            for page in 0..self.pages(&expanded)?.len() {
                let mut groups = 0usize;
                while let Some(item) = self
                    .page_items(&expanded, page as u16)?
                    .into_iter()
                    .find(|item| item.kind == "XObjectForm")
                {
                    groups += 1;
                    if groups > 20000 {
                        return Err(err("Word export contains too many groups"));
                    }
                    if self.prepare_grouped_text(&mut expanded, &item).is_err() {
                        // Preserve compositing instead of losing grouped content.
                        // Only this page falls back to an image; the source is untouched.
                        raster_pages.insert(page);
                        break;
                    }
                    expanded.undo.clear();
                    expanded.redo.clear();
                }
            }
            let doc = &expanded;
            let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let mut entries = vec![];
            let mut rels = String::from(
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            );
            let mut body = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>",
            );
            let mut image_id = 0u32;
            let mut budget = 0usize;
            for (page_index, page) in native.pages().iter().enumerate() {
                if page_index > 0 {
                    body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
                }
                if raster_pages.contains(&page_index) {
                    let info = self.pages(original)?[page_index].clone();
                    let scale = 2.0_f32.min(2048.0 / info.width.max(info.height));
                    let image = self.render(original, page_index as u16, scale)?;
                    let mut png = std::io::Cursor::new(vec![]);
                    image::write_buffer_with_format(&mut png, &image.rgba, image.width,
                        image.height, image::ExtendedColorType::Rgba8, image::ImageFormat::Png)
                        .map_err(|_| err("Word page encoding failed"))?;
                    budget += png.get_ref().len();
                    if budget + body.len() > MAX_FILE { return Err(err("Word export exceeds 64 MiB")); }
                    image_id += 1;
                    let filename = format!("image{image_id}.png");
                    entries.push((format!("word/media/{filename}"),png.into_inner()));
                    rels.push_str(&format!("<Relationship Id=\"rId{image_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{filename}\"/>"));
                    let fit = (451.0 / info.width).min(697.0 / info.height).min(1.0);
                    let cx = (info.width * fit * 12700.0) as u64;
                    let cy = (info.height * fit * 12700.0) as u64;
                    body.push_str(&format!("<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{image_id}\" name=\"Image {image_id}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{image_id}\" name=\"{filename}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rId{image_id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"));
                    continue;
                }
                let text = self.text_objects(doc, page_index as u16)?;
                if page.objects().len() > 20000 {
                    return Err(err("Word export page has too many objects"));
                }
                let mut segment_count = 0u32;
                let mut blocks: Vec<(f32, f32, String)> = vec![];
                // Only complete ruled grids become Word tables; ordinary aligned prose stays paragraphs.
                let mut vertical = vec![];
                let mut horizontal = vec![];
                for object in page.objects().iter() {
                    if let Some(p) = object.as_path_object() {
                        let segments = p.segments().transform(p.matrix()?);
                        segment_count = segment_count.saturating_add(segments.len());
                        if segment_count > 100000 {
                            return Err(err("Word export page has too many path segments"));
                        }
                        let mut prev: Option<(f32, f32)> = None;
                        for s in segments.iter() {
                            let point = (s.x().value, s.y().value);
                            if let Some(a) = prev
                                && s.segment_type() == PdfPathSegmentType::LineTo
                            {
                                if (a.0 - point.0).abs() < 1. && (a.1 - point.1).abs() > 10. {
                                    vertical.push((a.0, a.1.min(point.1), a.1.max(point.1)));
                                }
                                if (a.1 - point.1).abs() < 1. && (a.0 - point.0).abs() > 10. {
                                    horizontal.push((a.1, a.0.min(point.0), a.0.max(point.0)));
                                }
                            }
                            prev = Some(point);
                        }
                    }
                }
                let table_area = if vertical.len() >= 3 && horizontal.len() >= 3 {
                    Some(Bounds {
                        left: vertical.iter().map(|v| v.0).fold(f32::INFINITY, f32::min),
                        right: vertical
                            .iter()
                            .map(|v| v.0)
                            .fold(f32::NEG_INFINITY, f32::max),
                        bottom: horizontal.iter().map(|v| v.0).fold(f32::INFINITY, f32::min),
                        top: horizontal
                            .iter()
                            .map(|v| v.0)
                            .fold(f32::NEG_INFINITY, f32::max),
                    })
                } else {
                    None
                };
                let mut consumed = None;
                if let Some(area) = table_area
                    && let Ok(grid) = self.detect_table(doc, page_index as u16, area)
                    && grid.cells.len() >= 2
                    && grid.cells[0].len() >= 2
                {
                    blocks.push((area.top, area.left, table_xml(&grid, &text)));
                    consumed = Some(area);
                }
                let prose = text
                    .into_iter()
                    .filter(|t| {
                        !consumed.is_some_and(|a| {
                            a.contains(
                                (t.bounds.left + t.bounds.right) / 2.,
                                (t.bounds.top + t.bounds.bottom) / 2.,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                for p in paragraphs(&prose) {
                    blocks.push((p.bounds.top, p.bounds.left, paragraph_xml(&p)));
                }
                for object in page.objects().iter() {
                    if let Some(image) = object.as_image_object() {
                        let bounds = image.bounds()?;
                        let width = (bounds.width().value.abs() * 2.).clamp(1., 2048.) as i32;
                        let height = (bounds.height().value.abs() * 2.).clamp(1., 2048.) as i32;
                        let bitmap =
                            image.get_processed_bitmap_with_size(&native, width, height)?;
                        let rgba = bitmap.as_rgba_bytes();
                        let mut png = std::io::Cursor::new(vec![]);
                        image::write_buffer_with_format(
                            &mut png,
                            &rgba,
                            bitmap.width() as u32,
                            bitmap.height() as u32,
                            image::ExtendedColorType::Rgba8,
                            image::ImageFormat::Png,
                        )
                        .map_err(|_| err("Word image encoding failed"))?;
                        budget += png.get_ref().len();
                        if budget > MAX_FILE {
                            return Err(err("Word export exceeds 64 MiB"));
                        }
                        image_id += 1;
                        let filename = format!("image{image_id}.png");
                        entries.push((format!("word/media/{filename}"), png.into_inner()));
                        rels.push_str(&format!("<Relationship Id=\"rId{image_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{filename}\"/>"));
                        let cx = (bounds.width().value.abs().clamp(1., 10000.) * 12700.) as u64;
                        let cy = (bounds.height().value.abs().clamp(1., 10000.) * 12700.) as u64;
                        blocks.push((bounds.top().value,bounds.left().value,format!("<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{image_id}\" name=\"Image {image_id}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{image_id}\" name=\"{filename}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rId{image_id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>")));
                    }
                }
                blocks.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.total_cmp(&b.1)));
                for (_, _, block) in blocks {
                    body.push_str(&block);
                }
                if body.len() + budget > MAX_FILE {
                    return Err(err("Word export exceeds 64 MiB"));
                }
            }
            body.push_str("<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\"/></w:sectPr></w:body></w:document>");
            rels.push_str("</Relationships>");
            entries.extend([
    ("[Content_Types].xml".into(),br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec()),
    ("_rels/.rels".into(),br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec()),
    ("word/_rels/document.xml.rels".into(),rels.into_bytes()),("word/document.xml".into(),body.into_bytes())]);
            write_new(path, &package(&entries)?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{
        Object, Stream,
        content::{Content, Operation},
        dictionary,
    };
    fn generated_word_fixture() -> Vec<u8> {
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages = pdf.new_object_id();
        let font = pdf.add_object(
            dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica-BoldOblique"},
        );
        let mut ops = vec![];
        for (text, x, y) in [
            ("Title", 50, 280),
            ("A1", 50, 220),
            ("B1", 250, 220),
            ("A2", 50, 150),
            ("B2", 250, 150),
        ] {
            ops.extend([
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 15.into()]),
                Operation::new("Td", vec![x.into(), y.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ]);
        }
        for x in [20, 220, 440] {
            ops.extend([
                Operation::new("m", vec![x.into(), 100.into()]),
                Operation::new("l", vec![x.into(), 260.into()]),
                Operation::new("S", vec![]),
            ]);
        }
        for y in [100, 180, 260] {
            ops.extend([
                Operation::new("m", vec![20.into(), y.into()]),
                Operation::new("l", vec![440.into(), y.into()]),
                Operation::new("S", vec![]),
            ]);
        }
        let contents = pdf.add_object(Stream::new(
            lopdf::Dictionary::new(),
            Content { operations: ops }.encode().unwrap(),
        ));
        let page=pdf.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),500.into(),300.into()],"Contents"=>contents,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}});
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
    #[test]
    fn word_export_retains_text_style_table_and_image() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let folder = root.join("target/test-output");
        fs::create_dir_all(&folder).unwrap();
        let source = folder.join(format!("{}-word-source.pdf", std::process::id()));
        fs::write(&source, generated_word_fixture()).unwrap();
        let mut doc = engine.open(&source).unwrap();
        engine
            .place_picture(
                &mut doc,
                &Picture {
                    width: 2,
                    height: 2,
                    rgba: vec![255; 16],
                },
                Placement {
                    page: 0,
                    left: 20.,
                    bottom: 20.,
                    width: 30.,
                    height: 30.,
                },
            )
            .unwrap();
        let original = doc.bytes.clone();
        let path = folder.join(format!("{}-word.docx", std::process::id()));
        let _ = fs::remove_file(&path);
        engine.export_word(&doc, &path).unwrap();
        assert_eq!(doc.bytes, original);
        assert!(engine.export_word(&doc, &path).is_err());
        let bytes = fs::read(path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for expected in [
            "Title",
            "A1",
            "B1",
            "A2",
            "B2",
            "<w:tbl>",
            "<w:b/>",
            "<w:i/>",
            "w:val=\"30\"",
            "word/media/image1.png",
        ] {
            assert!(text.contains(expected), "missing {expected}");
        }
    }
    #[test]
    fn word_export_expands_groups_and_preserves_complex_pages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let folder = root.join("target/test-output");
        fs::create_dir_all(&folder).unwrap();
        let mut pdf = parse(&generated_word_fixture()).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let resources = pdf
            .get_dictionary(page)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .clone();
        let form=pdf.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","FormType"=>1,"BBox"=>vec![0.into(),0.into(),500.into(),300.into()],"Resources"=>resources},pdf.get_page_content(page)));
        pdf.get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "Resources",
                dictionary! {"XObject"=>dictionary!{"Fm"=>form}},
            );
        pdf.change_page_content(page, b"q /Fm Do Q".to_vec())
            .unwrap();
        let source = folder.join(format!("{}-word-group.pdf", std::process::id()));
        pdf.save(&source).unwrap();
        let doc = engine.open(&source).unwrap();
        let path = folder.join(format!("{}-word-group.docx", std::process::id()));
        let _ = fs::remove_file(&path);
        let original = doc.bytes.clone();
        let history = (doc.undo.clone(), doc.redo.clone(), doc.revision);
        engine.export_word(&doc, &path).unwrap();
        assert_eq!(doc.bytes, original);
        assert_eq!((doc.undo.clone(), doc.redo.clone(), doc.revision), history);
        let bytes = fs::read(&path).unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("Title"));
        pdf.get_object_mut(form)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("Group", dictionary! {"S"=>"Transparency"});
        pdf.save(&source).unwrap();
        let unsupported = engine.open(&source).unwrap();
        let rejected = folder.join(format!("{}-word-transparency.docx", std::process::id()));
        let _ = fs::remove_file(&rejected);
        engine.export_word(&unsupported, &rejected).unwrap();
        assert!(String::from_utf8_lossy(&fs::read(&rejected).unwrap()).contains("Title"));
        // Knockout compositing cannot be expanded safely, so preserve this page as an image.
        pdf.get_object_mut(form).unwrap().as_stream_mut().unwrap().dict
            .set("Group", dictionary! {"S"=>"Transparency","K"=>true});
        pdf.save(&source).unwrap();
        let complex = engine.open(&source).unwrap();
        let original = complex.bytes.clone();
        fs::remove_file(&rejected).unwrap();
        engine.export_word(&complex, &rejected).unwrap();
        let output=fs::read(&rejected).unwrap();
        assert!(String::from_utf8_lossy(&output).contains("word/media/image1.png"));
        assert!(String::from_utf8_lossy(&output).contains("<w:drawing>"));
        assert_eq!(complex.bytes, original);
        assert!(complex.undo.is_empty());
    }
    #[test]
    fn word_export_preserves_finnish_text_on_rotated_pages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let folder = root.join("target/test-output");
        fs::create_dir_all(&folder).unwrap();
        for name in ["unicode-cid", "rotated"] {
            let doc = engine
                .open(&root.join(format!("tests/fixtures/generated/{name}.pdf")))
                .unwrap();
            let path = folder.join(format!("{}-{name}-word.docx", std::process::id()));
            let _ = fs::remove_file(&path);
            engine.export_word(&doc, &path).unwrap();
            let bytes = fs::read(&path).unwrap();
            assert!(String::from_utf8_lossy(&bytes).contains("äöå€ AB"));
        }
    }
    #[test]
    fn word_runs_escape_xml_and_keep_styles() {
        let out = run("<&\u{0}", "ABCDEF+Example-BoldItalic", 12.5);
        assert!(out.contains("&lt;&amp;"));
        assert!(out.contains("<w:b/><w:i/>"));
        assert!(out.contains("w:val=\"25\""));
        assert!(!out.contains("ABCDEF+"));
    }
}
