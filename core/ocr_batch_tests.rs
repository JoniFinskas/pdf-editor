use super::*;

fn scanned_pages(e: &Engine, rotation: i64) -> Document {
    let path = temp("selected-ocr-source.pdf");
    fs::write(
        &path,
        generated("Existing", false, rotation, 0., false, false),
    )
    .unwrap();
    let doc = e.open(&path).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    let original = pdf.get_pages()[&1];
    let template = pdf.get_dictionary(original).unwrap().clone();
    let parent = template.get(b"Parent").unwrap().as_reference().unwrap();
    let mut kids = vec![];
    for i in 0..3 {
        let mut page = template.clone();
        if i < 2 {
            page.remove(b"Contents");
        }
        // Nonzero origins exercise the full PDFium page mapping.
        page.set(
            "CropBox",
            vec![10.into(), 15.into(), 490.into(), 285.into()],
        );
        kids.push(Object::Reference(pdf.add_object(page)));
    }
    let tree = pdf.get_dictionary_mut(parent).unwrap();
    tree.set("Kids", kids);
    tree.set("Count", 3);
    let doc = e.candidate(&doc, pdf).unwrap();
    fs::remove_file(path).unwrap();
    doc
}
fn words() -> Vec<crate::ocr::RecognizedWord> {
    (0..20)
        .map(|i| crate::ocr::RecognizedWord {
            text: "Repeated".into(),
            x: 25. + (i % 4) as f32 * 100.,
            y: 30. + (i / 4) as f32 * 35.,
            width: 80.,
            height: 22.,
        })
        .collect()
}

#[test]
fn selected_ocr_is_sequential_invisible_single_undo_and_one_mapping_per_page() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        let mut doc = scanned_pages(&e, rotation);
        let original = doc.bytes.clone();
        let before = (0..3)
            .map(|p| e.render(&doc, p, 1.).unwrap().rgba)
            .collect::<Vec<_>>();
        let control = OperationControl::default();
        let mut previous = 0;
        let mut calls = vec![];
        crate::ocr::MAPPING_SETUPS.with(|count| count.set(0));
        let report = e
            .with_operation(&control, || {
                e.ocr_pages_with(&mut doc, &[2, 1, 0], |page, image| {
                    assert!(image.width <= 2500 && image.height <= 2500);
                    let progress = control.progress().unwrap();
                    assert!(progress.completed >= previous);
                    previous = progress.completed;
                    assert_eq!(progress.total, 9);
                    calls.push(page);
                    Ok(words())
                })
            })
            .unwrap();
        assert_eq!(calls, vec![0, 1]);
        assert_eq!(
            report,
            OcrReport {
                pages_recognized: 2,
                words_added: 40,
                skipped_text: 1,
                skipped_empty: 0
            }
        );
        crate::ocr::MAPPING_SETUPS.with(|count| assert_eq!(count.get(), 2));
        assert_eq!(control.progress().unwrap().completed, 9);
        assert_eq!(doc.undo.len(), 1);
        assert_eq!(doc.revision, 1);
        for page in 0..3 {
            assert_eq!(
                before[page as usize],
                e.render(&doc, page, 1.).unwrap().rgba
            );
        }
        for page in 0..2 {
            let objects = e.text_objects(&doc, page).unwrap();
            assert_eq!(objects.len(), 20);
            assert!(objects.iter().all(|o| o.text == "Repeated"));
        }
        let output = temp("selected-ocr-roundtrip.pdf");
        let _ = fs::remove_file(&output);
        doc.save_as(&output).unwrap();
        assert_eq!(
            e.text_objects(&e.open(&output).unwrap(), 1).unwrap().len(),
            20
        );
        fs::remove_file(output).unwrap();
        let state = doc.bytes.clone();
        assert!(doc.undo());
        assert_eq!(doc.bytes, original);
        assert!(doc.redo());
        assert_eq!(doc.bytes, state);
    }
}

