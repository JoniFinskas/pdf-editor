use super::*;
use skrifa::{FontRef, MetadataProvider};

#[test]
fn installed_named_faces_match_their_actual_weight_and_slant_exactly() {
    for (name, weight, italic) in [
        ("ArialMT", 400., false),
        ("BOLDAA+ArialMT", 400., false),
        ("ITALIC+ArialMT", 400., false),
        ("Arial-BoldItalicMT", 700., true),
        ("Arial-Black", 900., false),
        ("SegoeUI-Semibold", 600., false),
    ] {
        let font = fonts::find(name, "AB unchanged Ää Öö Åå €").unwrap();
        let face = FontRef::new(&font.bytes).unwrap();
        assert!(
            (face.attributes().weight.value() - weight).abs() < 1.,
            "Wrong weight for {name}: {}",
            font.name
        );
        assert_eq!(
            face.attributes().style != skrifa::attribute::Style::Normal,
            italic
        );
        assert!(
            font.exact,
            "Installed named face was treated as a substitute: {name} -> {}",
            font.name
        );
    }
}

fn face_pdf(font: &fonts::Font, rotation: i64, embedded: bool) -> Vec<u8> {
    let mut pdf = parse(&generated("AB", false, rotation, 0.4, false, false)).unwrap();
    let font = fonts::embed(&mut pdf, font, "ABunchanged").unwrap();
    if !embedded {
        let cid = pdf
            .get_dictionary(font.id)
            .unwrap()
            .get(b"DescendantFonts")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let descriptor = pdf
            .get_dictionary(cid)
            .unwrap()
            .get(b"FontDescriptor")
            .unwrap()
            .as_reference()
            .unwrap();
        pdf.get_dictionary_mut(descriptor)
            .unwrap()
            .remove(b"FontFile2");
    }
    let page = pdf.get_pages()[&1];
    let mut content =
        Content::decode(&pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap()).unwrap();
    let mut first = true;
    for op in &mut content.operations {
        if op.operator == "Tj" {
            op.operands = vec![Object::String(
                font.encode(if first { "AB" } else { "unchanged" }).unwrap(),
                lopdf::StringFormat::Hexadecimal,
            )];
            first = false;
        }
    }
    // Exercise text-space scaling/skew as well as the page's display rotation.
    let matrix = content
        .operations
        .iter_mut()
        .find(|op| op.operator == "Tm")
        .unwrap();
    matrix.operands = [1.1, 0.15, -0.1, 0.9, 50., 200.].map(Object::Real).to_vec();
    pdf.change_page_content(page, content.encode().unwrap())
        .unwrap();
    pdf.get_dictionary_mut(page).unwrap().set(
        "Resources",
        dictionary! {"Font"=>dictionary! {"F1"=>font.id}},
    );
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn named_font_edit_stress_preserves_neighbors_history_and_saved_output() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for name in [
        "ArialMT",
        "Arial-BoldItalicMT",
        "Arial-Black",
        "SegoeUI-Semibold",
    ] {
        let font = fonts::find(name, "AB unchanged Ää Öö Åå € XYZ 123").unwrap();
        for rotation in [0, 90, 180, 270] {
            for embedded in [false, true] {
                let source = temp(&format!("font-edit-{name}-{rotation}-{embedded}.pdf"));
                let bytes = face_pdf(&font, rotation, embedded);
                fs::write(&source, &bytes).unwrap();
                let mut doc = e.open(&source).unwrap();
                doc.substitution = Substitution::Document;
                let mut history = vec![doc.bytes.clone()];
                for text in ["Ää Öö Åå € XYZ 123", "A", "AB"] {
                    let prior = doc.comparison_snapshot();
                    let before = e.text_objects(&doc, 0).unwrap();
                    let result = e.edit(&mut doc, &before[0], text).unwrap_or_else(|error| {
                        panic!("{name}, rotation={rotation}, embedded={embedded}: {error}")
                    });
                    assert!(
                        result.warning.is_none(),
                        "Unexpected substitution for {name}: {:?}",
                        result.warning
                    );
                    assert_eq!(
                        result.objects_changed, 1,
                        "Unrelated text reformatted for {name}"
                    );
                    let after = e.text_objects(&doc, 0).unwrap();
                    assert_eq!(after[0].text, text);
                    assert_eq!(after[0].font, before[0].font);
                    assert_eq!(after[0].size, before[0].size);
                    assert_eq!(after[0].color, before[0].color);
                    assert!(
                        after[0]
                            .matrix
                            .iter()
                            .zip(before[0].matrix)
                            .all(|(a, b)| (a - b).abs() < 0.001)
                    );
                    for (old, new) in before[1..].iter().zip(&after[1..]) {
                        assert_eq!(old.text, new.text);
                        assert_eq!(old.font, new.font);
                        assert!(
                            old.matrix
                                .iter()
                                .zip(new.matrix)
                                .all(|(a, b)| (a - b).abs() < 0.001)
                        );
                        for (a, b) in [
                            old.bounds.left,
                            old.bounds.bottom,
                            old.bounds.right,
                            old.bounds.top,
                        ]
                        .into_iter()
                        .zip([
                            new.bounds.left,
                            new.bounds.bottom,
                            new.bounds.right,
                            new.bounds.top,
                        ]) {
                            assert!((a - b).abs() < 0.001);
                        }
                    }
                    assert_outside_equal(&e, &prior, &doc, &before[0], &after[0]);
                    history.push(doc.bytes.clone());
                }
                let selected = e.text_objects(&doc, 0).unwrap().remove(0);
                if rotation == 0 {
                    let stable = doc.bytes.clone();
                    let revision = doc.revision();
                    assert!(e.edit(&mut doc, &selected, "\u{10ffff}").is_err());
                    assert_eq!(doc.bytes, stable);
                    assert_eq!(doc.revision(), revision);
                }
                e.edit(&mut doc, &selected, "").unwrap();
                history.push(doc.bytes.clone());
                assert_eq!(e.text_objects(&doc, 0).unwrap().len(), 2);
                for expected in history[..history.len() - 1].iter().rev() {
                    assert!(doc.undo());
                    assert_eq!(&doc.bytes, expected);
                }
                assert!(!doc.can_undo());
                for expected in &history[1..] {
                    assert!(doc.redo());
                    assert_eq!(&doc.bytes, expected);
                }
                assert!(!doc.can_redo());
                let output = temp(&format!("font-edit-saved-{name}-{rotation}-{embedded}.pdf"));
                if output.exists() {
                    fs::remove_file(&output).unwrap();
                }
                doc.save_as(&output).unwrap();
                let reopened = e.open(&output).unwrap();
                assert_eq!(
                    e.render(&reopened, 0, 1.5).unwrap().rgba,
                    e.render(&doc, 0, 1.5).unwrap().rgba
                );
                assert_eq!(fs::read(source).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn adding_and_formatting_installed_heavy_faces_preserves_the_requested_face() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for name in ["Arial-Black", "SegoeUI-Semibold"] {
        let mut doc = e.open(&fixture("unicode-cid.pdf")).unwrap();
        let original = doc.bytes.clone();
        e.add_text(
            &mut doc,
            &NewText {
                page: 0,
                x: 50.,
                y: 130.,
                text: "Heavy face Ää €".into(),
                font: name.into(),
                size: 18.,
                color: [20, 40, 80],
            },
        )
        .unwrap();
        let text = e.text_objects(&doc, 0).unwrap();
        assert!(
            text.iter()
                .find(|o| o.text == "Heavy face Ää €")
                .unwrap()
                .font
                .eq_ignore_ascii_case(name)
        );
        let before = doc.bytes.clone();
        let selected = text[0].clone();
        e.format_text(
            &mut doc,
            std::slice::from_ref(&selected),
            &TextFormat {
                font: name.into(),
                bold: false,
                italic: false,
                size: selected.size,
                color: [20, 40, 80],
            },
        )
        .unwrap();
        let formatted = e.text_objects(&doc, 0).unwrap();
        assert!(formatted[0].font.eq_ignore_ascii_case(name));
        assert_eq!(formatted[0].text, selected.text);
        assert!(doc.undo());
        assert_eq!(doc.bytes, before);
        assert!(doc.undo());
        assert_eq!(doc.bytes, original);
    }
}
