#[test]
fn generated_text_drag_tracks_pointer_keeps_ink_and_commits_once_on_release() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    app.snap_enabled = false;
    app.set_object_mode(true);
    workflow_frame(&mut app, &ctx, vec![]);
    let objects = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
    let before = objects.lines[0].objects[0].clone();
    let transform = objects.transform;
    let start = workflow_line(&app, &ctx, 0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    };
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.drag_preview.ready() {
        workflow_frame(&mut app, &ctx, vec![]);
        assert!(
            std::time::Instant::now() < deadline,
            "Native drag layers did not arrive"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let generation = app.doc.as_ref().unwrap().generation;
    let scale = app.doc.as_ref().unwrap().scale();
    for offset in [
        vec2(12., 5.),
        vec2(13., 6.),
        vec2(44., 28.),
        vec2(30., 20.),
        vec2(80., 60.),
    ] {
        workflow_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(start + offset)],
        );
        for _ in 0..2 {
            let doc = app.doc.as_ref().unwrap();
            let drag = doc.dragging_item.as_ref().unwrap();
            assert!((drag.delta - offset / scale).length() < 0.01);
            assert!((drag.preview.unwrap().min - drag.bounds.min - offset / scale).length() < 0.01);
            assert_eq!(doc.generation, generation);
            assert_eq!(app.busy, 0);
            assert!(!doc.dirty());
            workflow_frame(&mut app, &ctx, vec![]);
        }
    }
    workflow_frame(&mut app, &ctx, vec![button(start + vec2(90., 65.), false)]);
    assert_eq!(app.busy, 1);
    assert!(app.drag_preview.committing());
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.doc.as_ref().unwrap().generation, generation + 1);
    assert!(!app.drag_preview.committing());
    assert!(app.doc.as_ref().unwrap().dirty());
    let moved = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .flat_map(|line| &line.objects)
        .find(|object| object.index == before.index)
        .unwrap();
    let (dx, dy) = raster_delta(&transform, vec2(90., 65.) / scale).unwrap();
    assert!((moved.matrix[4] - before.matrix[4] - dx).abs() < 0.05);
    assert!((moved.matrix[5] - before.matrix[5] - dy).abs() < 0.05);
    assert_eq!(moved.text, before.text);
    assert_eq!(moved.font, before.font);
    assert_eq!(moved.color, before.color);
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    app.redo();
    workflow_settle(&mut app, &ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn escape_cancels_drag_without_mutating_generated_document() {
    let (mut app, ctx, _) = qol_worker();
    app.snap_enabled = false;
    app.set_object_mode(true);
    workflow_frame(&mut app, &ctx, vec![]);
    let start = workflow_line(&app, &ctx, 0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    };
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start + vec2(70., 50.))],
    );
    assert!(app.doc.as_ref().unwrap().dragging_item.is_some());
    workflow_frame(
        &mut app,
        &ctx,
        vec![workflow_key(Key::Escape, Default::default())],
    );
    assert!(app.doc.as_ref().unwrap().dragging_item.is_none());
    workflow_frame(&mut app, &ctx, vec![button(start + vec2(70., 50.), false)]);
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
}

fn gesture_button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    }
}

#[test]
fn text_font_handles_preserve_proportions_fixed_anchor_and_size_limits() {
    let font=FontResize {minimum:0.1,maximum:10.,size:Some(10.)};
    let original=Rect::from_min_max(pos2(20.,30.),pos2(140.,70.));
    for handle in 0..8 {
        for delta in [vec2(35.,8.),vec2(-22.,-13.),vec2(10000.,10000.)] {
            let result=font.drag(original,handle,delta);
            let factor=result.width()/original.width();
            assert!((factor-result.height()/original.height()).abs()<0.001);
            assert!((0.099..=10.001).contains(&factor));
            assert!((resize_handles(result)[(handle+4)%8]-resize_handles(original)[(handle+4)%8]).length()<0.001);
        }
    }
}

