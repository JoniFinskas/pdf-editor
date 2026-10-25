use super::*;

const PAYLOAD_MARKERS: &[&[u8]] = &[
    b"WORKFLOW_INFO_MARKER",
    b"WORKFLOW_XMP_MARKER",
    b"WORKFLOW_ATTACHMENT_MARKER",
    b"WORKFLOW_ACTION_MARKER",
];

fn appearance(pdf: &mut lopdf::Document, color: [f32; 3]) -> lopdf::ObjectId {
    pdf.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Form", "FormType"=>1,
            "BBox"=>vec![0.into(),0.into(),40.into(),40.into()],
            "Resources"=>lopdf::Dictionary::new(),
        },
        format!(
            "q {} {} {} rg 0 0 40 40 re f Q",
            color[0], color[1], color[2]
        )
        .into_bytes(),
    ))
}

fn workflow_fixture(with_appearances: bool) -> Vec<u8> {
    let mut pdf = parse(&generated("BEFORE_EDIT", false, 0, 0., false, false)).unwrap();
    let page = pdf.get_pages()[&1];
    let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let fonts = pdf.get_page_fonts(page).unwrap();
    let font = fonts.get(b"F1".as_slice()).unwrap();
    let hidden = content::encode(&pdf, font, "BEFORE_EDIT").unwrap();
    let mut contents = pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap();
    contents.extend(
        Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                Operation::new("Tr", vec![3.into()]),
                Operation::new(
                    "Tm",
                    vec![
                        1.into(),
                        0.into(),
                        0.into(),
                        1.into(),
                        30.into(),
                        140.into(),
                    ],
                ),
                Operation::new(
                    "Tj",
                    vec![Object::String(hidden, lopdf::StringFormat::Hexadecimal)],
                ),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .unwrap(),
    );
    pdf.change_page_content(page, contents).unwrap();
    let info = pdf.add_object(dictionary! {
        "Author"=>Object::string_literal("WORKFLOW_INFO_MARKER"),
        "Creator"=>Object::string_literal("Fixture creator"),
        "Producer"=>Object::string_literal("Fixture producer"),
        "ModDate"=>Object::string_literal("D:20200101000000Z"),
    });
    pdf.trailer.set("Info", info);
    let xmp = pdf.add_object(Stream::new(
        dictionary! {"Type"=>"Metadata","Subtype"=>"XML"},
        b"WORKFLOW_XMP_MARKER".to_vec(),
    ));
    let payload = pdf.add_object(Stream::new(
        dictionary! {"Type"=>"EmbeddedFile"},
        b"WORKFLOW_ATTACHMENT_MARKER".to_vec(),
    ));
    let filespec = pdf.add_object(
        dictionary! {"Type"=>"Filespec","F"=>Object::string_literal("fixture.txt"),
        "EF"=>dictionary! {"F"=>payload}},
    );
    let action = pdf.add_object(
        dictionary! {"S"=>"JavaScript","JS"=>Object::string_literal("WORKFLOW_ACTION_MARKER")},
    );
    let outline = pdf.add_object(dictionary! {"Type"=>"Outlines","Count"=>0});
    let catalog = pdf.get_dictionary_mut(root).unwrap();
    catalog.set("Metadata", xmp);
    catalog.set("Names",dictionary! {"EmbeddedFiles"=>dictionary! {"Names"=>vec![Object::string_literal("fixture"),filespec.into()]}});
    catalog.set("OpenAction", action);
    catalog.set("Outlines", outline);
    let mut comment = dictionary! {"Type"=>"Annot","Subtype"=>"Square","P"=>page,"F"=>4,
    "Rect"=>vec![20.into(),20.into(),60.into(),60.into()],
    "Contents"=>Object::string_literal("WORKFLOW_COMMENT_MARKER"),
    "T"=>Object::string_literal("Fixture annotation author")};
    let mut link = dictionary! {"Type"=>"Annot","Subtype"=>"Link","P"=>page,"F"=>4,
    "Rect"=>vec![80.into(),20.into(),120.into(),60.into()],
    "Border"=>vec![0.into(),0.into(),0.into()],
    "A"=>dictionary! {"S"=>"URI","URI"=>Object::string_literal("https://example.invalid/fixture")}};
    if with_appearances {
        comment.set("AP", dictionary! {"N"=>appearance(&mut pdf,[1.,0.,0.])});
        link.set("AP", dictionary! {"N"=>appearance(&mut pdf,[0.,0.,1.])});
    }
    let comment = pdf.add_object(comment);
    let link = pdf.add_object(link);
    let mut annotations = vec![comment.into(), link.into()];
    if with_appearances {
        let normal = appearance(&mut pdf, [0., 1., 0.]);
        let widget=pdf.add_object(dictionary! {"Type"=>"Annot","Subtype"=>"Widget","FT"=>"Tx","P"=>page,
            "T"=>Object::string_literal("WORKFLOW_FIELD_MARKER"),"V"=>Object::string_literal("fixture"),
            "F"=>4,"Rect"=>vec![140.into(),20.into(),180.into(),60.into()],
            "AP"=>dictionary! {"N"=>normal}});
        annotations.push(widget.into());
        pdf.get_dictionary_mut(root)
            .unwrap()
            .set("AcroForm", dictionary! {"Fields"=>vec![widget.into()]});
    }
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", annotations);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

fn no_payload_markers(pdf: &lopdf::Document) {
    fn inspect(object: &Object) {
        match object {
            Object::String(bytes, _) => {
                for marker in PAYLOAD_MARKERS {
                    assert!(!bytes.windows(marker.len()).any(|part| part == *marker));
                }
            }
            Object::Array(values) => {
                for value in values {
                    inspect(value);
                }
            }
            Object::Dictionary(dict) => {
                for (_, value) in dict.iter() {
                    inspect(value);
                }
            }
            Object::Stream(stream) => {
                inspect(&Object::Dictionary(stream.dict.clone()));
                let data = if stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|name| name == b"Image")
                {
                    stream.content.clone()
                } else {
                    stream.get_plain_content_with_limit(MAX_STREAM).unwrap()
                };
                for marker in PAYLOAD_MARKERS {
                    assert!(!data.windows(marker.len()).any(|part| part == *marker));
                }
            }
            _ => {}
        }
    }
    inspect(&Object::Dictionary(pdf.trailer.clone()));
    for object in pdf.objects.values() {
        inspect(object);
    }
}

