#[test]
fn generated_text_drag_tracks_pointer_keeps_ink_and_commits_once_on_release() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    app.snap_enabled = false;
    app.set_object_mode(true);
    workflow_frame(&mut app, &ctx, vec![]);
    let objects = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
    let before = objects.lines[0].objects[0].clone();
    let transform = objects.transform;
    let start = workflow_line(&app, &ctx, 0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    };
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.drag_preview.ready() {
        workflow_frame(&mut app, &ctx, vec![]);
        assert!(
            std::time::Instant::now() < deadline,
            "Native drag layers did not arrive"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let generation = app.doc.as_ref().unwrap().generation;
    let scale = app.doc.as_ref().unwrap().scale();
    for offset in [
        vec2(12., 5.),
        vec2(13., 6.),
        vec2(44., 28.),
        vec2(30., 20.),
        vec2(80., 60.),
    ] {
        workflow_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(start + offset)],
        );
        for _ in 0..2 {
            let doc = app.doc.as_ref().unwrap();
            let drag = doc.dragging_item.as_ref().unwrap();
            assert!((drag.delta - offset / scale).length() < 0.01);
            assert!((drag.preview.unwrap().min - drag.bounds.min - offset / scale).length() < 0.01);
            assert_eq!(doc.generation, generation);
            assert_eq!(app.busy, 0);
            assert!(!doc.dirty());
            workflow_frame(&mut app, &ctx, vec![]);
        }
    }
    workflow_frame(&mut app, &ctx, vec![button(start + vec2(90., 65.), false)]);
    assert_eq!(app.busy, 1);
    assert!(app.drag_preview.committing());
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.doc.as_ref().unwrap().generation, generation + 1);
    assert!(!app.drag_preview.committing());
    assert!(app.doc.as_ref().unwrap().dirty());
    let moved = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .flat_map(|line| &line.objects)
        .find(|object| object.index == before.index)
        .unwrap();
    let (dx, dy) = raster_delta(&transform, vec2(90., 65.) / scale).unwrap();
    assert!((moved.matrix[4] - before.matrix[4] - dx).abs() < 0.05);
    assert!((moved.matrix[5] - before.matrix[5] - dy).abs() < 0.05);
    assert_eq!(moved.text, before.text);
    assert_eq!(moved.font, before.font);
    assert_eq!(moved.color, before.color);
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    app.redo();
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn escape_cancels_drag_without_mutating_generated_document() {
    let (mut app, ctx, _) = qol_worker();
    app.snap_enabled = false;
    app.set_object_mode(true);
    workflow_frame(&mut app, &ctx, vec![]);
    let start = workflow_line(&app, &ctx, 0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    };
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start + vec2(70., 50.))],
    );
    assert!(app.doc.as_ref().unwrap().dragging_item.is_some());
    workflow_frame(
        &mut app,
        &ctx,
        vec![workflow_key(Key::Escape, Default::default())],
    );
    assert!(app.doc.as_ref().unwrap().dragging_item.is_none());
    workflow_frame(&mut app, &ctx, vec![button(start + vec2(70., 50.), false)]);
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
}