#[test]
fn generated_font_resize_shows_proportional_ink_and_size_before_one_commit() {
    let (mut app,ctx,source)=qol_worker();
    let bytes=std::fs::read(&source).unwrap();
    app.set_object_mode(true);
    for (zoom,snap) in [(0.75,false),(1.5,true)] {
        app.snap_enabled=snap;
        app.set_zoom(zoom,None);workflow_settle(&mut app,&ctx);
        workflow_frame_at(&mut app,&ctx,vec![],Some(ctx.input(|i|i.time)+1.));
        let at=workflow_line(&app,&ctx,0);workflow_click(&mut app,&ctx,at);
        let doc=app.doc.as_ref().unwrap();
        let objects=doc.pages[0].objects.as_ref().unwrap();
        let before=objects.lines[0].objects[0].clone();
        let original=bounds_rect(&objects.transform,doc.selected_item.as_ref().unwrap().bounds);
        let page=ctx.read_response(Id::new(("pdf-page",0usize))).unwrap().rect;
        let start=page.min+original.right_bottom().to_vec2()*doc.scale();
        let generation=doc.generation;
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)]);
        let end=start+vec2(55.,4.);
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end)]);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while !app.drag_preview.ready() {
            workflow_frame(&mut app,&ctx,vec![]);
            assert!(std::time::Instant::now()<deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let doc=app.doc.as_ref().unwrap();
        let drag=doc.dragging_item.as_ref().unwrap();
        let preview=drag.preview.unwrap();
        let factor=preview.width()/original.width();
        assert!(factor>1.);
        assert!((factor-preview.height()/original.height()).abs()<0.001);
        assert!((preview.min-original.min).length()<0.001);
        assert_eq!(doc.generation,generation);assert_eq!(app.busy,0);
        assert_eq!(app.drag_preview.placement(),Some(preview));
        let label=format!("{:.1} pt",drag.font_resize.as_ref().unwrap().size.unwrap()*factor);
        app.poll(&ctx);
        let mut painted=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(1000.,900.))),..Default::default()},|ui| {
            let p=theme::palette(ui);egui::CentralPanel::default().show(ui,|ui|app.canvas(ui,&p));
        });
        assert!(painted.shapes.iter().any(|shape|matches!(&shape.shape,egui::epaint::Shape::Text(t) if t.galley.text()==label)),"Live font size missing");
        painted.textures_delta.clear();
        workflow_frame(&mut app,&ctx,vec![gesture_button(end,false)]);
        assert_eq!(app.busy,1);workflow_settle(&mut app,&ctx);
        let objects=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
        let resized=objects.lines.iter().flat_map(|line|&line.objects).find(|o|o.index==before.index).unwrap();
        assert!((resized.size-before.size*factor).abs()<0.01);
        assert_eq!(resized.font,before.font);assert_eq!(resized.text,before.text);
        assert!(resized.matrix[..4].iter().zip(&before.matrix[..4]).all(|(a,b)|(a-b).abs()<0.001));
        app.undo();workflow_settle(&mut app,&ctx);
        assert!(!app.doc.as_ref().unwrap().dirty());
        assert!(!app.doc.as_ref().unwrap().can_undo);
    }
    assert_eq!(std::fs::read(source).unwrap(),bytes);
}