fn assert_rebuilt_structure(pdf: &lopdf::Document) {
    fn inspect(object: &Object) {
        match object {
            Object::Array(values) => {
                for value in values {
                    inspect(value);
                }
            }
            Object::Dictionary(dict) => {
                for key in [
                    b"Info".as_slice(),
                    b"Metadata",
                    b"PieceInfo",
                    b"LastModified",
                    b"CreationDate",
                    b"ModDate",
                    b"Author",
                    b"Creator",
                    b"Producer",
                    b"ID",
                    b"Prev",
                    b"EmbeddedFiles",
                    b"AF",
                    b"EF",
                    b"JavaScript",
                    b"JS",
                    b"OpenAction",
                    b"AA",
                    b"AcroForm",
                    b"Annots",
                    b"Outlines",
                    b"StructTreeRoot",
                    b"Font",
                    b"FontDescriptor",
                    b"ToUnicode",
                ] {
                    assert!(!dict.has(key), "Unexpected dictionary entry in rebuilt PDF");
                }
                assert!(
                    !dict
                        .get(b"Type")
                        .and_then(Object::as_name)
                        .is_ok_and(|v| matches!(
                            v,
                            b"Font"
                                | b"FontDescriptor"
                                | b"Metadata"
                                | b"EmbeddedFile"
                                | b"Filespec"
                                | b"Action"
                                | b"Annot"
                        ))
                );
                for (_, value) in dict.iter() {
                    inspect(value);
                }
            }
            Object::Stream(stream) => inspect(&Object::Dictionary(stream.dict.clone())),
            _ => {}
        }
    }
    inspect(&Object::Dictionary(pdf.trailer.clone()));
    for object in pdf.objects.values() {
        inspect(object);
    }
    no_payload_markers(pdf);
}

fn assert_render_close(expected: &Image, actual: &Image) {
    assert_eq!(
        (expected.width, expected.height),
        (actual.width, actual.height)
    );
    let difference: u64 = expected
        .rgba
        .iter()
        .zip(&actual.rgba)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    // Rebuild uses JPEG quality 95, so allow compression noise rather than
    // requiring byte-identical pixels around antialiased text edges.
    assert!(difference as f64 / (expected.rgba.len() as f64) < 3.0);
}

