use super::*;

#[test]
fn blank_documents_are_editable_unsaved_and_keep_separate_save_authority() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e
        .blank_document(BlankPdf {
            width: 612.,
            height: 792.,
            pages: 2,
        })
        .unwrap();
    assert!(doc.is_new());
    assert_eq!(doc.suggested_save_path(), PathBuf::from("Untitled.pdf"));
    assert!(!doc.can_undo());
    assert!(!doc.can_redo());
    assert_eq!(e.pages(&doc).unwrap().len(), 2);
    assert!(e.text_objects(&doc, 0).unwrap().is_empty());
    assert!(
        e.render(&doc, 0, 0.5)
            .unwrap()
            .rgba
            .iter()
            .all(|v| *v == 255)
    );
    e.add_text(
        &mut doc,
        &NewText {
            page: 0,
            x: 50.,
            y: 700.,
            text: "Created from scratch".into(),
            font: "Arial".into(),
            size: 12.,
            color: [0, 0, 0],
        },
    )
    .unwrap();
    let output = temp("created-working-copy.pdf");
    let _ = fs::remove_file(&output);
    let mut copy = doc.create_working_copy(&output, "").unwrap();
    let different = e.blank_document(BlankPdf::default()).unwrap();
    assert!(different.update_working_copy(&mut copy).is_err());
    let selected = e.text_objects(&doc, 0).unwrap().remove(0);
    e.edit(&mut doc, &selected, "Edited after creation")
        .unwrap();
    doc.update_working_copy(&mut copy).unwrap();
    let saved = e.open(&output).unwrap();
    assert!(e.find_text(&saved, "Edited after creation").unwrap().len() == 1);
    assert!(!saved.is_new());
    doc.undo();
    assert!(e.find_text(&doc, "Created from scratch").unwrap().len() == 1);
    let recovered = e
        .restore_recovery(&doc.original, &doc.recovery_bytes().unwrap())
        .unwrap();
    assert!(recovered.is_new());
    assert_eq!(recovered.original, doc.original);
    fs::remove_file(output).unwrap();
    for spec in [
        BlankPdf {
            pages: 0,
            ..Default::default()
        },
        BlankPdf {
            pages: 1001,
            ..Default::default()
        },
        BlankPdf {
            width: f32::NAN,
            ..Default::default()
        },
        BlankPdf {
            height: 14401.,
            ..Default::default()
        },
    ] {
        assert!(e.blank_document(spec).is_err());
    }
}
fn fictional_cv() -> CvSpec {
    CvSpec {name:"Avery Example".into(),headline:"Software developer".into(),contact:vec!["reader@example.org | +1 555 0100 | Example City".into()],letter:false,
        sections:vec![CvSection {heading:"Summary".into(),body:"Builds reliable software. Nordic text: ä ö å and € remain searchable.".into()},
            CvSection {heading:"Experience".into(),body:"Developer | Example Company | 2022–2026\n- Improved a fictional service and documented the outcome.".into()},
            CvSection {heading:"Education".into(),body:"BSc Computing | Example University | 2022".into()},CvSection {heading:"Skills".into(),body:"Rust, SQL, testing, accessibility".into()}]}
}
#[test]
fn cv_saved_pdf_has_selectable_ordered_text_and_remains_editable() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for letter in [false, true] {
        let mut spec = fictional_cv();
        spec.letter = letter;
        let mut doc = e.create_cv(&spec).unwrap();
        assert!(!doc.can_undo());
        assert!(doc.is_new());
        let check = e.check_cv(&doc).unwrap();
        assert_eq!(check.pages, 1);
        assert!(check.warnings.is_empty(), "{:?}", check.warnings);
        let output = temp("fictional-cv.pdf");
        let _ = fs::remove_file(&output);
        doc.save_as(&output).unwrap();
        let mut saved = e.open(&output).unwrap();
        let text = e.check_cv(&saved).unwrap().text;
        let mut position = 0;
        for value in [
            "Avery Example",
            "reader@example.org",
            "Summary",
            "Nordic text:",
            "Experience",
            "Developer",
            "Education",
            "BSc Computing",
            "Skills",
            "Rust, SQL",
        ] {
            let next = text[position..]
                .find(value)
                .unwrap_or_else(|| panic!("Missing or out-of-order {value}: {text}"));
            position += next + value.len();
        }
        assert!(!e.find_text(&saved, "ä ö å").unwrap().is_empty());
        let objects = e.text_objects(&saved, 0).unwrap();
        assert!(objects.iter().all(|o| o.size >= 10.
            && o.bounds.left >= 40.
            && o.bounds.right < e.pages(&saved).unwrap()[0].width - 39.));
        let line = objects
            .into_iter()
            .find(|o| o.text.contains("Software developer"))
            .unwrap();
        e.edit(&mut saved, &line, "Software engineer").unwrap();
        assert!(!e.find_text(&saved, "Software engineer").unwrap().is_empty());
        assert!(
            e.render(&saved, 0, 0.5)
                .unwrap()
                .rgba
                .iter()
                .any(|n| *n < 200)
        );
        assert!(saved.undo());
        assert!(
            !e.find_text(&saved, "Software developer")
                .unwrap()
                .is_empty()
        );
        let before = doc.bytes.clone();
        let object = e.text_objects(&doc, 0).unwrap().remove(0);
        e.style_text(&mut doc, &object, 18., [0, 0, 0]).unwrap();
        doc.undo();
        assert_eq!(doc.bytes, before);
        fs::remove_file(output).unwrap();
    }
}
#[test]
fn cv_wraps_paginates_without_tiny_text_or_stranded_headings_and_rejects_overflow() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut spec = fictional_cv();
    spec.sections[1].body = (0..140)
        .map(|n| {
            format!(
                "Achievement {n}: delivered a useful fictional improvement with clear results.\n"
            )
        })
        .collect();
    let doc = e.create_cv(&spec).unwrap();
    let pages = e.pages(&doc).unwrap();
    assert!(pages.len() > 2);
    for (page, info) in pages.iter().enumerate() {
        let text = e.text_objects(&doc, page as u16).unwrap();
        assert!(
            text.iter().all(|o| o.size >= 10.
                && o.bounds.bottom >= 40.
                && o.bounds.top <= info.height - 39.)
        );
        assert!(
            !text
                .last()
                .is_some_and(|o| spec.sections.iter().any(|s| s.heading == o.text))
        );
    }
    let check = e.check_cv(&doc).unwrap();
    assert!(check.text.contains("Achievement 139"));
    let mut multiline = fictional_cv();
    multiline.sections[0].heading = "Heading\n".repeat(12);
    assert!(e.create_cv(&multiline).is_err());
    spec.sections[0].body = "x".repeat(5000);
    assert!(e.create_cv(&spec).is_err());
    spec.sections[0].body = "short\n".repeat(2000);
    assert!(e.create_cv(&spec).is_err());
    let control = OperationControl::default();
    control.cancel();
    assert!(
        e.with_operation(&control, || e.create_cv(&fictional_cv()))
            .is_err()
    );
}

#[test]
fn cv_validation_identifies_draft_errors_before_document_creation() {
    let mut spec = fictional_cv();
    spec.sections[0].heading.clear();
    assert!(spec.validate().unwrap_err().to_string().contains("heading"));
    spec.sections[0].heading = "Summary".into();
    assert!(spec.validate().is_ok());
    spec.contact = vec!["reader@example.org".into(); 11];
    assert!(
        spec.validate()
            .unwrap_err()
            .to_string()
            .contains("10 contact")
    );
}
