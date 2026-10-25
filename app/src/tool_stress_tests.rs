use super::*;

fn show(app: &mut EditorApp, ctx: &Context, p: &Palette, tool: Tool, idle: bool) {
    let request = match tool {
        Tool::ImageCreate => app.image_create.show(ctx, p, idle),
        Tool::ScanCleanup => app.scan_cleanup.show(ctx, p, idle, 2),
        Tool::Replace => app.replace.show(ctx, p, idle),
        Tool::Forms => app.forms.show(ctx, p, idle),
        Tool::Signatures => app.signatures.show(ctx, p, None, 2, idle),
        Tool::Pictures => app.pictures.show(ctx, p, idle, 2),
        Tool::Markup => app.markup.show(ctx, p, idle, 2),
        Tool::Drawing => {
            app.drawing.show(ctx, p);
            None
        }
        Tool::Privacy => app.privacy.show(ctx, p, idle),
        Tool::Redact => app.redaction.show(ctx, p, idle, 2),
        Tool::Organize => app.organize.show(ctx, p, idle),
        Tool::Crop => app.crop.show(ctx, p, idle, 2),
        Tool::Stamps => app.stamps.show(ctx, p, idle),
        Tool::Export => app.export.show(ctx, p, idle),
        Tool::Tables => app.tables.show(ctx, p, idle),
        Tool::Word => app.word.show(ctx, p, idle),
        Tool::Compress => app.compression.show(ctx, p, idle),
        Tool::Sanitize => app.sanitize.show(ctx, p, idle),
        Tool::Compare => {
            app.compare.show(ctx, p);
            None
        }
        _ => unreachable!("Only tool windows belong in this matrix"),
    };
    assert!(
        request.is_none(),
        "Rendering or closing a tool must not submit work"
    );
}

#[test]
fn every_tool_window_keeps_close_reachable_when_idle_or_busy() {
    for size in [vec2(640.0, 420.0), vec2(1180.0, 820.0)] {
        for dark in [false, true] {
            for idle in [false, true] {
                for (tool, title) in [
                    (Tool::ImageCreate, "Images to PDF"),
                    (Tool::ScanCleanup, "Scan cleanup"),
                    (Tool::Replace, "Find and replace"),
                    (Tool::Forms, "Forms"),
                    (Tool::Signatures, "Signatures"),
                    (Tool::Pictures, "Images"),
                    (Tool::Markup, "Markup and notes"),
                    (Tool::Drawing, "Draw and shapes"),
                    (Tool::Privacy, "Search and redact"),
                    (Tool::Redact, "Secure redaction"),
                    (Tool::Organize, "Merge, split and extract"),
                    (Tool::Crop, "Crop page"),
                    (Tool::Stamps, "Headers, footers and Bates"),
                    (Tool::Export, "Export page images"),
                    (Tool::Tables, "Tables to Excel or CSV"),
                    (Tool::Word, "Export to Word"),
                    (Tool::Compress, "Compress images"),
                    (Tool::Sanitize, "Sanitize document"),
                    (Tool::Compare, "Compare PDFs"),
                ] {
                    let ctx = Context::default();
                    theme::install(&ctx);
                    ctx.set_visuals(if dark {
                        egui::Visuals::dark()
                    } else {
                        egui::Visuals::light()
                    });
                    let (mut app, _inbox, _outbox) = mock_app();
                    opened(&mut app, &ctx);
                    app.open_tool(tool);
                    let screen = Rect::from_min_size(Pos2::ZERO, size);
                    let frame = |app: &mut EditorApp, events| {
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(screen),
                                events,
                                ..Default::default()
                            },
                            |ui| show(app, &ctx, &theme::palette(ui), tool, idle),
                        );
                        output.textures_delta.clear();
                        output
                    };
                    for _ in 0..4 {
                        frame(&mut app, vec![]);
                    }
                    let close = ctx
                        .read_response(widgets::tool_id(&format!("close-tool-{title}")))
                        .unwrap();
                    assert!(
                        screen.contains_rect(close.rect),
                        "{title} close outside {size:?}: {:?}",
                        close.rect
                    );
                    let at = close.rect.center();
                    let click = click_at(at);
                    frame(&mut app, click[..2].to_vec());
                    frame(&mut app, click[2..].to_vec());
                    // egui animates closed windows briefly; drain that animation.
                    let mut output = frame(&mut app, vec![]);
                    for _ in 0..16 {
                        output = frame(&mut app, vec![]);
                    }
                    assert!(
                        !output.shapes.iter().any(|shape| matches!(&shape.shape,
                        Shape::Text(text) if text.galley.text() == title)),
                        "{title} did not close"
                    );
                }
            }
        }
    }
}
