// Document actions and dependent tool state track acknowledged changes.
#[test]
fn forms_panel_replaces_previous_document_fields_and_loads_only_when_open() {
    let (mut app, ctx, plain) = qol_worker();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let forms = root.join("tests/fixtures/generated/forms.pdf");
    let original = std::fs::read(&forms).unwrap();
    let folder = recovery_safety_folder("form-document-switch");
    let second = folder.join("generated-second-form.pdf");
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let mut generated = engine.open(&forms).unwrap();
    let field = engine
        .form_fields(&generated)
        .unwrap()
        .into_iter()
        .find(|f| matches!(f.value, pdf_editor_core::FieldValue::Text(_)))
        .unwrap();
    let value = pdf_editor_core::FieldValue::Text("Generated second document".into());
    engine.fill_field(&mut generated, &field, &value).unwrap();
    generated.save_as(&second).unwrap();
    let second_bytes = std::fs::read(&second).unwrap();
    app.open_path(forms.clone());
    workflow_settle(&mut app, &ctx);
    app.open_tool(Tool::Forms);
    workflow_settle(&mut app, &ctx);
    let previous = app
        .forms
        .fields
        .iter()
        .find(|f| f.name == field.name)
        .unwrap()
        .clone();
    assert_ne!(previous.value, value);
    app.open_path(second.clone());
    workflow_settle(&mut app, &ctx);
    assert!(app.forms.open);
    let current = app
        .forms
        .fields
        .iter()
        .find(|f| f.name == field.name)
        .unwrap();
    assert_eq!(current.value, value);
    assert_eq!(previous.revision, current.revision);
    app.forms.open = false;
    app.open_path(forms.clone());
    workflow_settle(&mut app, &ctx);
    assert!(
        app.forms.fields.is_empty(),
        "Closed panel loaded document fields"
    );
    app.open_tool(Tool::Forms);
    workflow_settle(&mut app, &ctx);
    assert_eq!(
        app.forms
            .fields
            .iter()
            .find(|f| f.name == field.name)
            .unwrap()
            .value,
        previous.value
    );
    app.open_path(plain);
    workflow_settle(&mut app, &ctx);
    assert!(app.forms.fields.is_empty());
    assert!(app.forms.designer.fields.is_empty());
    assert_eq!(std::fs::read(forms).unwrap(), original);
    assert_eq!(std::fs::read(&second).unwrap(), second_bytes);
    std::fs::remove_file(second).unwrap();
    std::fs::remove_dir(folder).unwrap();
}

#[test]
fn tool_list_refreshes_are_limited_to_open_panels_and_changed_generations() {
    for changed in [false, true] {
        for flags in 0..8 {
            let ctx = Context::default();
            let (mut app, inbox, _outbox) = mock_app();
            opened(&mut app, &ctx);
            app.markup.open = flags & 1 != 0;
            app.pictures.open = flags & 2 != 0;
            app.forms.open = flags & 4 != 0;
            app.handle(
                &ctx,
                Reply::Changed {
                    pages: vec![],
                    generation: 1 + u64::from(changed),
                    kind: ChangeKind::Edit,
                    can_undo: changed,
                    can_redo: false,
                    warning: None,
                    objects_changed: usize::from(changed),
                },
            );
            let requests: Vec<_> = inbox.try_iter().collect();
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| matches!(r, Request::Annotations))
                    .count(),
                usize::from(changed && app.markup.open)
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| matches!(r, Request::Pictures))
                    .count(),
                usize::from(changed && app.pictures.open)
            );
            assert_eq!(requests.iter().filter(|r|matches!(r,Request::FormFields)).count(),usize::from(changed && app.forms.open));
            assert_eq!(app.busy as usize, requests.len());
            if app.forms.open {
                app.busy=1;opened(&mut app,&ctx);
                assert_eq!(inbox.try_iter().filter(|r|matches!(r,Request::FormFields)).count(),1,"Opening a document must enumerate form fields only once");
            }
        }
    }
}

