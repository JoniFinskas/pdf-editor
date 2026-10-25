// Generated-only export through the real editor worker and pending-edit barrier.
#[test]
fn in_memory_sanitize_waits_for_pending_text_and_cancels_after_rejection() {
    let ctx = Context::default();
    let (mut app, inbox, _) = mock_app();
    opened(&mut app, &ctx);
    let (object, rect) = glyph("AB", 30., 40.);
    app.doc.as_mut().unwrap().edit = Some(EditState {
        page: 0,
        text: "Pending text".into(),
        line: Line {
            objects: vec![object],
            rect,
            text: "AB".into(),
            paragraph: None,
            fallback: None,
        },
        focus: false,
        rejected_text: None,
    });
    app.password_action(Request::Sanitize(Default::default()));
    assert!(matches!(inbox.try_recv().unwrap(), Request::Edit { .. }));
    assert!(matches!(app.pending_save, Some(Request::Sanitize(_))));
    app.handle(
        &ctx,
        Reply::Failed {
            message: "Generated edit rejection".into(),
            blocking: true,
            page: None,
        },
    );
    app.poll(&ctx);
    assert!(app.pending_save.is_none());
    assert!(inbox.try_recv().is_err());
}

#[test]
fn sanitized_export_commits_pending_text_but_keeps_editor_history_and_savepoint() {
    let (mut app, ctx, fixture) = qol_worker();
    let folder = recovery_safety_folder("sanitized-export");
    let source = std::fs::read(&fixture).unwrap();
    let at = workflow_line(&app, &ctx, 0);
    workflow_edit(&mut app, &ctx, at);
    workflow_frame(
        &mut app,
        &ctx,
        vec![workflow_key(Key::A, egui::Modifiers::COMMAND)],
    );
    workflow_frame(&mut app, &ctx, vec![egui::Event::Text("AB".into())]);
    let output = folder.join("verified.pdf");
    let options = pdf_editor_core::SanitizeOptions {
        flatten_visible: true,
        ..Default::default()
    };
    app.password_action(Request::ExportSanitized(options, output.clone()));
    workflow_settle(&mut app, &ctx);
    assert!(output.exists());
    assert!(app.doc.as_ref().unwrap().dirty());
    assert!(app.doc.as_ref().unwrap().can_undo);
    assert!(app.qol.working_copy.is_none());
    assert_eq!(std::fs::read(&fixture).unwrap(), source);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let saved = engine.open(&output).unwrap();
    engine.verify_sanitized(&saved, options).unwrap();
    assert!(engine.text_objects(&saved, 0).unwrap().is_empty());
    assert!(
        app.banners
            .iter()
            .any(|banner| banner.text.starts_with("Saved and verified:"))
    );
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn sanitized_export_rejects_recovery_destinations_and_failed_pending_edits() {
    let ctx = Context::default();
    let (mut app, inbox, _) = mock_app();
    opened(&mut app, &ctx);
    let folder = recovery_safety_folder("sanitize-reject");
    app.session = Some(Session::start_in(&folder).unwrap());
    let options = pdf_editor_core::SanitizeOptions::default();
    let destination = app
        .session
        .as_mut()
        .unwrap()
        .next_path()
        .with_file_name("export.pdf");
    app.password_action(Request::ExportSanitized(options, destination));
    assert!(inbox.try_recv().is_err());
    let (object, rect) = glyph("AB", 30., 40.);
    app.doc.as_mut().unwrap().edit = Some(EditState {
        page: 0,
        text: "Failed edit".into(),
        line: Line {
            objects: vec![object],
            rect,
            text: "AB".into(),
            paragraph: None,
            fallback: None,
        },
        focus: false,
        rejected_text: None,
    });
    let output = folder.join("verified.pdf");
    app.password_action(Request::ExportSanitized(options, output.clone()));
    assert!(matches!(inbox.try_recv().unwrap(), Request::Edit { .. }));
    app.handle(
        &ctx,
        Reply::Failed {
            message: "Generated edit rejection".into(),
            blocking: true,
            page: None,
        },
    );
    app.poll(&ctx);
    assert!(app.pending_save.is_none());
    assert!(!output.exists());
    assert!(inbox.try_recv().is_err());
    drop(app.session.take());
    std::fs::remove_dir_all(folder).unwrap();
}
