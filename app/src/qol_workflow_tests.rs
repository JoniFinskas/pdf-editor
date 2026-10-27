// Generated-document integration for selection, batch operations and working copies.
#[test]
fn generated_identical_font_format_preserves_saved_state_previews_and_redo() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    let object = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    let format = pdf_editor_core::TextFormat {
        font: "Arial".into(), bold: false, italic: false, size: object.size,
        color: object.color[..3].try_into().unwrap(),
    };
    app.document_action(Request::FormatText(vec![object], format.clone()));
    workflow_settle(&mut app, &ctx);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let copy = root.join(format!("target/font-noop-workflow-{}.pdf", std::process::id()));
    if copy.exists() { std::fs::remove_file(&copy).unwrap(); }
    app.document_action(Request::SaveAs(copy.clone()));
    workflow_settle(&mut app, &ctx);
    let saved = std::fs::read(&copy).unwrap();
    assert!(!app.doc.as_ref().unwrap().dirty());
    let object = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    app.document_action(Request::StyleText { object: object.clone(), size: object.size + 2., color: [30, 60, 90] });
    workflow_settle(&mut app, &ctx);
    app.document_action(Request::Undo);
    workflow_settle(&mut app, &ctx);
    let object = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    let generation = app.doc.as_ref().unwrap().generation;
    for _ in 0..3 {
        app.document_action(Request::FormatText(vec![object.clone()], format.clone()));
        workflow_settle(&mut app, &ctx);
        let doc = app.doc.as_ref().unwrap();
        assert_eq!(doc.generation, generation);
        assert!(!doc.dirty());
        assert!(doc.can_redo);
        assert!(doc.pages[0].objects.is_some());
    }
    app.document_action(Request::Redo);
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(&copy).unwrap(), saved);
    assert_eq!(std::fs::read(source).unwrap(), original);
    std::fs::remove_file(copy).unwrap();
}

#[test]
fn generated_identical_find_replace_keeps_clean_state_previews_and_redo() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    let object = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    let query = object.text.clone();
    app.document_action(Request::DeleteText(object));
    workflow_settle(&mut app, &ctx);
    app.document_action(Request::Undo);
    workflow_settle(&mut app, &ctx);
    let generation = app.doc.as_ref().unwrap().generation;
    for advanced in [false, true] {
        let request = if advanced {
            Request::ReplaceOptions { query: query.clone(), text: query.clone(), occurrence: None, options: Default::default() }
        } else {
            Request::Replace { query: query.clone(), text: query.clone(), occurrence: None }
        };
        app.document_action(request);
        workflow_settle(&mut app, &ctx);
        let doc = app.doc.as_ref().unwrap();
        assert!(!doc.dirty(), "Identical replacement marked the document dirty");
        assert_eq!(doc.generation, generation);
        assert!(doc.can_redo);
        assert!(!doc.can_undo);
        assert!(doc.pages[0].objects.is_some());
    }
    app.document_action(Request::Redo);
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn generated_unchanged_style_preserves_clean_state_previews_and_redo() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    for with_redo in [false, true] {
        let object = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
        if with_redo {
            app.document_action(Request::StyleText { object: object.clone(), size: object.size + 2., color: [30, 60, 90] });
            workflow_settle(&mut app, &ctx);
            assert!(app.doc.as_ref().unwrap().dirty());
            app.document_action(Request::Undo);
            workflow_settle(&mut app, &ctx);
        }
        let object = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
        let generation = app.doc.as_ref().unwrap().generation;
        for _ in 0..3 {
            app.document_action(Request::StyleText {
                object: object.clone(), size: object.size, color: object.color[..3].try_into().unwrap(),
            });
            workflow_settle(&mut app, &ctx);
            let doc = app.doc.as_ref().unwrap();
            assert_eq!(doc.generation, generation, "Unchanged style invalidated previews");
            assert!(!doc.dirty());
            assert!(!doc.can_undo);
            assert_eq!(doc.can_redo, with_redo);
            assert!(doc.pages[0].objects.is_some());
        }
    }
    app.document_action(Request::Redo);
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_ref().unwrap();
    assert!(doc.dirty());
    assert!(doc.can_undo);
    assert!(!doc.can_redo);
    let object = &doc.pages[0].objects.as_ref().unwrap().lines[0].objects[0];
    assert_eq!(object.color[..3], [30, 60, 90]);
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn generated_zoom_anchor_stays_on_a_later_page() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    for before in 1..=3 {
        app.document_action(Request::PageBatch(pdf_editor_core::PageBatch::Duplicate {
            pages: vec![0],
            before,
        }));
        workflow_settle(&mut app, &ctx);
    }
    app.go_to_page(1);
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_ref().unwrap();
    let zoom = doc.zoom;
    let generation = doc.generation;
    let page = ctx.read_response(Id::new(("pdf-page", 1usize))).unwrap().rect;
    let anchor = pos2(doc.viewport.center().x, doc.viewport.top() + 140.0);
    assert!(page.contains(anchor));
    let local = (anchor - page.min) / doc.scale();
    for next in [zoom * 1.5, zoom] {
        app.set_zoom(next, Some(anchor));
        workflow_settle(&mut app, &ctx);
        let doc = app.doc.as_ref().unwrap();
        let page = ctx.read_response(Id::new(("pdf-page", 1usize))).unwrap().rect;
        let actual = page.min + local * doc.scale();
        assert!((actual.y - anchor.y).abs() < 1.1, "Zoom moved the anchored page point: {actual:?} versus {anchor:?}");
        assert_eq!(doc.generation, generation);
    }
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn generated_zoom_anchor_stays_fixed_when_centered_page_overflows() {
    let (mut app, ctx, _) = qol_worker();
    let doc = app.doc.as_ref().unwrap();
    let zoom = 0.9 * (doc.viewport.width() - 2.0 * CANVAS_PAD)
        / (doc.pages[0].info.width * PT_TO_UI);
    app.set_zoom(zoom, None);
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_ref().unwrap();
    let page = ctx.read_response(Id::new(("pdf-page", 0usize))).unwrap().rect;
    let anchor = doc.viewport.center();
    let local = (anchor - page.min) / doc.scale();
    let generation = doc.generation;
    assert!(page.width() < doc.viewport.width());
    for next in [zoom * 2.0, zoom] {
        app.set_zoom(next, None);
        workflow_settle(&mut app, &ctx);
        let doc = app.doc.as_ref().unwrap();
        let page = ctx.read_response(Id::new(("pdf-page", 0usize))).unwrap().rect;
        let actual = page.min + local * doc.scale();
        assert!((actual.x - anchor.x).abs() < 1.1, "Zoom moved the anchored page point: {actual:?} versus {anchor:?}");
        assert_eq!(doc.generation, generation);
    }
}