#[test]
fn selected_ocr_skips_and_late_failure_or_cancel_preserve_all_history() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = scanned_pages(&e, 0);
    e.edit_pages(&mut doc, &PageEdit::Rotate { page: 0 })
        .unwrap();
    doc.undo();
    let original = doc.recovery_bytes().unwrap();
    for selected in [vec![], vec![0, 0], vec![10], vec![0; 101]] {
        assert!(
            e.ocr_pages_with(&mut doc, &selected, |_, _| panic!(
                "Invalid selection recognized"
            ))
            .is_err()
        );
        assert_eq!(doc.recovery_bytes().unwrap(), original);
    }
    let report = e
        .ocr_pages_with(&mut doc, &[0, 2], |_, _| Ok(vec![]))
        .unwrap();
    assert_eq!(
        report,
        OcrReport {
            skipped_text: 1,
            skipped_empty: 1,
            ..Default::default()
        }
    );
    assert_eq!(doc.recovery_bytes().unwrap(), original);
    assert!(
        e.ocr_pages_with(&mut doc, &[0, 1], |page, _| if page == 0 {
            Ok(words())
        } else {
            Err(err("Generated recognition failure"))
        })
        .is_err()
    );
    assert_eq!(doc.recovery_bytes().unwrap(), original);
    let control = OperationControl::default();
    assert!(
        e.with_operation(&control, || e.ocr_pages_with(
            &mut doc,
            &[0, 1],
            |page, _| {
                if page == 1 {
                    control.cancel();
                }
                Ok(words())
            }
        ))
        .is_err()
    );
    assert_eq!(doc.recovery_bytes().unwrap(), original);
}

#[test]
fn selected_ocr_releases_pdfium_lock_during_recognition() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = scanned_pages(&e, 0);
    let mut input = Some(doc.comparison_snapshot());
    let (tx, rx) = std::sync::mpsc::channel();
    let mut sender = Some(tx);
    let mut job = None;
    let result = e.ocr_pages_with(&mut doc, &[0], |_, _| {
        let independent = input.take().unwrap();
        let tx = sender.take().unwrap();
        job = Some(std::thread::spawn(move || {
            let e = engine();
            let count = e.pages(&independent).unwrap().len();
            tx.send(count).unwrap();
        }));
        let count = rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| err("PDFium stayed locked during recognition"))?;
        assert_eq!(count, 3);
        Ok(vec![])
    });
    if let Some(job) = job {
        job.join().unwrap();
    }
    result.unwrap();
}

#[test]
#[ignore = "Requires an installed Windows OCR recognition language"]
fn windows_ocr_selected_scans_are_searchable_and_atomic() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source = temp("native-batch-ocr.pdf");
    fs::write(
        &source,
        generated("HELLO WORLD", false, 0, 0., false, false),
    )
    .unwrap();
    let vector = e.open(&source).unwrap();
    let image = e.render(&vector, 0, 2.).unwrap();
    let picture = Picture {
        width: image.width,
        height: image.height,
        rgba: image.rgba,
    };
    let mut doc = scanned_pages(&e, 0);
    for page in [0, 1] {
        e.place_picture(
            &mut doc,
            &picture,
            Placement {
                page,
                left: 0.,
                bottom: 0.,
                width: 500.,
                height: 300.,
            },
        )
        .unwrap();
    }
    let mut doc = doc.comparison_snapshot();
    let original = doc.bytes.clone();
    let before = (0..2)
        .map(|p| e.render(&doc, p, 1.).unwrap().rgba)
        .collect::<Vec<_>>();
    let report = e.ocr_pages(&mut doc, &[0, 1, 2]).unwrap();
    assert_eq!(report.pages_recognized, 2);
    assert_eq!(report.skipped_text, 1);
    assert!(report.words_added >= 4);
    for page in [0, 1] {
        assert!(
            e.text_objects(&doc, page)
                .unwrap()
                .iter()
                .any(|o| o.text.to_uppercase().contains("HELLO"))
        );
        assert_eq!(
            e.render(&doc, page, 1.).unwrap().rgba,
            before[page as usize]
        );
    }
    assert_eq!(doc.undo.len(), 1);
    assert!(doc.undo());
    assert_eq!(doc.bytes, original);
    fs::remove_file(source).unwrap();
}