#[test]
fn perpendicular_edge_drag_does_not_create_a_phantom_edit() {
    let (mut app, ctx, source) = qol_worker();
    let bytes = std::fs::read(&source).unwrap();
    app.snap_enabled = false;
    app.set_object_mode(true);
    workflow_frame(&mut app, &ctx, vec![]);
    let at = workflow_line(&app, &ctx, 0);
    workflow_click(&mut app, &ctx, at);
    let doc = app.doc.as_ref().unwrap();
    let generation = doc.generation;
    let page = ctx
        .read_response(Id::new(("pdf-page", 0usize)))
        .unwrap()
        .rect;
    let bounds = bounds_rect(
        &doc.pages[0].objects.as_ref().unwrap().transform,
        doc.selected_item.as_ref().unwrap().bounds,
    );
    let start = page.min + resize_handles(bounds)[3].to_vec2() * doc.scale();
    workflow_frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            gesture_button(start, true),
        ],
    );
    let end = start + vec2(0., 24.);
    workflow_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    assert_eq!(
        app.doc
            .as_ref()
            .unwrap()
            .dragging_item
            .as_ref()
            .unwrap()
            .preview,
        Some(bounds)
    );
    workflow_frame(&mut app, &ctx, vec![gesture_button(end, false)]);
    assert_eq!(app.busy, 0, "Unchanged bounds dispatched a mutation");
    workflow_settle(&mut app, &ctx);
    let doc = app.doc.as_ref().unwrap();
    assert_eq!(doc.generation, generation);
    assert!(!doc.dirty());
    assert!(!doc.can_undo);
    assert!(!app.drag_preview.committing());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn generated_multi_object_gestures_keep_geometry_across_rotation_zoom_and_history() {
    let (mut app, ctx, source) = qol_worker();
    let bytes = std::fs::read(&source).unwrap();
    app.snap_enabled = false;
    app.set_object_mode(true);
    for rotation in 0..4 {
        if rotation > 0 {
            app.document_action(Request::Pages(pdf_editor_core::PageEdit::Rotate {
                page: 0,
            }));
            workflow_settle(&mut app, &ctx);
        }
        for zoom in [0.75, 1.5] {
            app.set_zoom(zoom, None);
            workflow_settle(&mut app, &ctx);
            for resize in [false, true] {
                let doc = app.doc.as_ref().unwrap();
                let objects = doc.pages[0].objects.as_ref().unwrap();
                let before = objects
                    .items
                    .iter()
                    .filter(|i| i.kind == "Text")
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>();
                assert_eq!(before.len(), 2);
                let transform = objects.transform;
                let bounds = before.iter().fold(Rect::NOTHING, |b, i| {
                    b.union(bounds_rect(&transform, i.bounds))
                });
                let scale = doc.scale();
                let generation = doc.generation;
                let page = ctx
                    .read_response(Id::new(("pdf-page", 0usize)))
                    .unwrap()
                    .rect;
                let at = if resize {
                    resize_handles(bounds)[4]
                } else {
                    bounds_rect(&transform, before[0].bounds).center()
                };
                let start = page.min + at.to_vec2() * scale;
                app.qol.objects = before.clone();
                app.doc.as_mut().unwrap().selected_item = app.qol.primary();
                workflow_frame(
                    &mut app,
                    &ctx,
                    vec![
                        egui::Event::PointerMoved(start),
                        gesture_button(start, true),
                    ],
                );
                let delta = vec2(45., 28.);
                workflow_frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(start + delta)],
                );
                let drag = app.doc.as_ref().unwrap().dragging_item.as_ref().unwrap();
                assert_eq!(drag.items.len(), 2);
                assert_eq!(drag.resize_handle, resize.then_some(4));
                assert_eq!(app.busy, 0);
                assert_eq!(app.doc.as_ref().unwrap().generation, generation);
                let delta = delta + vec2(8., 7.);
                let expected = if resize {
                    drag.font_resize.as_ref().unwrap().drag(bounds, 4, delta / scale)
                } else {
                    bounds.translate(delta / scale)
                };
                workflow_frame(&mut app, &ctx, vec![gesture_button(start + delta, false)]);
                assert_eq!(app.busy, 1);
                workflow_settle(&mut app, &ctx);
                let doc = app.doc.as_ref().unwrap();
                assert_eq!(doc.generation, generation + 1);
                let objects = doc.pages[0].objects.as_ref().unwrap();
                let actual = before.iter().fold(Rect::NOTHING, |b, old| {
                    let item = objects.items.iter().find(|i| i.index == old.index).unwrap();
                    b.union(bounds_rect(&objects.transform, item.bounds))
                });
                assert!(
                    (actual.min - expected.min).length() < 0.3,
                    "rotation={rotation}, zoom={zoom}, resize={resize}: {actual:?} != {expected:?}"
                );
                assert!((actual.max - expected.max).length() < 0.3);
                assert_eq!(app.qol.objects.len(), 2);
                app.undo();
                workflow_settle(&mut app, &ctx);
                let objects = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
                for old in &before {
                    let item = objects.items.iter().find(|i| i.index == old.index).unwrap();
                    assert_eq!(item.bounds, old.bounds);
                }
                app.redo();
                workflow_settle(&mut app, &ctx);
                let folder = recovery_safety_folder("gesture-roundtrip");
                let output = folder.join(format!("generated-{rotation}-{zoom}-{resize}.pdf"));
                app.document_action(Request::SaveAs(output.clone()));
                workflow_settle(&mut app, &ctx);
                let engine = pdf_editor_core::Engine::new(
                    &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .unwrap()
                        .join("target/debug/pdfium.dll"),
                )
                .unwrap();
                let saved = engine.open(&output).unwrap();
                let saved_items = engine.page_items(&saved, 0).unwrap();
                let objects = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
                for item in &objects.items {
                    assert_eq!(
                        item.bounds,
                        saved_items
                            .iter()
                            .find(|i| i.index == item.index)
                            .unwrap()
                            .bounds
                    );
                }
                app.undo();
                workflow_settle(&mut app, &ctx);
            }
        }
    }
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn generated_busy_page_drag_keeps_pointer_frames_free_of_mutations() {
    // A small native PDF generator keeps the stress input independent of saved files.
    let folder = recovery_safety_folder("busy-gesture");
    let source = folder.join("generated-busy.pdf");
    let mut stream = String::from("BT /F1 16 Tf 1 0 0 1 50 750 Tm (Drag target) Tj ET\n");
    for index in 0..2000 {
        stream.push_str(&format!(
            "{} {} 4 4 re f\n",
            15 + (index % 40) * 14,
            20 + (index / 40) * 13
        ));
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in &offsets[1..] {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    std::fs::write(&source, &bytes).unwrap();
    let (mut app, ctx, _) = qol_worker();
    app.open_path(source.clone());
    workflow_settle(&mut app, &ctx);
    app.set_zoom(0.75, None);
    app.set_object_mode(true);
    workflow_settle(&mut app, &ctx);
    assert_eq!(
        app.doc.as_ref().unwrap().pages[0]
            .objects
            .as_ref()
            .unwrap()
            .items
            .len(),
        2001
    );
    let start = workflow_line(&app, &ctx, 0);
    let generation = app.doc.as_ref().unwrap().generation;
    workflow_frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            gesture_button(start, true),
        ],
    );
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start + vec2(10., 15.))],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.drag_preview.ready() {
        workflow_frame(&mut app, &ctx, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut timings = vec![];
    for frame in 0..120 {
        let pointer = start + vec2(10. + (frame % 60) as f32, 15. + (frame % 20) as f32);
        let began = std::time::Instant::now();
        workflow_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(pointer)]);
        timings.push(began.elapsed().as_micros());
        assert_eq!(app.busy, 0);
        assert_eq!(app.doc.as_ref().unwrap().generation, generation);
        let drag = app.doc.as_ref().unwrap().dragging_item.as_ref().unwrap();
        assert_eq!(drag.snap_targets.len(), 2000);
        let rect = ctx
            .read_response(Id::new(("pdf-page", 0usize)))
            .unwrap()
            .rect;
        let expected =
            ((pointer - rect.min) / app.doc.as_ref().unwrap().scale()).to_pos2() - drag.anchor;
        assert!((drag.delta - expected).length() < 0.001);
    }
    timings.sort_unstable();
    println!(
        "generated 2001-object drag frame us: median={}, p95={}, max={}",
        timings[60], timings[114], timings[119]
    );
    workflow_frame(
        &mut app,
        &ctx,
        vec![workflow_key(Key::Escape, Default::default())],
    );
    workflow_frame(&mut app, &ctx, vec![gesture_button(start, false)]);
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn zoom_during_generated_drag_keeps_the_page_anchor_and_release_position() {
    let (mut app, ctx, _) = qol_worker();
    app.snap_enabled = false;
    app.set_object_mode(true);
    workflow_frame(&mut app, &ctx, vec![]);
    for resize in [false, true] {
        // Selection belongs to a new gesture, outside the previous double-click interval.
        workflow_frame_at(&mut app,&ctx,vec![],Some(ctx.input(|i|i.time)+1.));
        app.set_zoom(0.75, None);
        workflow_settle(&mut app, &ctx);
        let at = workflow_line(&app, &ctx, 0);
        workflow_click(&mut app, &ctx, at);
        let doc = app.doc.as_ref().unwrap();
        let objects = doc.pages[0].objects.as_ref().unwrap();
        let transform = objects.transform;
        let bounds = bounds_rect(&transform, doc.selected_item.as_ref().unwrap().bounds);
        let page = ctx
            .read_response(Id::new(("pdf-page", 0usize)))
            .unwrap()
            .rect;
        let start = if resize {
            page.min + resize_handles(bounds)[4].to_vec2() * doc.scale()
        } else {
            at
        };
        workflow_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                gesture_button(start, true),
            ],
        );
        let pointer = start + vec2(50., 20.);
        workflow_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(pointer)]);
        let anchor = app
            .doc
            .as_ref()
            .unwrap()
            .dragging_item
            .as_ref()
            .unwrap_or_else(||panic!("Drag did not start: resize={resize}, busy={}, editing={}, panning={}, start={start:?}, pointer={pointer:?}, response={:?}, press={:?}",app.busy,app.doc.as_ref().unwrap().edit.is_some(),app.doc.as_ref().unwrap().panning,ctx.read_response(Id::new(("pdf-page",0usize))),ctx.input(|i|i.pointer.press_origin())))
            .anchor;
        app.set_zoom(1.5, Some(pointer));
        workflow_settle(&mut app, &ctx);
        let doc = app.doc.as_ref().unwrap();
        let page = ctx
            .read_response(Id::new(("pdf-page", 0usize)))
            .unwrap()
            .rect;
        let end = pointer + vec2(18., 12.);
        let delta = ((end - page.min) / doc.scale()).to_pos2() - anchor;
        let expected = if resize {
            app.doc.as_ref().unwrap().dragging_item.as_ref().unwrap().font_resize.as_ref().unwrap().drag(bounds, 4, delta)
        } else {
            bounds.translate(delta)
        };
        assert_eq!(doc.dragging_item.as_ref().unwrap().anchor, anchor);
        let indices = doc
            .dragging_item
            .as_ref()
            .unwrap()
            .items
            .iter()
            .map(|i| i.index)
            .collect::<Vec<_>>();
        workflow_frame(&mut app, &ctx, vec![gesture_button(end, false)]);
        assert_eq!(app.busy, 1);
        workflow_settle(&mut app, &ctx);
        let objects = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
        let actual = objects
            .items
            .iter()
            .filter(|i| indices.contains(&i.index))
            .fold(Rect::NOTHING, |b, i| {
                b.union(bounds_rect(&objects.transform, i.bounds))
            });
        assert!((actual.min - expected.min).length() < 0.3);
        assert!((actual.max - expected.max).length() < 0.3);
        app.undo();
        workflow_settle(&mut app, &ctx);
        assert!(!app.doc.as_ref().unwrap().dirty());
    }
}
#[test]
fn generated_single_line_width_grip_remains_usable_after_typing() {
    let (mut app,ctx,source)=qol_worker();
    let bytes=std::fs::read(&source).unwrap();
    let at=workflow_line(&app,&ctx,0);
    workflow_edit(&mut app,&ctx,at);
    workflow_frame(&mut app,&ctx,vec![]);
    workflow_frame(&mut app,&ctx,vec![egui::Event::Text(" additional words to widen the draft".into())]);
    // Let TextEdit's changed layout reach the cached interaction geometry.
    workflow_frame(&mut app,&ctx,vec![]);
    workflow_frame(&mut app,&ctx,vec![]);
    workflow_frame(&mut app,&ctx,vec![]);
    let edit=app.doc.as_ref().unwrap().edit.as_ref().unwrap();
    assert!(edit.line.paragraph.is_none());
    let before=edit.line.objects[0].clone();
    let text=edit.text.clone();
    let id=Id::new(("text-width",edit.page,before.index,before.revision));
    let grip=ctx.read_response(id).unwrap().rect.center();
    let editor=ctx.read_response(Id::new(("edit-box",edit.page,before.index,before.revision))).unwrap().rect;
    assert!(!editor.contains(grip),"The text editor must not cover its width grip");
    assert!(app.doc.as_ref().unwrap().viewport.contains(grip),"Grip {grip:?} outside viewport {:?}, editor {editor:?}",app.doc.as_ref().unwrap().viewport);
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(grip),gesture_button(grip,true)]);
    assert!(app.doc.as_ref().unwrap().edit.is_some(),"Press closed editor: {:?}",ctx.read_response(id));
    assert!(ctx.read_response(id).unwrap().is_pointer_button_down_on(),"Grip {grip:?} did not own press at {:?}: {:?}; viewport {:?}",ctx.input(|i|i.pointer.interact_pos()),ctx.read_response(id),app.doc.as_ref().unwrap().viewport);
    let end=grip+vec2(30.,0.);
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end)]);
    assert!(app.doc.as_ref().unwrap().edit.is_some(),"Drag closed editor: {:?}",ctx.read_response(id));
    assert!(ctx.read_response(id).unwrap().dragged(),"Grip did not drag: {:?}",ctx.read_response(id));
    workflow_frame(&mut app,&ctx,vec![gesture_button(end,false)]);
    let edit=app.doc.as_ref().unwrap().edit.as_ref().unwrap();
    assert!(edit.line.paragraph.as_ref().unwrap().wrap_width.is_some());
    assert_eq!(edit.text,text);
    assert_eq!(edit.line.objects[0].size,before.size);
    workflow_frame(&mut app,&ctx,vec![workflow_key(Key::Escape,Default::default())]);
    assert!(app.doc.as_ref().unwrap().edit.is_none());
    assert!(!app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(),bytes);
}

