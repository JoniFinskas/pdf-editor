fn working_auto_canvas_frame(app:&mut EditorApp,ctx:&Context,time:f64) {
    workflow_frame_at(app,ctx,vec![],Some(time));app.working_auto_save_tick(ctx);
}
fn working_auto_frame(app:&mut EditorApp,ctx:&Context,time:f64) {
    let mut out=ctx.run_ui(egui::RawInput {time:Some(time),..Default::default()},|_|app.working_auto_save_tick(ctx));
    out.textures_delta.clear();
}
fn working_auto_wait(app:&mut EditorApp,ctx:&Context) {
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
    while app.file_save.in_flight.is_some() {
        app.poll(ctx);assert!(std::time::Instant::now()<deadline,"Auto-save did not finish");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
fn working_auto_mock(ctx:&Context)->(EditorApp,std::sync::mpsc::Receiver<Request>) {
    let (mut app,inbox,_)=mock_app();opened(&mut app,ctx);inbox.try_iter().for_each(drop);
    app.qol.working_copy=Some(PathBuf::from("generated-working-copy.pdf"));
    let doc=app.doc.as_mut().unwrap();doc.changes.edited();doc.generation+=1;
    (app,inbox)
}

#[test]
fn working_autosave_is_opt_in_debounces_edits_and_pauses_failed_writes() {
    use working_autosave::Mode;
    let ctx=Context::default();let(mut app,inbox)=working_auto_mock(&ctx);
    working_auto_frame(&mut app,&ctx,10.);working_auto_frame(&mut app,&ctx,100.);
    assert!(inbox.try_recv().is_err());assert_eq!(app.file_save.mode,Mode::Off);
    app.file_save.configure(Mode::AfterEdits);
    working_auto_frame(&mut app,&ctx,110.);
    app.doc.as_mut().unwrap().generation+=1;
    working_auto_frame(&mut app,&ctx,111.);working_auto_frame(&mut app,&ctx,112.9);
    assert!(inbox.try_recv().is_err());
    working_auto_frame(&mut app,&ctx,113.);
    let generation=app.doc.as_ref().unwrap().generation;
    assert!(matches!(inbox.try_recv().unwrap(),Request::AutoSaveWorkingCopy {generation:g} if g==generation));
    assert_eq!(app.busy,0,"Background saves must not block editing controls");
    app.handle(&ctx,Reply::AutoSaved {generation,result:Ok(Some(("generated-working-copy.pdf".into(),false)))});
    assert!(!app.doc.as_ref().unwrap().dirty());
    working_auto_frame(&mut app,&ctx,200.);assert!(inbox.try_recv().is_err());
    app.file_save.configure(Mode::Minute);
    app.doc.as_mut().unwrap().changes.edited();app.doc.as_mut().unwrap().generation+=1;
    working_auto_frame(&mut app,&ctx,300.);
    app.doc.as_mut().unwrap().generation+=1;
    working_auto_frame(&mut app,&ctx,350.);working_auto_frame(&mut app,&ctx,359.9);
    assert!(inbox.try_recv().is_err());working_auto_frame(&mut app,&ctx,360.);
    let generation=app.doc.as_ref().unwrap().generation;
    assert!(matches!(inbox.try_recv().unwrap(),Request::AutoSaveWorkingCopy {generation:g} if g==generation));
    app.busy=1;app.pending_action=Some(Request::Fonts);
    app.handle(&ctx,Reply::AutoSaved {generation,result:Err("generated disk error".into())});
    assert!(app.file_save.paused);assert_eq!(app.busy,1);assert!(app.pending_action.is_some());
    app.busy=0;app.pending_action=None;
    working_auto_frame(&mut app,&ctx,1000.);working_auto_frame(&mut app,&ctx,2000.);
    assert!(inbox.try_recv().is_err());
    assert_eq!(app.banners.iter().filter(|b|b.text.starts_with("Auto-save paused:")).count(),1);
    app.handle(&ctx,Reply::Saved("generated-working-copy.pdf".into()));
    assert!(!app.file_save.paused);assert!(app.banners.iter().all(|b|!b.text.starts_with("Auto-save paused:")));
}

#[test]
fn working_autosave_requires_a_copy_preserves_drafts_and_defers_busy_or_closing_states() {
    use working_autosave::Mode;
    let ctx=Context::default();let(mut app,inbox)=working_auto_mock(&ctx);
    app.file_save.configure(Mode::AfterEdits);app.qol.working_copy=None;
    working_auto_frame(&mut app,&ctx,10.);working_auto_frame(&mut app,&ctx,100.);
    assert!(inbox.try_recv().is_err());assert!(!app.picker.pending());
    app.qol.working_copy=Some("generated-working-copy.pdf".into());
    let(object,rect)=glyph("AB",30.,40.);
    app.doc.as_mut().unwrap().edit=Some(EditState {rejected_text:None,page:0,text:"BA".into(),focus:true,line:Line {paragraph:None,fallback:None,objects:vec![object],rect,text:"AB".into()}});
    working_auto_frame(&mut app,&ctx,110.);working_auto_frame(&mut app,&ctx,200.);
    assert!(inbox.try_recv().is_err());assert_eq!(app.doc.as_ref().unwrap().edit.as_ref().unwrap().text,"BA");
    app.cancel_edit();app.busy=1;working_auto_frame(&mut app,&ctx,201.);
    app.busy=0;app.file_save.suspended=true;working_auto_frame(&mut app,&ctx,202.);
    app.file_save.suspended=false;app.confirm=Some(Pending::Close);working_auto_frame(&mut app,&ctx,203.);
    assert!(inbox.try_recv().is_err());app.confirm=None;
    working_auto_frame(&mut app,&ctx,204.);assert!(matches!(inbox.try_recv().unwrap(),Request::AutoSaveWorkingCopy {..}));
}

#[test]
fn working_autosave_serializes_tabs_and_keeps_later_history_dirty() {
    use working_autosave::Mode;
    let ctx=Context::default();let(mut first,a)=working_auto_mock(&ctx);let(mut second,b)=working_auto_mock(&ctx);
    first.file_save.configure(Mode::AfterEdits);second.file_save.configure(Mode::AfterEdits);
    working_auto_frame(&mut first,&ctx,10.);working_auto_frame(&mut second,&ctx,10.);
    working_auto_frame(&mut first,&ctx,12.);working_auto_frame(&mut second,&ctx,12.);
    assert!(matches!(a.try_recv().unwrap(),Request::AutoSaveWorkingCopy {..}));assert!(b.try_recv().is_err());
    let generation=first.doc.as_ref().unwrap().generation;
    first.doc.as_mut().unwrap().changes.undone();first.doc.as_mut().unwrap().generation+=1;
    assert!(!first.doc.as_ref().unwrap().dirty());assert!(TabbedApp::needs_warning(&first));
    first.file_save.configure(Mode::Off);
    first.handle(&ctx,Reply::AutoSaved {generation,result:Ok(Some(("generated-working-copy.pdf".into(),false)))});
    assert!(first.doc.as_ref().unwrap().dirty());assert!(first.file_save.in_flight.is_none());
    working_auto_frame(&mut second,&ctx,13.);assert!(matches!(b.try_recv().unwrap(),Request::AutoSaveWorkingCopy {..}));
    second.file_save.reset_document();second.busy=3;
    second.handle(&ctx,Reply::AutoSaved {generation,result:Ok(Some(("obsolete.pdf".into(),false)))});
    assert_eq!(second.busy,3);assert!(second.doc.as_ref().unwrap().dirty());
}

#[test]
fn working_autosave_preferences_are_separate_bounded_and_shared_across_tabs() {
    use working_autosave::Mode;
    let folder=recovery_safety_folder("file-auto-preference");let config=folder.join("config.txt");
    let path=folder.join("working-copy-autosave.txt");let recovery=folder.join("app-settings.txt");
    assert_eq!(Mode::load(&path).unwrap(),Mode::Off);
    AutoSave(Some(1)).save(&recovery).unwrap();
    for mode in Mode::ALL {mode.save(&path).unwrap();assert_eq!(Mode::load(&path).unwrap(),mode);}
    std::fs::write(&path,b"working-copy-autosave=instant\n").unwrap();assert!(Mode::load(&path).is_err());
    std::fs::write(&path,vec![b'x';4096]).unwrap();assert!(Mode::load(&path).is_err());
    let ctx=Context::default();let(mut first,_)=working_auto_mock(&ctx);let(second,_)=working_auto_mock(&ctx);
    first.config_path=Some(config);first.set_file_save_mode(Mode::AfterEdits);
    let mut tabs=TabbedApp {sessions:Default::default(),native_guard:None,clipboard:Default::default(),tabs:vec![(0,first),(1,second)],active:0,next_id:2,closing:None,allow_exit:false,instance:None,merge_tab:None,dragging_tab:None};
    tabs.sync_working_auto_save(&ctx);
    assert!(tabs.tabs.iter().all(|(_,tab)|tab.file_save.mode==Mode::AfterEdits));
    assert_eq!(Mode::load(&path).unwrap(),Mode::AfterEdits);assert_eq!(AutoSave::load(&recovery).unwrap(),AutoSave(Some(1)));
    tabs.tabs[0].1.set_file_save_mode(Mode::Off);tabs.sync_working_auto_save(&ctx);
    assert!(tabs.tabs.iter().all(|(_,tab)|tab.file_save.mode==Mode::Off));
    assert_eq!(Mode::load(&path).unwrap(),Mode::Off);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn generated_working_autosave_updates_only_the_copy_and_survives_undo_external_changes_and_retry() {
    use working_autosave::Mode;
    let(mut app,ctx,source)=qol_worker();let original=std::fs::read(&source).unwrap();
    let folder=recovery_safety_folder("file-auto-real");let copy=folder.join("working.pdf");
    app.document_action(Request::SaveAs(copy.clone()));workflow_settle(&mut app,&ctx);
    let saved=std::fs::read(&copy).unwrap();app.file_save.configure(Mode::AfterEdits);
    let at=workflow_line(&app,&ctx,0);workflow_edit(&mut app,&ctx,at);
    workflow_frame(&mut app,&ctx,vec![workflow_key(Key::A,egui::Modifiers::COMMAND)]);
    workflow_frame(&mut app,&ctx,vec![egui::Event::Text("AB".into())]);
    working_auto_canvas_frame(&mut app,&ctx,10.);working_auto_canvas_frame(&mut app,&ctx,20.);
    assert_eq!(std::fs::read(&copy).unwrap(),saved);assert!(app.doc.as_ref().unwrap().edit.as_ref().unwrap().changed());
    workflow_frame(&mut app,&ctx,vec![workflow_key(Key::Enter,Default::default())]);workflow_settle(&mut app,&ctx);
    assert!(app.doc.as_ref().unwrap().dirty());assert!(app.doc.as_ref().unwrap().edit.is_none());
    working_auto_canvas_frame(&mut app,&ctx,30.);working_auto_canvas_frame(&mut app,&ctx,31.9);
    assert_eq!(std::fs::read(&copy).unwrap(),saved);
    working_auto_canvas_frame(&mut app,&ctx,32.);assert!(app.file_save.in_flight.is_some());assert_eq!(app.busy,0);
    working_auto_wait(&mut app,&ctx);assert!(!app.doc.as_ref().unwrap().dirty());
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine=pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    assert_eq!(engine.text_objects(&engine.open(&copy).unwrap(),0).unwrap()[0].text,"AB");
    assert_eq!(std::fs::read(&source).unwrap(),original);
    app.undo();workflow_settle(&mut app,&ctx);assert!(app.doc.as_ref().unwrap().dirty());
    app.redo();workflow_settle(&mut app,&ctx);assert!(!app.doc.as_ref().unwrap().dirty());
    let mut external=std::fs::read(&copy).unwrap();let middle=external.len()/2;external[middle]^=1;std::fs::write(&copy,&external).unwrap();
    let object=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    app.document_action(Request::Edit {objects:vec![object],text:"BA".into()});workflow_settle(&mut app,&ctx);
    working_auto_canvas_frame(&mut app,&ctx,40.);working_auto_canvas_frame(&mut app,&ctx,42.);working_auto_wait(&mut app,&ctx);
    assert!(app.file_save.paused);assert!(app.doc.as_ref().unwrap().dirty());
    working_auto_canvas_frame(&mut app,&ctx,1000.);
    assert_eq!(std::fs::read(&copy).unwrap(),external);assert_eq!(std::fs::read(&source).unwrap(),original);
    let retry=folder.join("retry.pdf");app.document_action(Request::SaveAs(retry.clone()));workflow_settle(&mut app,&ctx);
    assert!(!app.file_save.paused);
    let object=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    app.document_action(Request::Edit {objects:vec![object],text:"AB".into()});workflow_settle(&mut app,&ctx);
    working_auto_canvas_frame(&mut app,&ctx,1100.);working_auto_canvas_frame(&mut app,&ctx,1102.);working_auto_wait(&mut app,&ctx);
    assert_eq!(engine.text_objects(&engine.open(&retry).unwrap(),0).unwrap()[0].text,"AB");
    std::fs::remove_file(&retry).unwrap();
    let object=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    app.document_action(Request::Edit {objects:vec![object],text:"BA".into()});workflow_settle(&mut app,&ctx);
    working_auto_canvas_frame(&mut app,&ctx,1200.);working_auto_canvas_frame(&mut app,&ctx,1202.);working_auto_wait(&mut app,&ctx);
    assert!(app.file_save.paused);assert!(!retry.exists(),"A missing working copy must not be silently recreated");
    assert_eq!(std::fs::read(&copy).unwrap(),external);assert_eq!(std::fs::read(&source).unwrap(),original);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn generated_working_autosave_preserves_copy_encryption() {
    use working_autosave::Mode;
    let(mut app,ctx,source)=qol_worker();let original=std::fs::read(&source).unwrap();
    let folder=recovery_safety_folder("file-auto-encrypted");let path=folder.join("encrypted.pdf");
    app.document_action(Request::SavePassword(path.clone(),"generated-password".into()));workflow_settle(&mut app,&ctx);
    app.file_save.configure(Mode::AfterEdits);
    let object=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    app.document_action(Request::Edit {objects:vec![object],text:"AB".into()});workflow_settle(&mut app,&ctx);
    working_auto_frame(&mut app,&ctx,10.);working_auto_frame(&mut app,&ctx,12.);working_auto_wait(&mut app,&ctx);
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine=pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    assert!(engine.open(&path).is_err());assert!(engine.open_with_password(&path,"wrong").is_err());
    let reopened=engine.open_with_password(&path,"generated-password").unwrap();
    assert_eq!(engine.text_objects(&reopened,0).unwrap()[0].text,"AB");
    assert_eq!(std::fs::read(source).unwrap(),original);assert!(!app.doc.as_ref().unwrap().dirty());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn working_autosave_setting_can_be_selected_and_disabled_with_the_pointer() {
    use working_autosave::Mode;
    let folder=recovery_safety_folder("file-auto-settings-ui");
    let ctx=Context::default();theme::install(&ctx);
    let (mut app,_,_)=mock_app();app.config_path=Some(folder.join("config.txt"));
    let frame=|app:&mut EditorApp,events| {
        let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(900.,600.))),events,..Default::default()},|ui| {
            let p=theme::palette(ui);
            egui::CentralPanel::default().show(ui,|ui|app.working_auto_save_settings(ui,&p));
        });out.textures_delta.clear();out
    };
    let center=|out:&egui::FullOutput,label:&str|out.shapes.iter().filter_map(|s|match &s.shape {
        Shape::Text(t) if t.galley.text()==label=>Some(t.pos+t.galley.size()*0.5),_=>None
    }).next_back().unwrap_or_else(||panic!("Missing control: {label}"));
    let button=|pos,pressed|egui::Event::PointerButton {pos,pressed,button:egui::PointerButton::Primary,modifiers:Default::default()};
    frame(&mut app,vec![]);
    for mode in [Mode::AfterEdits,Mode::Minute,Mode::Off] {
        let out=frame(&mut app,vec![]);let at=center(&out,app.file_save.mode.label());
        frame(&mut app,vec![egui::Event::PointerMoved(at),button(at,true)]);
        frame(&mut app,vec![button(at,false)]);
        let out=frame(&mut app,vec![]);let at=center(&out,mode.label());
        frame(&mut app,vec![egui::Event::PointerMoved(at),button(at,true)]);
        frame(&mut app,vec![button(at,false)]);
        assert_eq!(app.file_save.mode,mode);
        assert_eq!(Mode::load(&folder.join("working-copy-autosave.txt")).unwrap(),mode);
        let out=frame(&mut app,vec![]);
        let explanation=out.shapes.iter().any(|s|matches!(&s.shape,Shape::Text(t) if t.galley.text()=="Use Save As once to create a working copy."));
        assert_eq!(explanation,mode!=Mode::Off);
    }
    std::fs::remove_dir_all(folder).unwrap();
}
