// Real worker/PDFium integration driven through egui pointer and keyboard events.
// All files and configuration belong to generated test sessions.
#[test]
fn document_tabs_drag_in_both_directions_without_switching_or_mutating_sessions() {
    let ctx=Context::default();theme::install(&ctx);
    let mut entries=vec![];let mut inboxes=vec![];
    for (id,name) in ["A.pdf","B.pdf","C.pdf"].iter().enumerate() {
        let (mut app,inbox,_)=mock_app();opened(&mut app,&ctx);
        app.doc.as_mut().unwrap().name=(*name).into();
        if id==1 {app.doc.as_mut().unwrap().changes.edited();}
        inbox.try_iter().for_each(drop);
        entries.push((id as u64,app));inboxes.push(inbox);
    }
    let mut app=TabbedApp {sessions:Default::default(),native_guard:None,clipboard:Default::default(),tabs:entries,active:1,next_id:3,closing:None,allow_exit:false,instance:None,merge_tab:None,dragging_tab:None};
    let frame=|app:&mut TabbedApp,events| {
        let mut actions=None;
        let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(800.,600.))),events,..Default::default()},|ui| {let p=theme::palette(ui);actions=Some(app.tab_bar(ui,&p));egui::CentralPanel::default().show(ui,|_|{});});
        out.textures_delta.clear();(out,actions.unwrap())
    };
    let center=|out:&egui::FullOutput,name:&str|out.shapes.iter().find_map(|s|match &s.shape {Shape::Text(t) if t.galley.text()==name=>Some(t.pos+t.galley.size()*0.5),_=>None}).unwrap();
    let button=|pos,pressed|egui::Event::PointerButton {pos,pressed,button:egui::PointerButton::Primary,modifiers:Default::default()};
    frame(&mut app,vec![]);
    let (out,_)=frame(&mut app,vec![]);
    let a=center(&out,"A.pdf");let c=center(&out,"C.pdf")+vec2(18.,0.);
    frame(&mut app,vec![egui::Event::PointerMoved(a),button(a,true)]);
    let (out,actions)=frame(&mut app,vec![egui::Event::PointerMoved(c)]);
    assert!(app.dragging_tab.is_some());assert_eq!(actions,(None,None,false));
    assert!(out.shapes.iter().any(|s|matches!(&s.shape,Shape::Rect(r) if r.stroke.width==1.5)));
    assert_eq!(app.tabs.iter().map(|t|t.0).collect::<Vec<_>>(),[0,1,2]);
    frame(&mut app,vec![button(c,false)]);
    assert_eq!(app.tabs.iter().map(|t|t.0).collect::<Vec<_>>(),[1,2,0]);
    assert_eq!(app.tabs[app.active].0,1);
    let (out,_)=frame(&mut app,vec![]);
    let a=center(&out,"A.pdf");let b=center(&out,"B.pdf")-vec2(18.,0.);
    frame(&mut app,vec![egui::Event::PointerMoved(a),button(a,true)]);
    frame(&mut app,vec![egui::Event::PointerMoved(b)]);
    frame(&mut app,vec![button(b,false)]);
    assert_eq!(app.tabs.iter().map(|t|t.0).collect::<Vec<_>>(),[0,1,2]);
    assert_eq!(app.tabs[app.active].0,1);
    for cancel in [false,true] {
        frame(&mut app,vec![egui::Event::PointerMoved(a),button(a,true)]);
        frame(&mut app,vec![egui::Event::PointerMoved(b)]);
        if cancel {frame(&mut app,vec![workflow_key(Key::Escape,Default::default())]);}
        let outside=pos2(500.,300.);
        frame(&mut app,vec![egui::Event::PointerMoved(outside),button(outside,false)]);
        assert!(app.dragging_tab.is_none());
        assert_eq!(app.tabs.iter().map(|t|t.0).collect::<Vec<_>>(),[0,1,2]);
    }
    assert!(app.tabs[1].1.doc.as_ref().unwrap().dirty());
    assert!(inboxes.iter().all(|inbox|inbox.try_recv().is_err()));
    let (out,_)=frame(&mut app,vec![]);
    let a=center(&out,"A.pdf");let b=center(&out,"B.pdf");
    let close=a+vec2((b.x-a.x-2.)*0.5-18.,0.);
    frame(&mut app,vec![egui::Event::PointerMoved(close),button(close,true)]);
    let (_,actions)=frame(&mut app,vec![button(close,false)]);
    assert_eq!(actions,(None,Some(0),false));
    assert!(app.dragging_tab.is_none());
    for id in 3..32 {
        let (mut tab,inbox,_)=mock_app();opened(&mut tab,&ctx);
        tab.doc.as_mut().unwrap().name=format!("Generated-{id:02}.pdf");
        inbox.try_iter().for_each(drop);inboxes.push(inbox);
        app.tabs.push((id,tab));
    }
    frame(&mut app,vec![]);let (out,_)=frame(&mut app,vec![]);
    let last_visible=|out:&egui::FullOutput|out.shapes.iter().filter_map(|s|match &s.shape {
        Shape::Text(t) if t.pos.x>=0. && t.pos.x<780. =>t.galley.text().strip_prefix("Generated-").and_then(|s|s.strip_suffix(".pdf")).and_then(|s|s.parse::<usize>().ok()),_=>None
    }).max().unwrap_or(0);
    let old_visible=last_visible(&out);
    let start=center(&out,"A.pdf");let edge=pos2(790.,start.y);
    frame(&mut app,vec![egui::Event::PointerMoved(start),button(start,true)]);
    let (mut out,_)=frame(&mut app,vec![egui::Event::PointerMoved(edge)]);
    for _ in 0..60 {out=frame(&mut app,vec![]).0;}
    assert!(last_visible(&out)>old_visible,"Dragging at the edge did not reveal more tabs");
    let outside=pos2(500.,300.);
    frame(&mut app,vec![egui::Event::PointerMoved(outside),button(outside,false)]);
    assert_eq!(app.tabs.iter().map(|t|t.0).collect::<Vec<_>>(),(0..32).collect::<Vec<_>>());
    assert_eq!(app.tabs[app.active].0,1);
    assert!(inboxes.iter().all(|inbox|inbox.try_recv().is_err()));
}

