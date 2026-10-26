fn font_readout_frame(app: &mut EditorApp, ctx: &Context, width: f32) -> Vec<String> {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 600.))),
            ..Default::default()
        },
        |ui| {
            let p = theme::palette(ui);
            egui::Panel::bottom("font-test-status").show(ui, |ui| app.status_bar(ui, &p));
            egui::CentralPanel::default().show(ui, |_| {});
            app.qol_windows(ui.ctx(), &p);
        },
    );
    output.textures_delta.clear();
    fn collect(shape: &Shape, text: &mut Vec<String>) {
        match shape {
            Shape::Text(value) => text.push(value.galley.text().to_owned()),
            Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let mut text = vec![];
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut text);
    }
    text
}

#[test]
fn generated_selected_text_displays_its_pdf_font_and_clears_stale_names() {
    let (mut app, ctx, source) = qol_worker();
    let bytes = std::fs::read(&source).unwrap();
    app.set_object_mode(true);
    workflow_frame(&mut app, &ctx, vec![]);
    let at = workflow_line(&app, &ctx, 0);
    workflow_click(&mut app, &ctx, at);
    let raw = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines[0]
        .objects[0]
        .font
        .clone();
    assert!(!raw.is_empty());
    let expected = format!("Font: {raw}");
    let text = font_readout_frame(&mut app, &ctx, 1000.);
    assert!(
        text.contains(&expected),
        "Selected PDF font is missing from the status bar: {text:?}"
    );
    app.set_object_mode(false);
    workflow_frame(&mut app, &ctx, vec![]);
    let at = workflow_line(&app, &ctx, 0);
    workflow_edit(&mut app, &ctx, at);
    assert!(font_readout_frame(&mut app, &ctx, 1000.).contains(&expected));
    app.cancel_edit();
    workflow_frame(&mut app, &ctx, vec![]);
    workflow_frame(&mut app, &ctx, vec![]);
    let page = ctx
        .read_response(Id::new(("pdf-page", 0usize)))
        .unwrap()
        .rect;
    workflow_click(&mut app, &ctx, page.min + vec2(10., 10.));
    assert!(app.qol.objects.is_empty());
    assert!(app.doc.as_ref().unwrap().selected_item.is_none());
    assert!(!font_readout_frame(&mut app, &ctx, 1000.).contains(&expected));
    assert!(!app.doc.as_ref().unwrap().dirty());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn font_readout_tracks_actual_replacements_and_keeps_mixed_and_draft_fonts_distinct() {
    let (mut app, ctx, source) = qol_worker();
    let bytes = std::fs::read(&source).unwrap();
    let original = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines[0]
        .objects[0]
        .clone();
    app.document_action(Request::FormatText(
        vec![original.clone()],
        pdf_editor_core::TextFormat {
            font: "Arial".into(),
            bold: false,
            italic: false,
            size: original.size,
            color: [original.color[0], original.color[1], original.color[2]],
        },
    ));
    workflow_settle(&mut app, &ctx);
    app.set_object_mode(true);
    let objects = app.doc.as_ref().unwrap().pages[0].objects.as_ref().unwrap();
    let current = objects
        .lines
        .iter()
        .flat_map(|l| &l.objects)
        .find(|o| o.text == original.text)
        .unwrap()
        .clone();
    assert_ne!(current.font, original.font);
    app.qol.objects = objects
        .items
        .iter()
        .filter(|i| i.index == current.index)
        .cloned()
        .collect();
    app.doc.as_mut().unwrap().selected_item = app.qol.primary();
    let expected = format!("Font: {}", current.font);
    assert!(font_readout_frame(&mut app, &ctx, 1000.).contains(&expected));
    app.qol.format_targets = app.formatting_targets();
    app.qol.family = "Unapplied replacement".into();
    app.qol.format_open = true;
    let generation = app.doc.as_ref().unwrap().generation;
    let mut text = vec![];
    for _ in 0..3 {
        text = font_readout_frame(&mut app, &ctx, 1000.);
    }
    assert!(text.contains(&format!("Current font: {}", current.font)));
    assert!(!text.contains(&"Current font: Unapplied replacement".into()));
    assert_eq!(app.qol.family, "Unapplied replacement");
    assert_eq!(app.doc.as_ref().unwrap().generation, generation);
    app.qol.format_targets.clear();
    app.qol.objects = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter(|i| i.kind == "Text")
        .cloned()
        .collect();
    app.doc.as_mut().unwrap().selected_item = app.qol.primary();
    for _ in 0..3 {
        text = font_readout_frame(&mut app, &ctx, 1000.);
    }
    assert!(text.contains(&"Font: Mixed fonts".into()));
    assert!(text.contains(&"Current font: Mixed fonts".into()));
    app.qol.format_open = false;
    app.qol.objects.clear();
    app.doc.as_mut().unwrap().selected_item = None;
    app.undo();
    workflow_settle(&mut app, &ctx);
    let at = workflow_line(&app, &ctx, 0);
    workflow_click(&mut app, &ctx, at);
    assert!(
        font_readout_frame(&mut app, &ctx, 1000.).contains(&format!("Font: {}", original.font))
    );
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn long_and_unknown_font_readouts_keep_minimum_width_controls_reachable() {
    let (mut app, ctx, _) = qol_worker();
    let object = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines[0]
        .objects[0]
        .clone();
    app.text_tool.selection = vec![object];
    for dark in [false, true] {
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        for name in [
            "ABCDEF+Font-With-A-Very-Long-PostScript-Name-字体-Åäö".repeat(8),
            String::new(),
        ] {
            app.text_tool.selection[0].font = name.clone();
            for _ in 0..3 {
                font_readout_frame(&mut app, &ctx, 640.);
            }
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(640., 600.));
            for key in ["go-to-page", "status-zoom", "toggle-pages"] {
                let rect = ctx.read_response(widgets::tool_id(key)).unwrap().rect;
                assert!(screen.contains_rect(rect), "{key}: {rect:?}");
            }
            let text = font_readout_frame(&mut app, &ctx, 640.);
            if name.is_empty() {
                assert!(text.contains(&"Font: Unknown".into()));
            } else {
                assert!(
                    text.iter()
                        .any(|s| s.starts_with("Font: Font-With-A-Very-Long"))
                );
            }
            assert_eq!(app.busy, 0);
            assert!(!app.doc.as_ref().unwrap().dirty());
        }
    }
}
