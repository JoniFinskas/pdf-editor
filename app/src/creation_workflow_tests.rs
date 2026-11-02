fn creation_frame(
    app: &mut EditorApp,
    ctx: &Context,
    events: Vec<egui::Event>,
    size: Vec2,
) -> Option<crate::create::Creation> {
    let mut selected = None;
    let mut check = false;
    let document = app.doc.is_some();
    let mut output = ctx.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            (selected, check) =
                app.create
                    .show(ui.ctx(), &theme::palette(ui), app.busy == 0, document);
        },
    );
    output.textures_delta.clear();
    if check {
        app.document_action(Request::CheckCv);
    }
    selected
}

#[test]
fn creation_cv_builder_pointer_create_worker_edit_save_and_text_review_roundtrip() {
    let (mut app, ctx, _) = qol_worker();
    let folder = recovery_safety_folder("created-cv");
    app.config_path = Some(folder.join("settings.txt"));
    // A new empty tab receives the form; the existing document isn't replaced.
    app.doc = None;
    app.open_tool(Tool::Cv);
    for _ in 0..3 {
        creation_frame(&mut app, &ctx, vec![], vec2(1000., 900.));
    }
    assert_eq!(
        ctx.read_response(widgets::tool_id("create-pdf"))
            .unwrap()
            .sense,
        Sense::hover()
    );
    ctx.memory_mut(|m| m.request_focus(Id::new("cv-name")));
    creation_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Avery Example".into())],
        vec2(1000., 900.),
    );
    assert_eq!(app.create.cv.name, "Avery Example");
    app.create.contact = "reader@example.org\nExample City".into();
    app.create.cv.headline = "Software developer".into();
    app.create.cv.sections[0].body = "Builds useful fictional tools with Rust.".into();
    for _ in 0..3 {
        creation_frame(&mut app, &ctx, vec![], vec2(1000., 900.));
    }
    let response = ctx.read_response(widgets::tool_id("create-pdf")).unwrap();
    assert_eq!(response.sense, Sense::click());
    assert!(
        response.interact_rect.contains_rect(response.rect),
        "Clipped Create button: {response:?}"
    );
    let at = tool_center(&ctx, "create-pdf");
    let events = click_at(at);
    let before = ctx.read_response(widgets::tool_id("create-pdf")).unwrap();
    let pressed = creation_frame(&mut app, &ctx, events[..2].to_vec(), vec2(1000., 900.));
    let after_press = ctx.read_response(widgets::tool_id("create-pdf"));
    let released = creation_frame(&mut app, &ctx, events[2..].to_vec(), vec2(1000., 900.));
    let creation = pressed.or(released).unwrap_or_else(|| {
        panic!(
            "Create click at {at:?}; before {before:?}; pressed {after_press:?}; released {:?}",
            ctx.read_response(widgets::tool_id("create-pdf"))
        )
    });
    app.create_here(creation);
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert!(app.doc.as_ref().unwrap().source.as_os_str().is_empty());
    assert!(app.qol.working_copy.is_none());
    assert_eq!(app.create.cv.name, "Avery Example");
    let object = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .flat_map(|l| l.objects.iter())
        .find(|o| o.text.contains("Software developer"))
        .unwrap()
        .clone();
    app.document_action(Request::Edit {
        objects: vec![object],
        text: "Software engineer".into(),
    });
    workflow_settle(&mut app, &ctx);
    app.open_tool(Tool::Cv);
    for _ in 0..3 {
        creation_frame(&mut app, &ctx, vec![], vec2(1000., 900.));
    }
    let at = tool_center(&ctx, "cv-check");
    let events = click_at(at);
    creation_frame(&mut app, &ctx, events[..2].to_vec(), vec2(1000., 900.));
    creation_frame(&mut app, &ctx, events[2..].to_vec(), vec2(1000., 900.));
    workflow_settle(&mut app, &ctx);
    let text = &app.create.check.as_ref().unwrap().text;
    assert!(text.contains("Software engineer"));
    assert!(text.find("reader@example.org").unwrap() < text.find("Summary").unwrap());
    let path = folder.join("CV.pdf");
    app.document_action(Request::SaveAs(path.clone()));
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    assert!(app.recent_files.contains(&path.canonicalize().unwrap()));
    assert!(
        !app.banners
            .iter()
            .any(|b| b.text.contains("original-file link"))
    );
    app.open_path(path.clone());
    workflow_settle(&mut app, &ctx);
    app.document_action(Request::CheckCv);
    workflow_settle(&mut app, &ctx);
    assert!(
        app.create
            .check
            .as_ref()
            .unwrap()
            .text
            .contains("Software engineer")
    );
    assert_eq!(app.create.cv.name, "Avery Example");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(folder.join("settings.txt")).unwrap();
    std::fs::remove_dir(folder).unwrap();
}

#[test]
fn blank_creation_is_dirty_editable_and_recoverable_without_fake_source_paths() {
    let (mut app, ctx, _) = qol_worker();
    app.doc = None;
    app.create_here(crate::create::Creation::Blank(pdf_editor_core::BlankPdf {
        pages: 2,
        ..Default::default()
    }));
    workflow_settle(&mut app, &ctx);
    assert!(app.text_tool.adding);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(app.doc.as_ref().unwrap().pages.len(), 2);
    assert!(app.recent_files.iter().all(|p| !p.as_os_str().is_empty()));
    let folder = recovery_safety_folder("created-recovery");
    app.session = Some(Session::start_in(&folder).unwrap());
    let snapshot = app.session.as_mut().unwrap().next_path();
    app.send(Request::Snapshot(snapshot.clone()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while app.session.as_ref().unwrap().current_path().is_none() {
        app.poll(&ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(super::qol_sessions::capture_tab(&app, true).is_some());
    drop(app.session.take());
    let orphan = crate::recovery::orphans_in(&folder).remove(0);
    let snapshot = orphan.copy.clone();
    app.restoring = Some(orphan);
    app.open_path(snapshot);
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(app.doc.as_ref().unwrap().pages.len(), 2);
    app.doc
        .as_mut()
        .unwrap()
        .restored_from
        .take()
        .unwrap()
        .discard();
    std::fs::remove_dir(folder).unwrap();
}

#[test]
fn creation_controls_fit_compact_view_and_keep_drafts_on_close() {
    for dark in [false, true] {
        let ctx = Context::default();
        theme::install(&ctx);
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let (mut app, _, _) = mock_app();
        app.open_tool(Tool::Cv);
        app.create.cv.name = "Avery Example".into();
        app.create.cv.sections[0].body = "Draft to retain".into();
        for _ in 0..3 {
            creation_frame(&mut app, &ctx, vec![], vec2(640., 720.));
        }
        let response = ctx.read_response(widgets::tool_id("create-pdf")).unwrap();
        assert_eq!(response.sense, Sense::click());
        assert!(
            response.interact_rect.contains_rect(response.rect),
            "Clipped Create button: {response:?}"
        );
        assert!(Rect::from_min_size(Pos2::ZERO, vec2(640., 720.)).contains_rect(response.rect));
        let at = tool_center(&ctx, "close-tool-Build CV");
        let events = click_at(at);
        creation_frame(&mut app, &ctx, events[..2].to_vec(), vec2(640., 720.));
        creation_frame(&mut app, &ctx, events[2..].to_vec(), vec2(640., 720.));
        assert!(!app.create.open);
        app.open_tool(Tool::Cv);
        assert_eq!(app.create.cv.sections[0].body, "Draft to retain");
    }
}
