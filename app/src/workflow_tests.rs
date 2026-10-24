// Real worker/PDFium integration driven through egui pointer and keyboard events.
// All files and configuration belong to generated test sessions.
#[test]
fn generated_recovery_snapshot_survives_crash_and_restores_unsaved_edits() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join(format!("target/recovery-workflow-{}", std::process::id()));
    let fixture = root.join("tests/fixtures/generated/unicode-cid.pdf");
    let original = std::fs::read(&fixture).unwrap();
    let ctx = Context::default();
    theme::install(&ctx);
    let (mut app, _, _) = mock_app();
    app.worker = Worker::spawn(
        ctx.clone(),
        root.join("target/debug/pdfium.dll"),
        Substitution::Line,
        Default::default(),
    );
    app.session = Some(Session::start_in(&folder).unwrap());
    app.autosave = AutoSave(Some(1));
    app.open_path(fixture.clone());
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.recovery_status(), "Recovery on");
    let at = workflow_line(&app, &ctx, 0);
    workflow_edit(&mut app, &ctx, at);
    workflow_frame(
        &mut app,
        &ctx,
        vec![workflow_key(Key::A, egui::Modifiers::COMMAND)],
    );
    workflow_frame(&mut app, &ctx, vec![egui::Event::Text("AB".into())]);
    assert_eq!(app.recovery_status(), "Draft not applied");
    workflow_click(&mut app, &ctx, pos2(950.0, 850.0));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.recovery_status(), "Recovery pending");
    app.doc.as_mut().unwrap().next_copy = Some(0.0);
    app.autosave_tick(&ctx);
    assert_eq!(app.recovery_status(), "Writing recovery…");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.doc.as_ref().unwrap().copying {
        app.poll(&ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(app.recovery_status(), "Recovery saved");
    assert!(
        recovery::orphans_in(&folder).is_empty(),
        "Live sessions must not be offered for restoration"
    );
    // Dropping the lock without normal close simulates process termination.
    drop(app.session.take());
    let mut found = recovery::orphans_in(&folder);
    assert_eq!(found.len(), 1);
    let orphan = found.remove(0);
    let copy = orphan.copy.clone();
    app.restoring = Some(orphan);
    app.open_path(copy.clone());
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(
        app.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .lines[0]
            .text,
        "AB"
    );
    let output = folder.join("restored-copy.pdf");
    app.password_action(Request::SaveAs(output.clone()));
    workflow_settle(&mut app, &ctx);
    assert!(
        !copy.exists(),
        "Successful Save As cleans the recovered snapshot"
    );
    assert_eq!(std::fs::read(&fixture).unwrap(), original);
    assert!(output.exists());
    std::fs::remove_file(output).unwrap();
    std::fs::remove_dir(&folder).unwrap();
}

#[test]
fn canvas_snapping_matches_the_committed_move_and_can_be_bypassed() {
    for kind in ["Text", "Image", "Path"] {
        for (enabled, alt, requested_dx, expected_dx) in [
            (true, false, 57.0, 60.0),
            (true, true, 57.0, 57.0),
            (false, false, 57.0, 57.0),
            (true, false, 117.0, 120.0),
        ] {
            let ctx = Context::default();
            theme::install(&ctx);
            let (mut app, inbox, _) = mock_app();
            opened(&mut app, &ctx);
            app.snap_enabled = enabled;
            app.doc.as_mut().unwrap().object_mode = true;
            let item = |index, left| pdf_editor_core::PageItem {
                page: 0,
                index,
                revision: 1,
                kind: kind.into(),
                bounds: pdf_editor_core::Bounds {
                    left,
                    bottom: 20.0,
                    right: left + 20.0,
                    top: 60.0,
                },
            };
            app.handle(
                &ctx,
                Reply::Objects {
                    generation: 1,
                    page: 0,
                    transform: PageTransform::identity(),
                    objects: vec![],
                    items: vec![item(0, 20.0), item(1, 100.0)],
                },
            );
            let modifiers = egui::Modifiers {
                alt,
                ..Default::default()
            };
            let frame = |app: &mut EditorApp, mut events: Vec<egui::Event>| {
                events.insert(0, egui::Event::ModifiersChanged(modifiers));
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 700.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let p = theme::palette(ui);
                        egui::CentralPanel::default().show(ui, |ui| app.canvas(ui, &p));
                    },
                );
                output.textures_delta.clear();
            };
            frame(&mut app, vec![]);
            frame(&mut app, vec![]);
            inbox.try_iter().for_each(drop);
            let rect = ctx
                .read_response(Id::new(("pdf-page", 0usize)))
                .unwrap()
                .rect;
            let start = rect.min + vec2(30.0, 40.0) * PT_TO_UI;
            let end = start + vec2(requested_dx, 0.0) * PT_TO_UI;
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                pressed,
                modifiers,
                button: egui::PointerButton::Primary,
            };
            frame(
                &mut app,
                vec![egui::Event::PointerMoved(start), button(start, true)],
            );
            frame(&mut app, vec![egui::Event::PointerMoved(end)]);
            let preview = app
                .doc
                .as_ref()
                .unwrap()
                .dragging_item
                .as_ref()
                .unwrap()
                .preview
                .unwrap();
            assert!((preview.left() - 20.0 - expected_dx).abs() < 0.01);
            assert!(inbox.try_recv().is_err(), "Preview must not mutate");
            frame(&mut app, vec![button(end, false)]);
            let Request::EditItems(_, pdf_editor_core::ObjectAction::Move { dx, dy }) =
                inbox.try_recv().unwrap()
            else {
                panic!()
            };
            assert!(
                (dx - expected_dx).abs() < 0.01,
                "{kind} enabled={enabled} alt={alt}: {dx}"
            );
            assert!(dy.abs() < 0.01);
        }
    }
}

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
    assert!(!app.tools_open);
    app.tools_query = "checkbox".into();
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
fn workflow_edit(app: &mut EditorApp, ctx: &Context, at: Pos2) {
    workflow_click(app,ctx,at);
    workflow_click(app,ctx,at);
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
fn generated_page_navigation_and_cached_tab_frame_timings() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let ctx = Context::default();
    theme::install(&ctx);
    let shared = crate::worker::SharedClipboard::default();
    let mut tabs = Vec::new();
    for _ in 0..2 {
        let (mut app, _, _) = mock_app();
        app.worker = Worker::spawn(
            ctx.clone(),
            root.join("target/debug/pdfium.dll"),
            Substitution::Line,
            shared.clone(),
        );
        tabs.push(app);
    }
    for iteration in 0..3 {
        for (tab, app) in tabs.iter_mut().enumerate() {
            let start = std::time::Instant::now();
            app.open_path(root.join("tests/fixtures/generated/multi-page.pdf"));
            workflow_settle(app, &ctx);
            println!(
                "navigation iteration={iteration} tab={tab} open_ready_us={}",
                start.elapsed().as_micros()
            );
            let last = app.doc.as_ref().unwrap().pages.len() - 1;
            assert!(last > 0);
            let start = std::time::Instant::now();
            app.go_to_page(last);
            for _ in 0..4 {
                workflow_frame(app, &ctx, vec![]);
            }
            workflow_settle(app, &ctx);
            assert_eq!(
                app.doc.as_ref().unwrap().current_page,
                last,
                "offset={:?} viewport={:?} pages={:?}",
                app.doc.as_ref().unwrap().offset,
                app.doc.as_ref().unwrap().viewport,
                app.doc
                    .as_ref()
                    .unwrap()
                    .pages
                    .iter()
                    .map(|p| (p.info.width, p.info.height))
                    .collect::<Vec<_>>()
            );
            println!(
                "navigation iteration={iteration} tab={tab} page_ready_us={}",
                start.elapsed().as_micros()
            );
        }
        // Returning to a tab paints its retained document, without reopening it.
        for (tab, app) in tabs.iter_mut().enumerate() {
            let generation = app.doc.as_ref().unwrap().generation;
            let start = std::time::Instant::now();
            workflow_frame(app, &ctx, vec![]);
            println!(
                "navigation iteration={iteration} tab={tab} cached_frame_us={}",
                start.elapsed().as_micros()
            );
            assert_eq!(app.doc.as_ref().unwrap().generation, generation);
            assert_eq!(app.busy, 0);
        }
    }
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
    workflow_edit(&mut source, &ctx, at);
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

#[test]
fn direct_object_selection_exposes_move_cursor_and_delete_key() {
    for kind in ["Text","Image","Path","XObjectForm"] {
        let ctx=Context::default();
        theme::install(&ctx);
        let (mut app,inbox,_)=mock_app();
        opened(&mut app,&ctx);
        app.handle(&ctx,Reply::Objects { generation:1,page:0,
            transform:PageTransform::identity(),objects:vec![],
            items:vec![pdf_editor_core::PageItem {page:0,index:0,revision:1,
                kind:kind.into(),bounds:pdf_editor_core::Bounds {
                    left:30.0,bottom:30.0,right:100.0,top:80.0
                }}] });
        let frame=|app:&mut EditorApp,events| { let mut output=ctx.run_ui(egui::RawInput {
            events,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(600.0,700.0))),
            ..Default::default()
        },|ui| {let p=theme::palette(ui);egui::CentralPanel::default().show(ui,|ui|app.canvas(ui,&p));}); output.textures_delta.clear(); output };
        frame(&mut app,vec![]);
        frame(&mut app,vec![]);
        inbox.try_iter().for_each(drop);
        let at=ctx.read_response(Id::new(("pdf-page",0usize))).unwrap().rect.min
            +vec2(60.0,50.0)*PT_TO_UI;
        let hover=frame(&mut app,vec![egui::Event::PointerMoved(at)]);
        assert_eq!(hover.platform_output.cursor_icon,CursorIcon::Grab,"{kind}");
        frame(&mut app,click_at(at));
        frame(&mut app,vec![]);
        assert!(app.doc.as_ref().unwrap().selected_item.is_some(),"{kind}");
        assert!(app.doc.as_ref().unwrap().edit.is_none());
        frame(&mut app,vec![workflow_key(Key::Delete,egui::Modifiers::NONE)]);
        assert!(inbox.try_iter().any(|r|matches!(r,Request::EditItems(items,
            pdf_editor_core::ObjectAction::Delete) if items.len()==1)),"{kind}");
    }
}