#[test]
fn generated_drag_aligns_different_glyph_heights_to_the_same_baseline() {
    let (mut app,ctx,source)=qol_worker();
    let bytes=std::fs::read(&source).unwrap();
    let transform=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap().transform;
    let info=&app.doc.as_ref().unwrap().pages[0].info;
    let (w,h)=(info.width,info.height);
    for (text,x,y) in [("HIT",w*0.15,h*0.5),("gyp",w*0.55,h*0.35)] {
        app.busy+=1;
        app.send(Request::AddText { text:pdf_editor_core::NewText {
            page:0,x,y,text:text.into(),font:"Arial".into(),size:14.,color:[0,0,0],
        },raster:transform.apply(x,y) });
        workflow_settle(&mut app,&ctx);
    }
    app.set_object_mode(true);
    workflow_frame_at(&mut app,&ctx,vec![],Some(ctx.input(|i|i.time)+1.));
    let doc=app.doc.as_ref().unwrap();
    let objects=doc.pages[0].objects.as_ref().unwrap();
    let index=objects.lines.iter().position(|line|line.text=="gyp").unwrap();
    let before=objects.lines[index].objects[0].clone();
    let target=objects.lines.iter().find(|line|line.text=="HIT").unwrap().objects[0].clone();
    let old_baseline=transform.apply(before.matrix[4],before.matrix[5]).1;
    let target_baseline=transform.apply(target.matrix[4],target.matrix[5]).1;
    let scale=doc.scale();
    let generation=doc.generation;
    let start=workflow_line(&app,&ctx,index);
    let end=start+vec2(20.,target_baseline-old_baseline+2.)*scale;
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)]);
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end)]);
    let drag=app.doc.as_ref().unwrap().dragging_item.as_ref().unwrap();
    let offset=drag.snap_baselines.moving.unwrap().1;
    assert!((drag.preview.unwrap().top()+offset-target_baseline).abs()<0.001);
    assert_eq!(app.doc.as_ref().unwrap().generation,generation);
    workflow_frame(&mut app,&ctx,vec![gesture_button(end,false)]);
    workflow_settle(&mut app,&ctx);
    let objects=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
    let moved=objects.lines.iter().flat_map(|line|&line.objects).find(|o|o.text=="gyp").unwrap();
    assert!((moved.matrix[5]-target.matrix[5]).abs()<0.01);
    assert_eq!(moved.size,before.size);
    assert_eq!(moved.font,before.font);
    app.undo();workflow_settle(&mut app,&ctx);
    let objects=app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
    assert_eq!(objects.lines.iter().flat_map(|line|&line.objects).find(|o|o.text=="gyp").unwrap().matrix,before.matrix);
    assert_eq!(std::fs::read(source).unwrap(),bytes);
}

