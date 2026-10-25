// Display-resolution rendering through the real worker with generated pages.
#[test]
fn high_zoom_tiles_refresh_after_pan_dpi_changes_edits_and_undo() {
    let (mut app, ctx, _) = qol_worker();
    ctx.set_pixels_per_point(2.);
    app.set_zoom(4., None);
    let settle = |app: &mut EditorApp| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            workflow_frame(app, &ctx, vec![]);
            if app.busy == 0
                && app
                    .doc
                    .as_ref()
                    .unwrap()
                    .pages
                    .iter()
                    .any(|page| page.tiles.ready())
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "High-resolution tiles did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let doc = app.doc.as_ref().unwrap();
        let cost = doc
            .pages
            .iter()
            .map(|page| {
                page.tiles.bytes()
                    + page
                        .texture
                        .as_ref()
                        .map_or(0, |texture| texture.handle.byte_size())
            })
            .sum::<usize>();
        assert!(cost <= 128 * 1024 * 1024);
    };
    settle(&mut app);
    app.doc.as_mut().unwrap().scroll_to = Some(vec2(400., 200.));
    settle(&mut app);
    ctx.set_pixels_per_point(2.5);
    settle(&mut app);
    app.busy += 1;
    app.send(Request::AddText {
        text: pdf_editor_core::NewText {
            page: 0,
            x: 20.,
            y: 20.,
            text: "AB".into(),
            font: "Arial".into(),
            size: 12.,
            color: [0, 0, 0],
        },
        raster: (20., 20.),
    });
    settle(&mut app);
    assert!(app.doc.as_ref().unwrap().dirty());
    app.undo();
    settle(&mut app);
    assert!(!app.doc.as_ref().unwrap().dirty());
}
