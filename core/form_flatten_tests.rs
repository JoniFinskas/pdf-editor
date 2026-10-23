use super::*;
#[test]
fn flatten_forms_preserves_values_appearances_annotations_and_history() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("flatten-forms.pdf");
    fs::write(&path, form_fixture()).unwrap();
    let mut doc = e.open(&path).unwrap();
    for (index, value) in [
        (0, FieldValue::Text("Filled value".into())),
        (1, FieldValue::Check(true)),
        (2, FieldValue::Radio(true)),
        (
            3,
            FieldValue::Choice {
                options: vec![],
                selected: Some(1),
            },
        ),
    ] {
        let fields = e.form_fields(&doc).unwrap();
        e.fill_field(&mut doc, &fields[index], &value).unwrap();
    }
    e.add_annotation(
        &mut doc,
        &Markup {
            page: 0,
            kind: MarkupKind::Note,
            bounds: Bounds {
                left: 300.,
                bottom: 150.,
                right: 320.,
                top: 170.,
            },
            color: [255, 220, 0],
            opacity: 1.,
            text: "Keep note".into(),
        },
    )
    .unwrap();
    let before = doc.bytes.clone();
    let pixels = e.render(&doc, 0, 1.).unwrap().rgba;
    assert_eq!(e.flatten_forms(&mut doc).unwrap(), 4);
    assert!(e.form_fields(&doc).unwrap().is_empty());
    assert_eq!(e.annotations(&doc).unwrap().len(), 1);
    assert_eq!(pixels, e.render(&doc, 0, 1.).unwrap().rgba);
    let native = e.pdfium.load_pdf_from_byte_slice(&doc.bytes, None).unwrap();
    assert!(
        native
            .pages()
            .get(0)
            .unwrap()
            .text()
            .unwrap()
            .all()
            .contains("Filled value")
    );
    drop(native);
    assert!(
        parse(&doc.bytes)
            .unwrap()
            .catalog()
            .unwrap()
            .get(b"AcroForm")
            .is_err()
    );
    let output = temp("flattened.pdf");
    let _ = fs::remove_file(&output);
    doc.save_as(&output).unwrap();
    let reopened = e.open(&output).unwrap();
    assert!(e.form_fields(&reopened).unwrap().is_empty());
    assert_eq!(pixels, e.render(&reopened, 0, 1.).unwrap().rgba);
    assert!(doc.undo());
    assert_eq!(doc.bytes, before);
    assert_eq!(e.form_fields(&doc).unwrap().len(), 4);
    assert!(doc.redo());
    assert!(e.form_fields(&doc).unwrap().is_empty());
    let flattened = doc.bytes.clone();
    assert_eq!(e.flatten_forms(&mut doc).unwrap(), 0);
    assert_eq!(doc.bytes, flattened);
}
#[test]
fn flatten_missing_appearance_is_atomic() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("flatten-missing.pdf");
    fs::write(&path, form_fixture()).unwrap();
    let mut doc = e.open(&path).unwrap();
    let before = doc.bytes.clone();
    assert!(e.flatten_forms(&mut doc).is_err());
    assert_eq!(doc.bytes, before);
    assert!(!doc.can_undo());
}