#[test]
fn generated_zoom_anchor_uses_the_width_of_the_pointed_page() {
    let (mut app, ctx, _) = qol_worker();
    app.document_action(Request::PageBatch(pdf_editor_core::PageBatch::Blank {
        before: 1,
        width: 720.0,
        height: 500.0,
    }));
    workflow_settle(&mut app, &ctx);
    for before in 2..=3 {
        app.document_action(Request::PageBatch(pdf_editor_core::PageBatch::Duplicate {
            pages: vec![0],
            before,
        }));
        workflow_settle(&mut app, &ctx);
    }
    app.set_zoom(1.0, None);
    app.go_to_page(2);
    workflow_settle(&mut app, &ctx);
    // The navigation target is clamped at the document end after layout.
    for _ in 0..2 {
        workflow_frame(&mut app, &ctx, vec![]);
    }
    let doc = app.doc.as_ref().unwrap();
    let page = ctx.read_response(Id::new(("pdf-page", 2usize))).unwrap().rect;
    let anchor = pos2(doc.viewport.center().x, doc.viewport.top() + 140.0);
    assert!(page.contains(anchor));
    let local = (anchor - page.min) / doc.scale();
    let generation = doc.generation;
    app.set_zoom(1.5, Some(anchor));
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_ref().unwrap();
    let page = ctx.read_response(Id::new(("pdf-page", 2usize))).unwrap().rect;
    let actual = page.min + local * doc.scale();
    assert!((actual - anchor).length() < 1.5, "Mixed page sizes moved the anchor: {actual:?} versus {anchor:?}");
    assert_eq!(doc.generation, generation);
}

