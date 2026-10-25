// Generated transformed text through pointer selection, native TextEdit and real worker.
#[test]
fn rotated_text_edit_applies_in_place_and_undo_preserves_its_matrix() {
    let (mut app, ctx, _) = qol_worker();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let source = root.join("tests/fixtures/generated/transformed.pdf");
    let bytes = std::fs::read(&source).unwrap();
    app.open_path(source.clone());
    workflow_settle(&mut app, &ctx);
    let original = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines[0]
        .objects[0]
        .clone();
    assert!(original.matrix[1].abs() > 0.1);
    let at = workflow_line(&app, &ctx, 0);
    workflow_edit(&mut app, &ctx, at);
    let doc = app.doc.as_ref().unwrap();
    let edit = doc.edit.as_ref().unwrap();
    let page = ctx
        .read_response(Id::new(("pdf-page", 0usize)))
        .unwrap()
        .rect;
    let (plane, size) = affine_edit::line_plane(
        &edit.line,
        &doc.pages[0].objects.as_ref().unwrap().transform,
        page,
        doc.scale(),
    )
    .unwrap();
    let start = plane.point(pos2(0., size.y / 2.));
    let end = plane.point(pos2(size.x.max(40.) + 100., size.y / 2.));
    for events in [
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    ] {
        workflow_frame(&mut app, &ctx, events);
    }
    assert!(app.doc.as_ref().unwrap().edit.is_some());
    workflow_frame(&mut app, &ctx, vec![egui::Event::Text("BA".into())]);
    workflow_frame(
        &mut app,
        &ctx,
        vec![workflow_key(Key::Enter, Default::default())],
    );
    workflow_settle(&mut app, &ctx);
    let changed = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines[0]
        .objects[0]
        .clone();
    assert_eq!(changed.text, "BA");
    assert_eq!(changed.matrix, original.matrix);
    assert!(app.doc.as_ref().unwrap().dirty());
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert_eq!(
        app.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .lines[0]
            .objects[0]
            .text,
        original.text
    );
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
}
