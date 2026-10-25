// Generated-document integration for selection, batch operations and working copies.
#[test]
fn qol_find_controls_fit_the_minimum_window() {
    let ctx=Context::default();theme::install(&ctx);
    let (mut app,_,_)=mock_app();opened(&mut app,&ctx);app.find.open=true;
    let bounds=Rect::from_min_size(Pos2::ZERO,vec2(640.0,420.0));
    for _ in 0..3 {let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(bounds),..Default::default()},|ui|app.floating_tools(ui.ctx(),bounds,&theme::palette(ui)));out.textures_delta.clear();}
    for key in ["search-options","find-previous","find-next","find-close"] {let rect=ctx.read_response(widgets::tool_id(key)).unwrap().rect;assert!(bounds.contains_rect(rect),"{key}: {rect:?}");}
}

fn qol_worker() -> (EditorApp, Context, PathBuf) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let ctx = Context::default();
    theme::install(&ctx);
    let (mut app, _, _) = mock_app();
    app.worker = Worker::spawn(
        ctx.clone(),
        root.join("target/debug/pdfium.dll"),
        Substitution::Line,
        Default::default(),
    );
    let fixture = root.join("tests/fixtures/generated/unicode-cid.pdf");
    app.open_path(fixture.clone());
    workflow_settle(&mut app, &ctx);
    (app, ctx, fixture)
}

