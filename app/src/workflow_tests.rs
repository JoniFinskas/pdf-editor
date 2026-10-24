// Real worker/PDFium integration driven through egui pointer and keyboard events.
// All files and configuration belong to generated test sessions.
#[test]
fn catalog_keyboard_search_categories_and_disabled_actions() {
    let ctx = Context::default();
    theme::install(&ctx);
    let (mut app, inbox, _) = mock_app();
    let draw = |app: &mut EditorApp, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(360.0, 600.0))),
                events,
                ..Default::default()
            },
            |ui| {
                let p = theme::palette(ui);
                app.tool_catalog(ui, &p);
            },
        );
        output.textures_delta.clear();
    };
    app.tools_focus = true;
    app.tools_query = "diff".into();
    draw(&mut app, vec![]);
    draw(&mut app, vec![]);
    draw(
        &mut app,
        vec![workflow_key(Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.compare.open);
    app.tools_query = "forms".into();
    app.tools_focus = true;
    draw(&mut app, vec![]);
    draw(
        &mut app,
        vec![workflow_key(Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!app.forms.open);
    assert!(inbox.try_recv().is_err());
    app.tools_query = "recovery".into();
    app.tools_focus = true;
    draw(&mut app, vec![]);
    draw(
        &mut app,
        vec![workflow_key(Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.settings_open);
    opened(&mut app, &ctx);
    app.tools_query.clear();
    app.tools_category = "Review & print".into();
    app.tools_selected = 0;
    app.tools_focus = true;
    draw(&mut app, vec![]);
    draw(
        &mut app,
        vec![workflow_key(Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert_eq!(app.tools_selected, 1);
    draw(
        &mut app,
        vec![workflow_key(Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(matches!(inbox.try_recv().unwrap(), Request::Print));
}

fn workflow_frame(app: &mut EditorApp, ctx: &Context, events: Vec<egui::Event>) {
    app.poll(ctx);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 900.0))),
            events,
            ..Default::default()
        },
        |ui| {
            app.shortcuts(ui.ctx());
            let p = theme::palette(ui);
            egui::CentralPanel::default().show(ui, |ui| app.canvas(ui, &p));
        },
    );
    output.textures_delta.clear();
}

fn workflow_settle(app: &mut EditorApp, ctx: &Context) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        workflow_frame(app, ctx, vec![]);
        assert!(
            !app.banners.iter().any(|b| b.kind == BannerKind::Error),
            "{}",
            app.banners
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
        let ready = app.busy == 0
            && app.doc.as_ref().is_some_and(|d| {
                let page = &d.pages[d.current_page];
                page.objects
                    .as_ref()
                    .is_some_and(|o| o.generation == d.generation)
                    && page.pending_render.is_none()
            });
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Workflow did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    workflow_frame(app, ctx, vec![]);
}

fn workflow_click(app: &mut EditorApp, ctx: &Context, at: Pos2) {
    let events = click_at(at);
    workflow_frame(app, ctx, events[..2].to_vec());
    workflow_frame(app, ctx, events[2..].to_vec());
}

fn workflow_key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn workflow_line(app: &EditorApp, ctx: &Context, index: usize) -> Pos2 {
    let doc = app.doc.as_ref().unwrap();
    let rect = ctx
        .read_response(Id::new(("pdf-page", 0usize)))
        .unwrap()
        .rect;
    rect.min
        + doc.pages[0].objects.as_ref().unwrap().lines[index]
            .rect
            .center()
            .to_vec2()
            * doc.scale()
}

#[test]
fn generated_workflow_edits_moves_resizes_copies_and_saves_through_real_workers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let fixture = root.join("tests/fixtures/generated/unicode-cid.pdf");
    let original = std::fs::read(&fixture).unwrap();
    let shared = crate::worker::SharedClipboard::default();
    let create = || {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, _, _) = mock_app();
        app.worker = Worker::spawn(
            ctx.clone(),
            root.join("target/debug/pdfium.dll"),
            Substitution::Line,
            shared.clone(),
        );
        app.open_path(fixture.clone());
        workflow_settle(&mut app, &ctx);
        (app, ctx)
    };
    let (mut source, ctx) = create();
    let at = workflow_line(&source, &ctx, 0);
    workflow_click(&mut source, &ctx, at);
    assert!(source.doc.as_ref().unwrap().edit.is_some());
    workflow_frame(
        &mut source,
        &ctx,
        vec![workflow_key(Key::A, egui::Modifiers::COMMAND)],
    );
    workflow_frame(&mut source, &ctx, vec![egui::Event::Text("AB".into())]);
    assert_eq!(
        source.doc.as_ref().unwrap().edit.as_ref().unwrap().text,
        "AB"
    );
    // Click away applies once; the actual worker must preserve the new content.
    workflow_click(&mut source, &ctx, pos2(950.0, 850.0));
    workflow_settle(&mut source, &ctx);
    let text = |app: &EditorApp| {
        app.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .lines[0]
            .text
            .clone()
    };
    assert_eq!(text(&source), "AB");
    source.undo();
    workflow_settle(&mut source, &ctx);
    assert_ne!(text(&source), "AB");
    source.redo();
    workflow_settle(&mut source, &ctx);
    assert_eq!(text(&source), "AB");
    source.set_object_mode(true);
    workflow_frame(&mut source, &ctx, vec![]);
    let start = workflow_line(&source, &ctx, 0);
    let pointer = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    let old = source.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .items[0]
        .bounds;
    workflow_frame(
        &mut source,
        &ctx,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    let end = start + vec2(40.0, 25.0);
    workflow_frame(&mut source, &ctx, vec![egui::Event::PointerMoved(end)]);
    workflow_frame(&mut source, &ctx, vec![pointer(end, false)]);
    workflow_settle(&mut source, &ctx);
    let moved = source.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .items[0]
        .bounds;
    assert!(moved.left > old.left + 20.0);
    let at = workflow_line(&source, &ctx, 0);
    workflow_click(&mut source, &ctx, at);
    let doc = source.doc.as_ref().unwrap();
    let objects = doc.pages[0].objects.as_ref().unwrap();
    let bounds = bounds_rect(
        &objects.transform,
        doc.selected_item.as_ref().unwrap().bounds,
    );
    let rect = ctx
        .read_response(Id::new(("pdf-page", 0usize)))
        .unwrap()
        .rect;
    let handle = rect.min + resize_handles(bounds)[4].to_vec2() * doc.scale();
    workflow_frame(
        &mut source,
        &ctx,
        vec![egui::Event::PointerMoved(handle), pointer(handle, true)],
    );
    let end = handle + vec2(18.0, 12.0);
    workflow_frame(&mut source, &ctx, vec![egui::Event::PointerMoved(end)]);
    workflow_frame(&mut source, &ctx, vec![pointer(end, false)]);
    workflow_settle(&mut source, &ctx);
    let resized = source.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .items[0]
        .bounds;
    assert!(resized.right - resized.left > moved.right - moved.left + 5.0);
    let at = workflow_line(&source, &ctx, 0);
    workflow_click(&mut source, &ctx, at);
    workflow_frame(&mut source, &ctx, vec![egui::Event::Copy]);
    workflow_settle(&mut source, &ctx);
    let (mut destination, other_ctx) = create();
    destination.set_object_mode(true);
    let count = destination.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .items
        .len();
    workflow_frame(
        &mut destination,
        &other_ctx,
        vec![egui::Event::Paste(String::new())],
    );
    workflow_settle(&mut destination, &other_ctx);
    assert_eq!(
        destination.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .items
            .len(),
        count + 1
    );
    destination.undo();
    workflow_settle(&mut destination, &other_ctx);
    assert_eq!(
        text(&source),
        "AB",
        "Destination undo must not affect the source tab"
    );
    destination.redo();
    workflow_settle(&mut destination, &other_ctx);
    let output = root.join(format!("target/workflow-saved-{}.pdf", std::process::id()));
    if output.exists() {
        std::fs::remove_file(&output).unwrap();
    }
    destination.password_action(Request::SaveAs(output.clone()));
    workflow_settle(&mut destination, &other_ctx);
    assert!(!destination.doc.as_ref().unwrap().dirty());
    destination.open_path(output.clone());
    workflow_settle(&mut destination, &other_ctx);
    assert!(
        destination.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .any(|l| l.text == "AB")
    );
    assert_eq!(std::fs::read(fixture).unwrap(), original);
    std::fs::remove_file(output).unwrap();
}
