#[test]
fn ocr_uses_selected_pages_and_all_skipped_keeps_document_clean() {
    let ctx = Context::default();
    let (mut app, inbox, _outbox) = mock_app();
    opened(&mut app, &ctx);
    app.qol.pages.insert(2);
    app.qol.pages.insert(0);
    app.open_tool(Tool::Ocr);
    assert!(matches!(inbox.try_recv().unwrap(),Request::OcrPages(pages) if pages==vec![0,2]));
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    let generation = app.doc.as_ref().unwrap().generation;
    assert!(!app.doc.as_ref().unwrap().dirty());
    app.open_tool(Tool::Ocr);
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.doc.as_ref().unwrap().generation, generation);
    assert!(!app.doc.as_ref().unwrap().dirty());
    assert!(app.banners.iter().any(|b| b.text.contains("1 with text")));
    assert_eq!(std::fs::read(source).unwrap(), original);
}
