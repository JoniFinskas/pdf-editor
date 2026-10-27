#[test]
fn hyperlink_worker_crud_keeps_selection_and_tracks_undo_without_touching_source() {
    use pdf_editor_core::{LinkSpec, LinkTarget};
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    app.markup.links_mode = true;
    app.open_tool(Tool::Markup);
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.links.items.is_empty());
    let mut spec = LinkSpec {
        page: 0,
        bounds: pdf_editor_core::Bounds {
            left: 30.,
            bottom: 40.,
            right: 160.,
            top: 70.,
        },
        target: LinkTarget::uri("https://example.org").unwrap(),
    };
    app.document_action(Request::AddLink(spec.clone()));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.markup.links.items.len(), 1);
    app.markup.links.selected = Some(0);
    app.markup.links.draft = spec.clone();
    let selected = app.markup.links.items[0].clone();
    let generation = app.doc.as_ref().unwrap().generation;
    app.document_action(Request::UpdateLink(selected.clone(), spec.clone()));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.doc.as_ref().unwrap().generation, generation);
    assert_eq!(app.markup.links.selected, Some(0));
    spec.target = LinkTarget::uri("mailto:reader@example.org").unwrap();
    spec.bounds.left += 10.;
    app.document_action(Request::UpdateLink(selected, spec.clone()));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.markup.links.selected, Some(0));
    assert_eq!(app.markup.links.draft, spec);
    let before_failure = app.doc.as_ref().unwrap().generation;
    let selected = app.markup.links.items[0].clone();
    let mut invalid = spec.clone();
    invalid.target = LinkTarget::Uri("javascript:unused()".into());
    app.document_action(Request::UpdateLink(selected.clone(), invalid));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.busy > 0 {
        workflow_frame(&mut app, &ctx, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(app.engine_alive);
    assert_eq!(app.doc.as_ref().unwrap().generation, before_failure);
    assert_eq!(app.markup.links.selected, Some(0));
    app.banners.clear();
    app.document_action(Request::DeleteLink(selected));
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.links.items.is_empty());
    assert_eq!(app.markup.links.selected, None);
    app.undo();
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.markup.links.items[0].link, spec);
    app.redo();
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.links.items.is_empty());
    app.open_path(source.clone());
    workflow_settle(&mut app, &ctx);
    assert!(app.markup.links.items.is_empty());
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn annotation_update_keeps_selected_row_and_acknowledged_draft() {
    let (mut app, ctx, source) = qol_worker();
    let original = std::fs::read(&source).unwrap();
    app.open_tool(Tool::Markup);
    workflow_settle(&mut app, &ctx);
    let mut markup = app.markup.draft.clone();
    markup.kind = pdf_editor_core::MarkupKind::Note;
    markup.text = "Generated comment".into();
    app.document_action(Request::AddAnnotation(markup.clone()));
    workflow_settle(&mut app, &ctx);
    app.markup.selected = Some(0);
    app.markup.draft = markup.clone();
    markup.text = "Updated generated comment".into();
    app.document_action(Request::UpdateAnnotation(
        app.markup.items[0].clone(),
        markup.clone(),
    ));
    workflow_settle(&mut app, &ctx);
    assert_eq!(app.markup.selected, Some(0));
    assert_eq!(app.markup.draft, markup);
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn link_area_drag_consumes_canvas_input_and_maps_all_corners() {
    for matrix in [
        [1., 0., 0., -1., -10., 285.],
        [0., 1., 1., 0., -15., -10.],
        [-1., 0., 0., 1., 490., -15.],
        [0., -1., -1., 0., 285., 490.],
    ] {
        let ctx = Context::default();
        theme::install(&ctx);
        let (mut app, inbox, _outbox) = mock_app();
        opened(&mut app, &ctx);
        app.markup.open = true;
        app.markup.links_mode = true;
        app.markup.links.selecting = true;
        let transform = PageTransform::for_test(matrix);
        app.handle(
            &ctx,
            Reply::Objects {
                generation: 1,
                page: 0,
                transform,
                objects: vec![],
                items: vec![],
            },
        );
        for _ in 0..3 {
            workflow_frame(&mut app, &ctx, vec![]);
        }
        inbox.try_iter().for_each(drop);
        let rect = ctx
            .read_response(Id::new(("pdf-page", 0usize)))
            .unwrap()
            .rect;
        let scale = app.doc.as_ref().unwrap().scale();
        let start = rect.min + vec2(30., 40.) * scale;
        let end = rect.min + vec2(130., 100.) * scale;
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
        workflow_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
        workflow_frame(&mut app, &ctx, vec![button(end, false)]);
        assert!(!app.markup.links.selecting);
        let expected = raster_bounds(
            &transform,
            Rect::from_min_max(pos2(30., 40.), pos2(130., 100.)),
        )
        .unwrap();
        for (a, b) in [
            app.markup.links.draft.bounds.left,
            app.markup.links.draft.bounds.bottom,
            app.markup.links.draft.bounds.right,
            app.markup.links.draft.bounds.top,
        ]
        .into_iter()
        .zip([expected.left, expected.bottom, expected.right, expected.top])
        {
            assert!((a - b).abs() < 0.1, "{a} != {b}");
        }
        assert!(app.doc.as_ref().unwrap().edit.is_none());
        assert!(app.doc.as_ref().unwrap().dragging_item.is_none());
        assert!(!inbox.try_iter().any(|r| matches!(
            r,
            Request::EditItems(..)
                | Request::Edit { .. }
                | Request::AddText { .. }
                | Request::Draw(_)
        )));
    }
}

#[test]
fn large_markup_and_link_lists_build_only_visible_rows() {
    use pdf_editor_core::{Annotation, LinkSpec, LinkTarget, PdfLink};
    for links in [false, true] {
        let ctx = Context::default();
        theme::install(&ctx);
        let mut panel = crate::markup::MarkupTool::default();
        panel.open = true;
        panel.links_mode = links;
        let markup = panel.draft.clone();
        panel.items = (0..10_000)
            .map(|index| Annotation {
                index,
                revision: 0,
                markup: markup.clone(),
            })
            .collect();
        panel.links.items = (0..10_000)
            .map(|index| PdfLink {
                index,
                revision: 0,
                object_id: None,
                link: LinkSpec {
                    page: 0,
                    bounds: markup.bounds,
                    target: LinkTarget::Uri(format!("https://example.org/{index}")),
                },
            })
            .collect();
        let mut output = None;
        for _ in 0..3 {
            let mut current = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280., 900.))),
                    ..Default::default()
                },
                |ui| {
                    let p = theme::palette(ui);
                    assert!(panel.show(&ctx, &p, true, 1).is_none());
                },
            );
            current.textures_delta.clear();
            output = Some(current);
        }
        let texts: Vec<_> = output
            .unwrap()
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        let rows = texts.iter().filter(|t| t.as_str() == "Page 1").count();
        assert!(rows > 0 && rows < 20, "Built {rows} rows");
    }
}