#[test]
fn keyboard_password_submission_opens_a_generated_encrypted_document() {
    let (mut app,ctx,source)=qol_worker();
    let original=std::fs::read(&source).unwrap();
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let folder=recovery_safety_folder("keyboard-encrypted");
    let path=folder.join("generated-encrypted.pdf");
    let engine=pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let document=engine.open(&source).unwrap();
    let _copy=document.create_working_copy(&path,"generated-password").unwrap();
    app.open_path(path.clone());
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
    while app.busy>0 {
        app.poll(&ctx);
        assert!(std::time::Instant::now()<deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let request={
        let mut frame=|events| {
            let mut request=None;
            let mut output=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(1000.,900.))),events,..Default::default()},|ui|request=app.password.show(ui.ctx(),&theme::palette(ui),true,Id::new("generated-encrypted-dialog")));
            output.textures_delta.clear();request
        };
        for _ in 0..3 {assert!(frame(vec![]).is_none());}
        frame(vec![egui::Event::Text("generated-password".into())]);
        frame(vec![workflow_key(Key::Enter,Default::default())]).unwrap()
    };
    app.document_action(request);
    workflow_settle(&mut app,&ctx);
    let doc=app.doc.as_ref().unwrap();
    assert!(doc.protected);
    assert_eq!(doc.source,path);
    assert!(!doc.dirty());
    assert!(app.qol.working_copy.is_none());
    assert_eq!(std::fs::read(source).unwrap(),original);
    std::fs::remove_dir_all(folder).unwrap();
}