#[test]
fn qol_text_highlight_batch_pages_and_working_copy_survive_real_worker_roundtrips() {
    let (mut app, ctx, fixture) = qol_worker();
    let source = std::fs::read(&fixture).unwrap();
    app.qol.text_mode = true;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.qol.characters.contains_key(&0) {
        workflow_frame(&mut app, &ctx, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let chars = &app.qol.characters[&0];
    let last = chars
        .iter()
        .position(|c| c.text.contains(['\r', '\n']))
        .unwrap_or(chars.len())
        .saturating_sub(1);
    let expected = pdf_editor_core::selected_text(chars, 0, last).text;
    let doc = app.doc.as_ref().unwrap();
    let objects = doc.pages[0].objects.as_ref().unwrap();
    let rect = ctx
        .read_response(Id::new(("pdf-page", 0usize)))
        .unwrap()
        .rect;
    let point = |index: usize| {
        rect.min
            + bounds_rect(&objects.transform, chars[index].bounds)
                .center()
                .to_vec2()
                * doc.scale()
    };
    let (start, end) = (point(0), point(last));
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    workflow_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    workflow_frame(&mut app, &ctx, vec![button(end, false)]);
    let (copied, regions) = app.qol.selection().unwrap();
    assert_eq!(copied, expected);
    assert!(!regions.is_empty());
    app.busy += 1;
    app.send(Request::HighlightText(regions));
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert!(app.doc.as_ref().unwrap().can_undo);
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    app.redo();
    workflow_settle(&mut app, &ctx);
    app.qol.text_mode = false;
    for batch in [
        pdf_editor_core::PageBatch::Duplicate {
            pages: vec![0],
            before: 1,
        },
        pdf_editor_core::PageBatch::Blank {
            before: 1,
            width: 300.0,
            height: 400.0,
        },
        pdf_editor_core::PageBatch::Rotate(vec![0, 2]),
        pdf_editor_core::PageBatch::Move {
            pages: vec![0, 2],
            before: 3,
        },
        pdf_editor_core::PageBatch::Delete(vec![0]),
    ] {
        app.busy += 1;
        app.send(Request::PageBatch(batch));
        workflow_settle(&mut app, &ctx);
    }
    assert_eq!(app.doc.as_ref().unwrap().pages.len(), 2);
    let root = fixture
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let folder = root.join(format!("target/qol-workflow-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let copy = folder.join("working.pdf");
    app.document_action(Request::SaveAs(copy.clone()));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.qol.working_copy.as_ref(), Some(&copy));
    assert!(!app.doc.as_ref().unwrap().dirty());
    for width in [310.0, 320.0] {
        app.busy += 1;
        app.send(Request::PageBatch(pdf_editor_core::PageBatch::Blank {
            before: 0,
            width,
            height: 400.0,
        }));
        workflow_settle(&mut app, &ctx);
        workflow_frame(
            &mut app,
            &ctx,
            vec![workflow_key(Key::S, egui::Modifiers::COMMAND)],
        );
        workflow_settle(&mut app, &ctx);
        assert!(!app.doc.as_ref().unwrap().dirty());
        let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let saved = engine.open(&copy).unwrap();
        assert_eq!(engine.pages(&saved).unwrap()[0].width, width);
    }
    std::fs::write(&copy, b"generated external change").unwrap();
    app.save();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.busy > 0 {
        app.poll(&ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        app.banners
            .iter()
            .any(|b| b.kind == BannerKind::Error && b.text.contains("Save As"))
    );
    assert_eq!(std::fs::read(&copy).unwrap(), b"generated external change");
    assert_eq!(std::fs::read(&fixture).unwrap(), source);
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_dir(folder).unwrap();
}

#[test]
fn qol_multiple_object_nudges_keep_selection_and_advanced_search_uses_options() {
    let (mut app, ctx, _) = qol_worker();
    let objects = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
    let picked: Vec<_> = objects
        .items
        .iter()
        .filter(|i| i.kind == "Text")
        .take(2)
        .cloned()
        .collect();
    assert_eq!(picked.len(), 2);
    app.qol.objects = picked.clone();
    app.doc.as_mut().unwrap().selected_item = app.qol.primary();
    for _ in 0..2 {
        workflow_frame(
            &mut app,
            &ctx,
            vec![workflow_key(Key::ArrowRight, egui::Modifiers::NONE)],
        );
        workflow_settle(&mut app, &ctx);
        assert_eq!(app.qol.objects.len(), 2);
    }
    for (before, after) in picked.iter().zip(&app.qol.objects) {
        assert!((after.bounds.left - before.bounds.left - 2.0).abs() < 0.05);
    }
    app.busy += 1;
    app.send(Request::LayoutItems(
        app.qol.objects.clone(),
        pdf_editor_core::ObjectLayout::AlignLeft,
    ));
    workflow_settle(&mut app, &ctx);
    assert!((app.qol.objects[0].bounds.left - app.qol.objects[1].bounds.left).abs() < 0.05);
    app.qol.search_open = true;
    app.qol.whole_word = true;
    app.qol.case_sensitive = true;
    app.qol.scope = 1;
    let text = "AB".to_owned();
    app.send(Request::Characters(0));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.qol.characters.contains_key(&0) {
        app.poll(&ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let native = app.qol.characters[&0]
        .iter()
        .map(|c| c.text.as_str())
        .collect::<String>();
    assert!(
        native.contains(&text),
        "Run query {text:?} differs from reading order {native:?}"
    );
    app.find.query = text.clone();
    app.send(Request::Find(text));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.qol.hits.is_empty() {
        app.poll(&ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!app.find.matches.is_empty());
    assert!(
        app.qol
            .hits
            .iter()
            .all(|h| h.page == 0 && !h.excerpt.is_empty())
    );
    app.qol.pending_view = Some(crate::session_state::SessionTab {
        source: PathBuf::from("generated.pdf"),
        recovery: None,
        current_page: 0,
        zoom: 1.5,
        scroll: 80.0,
    });
    app.sessions_apply_view();
    assert_eq!(app.doc.as_ref().unwrap().zoom, 1.5);
    assert_eq!(app.doc.as_ref().unwrap().scroll_to, Some(vec2(0.0, 80.0)));
}

#[test]
fn qol_windows_keep_close_reachable_in_small_light_and_dark_views() {
    for dark in [false, true] {
        for size in [vec2(640.0, 420.0), vec2(1180.0, 820.0)] {
            let ctx = Context::default();
            theme::install(&ctx);
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            for title in [
                "Selection tools",
                "Search results",
                "Text formatting",
                "Insert blank page",
            ] {
                let (mut app, _, _) = mock_app();
                opened(&mut app, &ctx);
                app.qol.arrange_open = title == "Selection tools";
                app.qol.search_open = title == "Search results";
                app.qol.format_open = title == "Text formatting";
                app.qol.pages_open = title == "Insert blank page";
                app.busy = 1;
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |ui| app.qol_windows(ui.ctx(), &theme::palette(ui)),
                    );
                    output.textures_delta.clear();
                }
                let close = ctx
                    .read_response(widgets::tool_id(&format!("close-tool-{title}")))
                    .unwrap();
                assert!(
                    Rect::from_min_size(Pos2::ZERO, size).contains_rect(close.rect),
                    "{title}: {:?}",
                    close.rect
                );
            }
        }
    }
}

#[test]
fn qol_search_options_entry_and_font_dialog_keep_their_targets() {
    let ctx = Context::default();
    theme::install(&ctx);
    let (mut app, inbox, _) = mock_app();
    opened(&mut app, &ctx);
    tool_frame(&mut app, &ctx, vec![], 2);
    let at = tool_center(&ctx, "search-options");
    let events = click_at(at);
    tool_frame(&mut app, &ctx, events[..2].to_vec(), 2);
    tool_frame(&mut app, &ctx, events[2..].to_vec(), 2);
    assert!(app.qol.search_open);
    assert!(matches!(
        inbox.try_recv().unwrap(),
        Request::FindOptions(..)
    ));
    let (mut app, ctx, _) = qol_worker();
    let at = workflow_line(&app, &ctx, 0);
    workflow_edit(&mut app, &ctx, at);
    assert!(!app.text_tool.selection.is_empty());
    app.qol.format_targets = app.formatting_targets();
    app.qol.format_open = true;
    let count = app.qol.format_targets.len();
    app.commit_edit();
    assert!(app.text_tool.selection.is_empty());
    assert_eq!(app.qol.format_targets.len(), count);
    assert!(count > 0);
}

#[test]
fn qol_failed_recovery_retries_and_replacement_counts_reject_changed_scope() {
    let ctx = Context::default();
    let (mut app, _, _) = mock_app();
    opened(&mut app, &ctx);
    app.autosave = AutoSave(Some(1));
    let doc = app.doc.as_mut().unwrap();
    doc.changes.edited();
    doc.copying = true;
    let generation = doc.generation;
    app.handle(
        &ctx,
        Reply::Snapshot {
            generation,
            path: PathBuf::from("target/missing-generated-recovery.pdf"),
            error: Some("simulated disk error".into()),
        },
    );
    assert!(!app.doc.as_ref().unwrap().copying);
    assert_ne!(
        app.doc.as_ref().unwrap().copied_generation,
        Some(generation)
    );
    assert!(app.doc.as_ref().unwrap().next_copy.is_some());
    app.replace.query = "AB".into();
    app.qol.scope = 1;
    app.qol.replace_context = Some((generation, app.current_search_options()));
    app.doc.as_mut().unwrap().current_page = 1;
    app.handle(&ctx, Reply::ReplacementCount("AB".into(), 4));
    assert!(app.replace.count.is_none());
}