#[test]
fn open_annotation_and_picture_lists_follow_edits_and_history() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    app.open_tool(Tool::Markup);
    app.open_tool(Tool::Pictures);
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.items.is_empty());
    assert!(app.pictures.items.is_empty());
    let mut note = app.markup.draft.clone();
    note.kind = pdf_editor_core::MarkupKind::Note;
    note.text = "Generated note".into();
    app.markup.draft.text = "Unapplied annotation draft".into();
    app.document_action(Request::AddAnnotation(note));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.markup.items.len(), 1);
    assert_eq!(app.markup.items[0].markup.text, "Generated note");
    assert_eq!(app.markup.draft.text, "Unapplied annotation draft");
    let picture = pdf_editor_core::Picture {
        width: 2,
        height: 2,
        rgba: vec![128; 16],
    };
    let mut placement = pdf_editor_core::Placement {
        page: 0,
        left: 50.,
        bottom: 100.,
        width: 20.,
        height: 20.,
    };
    app.document_action(Request::PlacePicture(picture, placement));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.pictures.items.len(), 1);
    for step in 0..4 {
        let annotation = app.markup.items[0].clone();
        let mut update = annotation.markup.clone();
        update.text = format!("Generated note {step}");
        app.document_action(Request::UpdateAnnotation(annotation, update.clone()));
        workflow_settle(&mut app, &ctx);
        assert_eq!(app.markup.items[0].markup.text, update.text);
        let picture = app.pictures.items[0].clone();
        let before = placement.left;
        placement.left += 5.;
        app.markup.selected = Some(0);
        app.document_action(Request::MovePicture(picture, placement));
        workflow_settle(&mut app, &ctx);
        assert_eq!(app.markup.selected, Some(0));
        assert_eq!(app.pictures.items[0].placement.left, placement.left);
        app.undo();
        workflow_settle(&mut app, &ctx);
        assert_eq!(app.pictures.items[0].placement.left, before);
        app.redo();
        workflow_settle(&mut app, &ctx);
        assert_eq!(app.pictures.items[0].placement.left, placement.left);
        assert_eq!(app.markup.items[0].markup.text, update.text);
    }
    app.markup.selected = Some(0);
    app.pictures.selected = Some(0);
    app.document_action(Request::DeleteAnnotation(app.markup.items[0].clone()));
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.items.is_empty());
    assert!(app.markup.selected.is_none());
    assert_eq!(app.pictures.selected, Some(0));
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.markup.items.len(), 1);
    app.redo();
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.items.is_empty());
    assert_eq!(app.markup.draft.text, "Unapplied annotation draft");
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.markup.items.len(), 1);
    app.markup.selected = Some(0);
    app.open_path(source.clone());
    assert_eq!(app.markup.items.len(), 1);
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.items.is_empty());
    assert!(app.pictures.items.is_empty());
    assert!(app.markup.selected.is_none());
    assert!(app.pictures.selected.is_none());
    assert_eq!(std::fs::read(source).unwrap(), original);
}

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
    for case in 0..9 {
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
            8 => Request::SplitBookmarks("generated-chapters".into()),
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

fn form_panel_frame(app:&mut EditorApp,ctx:&Context,events:Vec<egui::Event>)->egui::FullOutput {
    app.poll(ctx);
    let mut request=None;
    let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(1000.,900.))),events,..Default::default()},|ui| {
        let p=theme::palette(ui);egui::CentralPanel::default().show(ui,|_|{});
        request=app.forms.show(ctx,&p,app.busy==0);
    });out.textures_delta.clear();
    if let Some(request)=request {app.document_action(request);}
    out
}
fn form_panel_text_center(out:&egui::FullOutput,label:&str)->Pos2 {
    out.shapes.iter().find_map(|s|match &s.shape {Shape::Text(t) if t.galley.text()==label=>Some(t.pos+t.galley.size()*0.5),_=>None}).unwrap_or_else(||panic!("Missing form text: {label}"))
}
fn form_panel_click(app:&mut EditorApp,ctx:&Context,at:Pos2) {
    let button=|pressed|egui::Event::PointerButton {pos:at,pressed,button:egui::PointerButton::Primary,modifiers:Default::default()};
    form_panel_frame(app,ctx,vec![egui::Event::PointerMoved(at),button(true)]);
    form_panel_frame(app,ctx,vec![button(false)]);
}
#[test]
fn generated_form_edits_keep_other_drafts_and_refresh_handles_after_apply() {
    use pdf_editor_core::{Bounds,DesignFieldKind,FieldValue,FormDesignEdit,NewFormField};
    let(mut app,ctx,source)=qol_worker();let original=std::fs::read(&source).unwrap();
    for (i,name) in ["First field","Second field"].iter().enumerate() {
        app.document_action(Request::DesignForm(FormDesignEdit::Add(NewFormField {page:0,name:(*name).into(),required:false,kind:DesignFieldKind::Text,
            bounds:Bounds {left:36.,right:220.,bottom:100.+i as f32*45.,top:128.+i as f32*45.}})));
        workflow_settle(&mut app,&ctx);
    }
    app.open_tool(Tool::Forms);workflow_settle(&mut app,&ctx);
    let folder=recovery_safety_folder("form-multiple-drafts");let path=folder.join("working.pdf");
    app.document_action(Request::SaveAs(path.clone()));workflow_settle(&mut app,&ctx);
    let saved=std::fs::read(&path).unwrap();assert!(!TabbedApp::needs_warning(&app));
    form_panel_frame(&mut app,&ctx,vec![]);form_panel_frame(&mut app,&ctx,vec![]);
    let generation=app.doc.as_ref().unwrap().generation;
    let out=form_panel_frame(&mut app,&ctx,vec![]);let apply=form_panel_text_center(&out,"Apply");
    form_panel_click(&mut app,&ctx,apply);assert_eq!(app.busy,0);assert_eq!(app.doc.as_ref().unwrap().generation,generation);
    for (name,value) in [("First field","First entry"),("Second field","Second draft")] {
        let out=form_panel_frame(&mut app,&ctx,vec![]);
        let at=form_panel_text_center(&out,name)+vec2(70.,31.);
        form_panel_click(&mut app,&ctx,at);
        form_panel_frame(&mut app,&ctx,vec![egui::Event::Text(value.into())]);
        assert_eq!(app.forms.fields.iter().find(|f|f.name==name).unwrap().value,FieldValue::Text(value.into()));
    }
    assert!(app.forms.has_drafts());assert!(TabbedApp::needs_warning(&app));assert!(!app.doc.as_ref().unwrap().dirty());
    let blocked=folder.join("unfinished.pdf");app.document_action(Request::SaveAs(blocked.clone()));
    assert_eq!(app.busy,0);assert!(!blocked.exists());
    app.file_save.configure(working_autosave::Mode::AfterEdits);
    let old_revision=app.forms.fields[0].revision;
    let out=form_panel_frame(&mut app,&ctx,vec![]);let at=form_panel_text_center(&out,"Apply");
    form_panel_click(&mut app,&ctx,at);workflow_settle(&mut app,&ctx);
    let fresh=app.forms.fields[0].revision>old_revision;
    working_auto_frame(&mut app,&ctx,30.);working_auto_frame(&mut app,&ctx,32.);
    assert!(app.file_save.in_flight.is_none());assert_eq!(std::fs::read(&path).unwrap(),saved);
    app.document_action(Request::FormFields);workflow_settle(&mut app,&ctx);
    assert_eq!((fresh,app.forms.fields[1].value.clone()),(true,FieldValue::Text("Second draft".into())),"Applying one field must retain other drafts and refresh native handles");
    let second=app.forms.fields[1].clone();app.document_action(Request::FillField(second.clone(),second.value));workflow_settle(&mut app,&ctx);
    assert!(!app.forms.has_drafts());
    app.document_action(Request::SaveWorkingCopy);workflow_settle(&mut app,&ctx);assert!(!TabbedApp::needs_warning(&app));
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine=pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let fields=engine.form_fields(&engine.open(&path).unwrap()).unwrap();
    assert_eq!(fields[0].value,FieldValue::Text("First entry".into()));assert_eq!(fields[1].value,FieldValue::Text("Second draft".into()));
    app.undo();workflow_settle(&mut app,&ctx);assert_eq!(app.forms.fields[1].value,FieldValue::Text(String::new()));
    app.redo();workflow_settle(&mut app,&ctx);assert_eq!(app.forms.fields[1].value,FieldValue::Text("Second draft".into()));
    assert!(!TabbedApp::needs_warning(&app));
    form_panel_frame(&mut app,&ctx,vec![]);let out=form_panel_frame(&mut app,&ctx,vec![]);
    let at=form_panel_text_center(&out,"First field")+vec2(70.,31.);form_panel_click(&mut app,&ctx,at);
    form_panel_frame(&mut app,&ctx,vec![workflow_key(Key::A,egui::Modifiers::COMMAND)]);
    form_panel_frame(&mut app,&ctx,vec![egui::Event::Text("Discard this draft".into())]);assert!(app.forms.has_drafts());
    form_panel_frame(&mut app,&ctx,vec![]);
    let at=ctx.read_response(widgets::tool_id("form-revert-0-0")).unwrap().rect.center();form_panel_click(&mut app,&ctx,at);
    assert_eq!(app.forms.fields[0].value,FieldValue::Text("First entry".into()));assert!(!app.forms.has_drafts());assert!(!TabbedApp::needs_warning(&app));
    assert_eq!(std::fs::read(source).unwrap(),original);std::fs::remove_file(path).unwrap();std::fs::remove_dir(folder).unwrap();
}
