use super::*;

#[test]
fn mixed_feature_sequences_roundtrip_and_undo_at_every_page_rotation() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        let source = temp(&format!("stress-mixed-{rotation}.pdf"));
        let seed = generated("Original", false, rotation, 0.0, false, false);
        fs::write(&source, &seed).unwrap();
        let mut doc = e.open(&source).unwrap();
        let mut history = vec![doc.bytes.clone()];
        macro_rules! step {
            ($operation:expr) => {{
                $operation.unwrap();
                if history.last().unwrap() != &doc.bytes {
                    history.push(doc.bytes.clone());
                }
                for page in 0..e.pages(&doc).unwrap().len() as u16 {
                    e.render(&doc, page, 0.5).unwrap();
                    e.thumbnail(&doc, page, 64).unwrap();
                    e.text_objects(&doc, page).unwrap();
                }
            }};
        }
        step!(e.add_text(
            &mut doc,
            &NewText {
                page: 0,
                x: 40.0,
                y: 140.0,
                text: "Stress label".into(),
                font: "Arial".into(),
                size: 14.0,
                color: [20, 40, 60],
            }
        ));
        let added = e.text_objects(&doc, 0).unwrap().pop().unwrap();
        step!(e.style_text(&mut doc, &added, 17.0, [80, 30, 10]));
        step!(e.replace_text(&mut doc, "Stress label", "Updated label", None));
        assert_eq!(e.find_text(&doc, "Updated label").unwrap().len(), 1);
        let added = e.text_objects(&doc, 0).unwrap().pop().unwrap();
        let item = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|item| item.index == added.index)
            .unwrap();
        let clipboard = e
            .copy_page_items(&doc, std::slice::from_ref(&item))
            .unwrap();
        step!(e.edit_page_items(&mut doc, &[item], ObjectAction::Move { dx: 5.0, dy: -5.0 }));
        step!(e.place_picture(
            &mut doc,
            &Picture {
                width: 16,
                height: 16,
                rgba: [30, 140, 200, 255].repeat(256),
            },
            Placement {
                page: 0,
                left: 280.0,
                bottom: 110.0,
                width: 40.0,
                height: 40.0
            }
        ));
        let image = e.placed_pictures(&doc).unwrap().pop().unwrap();
        step!(e.move_picture(
            &mut doc,
            &image,
            Placement {
                page: 0,
                left: 290.0,
                bottom: 105.0,
                width: 50.0,
                height: 35.0,
            }
        ));
        for kind in [
            MarkupKind::Highlight,
            MarkupKind::Underline,
            MarkupKind::Strikethrough,
            MarkupKind::Note,
        ] {
            step!(e.add_annotation(
                &mut doc,
                &Markup {
                    page: 0,
                    kind,
                    bounds: Bounds {
                        left: 350.0,
                        bottom: 130.0,
                        right: 390.0,
                        top: 150.0
                    },
                    color: [230, 150, 50],
                    opacity: 0.7,
                    text: "Generated note äöå".into(),
                }
            ));
        }
        for kind in [
            DrawingKind::Freehand,
            DrawingKind::Rectangle,
            DrawingKind::Ellipse,
            DrawingKind::Line,
            DrawingKind::Arrow,
        ] {
            step!(e.add_drawing(
                &mut doc,
                &Drawing {
                    page: 0,
                    kind,
                    points: vec![[350.0, 30.0], [390.0, 60.0]],
                    color: [0, 90, 180],
                    width: 2.0,
                    opacity: 0.8,
                }
            ));
        }
        step!(e.design_form(
            &mut doc,
            FormDesignEdit::Add(NewFormField {
                page: 0,
                name: "Generated field".into(),
                required: true,
                bounds: Bounds {
                    left: 200.0,
                    bottom: 30.0,
                    right: 310.0,
                    top: 60.0
                },
                kind: DesignFieldKind::Text,
            })
        ));
        let field = e.form_fields(&doc).unwrap().remove(0);
        step!(e.fill_field(&mut doc, &field, &FieldValue::Text("Filled".into())));
        assert_eq!(
            e.form_fields(&doc).unwrap()[0].value,
            FieldValue::Text("Filled".into())
        );
        step!(e.flatten_forms(&mut doc));
        assert!(e.form_fields(&doc).unwrap().is_empty());
        step!(e.stamp_bates(
            &mut doc,
            &RangeStamp {
                stamp: TextStamp {
                    template: String::new(),
                    font: "Arial".into(),
                    size: 10.0,
                    color: [0, 0, 0],
                    opacity: 1.0,
                    x: 20.0,
                    y: 20.0
                },
                pages: "1".into(),
                date: "2026-10-25".into(),
                placement: StampPlacement::Footer,
            },
            &BatesNumber {
                prefix: "TEST-".into(),
                start: 9,
                digits: 4
            }
        ));
        step!(e.compress_images(&mut doc, 128, 80));
        step!(e.edit_pages(
            &mut doc,
            &PageEdit::Insert {
                path: fixture("kerning.pdf"),
                before: 1
            }
        ));
        step!(e.paste_page_items(&mut doc, &clipboard, 1));
        let revision = doc.revision;
        step!(e.edit_bookmarks(
            &mut doc,
            revision,
            BookmarkEdit::Add {
                title: "Generated destination".into(),
                page: 1,
                parent: None,
            }
        ));
        step!(e.edit_pages(&mut doc, &PageEdit::Move { from: 1, to: 0 }));
        assert_eq!(e.bookmarks(&doc).unwrap().items[0].page, Some(0));
        step!(e.edit_pages(&mut doc, &PageEdit::Rotate { page: 0 }));
        step!(e.crop_page(
            &mut doc,
            0,
            Bounds {
                left: 5.0,
                bottom: 5.0,
                right: 480.0,
                top: 280.0
            }
        ));
        let word = temp(&format!("stress-mixed-{rotation}.docx"));
        e.export_word(&doc, &word).unwrap();
        let saved = temp(&format!("stress-mixed-{rotation}-saved.pdf"));
        doc.save_as(&saved).unwrap();
        let reopened = e.open(&saved).unwrap();
        assert_eq!(e.pages(&reopened).unwrap().len(), 2);
        for page in 0..2 {
            assert_eq!(
                e.render(&doc, page, 0.5).unwrap().rgba,
                e.render(&reopened, page, 0.5).unwrap().rgba
            );
        }
        for expected in history[..history.len() - 1].iter().rev() {
            assert!(doc.undo());
            assert_eq!(&doc.bytes, expected);
        }
        assert!(!doc.can_undo());
        assert_eq!(doc.bytes, seed);
        for expected in history.iter().skip(1) {
            assert!(doc.redo());
            assert_eq!(&doc.bytes, expected);
        }
        assert!(!doc.can_redo());
        assert_eq!(fs::read(&source).unwrap(), seed);
        for path in [source, saved, word] {
            fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn repeated_invalid_operations_preserve_document_and_redo_branch() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("unicode-cid.pdf")).unwrap();
    let selected = e.text_objects(&doc, 0).unwrap().remove(0);
    e.edit(&mut doc, &selected, "AB").unwrap();
    assert!(doc.undo());
    let original = doc.bytes.clone();
    let state = (doc.revision, doc.undo.clone(), doc.redo.clone());
    for _ in 0..12 {
        macro_rules! reject {
            ($operation:expr) => {{
                assert!($operation.is_err(), "{}", stringify!($operation));
                assert_eq!(doc.bytes, original);
                assert_eq!((doc.revision, doc.undo.clone(), doc.redo.clone()), state);
            }};
        }
        let selected = e.text_objects(&doc, 0).unwrap().remove(0);
        reject!(e.edit(&mut doc, &selected, "invalid\0text"));
        reject!(e.style_text(&mut doc, &selected, f32::NAN, [0, 0, 0]));
        reject!(e.edit_pages(&mut doc, &PageEdit::Delete { page: u16::MAX }));
        reject!(e.edit_pages(
            &mut doc,
            &PageEdit::Move {
                from: 0,
                to: u16::MAX
            }
        ));
        reject!(e.edit_pages(
            &mut doc,
            &PageEdit::Insert {
                path: fixture("malformed.pdf"),
                before: 0
            }
        ));
        reject!(e.merge_pdfs(
            &mut doc,
            &[fixture("kerning.pdf"), fixture("malformed.pdf")]
        ));
        reject!(e.crop_page(
            &mut doc,
            0,
            Bounds {
                left: 10.0,
                bottom: 10.0,
                right: 1.0,
                top: 1.0
            }
        ));
        reject!(e.redact(
            &mut doc,
            0,
            Bounds {
                left: f32::NAN,
                bottom: 10.0,
                right: 20.0,
                top: 20.0
            }
        ));
        reject!(e.compress_images(&mut doc, 0, 80));
        reject!(e.add_drawing(
            &mut doc,
            &Drawing {
                page: 0,
                kind: DrawingKind::Line,
                points: vec![[0.0, 0.0], [f32::INFINITY, 1.0]],
                color: [0, 0, 0],
                width: 1.0,
                opacity: 1.0
            }
        ));
        reject!(e.design_form(
            &mut doc,
            FormDesignEdit::Add(NewFormField {
                page: u16::MAX,
                name: "Invalid page".into(),
                required: false,
                bounds: Bounds {
                    left: 0.0,
                    bottom: 0.0,
                    right: 20.0,
                    top: 20.0
                },
                kind: DesignFieldKind::Text,
            })
        ));
        reject!(e.replace_text(&mut doc, "unchanged", "AB", Some(usize::MAX)));
        let revision = doc.revision;
        reject!(e.edit_bookmarks(
            &mut doc,
            revision,
            BookmarkEdit::Add {
                title: "Invalid page".into(),
                page: u16::MAX,
                parent: None,
            }
        ));
        reject!(e.render(&doc, 0, f32::INFINITY));
        reject!(e.thumbnail(&doc, 0, 0));
        reject!(e.find_text(&doc, "invalid\0query"));
        assert!(doc.can_redo());
    }
    assert!(doc.redo());
    assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, "AB");
}