#[test]
fn link_area_button_stays_armed_after_its_click_release() {
    let ctx = Context::default();
    theme::install(&ctx);
    let mut links = crate::links::Links::default();
    let mut frame = |events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000., 900.))),
                events,
                ..Default::default()
            },
            |ui| {
                let p = theme::palette(ui);
                links.show(ui, &p, true, 2);
                links.finish_frame(&ctx);
            },
        );
        output.textures_delta.clear();
        output.shapes
    };
    let mut shapes = vec![];
    for _ in 0..3 {
        shapes = frame(vec![]);
    }
    let point = button_center(&shapes, "Select area on page");
    frame(click_at(point));
    assert!(links.selecting);
}

#[test]
fn link_area_selection_waits_for_transform_without_panning_or_editing() {
    let ctx = Context::default();
    theme::install(&ctx);
    let (mut app, inbox, _) = mock_app();
    opened(&mut app, &ctx);
    app.markup.open = true;
    app.markup.links_mode = true;
    app.markup.links.selecting = true;
    for _ in 0..3 {
        workflow_frame(&mut app, &ctx, vec![]);
    }
    inbox.try_iter().for_each(drop);
    let rect = ctx
        .read_response(Id::new(("pdf-page", 0usize)))
        .unwrap()
        .rect;
    let start = rect.center();
    workflow_frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                pressed: true,
                button: egui::PointerButton::Primary,
                modifiers: Default::default(),
            },
        ],
    );
    workflow_frame(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(start + vec2(30., 20.))],
    );
    let doc = app.doc.as_ref().unwrap();
    assert!(!doc.panning);
    assert!(doc.edit.is_none());
    assert!(doc.dragging_item.is_none());
    assert!(app.markup.links.selecting);
    assert!(!inbox.try_iter().any(|r| matches!(
        r,
        Request::EditItems(..) | Request::Edit { .. } | Request::AddText { .. } | Request::Draw(_)
    )));
}