#[test]
fn generated_text_width_drag_reflows_without_scaling_or_losing_focus() {
    let (mut app, ctx, _) = qol_worker();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = root.join("tests/fixtures/generated/paragraph-Left.pdf");
    let bytes = std::fs::read(&source).unwrap();
    app.open_path(source.clone());
    workflow_settle(&mut app,&ctx);
    let at = workflow_line(&app,&ctx,0);
    workflow_edit(&mut app,&ctx,at);
    workflow_frame(&mut app,&ctx,vec![]);
    let doc = app.doc.as_ref().unwrap();
    let edit = doc.edit.as_ref().expect("Paragraph editor opened");
    let before = edit.line.objects[0].clone();
    let old_width = edit.line.rect.width();
    let scale = doc.scale();
    let generation = doc.generation;
    let id = Id::new(("text-width",0usize,before.index,before.revision));
    let start = ctx.read_response(id).expect("Text width handle").rect.center();
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)]);
    for dx in [30.,70.,140.] {
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+vec2(dx,0.))]);
        let doc=app.doc.as_ref().unwrap();
        let edit=doc.edit.as_ref().expect("Width drag keeps draft open");
        assert!((edit.line.rect.width()-old_width-dx/scale).abs()<0.1);
        assert!(edit.line.paragraph.as_ref().unwrap().wrap_width.is_some());
        assert!(edit.changed());
        assert_eq!(edit.line.objects[0].size,before.size);
        assert_eq!(doc.generation,generation);
        assert_eq!(app.busy,0);
    }
    workflow_frame(&mut app,&ctx,vec![gesture_button(start+vec2(140.,0.),false)]);
    assert!(app.doc.as_ref().unwrap().edit.is_some());
    assert_eq!(app.busy,0);
    workflow_frame(&mut app,&ctx,vec![workflow_key(Key::Enter,egui::Modifiers::CTRL)]);
    assert_eq!(app.busy,1);
    workflow_settle(&mut app,&ctx);
    let doc=app.doc.as_ref().unwrap();
    assert!(doc.edit.is_none());
    let objects=doc.pages[0].objects.as_ref().unwrap();
    let edited=objects.lines.iter().flat_map(|line|&line.objects).filter(|o|o.text!="untouched").collect::<Vec<_>>();
    assert!(edited.len()<3,"Widening reflows original soft wraps");
    assert!(edited.iter().all(|o|o.size==before.size && o.font==before.font && o.matrix[..4]==before.matrix[..4]));
    app.undo();workflow_settle(&mut app,&ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    app.redo();workflow_settle(&mut app,&ctx);
    assert!(app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(),bytes);
}