#[path = "tool_stress_tests.rs"]
mod tool_stress_tests;

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
    app.autosave_tick(&ctx);
    assert!(!app.doc.as_ref().unwrap().copying);
    assert!(app.doc.as_ref().unwrap().next_copy.is_none());
    workflow_click(&mut app, &ctx, pos2(950.0, 850.0));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.recovery_status(), "Recovery pending");
    app.autosave_tick(&ctx);
    let due = app.doc.as_ref().unwrap().next_copy.unwrap();
    assert_eq!(due, ctx.input(|i| i.time) + 60.0);
    workflow_frame_at(&mut app, &ctx, vec![], Some(due - 0.1));
    app.autosave_tick(&ctx);
    assert!(!app.doc.as_ref().unwrap().copying);
    assert_eq!(app.recovery_status(), "Recovery pending");
    workflow_frame_at(&mut app, &ctx, vec![], Some(due));
    app.autosave_tick(&ctx);
    assert_eq!(app.recovery_status(), "Writing recovery…");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.doc.as_ref().unwrap().copying {
        app.poll(&ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(app.recovery_status(), "Recovery saved");
    app.autosave_tick(&ctx);
    assert!(!app.doc.as_ref().unwrap().copying);
    assert!(app.doc.as_ref().unwrap().next_copy.is_none());
    assert_eq!(std::fs::read(&fixture).unwrap(), original);
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
    assert!(app.doc.as_ref().unwrap().can_undo);
    app.undo();workflow_settle(&mut app,&ctx);
    assert_ne!(app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].text,"AB");
    app.redo();workflow_settle(&mut app,&ctx);
    assert_eq!(app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].text,"AB");
    let output = folder.join("restored-copy.pdf");
    app.document_action(Request::SaveAs(output.clone()));
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
            let previews: Vec<_> = inbox.try_iter().collect();
            assert_eq!(previews.len(), 1, "Prepare one cached drag preview");
            assert!(previews.iter().all(|request| matches!(request, Request::ObjectPreview {..})), "Preview must not mutate");
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
    workflow_frame_at(app,ctx,events,None);
}
fn workflow_frame_at(app:&mut EditorApp,ctx:&Context,events:Vec<egui::Event>,time:Option<f64>) {
    app.poll(ctx);
    let mut output = ctx.run_ui(
        egui::RawInput {
            time,
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
    // Clicking empty page space also dismisses an unchanged text draft and toolbar.
    let outside=ctx.read_response(Id::new(("pdf-page",0usize))).unwrap().rect.min
        +vec2(10.0,10.0);
    workflow_click(&mut source,&ctx,outside);
    assert!(source.doc.as_ref().unwrap().edit.is_none());
    assert!(source.text_tool.selection.is_empty());
    workflow_edit(&mut source,&ctx,at);
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
    destination.document_action(Request::SaveAs(output.clone()));
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

#[test]
fn glyph_text_selection_deletes_the_logical_block_and_undo_restores_it() {
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ctx=Context::default();
    theme::install(&ctx);
    let (mut app,_,_)=mock_app();
    app.worker=Worker::spawn(ctx.clone(),root.join("target/debug/pdfium.dll"),
        Substitution::Line,Default::default());
    app.open_path(root.join("tests/fixtures/generated/glyph-runs.pdf"));
    workflow_settle(&mut app,&ctx);
    let before=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().items.len();
    let line=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].clone();
    assert!(line.objects.len()>1);
    let at=workflow_line(&app,&ctx,0);
    workflow_click(&mut app,&ctx,at);
    let doc=app.doc.as_ref().unwrap();
    let selected=selection_items(doc.pages[0].objects.as_ref().unwrap(),doc.selected_item.as_ref().unwrap());
    assert_eq!(selected.len(),line.objects.len());
    workflow_frame(&mut app,&ctx,vec![workflow_key(Key::Delete,egui::Modifiers::NONE)]);
    workflow_settle(&mut app,&ctx);
    assert_eq!(app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().items.len(),
        before-selected.len());
    app.undo();
    workflow_settle(&mut app,&ctx);
    assert_eq!(app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().items.len(),before);
}

#[test]
fn repeated_worker_failures_release_busy_states_and_keep_the_editor_usable() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ctx = Context::default();
    theme::install(&ctx);
    let (mut app, _, _) = mock_app();
    app.worker = Worker::spawn(ctx.clone(), root.join("target/debug/pdfium.dll"),
        Substitution::Line, Default::default());
    let source = root.join("tests/fixtures/generated/unicode-cid.pdf");
    let original = std::fs::read(&source).unwrap();
    let collision = root.join(format!("target/stress-save-collision-{}.pdf", std::process::id()));
    std::fs::write(&collision, b"existing generated output").unwrap();
    app.open_path(source.clone());
    workflow_settle(&mut app, &ctx);
    let wait = |app: &mut EditorApp| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.busy > 0 {
            app.poll(&ctx);
            assert!(std::time::Instant::now() < deadline, "Failed operation did not settle");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(app.engine_alive);
    };
    for iteration in 0..24 {
        let generation = app.doc.as_ref().unwrap().generation;
        let line = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].clone();
        let draft = format!("invalid\0draft {iteration}");
        app.doc.as_mut().unwrap().edit = Some(EditState {
            page: 0, line, text: draft.clone(), focus: false, rejected_text: None,
        });
        assert!(app.commit_edit());
        wait(&mut app);
        assert_eq!(app.doc.as_ref().unwrap().edit.as_ref().unwrap().text, draft);
        assert_eq!(app.doc.as_ref().unwrap().generation, generation);
        assert!(app.doc.as_ref().unwrap().pending_edit.is_none());
        for _ in 0..3 { assert!(!app.commit_edit()); }
        assert_eq!(app.busy, 0);
        app.cancel_edit();
        app.open_path(root.join("tests/fixtures/generated/malformed.pdf"));
        wait(&mut app);
        assert!(!app.opening);
        assert_eq!(app.doc.as_ref().unwrap().generation, generation);
        app.document_action(Request::SaveAs(collision.clone()));
        wait(&mut app);
        assert!(!app.doc.as_ref().unwrap().dirty());
        assert!(app.pending_action.is_none());
        assert!(app.banners.len() <= 4);
        app.banners.clear();
        let at = workflow_line(&app, &ctx, 0);
        workflow_click(&mut app, &ctx, at);
        assert!(app.doc.as_ref().unwrap().selected_item.is_some());
        workflow_frame(&mut app, &ctx, vec![workflow_key(Key::Escape, egui::Modifiers::NONE)]);
    }
    assert_eq!(std::fs::read(source).unwrap(), original);
    assert_eq!(std::fs::read(&collision).unwrap(), b"existing generated output");
    std::fs::remove_file(collision).unwrap();
}