fn assert_visible_regions(
    engine: &Engine,
    source: &Document,
    expected: &Image,
    actual: &Image,
    edited: Bounds,
) {
    // Local checks make the annotation switches and visible edit significant
    // even when their rectangles occupy a small part of the page.
    for (x, y) in [(40., 40.), (100., 40.), (160., 40.)] {
        let (x, y) = engine.page_to_pixel(source, 0, 2., x, y).unwrap();
        let at = (y as usize * expected.width as usize + x as usize) * 4;
        assert!(
            expected.rgba[at..at + 3]
                .iter()
                .zip(&actual.rgba[at..at + 3])
                .all(|(a, b)| a.abs_diff(*b) <= 8)
        );
    }
    let a = engine
        .page_to_pixel(source, 0, 2., edited.left - 2., edited.bottom - 2.)
        .unwrap();
    let b = engine
        .page_to_pixel(source, 0, 2., edited.right + 2., edited.top + 2.)
        .unwrap();
    let mut difference = 0_u64;
    let mut channels = 0_usize;
    for y in a.1.min(b.1).max(0)..a.1.max(b.1).min(expected.height as i32) {
        for x in a.0.min(b.0).max(0)..a.0.max(b.0).min(expected.width as i32) {
            let at = (y as usize * expected.width as usize + x as usize) * 4;
            for c in 0..3 {
                difference += u64::from(expected.rgba[at + c].abs_diff(actual.rgba[at + c]));
                channels += 1;
            }
        }
    }
    assert!(channels > 0 && difference as f64 / (channels as f64) < 8.0);
}

#[test]
fn rebuilt_sanitize_option_matrix_saves_visible_edits_without_payloads_or_history() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for remove_comments in [false, true] {
        for remove_links in [false, true] {
            let input = temp(&format!(
                "sanitize-workflow-rebuild-{remove_comments}-{remove_links}.pdf"
            ));
            let output = temp(&format!(
                "sanitize-workflow-rebuild-saved-{remove_comments}-{remove_links}.pdf"
            ));
            fs::write(&input, workflow_fixture(true)).unwrap();
            let mut doc = e.open(&input).unwrap();
            let original = doc.bytes.clone();
            let original_render = e.render(&doc, 0, 2.).unwrap();
            let target = e.text_objects(&doc, 0).unwrap().remove(0);
            e.edit(&mut doc, &target, "VISIBLE_EDIT").unwrap();
            let edited = doc.bytes.clone();
            let edited_bounds = e.text_objects(&doc, 0).unwrap()[0].bounds;
            let mut expected_pdf = parse(&edited).unwrap();
            let page = expected_pdf.get_pages()[&1];
            let annotations = expected_pdf
                .get_dictionary(page)
                .unwrap()
                .get(b"Annots")
                .unwrap()
                .as_array()
                .unwrap()
                .clone();
            let kept: Vec<_> = annotations
                .into_iter()
                .filter(|value| {
                    let dict = expected_pdf
                        .dereference(value)
                        .unwrap()
                        .1
                        .as_dict()
                        .unwrap();
                    match dict.get(b"Subtype").unwrap().as_name().unwrap() {
                        b"Widget" => true,
                        b"Link" => !remove_links,
                        _ => !remove_comments,
                    }
                })
                .collect();
            expected_pdf
                .get_dictionary_mut(page)
                .unwrap()
                .set("Annots", kept);
            let expected = e.candidate(&doc, expected_pdf).unwrap();
            let expected_render = e.render(&expected, 0, 2.).unwrap();
            assert_ne!(original_render.rgba, expected_render.rgba);
            let report = e
                .sanitize(
                    &mut doc,
                    SanitizeOptions {
                        flatten_visible: true,
                        remove_comments,
                        remove_links,
                    },
                )
                .unwrap();
            assert_eq!(report.rasterized_pages, 1);
            let rebuilt = doc.bytes.clone();
            assert_rebuilt_structure(&parse(&rebuilt).unwrap());
            assert_eq!(
                rebuilt.windows(5).filter(|part| *part == b"%%EOF").count(),
                1
            );
            assert!(e.page_text_characters(&doc, 0).unwrap().is_empty());
            assert!(e.form_fields(&doc).unwrap().is_empty());
            let actual_render = e.render(&doc, 0, 2.).unwrap();
            assert_render_close(&expected_render, &actual_render);
            assert_visible_regions(
                &e,
                &expected,
                &expected_render,
                &actual_render,
                edited_bounds,
            );
            let _ = fs::remove_file(&output);
            doc.save_as(&output).unwrap();
            let reopened = e.open(&output).unwrap();
            assert!(!reopened.can_undo());
            assert_rebuilt_structure(&parse(&reopened.bytes).unwrap());
            assert!(e.page_text_characters(&reopened, 0).unwrap().is_empty());
            let reopened_render = e.render(&reopened, 0, 2.).unwrap();
            assert_render_close(&expected_render, &reopened_render);
            assert_visible_regions(
                &e,
                &expected,
                &expected_render,
                &reopened_render,
                edited_bounds,
            );
            assert!(doc.undo());
            assert_eq!(doc.bytes, edited);
            assert!(doc.undo());
            assert_eq!(doc.bytes, original);
            assert!(doc.redo());
            assert_eq!(doc.bytes, edited);
            assert!(doc.redo());
            assert_eq!(doc.bytes, rebuilt);
            assert_rebuilt_structure(&parse(&fs::read(&output).unwrap()).unwrap());
            fs::remove_file(input).unwrap();
            fs::remove_file(output).unwrap();
        }
    }
}

