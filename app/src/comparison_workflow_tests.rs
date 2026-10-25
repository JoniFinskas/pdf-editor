// Real editor snapshots and comparison workers, with generated documents only.
fn comparison_settle(app: &mut EditorApp, ctx: &Context) -> (usize, usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        workflow_frame(app, ctx, vec![]);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 900.))),
                ..Default::default()
            },
            |ui| app.compare.show(ui.ctx(), &theme::palette(ui)),
        );
        output.textures_delta.clear();
        assert!(
            app.compare.test_error().is_none(),
            "Comparison failed: {:?}",
            app.compare.test_error()
        );
        if app.busy == 0
            && app.pending_comparison.is_none()
            && let Some(totals) = app.compare.test_totals()
        {
            return totals;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Comparison did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn comparison_detects_unsaved_text_over_an_image_and_keeps_original_after_save() {
    let (mut app, ctx, _fixture) = qol_worker();
    let folder = recovery_safety_folder("comparison-image");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let picture = pdf_editor_core::Picture {
        width: 80,
        height: 100,
        rgba: vec![235; 80 * 100 * 4],
    };
    let source = folder.join("generated-scan.pdf");
    let png = pdf_editor_core::store_signature(&folder, &picture).unwrap();
    engine
        .images_to_pdf(&[png], pdf_editor_core::ImagePageSize::A4)
        .unwrap()
        .save_as(&source)
        .unwrap();
    let original = std::fs::read(&source).unwrap();
    app.open_path(source.clone());
    workflow_settle(&mut app, &ctx);
    app.busy += 1;
    app.send(Request::AddText {
        text: pdf_editor_core::NewText {
            page: 0,
            x: 0.,
            y: 0.,
            text: "Revised number".into(),
            font: "Arial".into(),
            size: 24.,
            color: [220, 0, 0],
        },
        raster: (80., 100.),
    });
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    app.open_tool(Tool::Compare);
    let changed = comparison_settle(&mut app, &ctx);
    assert!(changed.0 > 0 && changed.1 > 0);
    assert_eq!(app.compare.test_origin(), Some(source.as_path()));
    let copy = folder.join("arbitrary-copy.pdf");
    app.password_action(Request::SaveAs(copy.clone()));
    workflow_settle(&mut app, &ctx);
    app.open_tool(Tool::Compare);
    assert!(comparison_settle(&mut app, &ctx).1 > 0);
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert_eq!(comparison_settle(&mut app, &ctx), (0, 0));
    app.redo();
    workflow_settle(&mut app, &ctx);
    assert!(comparison_settle(&mut app, &ctx).1 > 0);
    app.save();
    workflow_settle(&mut app, &ctx);
    app.open_tool(Tool::Compare);
    assert!(comparison_settle(&mut app, &ctx).0 > 0);
    assert_eq!(std::fs::read(&source).unwrap(), original);
    app.compare.reset_document();
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn comparison_reopens_saved_copies_with_recorded_original_and_includes_pending_typing() {
    let (mut app, ctx, fixture) = qol_worker();
    let folder = recovery_safety_folder("comparison-lineage");
    app.config_path = Some(folder.join("settings.txt"));
    let source = folder.join("source.pdf");
    std::fs::copy(&fixture, &source).unwrap();
    app.open_path(source.clone());
    workflow_settle(&mut app, &ctx);
    let at = workflow_line(&app, &ctx, 0);
    workflow_edit(&mut app, &ctx, at);
    workflow_frame(
        &mut app,
        &ctx,
        vec![workflow_key(Key::A, egui::Modifiers::COMMAND)],
    );
    workflow_frame(&mut app, &ctx, vec![egui::Event::Text("AB".into())]);
    app.open_tool(Tool::Compare);
    assert!(comparison_settle(&mut app, &ctx).0 > 0);
    let copy = folder.join("custom-version.pdf");
    app.password_action(Request::SaveAs(copy.clone()));
    workflow_settle(&mut app, &ctx);
    app.open_path(copy.clone());
    workflow_settle(&mut app, &ctx);
    app.open_tool(Tool::Compare);
    assert!(comparison_settle(&mut app, &ctx).0 > 0);
    assert_eq!(
        app.compare.test_origin().unwrap().canonicalize().unwrap(),
        source.canonicalize().unwrap()
    );
    assert!(!app.doc.as_ref().unwrap().dirty());
    app.compare.reset_document();
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn comparison_waits_for_success_and_discards_a_late_snapshot_after_close() {
    let ctx = Context::default();
    let (mut app, inbox, _) = mock_app();
    opened(&mut app, &ctx);
    let (object, rect) = glyph("Original", 30., 40.);
    app.doc.as_mut().unwrap().edit = Some(EditState {
        page: 0,
        text: "Rejected".into(),
        focus: true,
        rejected_text: None,
        line: Line {
            paragraph: None,
            fallback: None,
            objects: vec![object],
            rect,
            text: "Original".into(),
        },
    });
    app.open_tool(Tool::Compare);
    assert_eq!(app.busy, 1);
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
    assert!(app.compare.test_error().is_some());
    assert!(app.pending_comparison.is_none());
    assert!(inbox.try_recv().is_err());
    app.cancel_edit();
    app.banners.clear();
    app.open_tool(Tool::Compare);
    let id = app.pending_comparison.unwrap();
    app.compare.reset_document();
    app.poll(&ctx);
    assert!(inbox.try_recv().is_err());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let doc = std::sync::Arc::new(
        engine
            .open(&root.join("tests/fixtures/generated/unicode-cid.pdf"))
            .unwrap(),
    );
    app.handle(
        &ctx,
        Reply::ComparisonSnapshot {
            id,
            generation: app.doc.as_ref().unwrap().generation,
            original: doc.clone(),
            current: doc,
        },
    );
    assert!(!app.compare.open);
    assert!(app.compare.test_totals().is_none());
}

#[test]
fn comparison_of_protected_document_uses_memory_without_plaintext_exports() {
    let (mut app, ctx, fixture) = qol_worker();
    let encrypted = fixture.with_file_name("encrypted.pdf");
    app.password_action(Request::OpenPassword(encrypted, "fixture-reader".into()));
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().protected);
    app.open_tool(Tool::Compare);
    assert_eq!(comparison_settle(&mut app, &ctx), (0, 0));
    assert!(app.compare_origin.is_none());
    assert!(app.qol.working_copy.is_none());
    app.compare.reset_document();
}

#[test]
fn comparison_refresh_preserves_an_explicit_original_instead_of_resetting_it() {
    let (mut app,ctx,fixture)=qol_worker();
    let chosen=fixture.with_file_name("paragraph-Left.pdf");
    let id=app.compare.open_current(fixture,Some(chosen.clone()),None);app.pending_comparison=Some(id);
    comparison_settle(&mut app,&ctx);
    app.refresh_comparison();comparison_settle(&mut app,&ctx);
    assert_eq!(app.compare.test_origin(),Some(chosen.as_path()));
    app.open_tool(Tool::Compare);comparison_settle(&mut app,&ctx);
    assert_eq!(app.compare.test_origin(),Some(chosen.as_path()));app.compare.reset_document();
}