#[test]
fn generated_drag_release_over_another_handle_does_not_follow_later_clicks() {
    let(mut app,ctx,source)=qol_worker();let original=std::fs::read(&source).unwrap();
    app.snap_enabled=false;app.set_object_mode(true);workflow_frame(&mut app,&ctx,vec![]);
    let start=workflow_line(&app,&ctx,0);workflow_click(&mut app,&ctx,start);
    let doc=app.doc.as_ref().unwrap();let item=doc.selected_item.as_ref().unwrap();
    let objects=doc.pages[0].objects.as_ref().unwrap();
    let page=ctx.read_response(Id::new(("pdf-page",0usize))).unwrap().rect;
    let end=to_screen(bounds_rect(&objects.transform,item.bounds),page,doc.scale()).right_bottom();
    let generation=doc.generation;
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)]);
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+vec2(24.,18.))]);
    assert!(app.doc.as_ref().unwrap().dragging_item.is_some());
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end),gesture_button(end,false)]);
    assert!(app.doc.as_ref().unwrap().dragging_item.is_none(),"Releasing over a handle left the old drag attached");
    workflow_settle(&mut app,&ctx);let moved=app.doc.as_ref().unwrap().generation;
    assert_eq!(moved,generation+1);
    for at in [start+vec2(180.,90.),pos2(960.,820.)] {
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(at)]);workflow_click(&mut app,&ctx,at);
        workflow_settle(&mut app,&ctx);assert!(app.doc.as_ref().unwrap().dragging_item.is_none());assert_eq!(app.doc.as_ref().unwrap().generation,moved);
    }
    app.undo();workflow_settle(&mut app,&ctx);assert!(!app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(),original);
}