#[test]
fn generated_zoom_anchor_keeps_page_gap_spacing_fixed() {
    let (mut app, ctx, _) = qol_worker();
    for before in 1..=3 {
        app.document_action(Request::PageBatch(pdf_editor_core::PageBatch::Duplicate {
            pages: vec![0],
            before,
        }));
        workflow_settle(&mut app, &ctx);
    }
    app.go_to_page(1);
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_mut().unwrap();
    doc.scroll_to = Some(doc.offset - vec2(0.0, 120.0));
    workflow_settle(&mut app, &ctx);
    let first = ctx.read_response(Id::new(("pdf-page", 0usize))).unwrap().rect;
    let second = ctx.read_response(Id::new(("pdf-page", 1usize))).unwrap().rect;
    let doc = app.doc.as_ref().unwrap();
    let anchor = pos2(doc.viewport.center().x, (first.bottom() + second.top()) / 2.0);
    assert!(doc.viewport.contains(anchor));
    let gap_offset = anchor.y - first.bottom();
    let next = doc.zoom * 1.5;
    app.set_zoom(next, Some(anchor));
    workflow_settle(&mut app, &ctx);
    let first = ctx.read_response(Id::new(("pdf-page", 0usize))).unwrap().rect;
    assert!((anchor.y - first.bottom() - gap_offset).abs() < 1.1);
}

#[test]
fn generated_zoom_anchor_accumulates_requests_before_the_next_frame() {
    let (mut app, ctx, _) = qol_worker();
    app.set_zoom(2.0, None);
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_mut().unwrap();
    doc.scroll_to = Some(vec2(200.0, 0.0));
    workflow_settle(&mut app, &ctx);
    for _ in 0..2 {
        workflow_frame(&mut app, &ctx, vec![]);
    }
    let doc = app.doc.as_ref().unwrap();
    let page = ctx.read_response(Id::new(("pdf-page", 0usize))).unwrap().rect;
    let anchor = doc.viewport.center();
    let local = (anchor - page.min) / doc.scale();
    app.set_zoom(2.5, Some(anchor));
    app.set_zoom(3.0, Some(anchor));
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_ref().unwrap();
    let page = ctx.read_response(Id::new(("pdf-page", 0usize))).unwrap().rect;
    let actual = page.min + local * doc.scale();
    assert!((actual.x - anchor.x).abs() < 1.1, "Queued zooms moved the anchor: {actual:?} versus {anchor:?}");
}