#[test]
fn ordinary_sanitize_option_matrix_persists_removals_and_keeps_editability() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for remove_comments in [false, true] {
        for remove_links in [false, true] {
            let input = temp(&format!(
                "sanitize-workflow-ordinary-{remove_comments}-{remove_links}.pdf"
            ));
            let output = temp(&format!(
                "sanitize-workflow-ordinary-saved-{remove_comments}-{remove_links}.pdf"
            ));
            fs::write(&input, workflow_fixture(false)).unwrap();
            let mut doc = e.open(&input).unwrap();
            let target = e.text_objects(&doc, 0).unwrap().remove(0);
            e.edit(&mut doc, &target, "VISIBLE_EDIT").unwrap();
            let edited = doc.bytes.clone();
            let report = e
                .sanitize(
                    &mut doc,
                    SanitizeOptions {
                        flatten_visible: false,
                        remove_comments,
                        remove_links,
                    },
                )
                .unwrap();
            assert_eq!(report.hidden_text_blocks, 1);
            assert_eq!(report.comments, usize::from(remove_comments));
            assert_eq!(report.links, usize::from(remove_links));
            let cleaned = doc.bytes.clone();
            let _ = fs::remove_file(&output);
            doc.save_as(&output).unwrap();
            let reopened = e.open(&output).unwrap();
            assert!(!reopened.can_undo());
            let text = e
                .page_text_characters(&reopened, 0)
                .unwrap()
                .into_iter()
                .map(|c| c.text)
                .collect::<String>();
            assert!(text.contains("VISIBLE_EDIT"));
            assert!(!text.contains("BEFORE_EDIT"));
            let pdf = parse(&reopened.bytes).unwrap();
            assert!(!pdf.trailer.has(b"Info"));
            let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
            let catalog = pdf.get_dictionary(root).unwrap();
            assert!(!catalog.has(b"Metadata"));
            assert!(!catalog.has(b"OpenAction"));
            no_payload_markers(&pdf);
            let page = pdf.get_pages()[&1];
            let annots = pdf
                .get_dictionary(page)
                .unwrap()
                .get(b"Annots")
                .unwrap()
                .as_array()
                .unwrap();
            assert_eq!(
                annots.len(),
                usize::from(!remove_comments) + usize::from(!remove_links)
            );
            for value in annots {
                let annotation = pdf.dereference(value).unwrap().1.as_dict().unwrap();
                if annotation.get(b"Subtype").unwrap().as_name().unwrap() == b"Link" {
                    let action = annotation.get(b"A").unwrap().as_dict().unwrap();
                    assert_eq!(action.len(), 2);
                    assert_eq!(action.get(b"S").unwrap().as_name().unwrap(), b"URI");
                }
            }
            assert!(doc.undo());
            assert_eq!(doc.bytes, edited);
            assert!(doc.redo());
            assert_eq!(doc.bytes, cleaned);
            fs::remove_file(input).unwrap();
            fs::remove_file(output).unwrap();
        }
    }
}