#[test]
fn generated_drag_focus_loss_or_pointer_capture_loss_cancels_without_changes() {
    for event in [egui::Event::WindowFocused(false),egui::Event::PointerGone] {
        let(mut app,ctx,source)=qol_worker();let original=std::fs::read(&source).unwrap();
        app.snap_enabled=false;app.set_object_mode(true);workflow_frame(&mut app,&ctx,vec![]);
        let start=workflow_line(&app,&ctx,0);let generation=app.doc.as_ref().unwrap().generation;
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)]);
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+vec2(50.,30.))]);
        assert!(app.doc.as_ref().unwrap().dragging_item.is_some());
        workflow_frame(&mut app,&ctx,vec![event]);
        assert!(app.doc.as_ref().unwrap().dragging_item.is_none(),"An interrupted drag stayed attached to the pointer");
        assert_eq!(app.busy,0);assert_eq!(app.doc.as_ref().unwrap().generation,generation);assert!(!app.doc.as_ref().unwrap().dirty());
        let control=Some(start+vec2(60.,65.));
        drag_controls_frame(&mut app,&ctx,vec![egui::Event::WindowFocused(true)],control,true);
        let(out,_)=drag_controls_frame(&mut app,&ctx,vec![],control,true);
        let at=out.shapes.iter().find_map(|s|match &s.shape {Shape::Text(t) if t.galley.text()=="Other control"=>Some(t.pos+t.galley.size()*0.5),_=>None}).unwrap();
        drag_controls_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(at),gesture_button(at,true)],control,true);
        let(_,clicked)=drag_controls_frame(&mut app,&ctx,vec![gesture_button(at,false)],control,true);
        assert!(clicked,"Interrupted capture stole the next control click");assert!(app.doc.as_ref().unwrap().dragging_item.is_none());assert_eq!(app.doc.as_ref().unwrap().generation,generation);
        assert_eq!(std::fs::read(source).unwrap(),original);
    }
}

#[test]
fn generated_drag_with_secondary_button_never_moves_pdf_objects() {
    let(mut app,ctx,_)=qol_worker();app.set_object_mode(true);workflow_frame(&mut app,&ctx,vec![]);
    let start=workflow_line(&app,&ctx,0);let generation=app.doc.as_ref().unwrap().generation;
    let button=|pos,pressed|egui::Event::PointerButton {pos,pressed,button:egui::PointerButton::Secondary,modifiers:Default::default()};
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),button(start,true)]);
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+vec2(45.,25.))]);
    assert!(app.doc.as_ref().unwrap().dragging_item.is_none(),"A context-menu gesture started an object drag");
    workflow_frame(&mut app,&ctx,vec![button(start+vec2(45.,25.),false)]);workflow_settle(&mut app,&ctx);
    assert_eq!(app.doc.as_ref().unwrap().generation,generation);assert!(!app.doc.as_ref().unwrap().dirty());
}

fn drag_controls_frame(app:&mut EditorApp,ctx:&Context,events:Vec<egui::Event>,control:Option<Pos2>,canvas:bool)->(egui::FullOutput,bool) {
    app.poll(ctx);let mut clicked=false;
    let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,vec2(1000.,900.))),events,..Default::default()},|ui| {
        app.update_item_drag(ctx);let p=theme::palette(ui);
        egui::CentralPanel::default().show(ui,|ui|if canvas {app.canvas(ui,&p);});
        if let Some(pos)=control {egui::Window::new("Generated control").fixed_pos(pos).collapsible(false).resizable(false).show(ctx,|ui|clicked=ui.button("Other control").clicked());}
    });out.textures_delta.clear();(out,clicked)
}

#[test]
fn generated_drag_drop_over_a_control_finishes_once_and_the_control_still_clicks() {
    let(mut app,ctx,source)=qol_worker();let original=std::fs::read(&source).unwrap();app.snap_enabled=false;app.set_object_mode(true);
    workflow_frame(&mut app,&ctx,vec![]);let start=workflow_line(&app,&ctx,0);let control=Some(start+vec2(50.,35.));
    drag_controls_frame(&mut app,&ctx,vec![],control,true);let(out,_)=drag_controls_frame(&mut app,&ctx,vec![],control,true);
    let end=out.shapes.iter().find_map(|s|match &s.shape {Shape::Text(t) if t.galley.text()=="Other control"=>Some(t.pos+t.galley.size()*0.5),_=>None}).unwrap();
    let generation=app.doc.as_ref().unwrap().generation;
    drag_controls_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)],control,true);
    drag_controls_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+vec2(25.,20.))],control,true);
    assert!(app.doc.as_ref().unwrap().dragging_item.is_some());
    let(_,clicked)=drag_controls_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end),gesture_button(end,false)],control,true);
    assert!(!clicked);assert!(app.doc.as_ref().unwrap().dragging_item.is_none());assert_eq!(app.busy,1);
    workflow_settle(&mut app,&ctx);assert_eq!(app.doc.as_ref().unwrap().generation,generation+1);
    drag_controls_frame(&mut app,&ctx,vec![],control,true);
    drag_controls_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end),gesture_button(end,true)],control,true);
    let(_,clicked)=drag_controls_frame(&mut app,&ctx,vec![gesture_button(end,false)],control,true);
    assert!(clicked,"The old drag stole a later control click");assert!(app.doc.as_ref().unwrap().dragging_item.is_none());assert_eq!(app.doc.as_ref().unwrap().generation,generation+1);
    app.undo();workflow_settle(&mut app,&ctx);assert!(!app.doc.as_ref().unwrap().dirty());assert_eq!(std::fs::read(source).unwrap(),original);
}

