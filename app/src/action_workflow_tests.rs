// Pending actions must never process an older document after a rejected draft.
#[test]
fn print_waits_for_pending_edit_and_cancels_on_failure() {
    let ctx = Context::default();
    let (mut app, inbox, _outbox) = mock_app();
    opened(&mut app, &ctx);
    let (object, rect) = glyph("AB", 30., 40.);
    app.doc.as_mut().unwrap().edit = Some(EditState {
        page: 0,
        text: "invalid\0draft".into(),
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
    app.open_tool(Tool::Print);
    assert!(matches!(inbox.try_recv().unwrap(), Request::Edit { .. }));
    assert!(
        inbox.try_recv().is_err(),
        "Print ran before the edit succeeded"
    );
    assert!(matches!(app.pending_action, Some(Request::Print)));
    app.handle(
        &ctx,
        Reply::Failed {
            message: "Generated edit rejection".into(),
            blocking: true,
            page: None,
        },
    );
    app.poll(&ctx);
    assert!(app.engine_alive);
    assert!(app.pending_action.is_none());
    assert_eq!(app.busy, 0);
    assert!(inbox.try_recv().is_err());
    assert_eq!(
        app.doc.as_ref().unwrap().edit.as_ref().unwrap().text,
        "invalid\0draft"
    );
}

#[test]
fn document_actions_do_not_run_after_rejected_drafts() {
    for case in 0..8 {
        let ctx = Context::default();
        let (mut app, inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        let (object, rect) = glyph("AB", 30., 40.);
        app.doc.as_mut().unwrap().edit = Some(EditState {
            page: 0,
            text: "invalid\0draft".into(),
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
        let request = || match case {
            0 => Request::ExportWord("generated.docx".into()),
            1 => Request::ExportImages(
                "1".into(),
                72,
                pdf_editor_core::PageImageFormat::Png,
                "generated-images".into(),
            ),
            2 => Request::Extract("1".into(), "generated.pdf".into()),
            3 => Request::Split("1".into(), "generated-split".into()),
            4 => Request::Sanitize(Default::default()),
            5 => Request::Compress(512, 80),
            6 => Request::PageBatch(pdf_editor_core::PageBatch::Blank {
                before: 0,
                width: 300.,
                height: 400.,
            }),
            _ => Request::Print,
        };
        app.document_action(request());
        assert!(matches!(inbox.try_recv().unwrap(), Request::Edit { .. }));
        assert!(inbox.try_recv().is_err(), "case {case}");
        app.handle(
            &ctx,
            Reply::Failed {
                message: "Generated edit rejection".into(),
                blocking: true,
                page: None,
            },
        );
        app.poll(&ctx);
        assert!(app.engine_alive);
        assert_eq!(app.busy, 0);
        assert!(app.pending_action.is_none());
        app.document_action(request());
        assert!(
            inbox.try_recv().is_err(),
            "Rejected draft bypassed by case {case}"
        );
        assert_eq!(app.busy, 0);
    }
}

#[test]
fn repeated_extracts_wait_for_successful_edits_and_keep_failed_drafts_private() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    let folder = recovery_safety_folder("pending-extract");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    for iteration in 0..12 {
        let line = app.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .lines[0]
            .clone();
        let text = if iteration % 2 == 0 { "AB" } else { "BA" };
        app.doc.as_mut().unwrap().edit = Some(EditState {
            page: 0,
            text: text.into(),
            line,
            focus: false,
            rejected_text: None,
        });
        let output = folder.join(format!("generated-{iteration}.pdf"));
        app.document_action(Request::Extract("1".into(), output.clone()));
        workflow_settle(&mut app, &ctx);
        let saved = engine.open(&output).unwrap();
        assert_eq!(engine.text_objects(&saved, 0).unwrap()[0].text, text);
        assert!(app.doc.as_ref().unwrap().dirty());
        let line = app.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .lines[0]
            .clone();
        app.doc.as_mut().unwrap().edit = Some(EditState {
            page: 0,
            text: "invalid\0draft".into(),
            line,
            focus: false,
            rejected_text: None,
        });
        let failed = folder.join(format!("rejected-{iteration}.pdf"));
        app.document_action(Request::Extract("1".into(), failed.clone()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.busy > 0 {
            app.poll(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!failed.exists());
        assert!(app.pending_action.is_none());
        assert_eq!(
            app.doc.as_ref().unwrap().edit.as_ref().unwrap().text,
            "invalid\0draft"
        );
        app.cancel_edit();
        app.banners.clear();
    }
    assert_eq!(std::fs::read(source).unwrap(), original);
    std::fs::remove_dir_all(folder).unwrap();
}