#[test]
fn qol_generated_search_replies_cannot_restore_an_old_scope() {
    fn next_search(app: &mut EditorApp, ctx: &Context) -> Reply {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut search = None;
            for reply in app.worker.poll().expect("Generated worker disconnected") {
                if matches!(reply, Reply::Found { .. } | Reply::SearchResults { .. }) {
                    assert!(search.is_none());
                    search = Some(reply);
                } else {
                    app.handle(ctx, reply);
                }
            }
            if let Some(reply) = search {
                return reply;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Search reply timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    app.document_action(Request::PageBatch(pdf_editor_core::PageBatch::Duplicate {
        pages: vec![0],
        before: 1,
    }));
    workflow_settle(&mut app, &ctx);
    for page in [0, 1] {
        app.send(Request::Characters(page));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.qol.characters.len() < 2 {
        app.poll(&ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let first = app.qol.characters[&0]
        .iter()
        .position(|c| c.text == "A")
        .unwrap();
    let last = app.qol.characters[&0]
        .iter()
        .position(|c| c.text == "B")
        .unwrap();
    // Hold genuine worker results while changing the UI, without relying on timing.
    for refresh_only in [false, true] {
        for case in 0..6 {
            app.qol.search_open = case != 0;
            app.qol.case_sensitive = false;
            app.qol.whole_word = false;
            app.qol.scope = match case {
                1 => 1,
                4 => 2,
                5 => 3,
                _ => 0,
            };
            app.qol.pages.clear();
            app.qol.pages.insert(0);
            app.qol.anchor = Some((0, first));
            app.qol.end = Some((0, last));
            app.go_to_page(0);
            app.find.query = match case {
                2 => "ab",
                3 => "A",
                _ => "AB",
            }
            .into();
            app.find.matches.clear();
            app.qol.hits.clear();
            app.find.active = 7;
            app.find.refresh_only = refresh_only;
            app.send(Request::Find(app.find.query.clone()));
            let stale = next_search(&mut app, &ctx);
            assert!(match &stale {
                Reply::Found { matches, .. } => !matches.is_empty(),
                Reply::SearchResults { hits, .. } => !hits.is_empty(),
                _ => unreachable!(),
            });
            app.go_to_page(1);
            match case {
                0 => app.qol.scope = 1,
                2 => app.qol.case_sensitive = true,
                3 => app.qol.whole_word = true,
                4 => {
                    app.qol.pages.clear();
                    app.qol.pages.insert(1);
                }
                5 => {
                    app.qol.anchor = Some((1, first));
                    app.qol.end = Some((1, last));
                }
                _ => {}
            }
            let scroll = app.doc.as_ref().unwrap().scroll_to;
            app.send(Request::Find(app.find.query.clone()));
            app.handle(&ctx, stale);
            assert!(
                app.find.matches.is_empty(),
                "Stale case {case} populated matches"
            );
            assert!(
                app.qol.hits.is_empty(),
                "Stale case {case} populated excerpts"
            );
            assert_eq!(app.doc.as_ref().unwrap().current_page, 1);
            assert_eq!(app.doc.as_ref().unwrap().scroll_to, scroll);
            assert_eq!(app.find.active, 7);
            assert_eq!(app.find.refresh_only, refresh_only);
            let current = next_search(&mut app, &ctx);
            app.handle(&ctx, current);
            assert!(!app.find.refresh_only);
            assert_eq!(app.find.active, 0);
            if matches!(case, 2 | 3) {
                assert!(app.find.matches.is_empty());
                assert!(app.qol.hits.is_empty());
            } else {
                assert!(!app.find.matches.is_empty());
                assert!(!app.qol.hits.is_empty());
                assert!(app.find.matches.iter().all(|(page, _)| *page == 1));
                assert!(app.qol.hits.iter().all(|hit| hit.page == 1));
            }
        }
    }
    assert!(!app.banners.iter().any(|b| b.kind == BannerKind::Error));
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn qol_scoped_search_refresh_uses_the_remaining_page_selection() {
    for scope in [1, 2] {
        let ctx = Context::default();
        let (mut app, inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        app.doc.as_mut().unwrap().current_page = 1;
        app.qol.scope = scope;
        app.qol.pages.extend([0, 1]);
        app.find.query = "AB".into();
        app.handle(&ctx, Reply::Changed {
            pages: vec![PageInfo { width: 300., height: 400. }],
            generation: 2,
            kind: ChangeKind::Edit,
            can_undo: true,
            can_redo: false,
            warning: None,
            objects_changed: 1,
        });
        assert_eq!(app.doc.as_ref().unwrap().current_page, 0);
        assert!(app.qol.pages.is_empty());
        let Request::FindOptions(query, options) = inbox.try_recv().unwrap() else {
            panic!("Scoped search was not refreshed");
        };
        assert_eq!(query, "AB");
        assert_eq!(options.pages, Some(vec![0]), "scope {scope}");
        assert_eq!(options, app.current_search_options());
    }
}

#[test]
fn qol_current_page_search_survives_deleting_the_last_page_and_history() {
    fn settle_search(app: &mut EditorApp, ctx: &Context, page: u16) {
        workflow_settle(app, ctx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.find.refresh_only {
            app.poll(ctx);
            assert!(!app.banners.iter().any(|b| b.kind == BannerKind::Error), "{}",
                app.banners.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("; "));
            assert!(std::time::Instant::now() < deadline, "Search did not settle");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!app.qol.hits.is_empty());
        assert!(app.qol.hits.iter().all(|hit| hit.page == page));
        assert!(!app.find.matches.is_empty());
        assert!(app.find.matches.iter().all(|(p, _)| *p == page));
    }
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    let query = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].text.clone();
    app.document_action(Request::PageBatch(pdf_editor_core::PageBatch::Duplicate {
        pages: vec![0], before: 1,
    }));
    workflow_settle(&mut app, &ctx);
    app.qol.scope = 1;
    app.find.query = query;
    app.go_to_page(1);
    app.find.refresh_only = true;
    app.send(Request::FindOptions(app.find.query.clone(), app.current_search_options()));
    settle_search(&mut app, &ctx, 1);
    assert_eq!(app.doc.as_ref().unwrap().current_page, 1);
    app.document_action(Request::PageBatch(pdf_editor_core::PageBatch::Delete(vec![1])));
    settle_search(&mut app, &ctx, 0);
    for _ in 0..4 {
        app.undo();
        settle_search(&mut app, &ctx, 0);
        assert_eq!(app.doc.as_ref().unwrap().pages.len(), 2);
        app.go_to_page(1);
        workflow_settle(&mut app, &ctx);
        assert_eq!(app.doc.as_ref().unwrap().current_page, 1);
        app.redo();
        settle_search(&mut app, &ctx, 0);
        assert_eq!(app.doc.as_ref().unwrap().pages.len(), 1);
    }
    assert_eq!(std::fs::read(source).unwrap(), original);
}

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