#[test]
fn generated_drag_rapid_release_and_next_press_use_the_release_position_once() {
    let(mut app,ctx,_)=qol_worker();app.snap_enabled=false;app.set_object_mode(true);workflow_frame(&mut app,&ctx,vec![]);
    let start=workflow_line(&app,&ctx,0);let doc=app.doc.as_ref().unwrap();let before=doc.pages[0].objects.as_ref().unwrap().lines[0].objects[0].clone();
    let transform=doc.pages[0].objects.as_ref().unwrap().transform;let scale=doc.scale();let generation=doc.generation;
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)]);
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+vec2(30.,15.))]);
    let end=start+vec2(55.,30.);let next=start+vec2(240.,130.);
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end),gesture_button(end,false),egui::Event::PointerMoved(next),gesture_button(next,true)]);
    assert!(app.doc.as_ref().unwrap().dragging_item.is_none());workflow_frame(&mut app,&ctx,vec![gesture_button(next,false)]);workflow_settle(&mut app,&ctx);
    let doc=app.doc.as_ref().unwrap();assert_eq!(doc.generation,generation+1);
    let moved=doc.pages[0].objects.as_ref().unwrap().lines.iter().flat_map(|line|&line.objects).find(|o|o.index==before.index).unwrap();
    let(dx,dy)=raster_delta(&transform,(end-start)/scale).unwrap();assert!((moved.matrix[4]-before.matrix[4]-dx).abs()<0.05);assert!((moved.matrix[5]-before.matrix[5]-dy).abs()<0.05);
}

#[test]
fn generated_drag_hidden_canvas_tab_switch_and_undo_cancel_pending_geometry() {
    for mode in 0..3 {
        let(mut app,ctx,source)=qol_worker();let original=std::fs::read(&source).unwrap();app.snap_enabled=false;app.set_object_mode(true);workflow_frame(&mut app,&ctx,vec![]);
        let start=workflow_line(&app,&ctx,0);let generation=app.doc.as_ref().unwrap().generation;
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),gesture_button(start,true)]);
        workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+vec2(65.,40.))]);assert!(app.doc.as_ref().unwrap().dragging_item.is_some());
        if mode==0 {
            app.settings_open=true;drag_controls_frame(&mut app,&ctx,vec![gesture_button(start+vec2(65.,40.),false)],None,false);
            app.settings_open=false;
        } else if mode==1 {
            let (mut other,_,_)=mock_app();opened(&mut other,&ctx);
            let mut tabs=TabbedApp {tabs:vec![(0,app),(1,other)],active:0,next_id:2,sessions:Default::default(),native_guard:None,clipboard:Default::default(),closing:None,allow_exit:false,instance:None,merge_tab:None,dragging_tab:None};
            let mut out=ctx.run_ui(egui::RawInput {events:vec![workflow_key(Key::Tab,egui::Modifiers::COMMAND)],..Default::default()},|_|tabs.sessions_shortcuts(&ctx));out.textures_delta.clear();
            assert_eq!(tabs.active,1);app=tabs.tabs.remove(0).1;
        } else {app.undo();}
        assert!(app.doc.as_ref().unwrap().dragging_item.is_none());assert!(app.drag_preview.placement().is_none());
        workflow_frame(&mut app,&ctx,vec![gesture_button(start+vec2(65.,40.),false)]);workflow_settle(&mut app,&ctx);
        assert_eq!(app.doc.as_ref().unwrap().generation,generation);assert!(!app.doc.as_ref().unwrap().dirty());assert_eq!(std::fs::read(source).unwrap(),original);
    }
}

#[test]
fn generated_drag_fast_resize_keeps_the_captured_handle_and_commits_on_release() {
    let(mut app,ctx,_)=qol_worker();app.snap_enabled=false;app.set_object_mode(true);workflow_frame(&mut app,&ctx,vec![]);
    let at=workflow_line(&app,&ctx,0);workflow_click(&mut app,&ctx,at);
    let doc=app.doc.as_ref().unwrap();let item=doc.selected_item.as_ref().unwrap();let objects=doc.pages[0].objects.as_ref().unwrap();
    let page=ctx.read_response(Id::new(("pdf-page",0usize))).unwrap().rect;
    let start=resize_handles(to_screen(bounds_rect(&objects.transform,item.bounds),page,doc.scale()))[4];let generation=doc.generation;
    workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start)]);
    workflow_frame(&mut app,&ctx,vec![gesture_button(start,true)]);
    let end=start+vec2(42.,18.);workflow_frame(&mut app,&ctx,vec![egui::Event::PointerMoved(end)]);
    assert_eq!(app.doc.as_ref().unwrap().dragging_item.as_ref().unwrap().resize_handle,Some(4));
    workflow_frame(&mut app,&ctx,vec![gesture_button(end,false)]);assert!(app.doc.as_ref().unwrap().dragging_item.is_none());
    workflow_settle(&mut app,&ctx);assert_eq!(app.doc.as_ref().unwrap().generation,generation+1);
    app.undo();workflow_settle(&mut app,&ctx);assert!(!app.doc.as_ref().unwrap().dirty());
}