#[test]
fn verified_sanitized_export_reopens_both_modes_without_changing_editor_or_source() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for flatten_visible in [false, true] {
        for remove_comments in [false, true] {
            for remove_links in [false, true] {
                let input = temp(&format!(
                    "verify-input-{flatten_visible}-{remove_comments}-{remove_links}.pdf"
                ));
                let output = temp(&format!(
                    "verify-output-{flatten_visible}-{remove_comments}-{remove_links}.pdf"
                ));
                let _ = fs::remove_file(&output);
                fs::write(&input, workflow_fixture(flatten_visible)).unwrap();
                let mut doc = e.open(&input).unwrap();
                let target = e.text_objects(&doc, 0).unwrap().remove(0);
                e.edit(&mut doc, &target, "VISIBLE_EDIT").unwrap();
                let before = doc.bytes.clone();
                let revision = doc.revision();
                let options = SanitizeOptions {
                    flatten_visible,
                    remove_comments,
                    remove_links,
                };
                assert!(e.verify_sanitized(&doc, options).is_err());
                e.export_sanitized(&doc, options, &output).unwrap();
                assert_eq!(doc.bytes, before);
                assert_eq!(doc.revision(), revision);
                assert!(doc.can_undo());
                assert_eq!(fs::read(&input).unwrap(), workflow_fixture(flatten_visible));
                let saved = e.open(&output).unwrap();
                e.verify_sanitized(&saved, options).unwrap();
                let bytes = fs::read(&output).unwrap();
                assert!(e.export_sanitized(&doc, options, &output).is_err());
                assert_eq!(fs::read(&output).unwrap(), bytes);
                if flatten_visible {
                    assert_rebuilt_structure(&parse(&bytes).unwrap());
                } else {
                    no_payload_markers(&parse(&bytes).unwrap());
                }
                assert!(e.export_sanitized(&doc, options, &input).is_err());
                fs::remove_file(input).unwrap();
                fs::remove_file(output).unwrap();
            }
        }
    }
}

#[test]
fn verified_export_rejects_original_content_aliases_including_indirect_arrays() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for array in [false, true] {
        let mut pdf = parse(&workflow_fixture(false)).unwrap();
        let page = pdf.get_pages()[&1];
        let mut bytes = pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap();
        bytes.extend_from_slice(b"\nBT 3 Tr (ALIAS_HIDDEN_PAYLOAD) Tj ET\n");
        pdf.change_page_content(page, bytes).unwrap();
        let contents = pdf
            .get_dictionary(page)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .clone();
        if array {
            let group = pdf.add_object(Object::Array(vec![contents.clone()]));
            pdf.get_dictionary_mut(page).unwrap().set("Contents", group);
        }
        pdf.catalog_mut().unwrap().set("Private", contents);
        let input = temp(&format!("alias-input-{array}.pdf"));
        let output = temp(&format!("alias-output-{array}.pdf"));
        let _ = fs::remove_file(&output);
        let mut source = Vec::new();
        pdf.save_to(&mut source).unwrap();
        fs::write(&input, &source).unwrap();
        let mut doc = e.open(&input).unwrap();
        let original = doc.bytes.clone();
        assert!(
            e.export_sanitized(&doc, SanitizeOptions::default(), &output)
                .is_err()
        );
        assert!(!output.exists());
        assert_eq!(doc.bytes, original);
        assert!(e.sanitize(&mut doc, SanitizeOptions::default()).is_err());
        assert_eq!(doc.bytes, original);
        let options = SanitizeOptions {
            flatten_visible: true,
            ..Default::default()
        };
        e.export_sanitized(&doc, options, &output).unwrap();
        let clean = e.open(&output).unwrap();
        e.verify_sanitized(&clean, options).unwrap();
        assert!(
            parse(&clean.bytes)
                .unwrap()
                .objects
                .values()
                .filter_map(|object| object.as_stream().ok())
                .all(|stream| !stream
                    .get_plain_content_with_limit(MAX_STREAM)
                    .unwrap_or_else(|_| stream.content.clone())
                    .windows(20)
                    .any(|part| part == b"ALIAS_HIDDEN_PAYLOAD"))
        );
        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
    }
}

