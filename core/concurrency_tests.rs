use super::*;

#[test]
fn concurrent_document_workflows_preserve_text_pixels_and_history() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let e = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let source = root.join("tests/fixtures/generated/unicode-cid.pdf");
    let original = fs::read(&source).unwrap();
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for worker in 0..8 {
            let (e, source, original, barrier) = (&e, &source, &original, &barrier);
            scope.spawn(move || {
                barrier.wait();
                for iteration in 0..16 {
                    let mut doc = e.open(source).unwrap();
                    let before = e.render(&doc, 0, 0.5).unwrap();
                    let selected = e.text_objects(&doc, 0).unwrap().remove(0);
                    e.edit(&mut doc, &selected, "AB").unwrap_or_else(|error| {
                        panic!("worker={worker} iteration={iteration}: {error}")
                    });
                    assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, "AB");
                    assert!(!e.find_text(&doc, "AB").unwrap().is_empty());
                    assert!(e.form_fields(&doc).unwrap().is_empty());
                    assert!(doc.undo());
                    assert_eq!(&doc.bytes, original);
                    assert_eq!(e.render(&doc, 0, 0.5).unwrap().rgba, before.rgba);
                    assert!(doc.redo());
                    assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, "AB");
                    let output = root.join(format!(
                        "target/concurrent-{}-{worker}-{iteration}.pdf",
                        std::process::id()
                    ));
                    e.extract_pages(&doc, &[0], &output).unwrap();
                    let reopened = e.open(&output).unwrap();
                    assert_eq!(e.text_objects(&reopened, 0).unwrap()[0].text, "AB");
                    fs::remove_file(output).unwrap();
                }
            });
        }
    });
    assert_eq!(fs::read(source).unwrap(), original);
}