#[test]
fn inserting_and_extracting_form_pages_keeps_fields_editable() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source = temp("stress-form-source.pdf");
    fs::write(&source, form_fixture()).unwrap();
    let mut form = e.open(&source).unwrap();
    let field = e.form_fields(&form).unwrap().remove(0);
    e.fill_field(
        &mut form,
        &field,
        &FieldValue::Text("Generated field value".into()),
    )
    .unwrap();
    let filled = temp("stress-form-filled.pdf");
    form.save_as(&filled).unwrap();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = doc.bytes.clone();
    e.edit_pages(
        &mut doc,
        &PageEdit::Insert {
            path: filled.clone(),
            before: 1,
        },
    )
    .unwrap();
    let fields = e.form_fields(&doc).unwrap();
    assert_eq!(fields.len(), 4);
    assert!(fields.iter().all(|f| f.page == 1));
    assert_eq!(
        fields[0].value,
        FieldValue::Text("Generated field value".into())
    );
    e.fill_field(
        &mut doc,
        &fields[0],
        &FieldValue::Text("Edited after insertion".into()),
    )
    .unwrap();
    let output = temp("stress-form-extracted.pdf");
    e.extract_pages(&doc, &[1], &output).unwrap();
    let extracted = e.open(&output).unwrap();
    assert_eq!(
        e.form_fields(&extracted).unwrap()[0].value,
        FieldValue::Text("Edited after insertion".into())
    );
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.bytes, before);
    for path in [source, filled, output] {
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn repeated_form_imports_keep_names_resources_and_values_independent() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source = temp("stress-repeated-forms.pdf");
    fs::write(&source, form_fixture()).unwrap();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    for before in [1, 2] {
        e.edit_pages(
            &mut doc,
            &PageEdit::Insert {
                path: source.clone(),
                before,
            },
        )
        .unwrap();
    }
    let fields = e.form_fields(&doc).unwrap();
    assert_eq!(fields.len(), 8);
    assert_ne!(fields[0].name, fields[4].name);
    for (index, text) in [(0, "First copy"), (4, "Second copy")] {
        let fields = e.form_fields(&doc).unwrap();
        e.fill_field(&mut doc, &fields[index], &FieldValue::Text(text.into()))
            .unwrap();
    }
    let fields = e.form_fields(&doc).unwrap();
    assert_eq!(fields[0].value, FieldValue::Text("First copy".into()));
    assert_eq!(fields[4].value, FieldValue::Text("Second copy".into()));
    let output = temp("stress-repeated-forms-saved.pdf");
    doc.save_as(&output).unwrap();
    let reopened = e.open(&output).unwrap();
    assert_eq!(e.form_fields(&reopened).unwrap().len(), 8);
    for page in [1, 2] {
        assert_eq!(
            e.render(&doc, page, 1.0).unwrap().rgba,
            e.render(&reopened, page, 1.0).unwrap().rgba
        );
    }
    for path in [source, output] {
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn extracting_shared_radio_fields_excludes_widgets_on_other_pages() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    e.design_form(
        &mut doc,
        FormDesignEdit::Add(NewFormField {
            page: 0,
            name: "Shared radio".into(),
            required: true,
            bounds: Bounds {
                left: 20.0,
                bottom: 20.0,
                right: 100.0,
                top: 80.0,
            },
            kind: DesignFieldKind::Radio {
                options: vec!["First".into(), "Second".into()],
            },
        }),
    )
    .unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    let pages = pdf.get_pages();
    let mut widgets = pdf
        .get_dictionary(pages[&1])
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let second = widgets.pop().unwrap();
    pdf.get_dictionary_mut(pages[&1])
        .unwrap()
        .set("Annots", widgets);
    pdf.get_dictionary_mut(second.as_reference().unwrap())
        .unwrap()
        .set("P", pages[&2]);
    pdf.get_dictionary_mut(pages[&2])
        .unwrap()
        .set("Annots", vec![second]);
    doc = e.candidate(&doc, pdf).unwrap();
    let fields = e.form_fields(&doc).unwrap();
    assert_eq!(fields.len(), 2);
    e.fill_field(&mut doc, &fields[1], &FieldValue::Radio(true))
        .unwrap();
    let output = temp("stress-radio-extracted.pdf");
    e.extract_pages(&doc, &[1], &output).unwrap();
    let mut extracted = e.open(&output).unwrap();
    let fields = e.form_fields(&extracted).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].value, FieldValue::Radio(true));
    assert_eq!(e.designed_fields(&extracted).unwrap().len(), 1);
    let pdf = parse(&extracted.bytes).unwrap();
    let root = pdf
        .catalog()
        .unwrap()
        .get_deref(b"AcroForm", &pdf)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Fields")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .as_reference()
        .unwrap();
    assert_eq!(
        pdf.get_dictionary(root)
            .unwrap()
            .get(b"Kids")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let before = extracted.bytes.clone();
    assert!(
        e.fill_field(&mut extracted, &fields[0], &FieldValue::Radio(false))
            .is_err()
    );
    assert_eq!(extracted.bytes, before);
    e.fill_field(&mut extracted, &fields[0], &FieldValue::Radio(true))
        .unwrap();
    assert_eq!(
        e.form_fields(&extracted).unwrap()[0].value,
        FieldValue::Radio(true)
    );
    fs::remove_file(output).unwrap();
}

#[test]
fn unsupported_form_transfers_fail_before_writing_or_changing_history() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source = temp("stress-unsupported-form.pdf");
    fs::write(&source, form_fixture()).unwrap();
    let form = e.open(&source).unwrap();
    let output = temp("stress-repeated-form-pages.pdf");
    assert!(e.extract_pages(&form, &[0, 0], &output).is_err());
    assert!(!output.exists());
    let mut pdf = parse(&form.bytes).unwrap();
    let mut acro = pdf
        .catalog()
        .unwrap()
        .get_deref(b"AcroForm", &pdf)
        .unwrap()
        .as_dict()
        .unwrap()
        .clone();
    acro.set("XFA", Object::Array(vec![]));
    pdf.catalog_mut().unwrap().set("AcroForm", acro);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    let xfa = temp("stress-xfa-form.pdf");
    fs::write(&xfa, bytes).unwrap();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = doc.bytes.clone();
    assert!(
        e.edit_pages(
            &mut doc,
            &PageEdit::Insert {
                path: xfa.clone(),
                before: 1
            }
        )
        .is_err()
    );
    assert_eq!(doc.bytes, before);
    assert!(!doc.can_undo());
    for path in [source, xfa] {
        fs::remove_file(path).unwrap();
    }
}