#[test]
fn rebuilt_verification_rejects_unrecognized_catalog_extras() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("raster-verify-extra.pdf");
    fs::write(&path, workflow_fixture(false)).unwrap();
    let mut doc = e.open(&path).unwrap();
    let options = SanitizeOptions {
        flatten_visible: true,
        ..Default::default()
    };
    e.sanitize(&mut doc, options).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    pdf.catalog_mut()
        .unwrap()
        .set("Private", Object::string_literal("HIDDEN_CATALOG_PAYLOAD"));
    let extra = e.candidate(&doc, pdf).unwrap();
    assert!(e.verify_sanitized(&extra, options).is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn rebuilt_verification_rejects_embedded_jpeg_metadata() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let input = temp("verify-jpeg-extra.pdf");
    fs::write(&input, workflow_fixture(false)).unwrap();
    let mut doc = e.open(&input).unwrap();
    let options = SanitizeOptions {
        flatten_visible: true,
        ..Default::default()
    };
    e.sanitize(&mut doc, options).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    let image = pdf
        .objects
        .values_mut()
        .find_map(|object| {
            object.as_stream_mut().ok().filter(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|kind| kind == b"Image")
            })
        })
        .unwrap();
    let payload = b"Exif\0\0SYNTHETIC_MARKER!";
    let mut bytes = vec![255, 216, 255, 225];
    bytes.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&image.content[2..]);
    image.set_content(bytes);
    let extra = e.candidate(&doc, pdf).unwrap();
    assert_eq!(
        e.render(&doc, 0, 1.).unwrap().rgba,
        e.render(&extra, 0, 1.).unwrap().rgba
    );
    assert!(e.verify_sanitized(&extra, options).is_err());
    fs::remove_file(input).unwrap();
}

#[test]
fn editable_export_keeps_legitimately_shared_page_content_editable() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for shared_array in [false, true] {
        let mut pdf = parse(&generated("AB", false, 0, 0., false, false)).unwrap();
        let page = pdf.get_pages()[&1];
        if shared_array {
            let contents = pdf
                .get_dictionary(page)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .clone();
            let array = pdf.add_object(Object::Array(vec![contents]));
            pdf.get_dictionary_mut(page).unwrap().set("Contents", array);
        }
        let child = pdf.add_object(pdf.get_dictionary(page).unwrap().clone());
        let parent = pdf
            .get_dictionary(page)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        pdf.get_dictionary_mut(parent)
            .unwrap()
            .set("Kids", vec![page.into(), child.into()]);
        pdf.get_dictionary_mut(parent).unwrap().set("Count", 2);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        let input = temp(&format!("shared-content-{shared_array}.pdf"));
        let output = temp(&format!("shared-content-clean-{shared_array}.pdf"));
        let _ = fs::remove_file(&output);
        fs::write(&input, bytes).unwrap();
        let doc = e.open(&input).unwrap();
        e.export_sanitized(&doc, SanitizeOptions::default(), &output)
            .unwrap();
        let clean = e.open(&output).unwrap();
        assert_eq!(e.pages(&clean).unwrap().len(), 2);
        for page in [0, 1] {
            assert_eq!(e.text_objects(&clean, page).unwrap()[0].text, "AB");
        }
        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
    }
}

#[test]
fn verification_bypass_repeated_page_references_cannot_hide_content_aliases() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut pdf = parse(&workflow_fixture(false)).unwrap();
    let page = pdf.get_pages()[&1];
    let parent = pdf
        .get_dictionary(page)
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    pdf.get_dictionary_mut(parent)
        .unwrap()
        .set("Kids", vec![page.into(), page.into()]);
    pdf.get_dictionary_mut(parent).unwrap().set("Count", 2);
    let contents = pdf
        .get_dictionary(page)
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .clone();
    pdf.catalog_mut().unwrap().set("Private", contents);
    let input = temp("repeated-page-alias.pdf");
    let output = temp("repeated-page-clean.pdf");
    let _ = fs::remove_file(&output);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    fs::write(&input, bytes).unwrap();
    let doc = e.open(&input).unwrap();
    assert!(
        e.export_sanitized(&doc, SanitizeOptions::default(), &output)
            .is_err()
    );
    assert!(!output.exists());
    fs::remove_file(input).unwrap();
}

#[test]
fn verification_bypass_forged_xref_streams_cannot_hide_unused_payloads() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let input = temp("forged-xref-source.pdf");
    fs::write(&input, workflow_fixture(false)).unwrap();
    let mut doc = e.open(&input).unwrap();
    let options = SanitizeOptions {
        flatten_visible: true,
        ..Default::default()
    };
    e.sanitize(&mut doc, options).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    pdf.add_object(Stream::new(
        dictionary! {"Type"=>"XRef","W"=>vec![1.into(),2.into(),1.into()],"Size"=>1},
        b"HIDDEN_UNUSED_PAYLOAD".to_vec(),
    ));
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let mut extra = doc.comparison_snapshot();
    extra.bytes = bytes;
    assert!(e.verify_sanitized(&extra, options).is_err());
    fs::remove_file(input).unwrap();
}
