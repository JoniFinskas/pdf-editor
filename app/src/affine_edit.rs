//! Native TextEdit keyboard/history with affine paint and pointer coordinates.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Plane {
    pub origin: Pos2,
    pub x: Vec2,
    pub y: Vec2,
}
impl Plane {
    pub fn point(self, p: Pos2) -> Pos2 {
        self.origin + self.x * p.x + self.y * p.y
    }
    pub fn inverse(self, p: Pos2) -> Option<Pos2> {
        let det = self.x.x * self.y.y - self.x.y * self.y.x;
        if det.abs() < 0.0001 {
            return None;
        }
        let delta = p - self.origin;
        Some(pos2(
            (delta.x * self.y.y - delta.y * self.y.x) / det,
            (self.x.x * delta.y - self.x.y * delta.x) / det,
        ))
    }
    pub fn rect(self, r: Rect) -> Rect {
        Rect::from_points(&[
            self.point(r.left_top()),
            self.point(r.right_top()),
            self.point(r.right_bottom()),
            self.point(r.left_bottom()),
        ])
    }
}

pub(super) fn plane(
    object: &TextObject,
    transform: &PageTransform,
    page: Rect,
    scale: f32,
) -> Option<(Plane, Vec2)> {
    let points = object.quad.map(|[x, y]| {
        let (x, y) = transform.apply(x, y);
        page.min + vec2(x, y) * scale
    });
    let baseline = points[2] - points[3];
    let down = points[0] - points[3];
    let width = baseline.length();
    let height = down.length();
    if width < 0.1 || height < 0.1 {
        return None;
    }
    let [a, b, c, d, _, _] = object.matrix;
    let font = (object.size * (a * d - b * c).abs().sqrt() * scale).clamp(6., 240.);
    let origin = transform.apply(0., 0.);
    let along = transform.apply(a, b);
    let normal = transform.apply(c, d);
    let horizontal = (along.0 - origin.0).hypot(along.1 - origin.1) * scale * object.size / font;
    let vertical = (normal.0 - origin.0).hypot(normal.1 - origin.1) * scale * object.size / font;
    if !horizontal.is_finite() || !vertical.is_finite() || horizontal < 0.0001 || vertical < 0.0001
    {
        return None;
    }
    let plane = Plane {
        origin: points[3],
        x: baseline / width * horizontal,
        y: down / height * vertical,
    };
    plane.inverse(points[0])?;
    Some((plane, vec2(width / horizontal, height / vertical)))
}

pub(super) fn line_plane(
    line: &Line,
    transform: &PageTransform,
    page: Rect,
    scale: f32,
) -> Option<(Plane, Vec2)> {
    let (mut plane, _) = plane(line.objects.first()?, transform, page, scale)?;
    let mut bounds = Rect::NOTHING;
    for object in &line.objects {
        for [x, y] in object.quad {
            let (x, y) = transform.apply(x, y);
            bounds.extend_with(plane.inverse(page.min + vec2(x, y) * scale)?);
        }
    }
    plane.origin = plane.point(bounds.min);
    Some((plane, bounds.size()))
}

fn paint_transformed(ui: &Ui, layer: egui::LayerId, plane: Plane, virtual_origin: Pos2) {
    let shapes = ui.ctx().graphics_mut(|graphics| {
        let list = graphics.entry(layer);
        let copy = list.all_entries().cloned().collect::<Vec<_>>();
        *list = Default::default();
        copy
    });
    for primitive in ui.ctx().tessellate(shapes, ui.ctx().pixels_per_point()) {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
            continue;
        };
        let mut clipped = egui::epaint::Mesh::with_texture(mesh.texture_id);
        for indices in mesh.indices.as_chunks::<3>().0 {
            let mut polygon = indices
                .iter()
                .map(|&index| mesh.vertices[index as usize])
                .collect::<Vec<_>>();
            for edge in 0..4 {
                let distance = |v: egui::epaint::Vertex| match edge {
                    0 => v.pos.x - primitive.clip_rect.left(),
                    1 => primitive.clip_rect.right() - v.pos.x,
                    2 => v.pos.y - primitive.clip_rect.top(),
                    _ => primitive.clip_rect.bottom() - v.pos.y,
                };
                let mut next = Vec::new();
                for index in 0..polygon.len() {
                    let a = polygon[index];
                    let b = polygon[(index + 1) % polygon.len()];
                    let da = distance(a);
                    let db = distance(b);
                    if da >= 0. {
                        next.push(a);
                    }
                    if (da >= 0.) != (db >= 0.) {
                        let t = da / (da - db);
                        let ca = a.color.to_array();
                        let cb = b.color.to_array();
                        let color = std::array::from_fn::<_, 4, _>(|i| {
                            egui::lerp(ca[i] as f32..=cb[i] as f32, t).round() as u8
                        });
                        next.push(egui::epaint::Vertex {
                            pos: a.pos + (b.pos - a.pos) * t,
                            uv: a.uv + (b.uv - a.uv) * t,
                            color: Color32::from_rgba_premultiplied(
                                color[0], color[1], color[2], color[3],
                            ),
                        });
                    }
                }
                polygon = next;
            }
            let base = clipped.vertices.len() as u32;
            for mut vertex in polygon {
                vertex.pos = plane.point((vertex.pos - virtual_origin).to_pos2());
                clipped.vertices.push(vertex);
            }
            let count = clipped.vertices.len() as u32 - base;
            for index in 1..count.saturating_sub(1) {
                clipped
                    .indices
                    .extend_from_slice(&[base, base + index, base + index + 1]);
            }
        }
        ui.painter().add(Shape::mesh(clipped));
    }
}

pub(super) fn show(
    ui: &mut Ui,
    edit: &mut EditState,
    id: Id,
    geometry: (Plane, Vec2),
    font: FontId,
    color: Color32,
    p: &Palette,
) -> Option<bool> {
    let (mut plane, size) = geometry;
    let width = size.x.clamp(40., 16000.);
    let height = size.y.max(font.size * 1.25);
    plane.origin -= plane.y * ((height - size.y) / 2.);
    let local = Rect::from_min_size(Pos2::ZERO, vec2(width, height)).expand2(vec2(3., 1.));
    let corners = [
        local.left_top(),
        local.right_top(),
        local.right_bottom(),
        local.left_bottom(),
    ]
    .map(|point| plane.point(point));
    ui.painter().add(egui::epaint::PathShape::convex_polygon(
        corners.to_vec(),
        Color32::WHITE,
        Stroke::new(1.5, p.accent),
    ));
    let pointer = ui.input(|input| input.pointer.clone());
    let inside = pointer
        .interact_pos()
        .and_then(|pos| plane.inverse(pos))
        .is_some_and(|pos| local.contains(pos));
    let virtual_origin = pos2(-20000., -20000.);
    let rect = Rect::from_min_size(virtual_origin, vec2(width, height));
    let galley = ui.fonts_mut(|fonts| fonts.layout_no_wrap(edit.text.clone(), font.clone(), color));
    let hit = ui.ctx().data(|data| {
        data.get_temp::<(Id, std::sync::Arc<egui::Galley>, Vec2)>(Id::new("affine-editor-hit"))
            .filter(|(old, _, _)| *old == id)
    });
    if inside {
        ui.ctx().set_cursor_icon(CursorIcon::Text);
    }
    let selecting = ui.ctx().data_mut(|data| {
        let key = id.with("affine-selection-drag");
        let captured = if pointer.primary_pressed() {
            inside
        } else {
            data.get_temp::<bool>(key).unwrap_or(false)
        };
        data.insert_temp(key, captured && pointer.primary_down());
        captured && (pointer.primary_down() || pointer.primary_released())
    });
    if selecting && let Some(position) = pointer.interact_pos() {
        ui.memory_mut(|memory| memory.request_focus(id));
        let local = plane.inverse(position).unwrap();
        let cursor = if let Some((_, galley, offset)) = &hit {
            galley.cursor_from_pos(local.to_vec2() - *offset)
        } else {
            galley.cursor_from_pos(local.to_vec2() - vec2(3., (height - galley.size().y) / 2.))
        };
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        let anchor = state
            .cursor
            .char_range()
            .map_or(cursor, |range| range.secondary);
        let range = if pointer.button_triple_clicked(egui::PointerButton::Primary) {
            egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(edit.text.chars().count()),
            )
        } else if pointer.button_double_clicked(egui::PointerButton::Primary) {
            let chars = edit.text.chars().collect::<Vec<_>>();
            let mut start = cursor.index.0.min(chars.len());
            let mut end = start;
            while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                start -= 1;
            }
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            egui::text::CCursorRange::two(
                egui::text::CCursor::new(start),
                egui::text::CCursor::new(end),
            )
        } else if pointer.primary_pressed() && !ui.input(|input| input.modifiers.shift) {
            egui::text::CCursorRange::one(cursor)
        } else {
            egui::text::CCursorRange {
                primary: cursor,
                secondary: anchor,
                h_pos: None,
            }
        };
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(), id);
    }
    let escape = ui.input(|input| input.key_pressed(Key::Escape));
    let enter = ui.input(|input| input.key_pressed(Key::Enter));
    let initial = edit.focus;
    if initial {
        ui.memory_mut(|memory| memory.request_focus(id));
        edit.focus = false;
    }
    // Keep the real widget outside the viewport. Pointer handling above maps
    // into text coordinates; the native editor retains keyboard, IME and undo.
    let layer = egui::LayerId::new(egui::Order::Foreground, id.with("affine-paint"));
    let visible = ui.interact(plane.rect(local), id.with("affine-menu"), Sense::click());
    let spelling = crate::spelling::prepare(ui, id, &edit.text);
    let spell_font = font.clone();
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, width: f32| {
        spelling.layout(ui, text, width, &spell_font, color)
    };
    let mut child = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
    child.set_clip_rect(Rect::EVERYTHING);
    let saved_pointer = ui.input_mut(|input| std::mem::take(&mut input.pointer));
    let paragraph = edit.line.paragraph.is_some();
    let output = crate::paragraph::text_edit(&mut edit.text, paragraph)
        .id(id)
        .font(font)
        .text_color(color)
        .layouter(&mut layouter)
        .frame(Frame::NONE)
        .margin(Margin::symmetric(3, 0))
        .desired_width(width - 6.)
        .vertical_align(Align::Center)
        .char_limit(16384)
        .show(&mut child);
    ui.input_mut(|input| input.pointer = saved_pointer);
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            Id::new("affine-editor-hit"),
            (
                id,
                output.galley.clone(),
                output.galley_pos - virtual_origin,
            ),
        )
    });
    paint_transformed(ui, layer, plane, virtual_origin);
    spelling.menu(&visible, &mut edit.text);
    ui.ctx().output_mut(|output| {
        if let Some(ime) = output.ime.as_mut() {
            let map = |r: Rect| plane.rect(r.translate(-virtual_origin.to_vec2()));
            ime.rect = map(ime.rect);
            ime.cursor_rect = map(ime.cursor_rect);
        }
    });
    if initial {
        return None;
    }
    if escape {
        return Some(false);
    }
    if enter && (!paragraph || ui.input(|input| input.modifiers.ctrl)) {
        return Some(true);
    }
    if pointer.primary_clicked()
        && !inside
        && !selecting
        && edit.rejected_text.as_ref() != Some(&edit.text)
    {
        return Some(true);
    }
    if output.response.lost_focus()
        && !egui::Popup::is_any_open(ui.ctx())
        && edit.rejected_text.as_ref() != Some(&edit.text)
    {
        return Some(true);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }
    #[test]
    fn rotated_and_skewed_editors_keep_keyboard_pointer_selection_and_cancel() {
        for (x, y) in [
            (vec2(0.866, -0.5), vec2(0.5, 0.866)),
            (vec2(0., 1.), vec2(-1., 0.)),
            (vec2(-1., 0.), vec2(0., -1.)),
            (vec2(0., -1.), vec2(1., 0.)),
            (vec2(1., 0.2), vec2(0.3, 1.)),
        ] {
            let ctx = Context::default();
            theme::install(&ctx);
            let id = Id::new("generated-affine-editor");
            let plane = Plane {
                origin: pos2(350., 250.),
                x,
                y,
            };
            let object = TextObject {
                page: 0,
                index: 0,
                revision: 0,
                text: "AB".into(),
                font: "Arial".into(),
                size: 20.,
                quad: [[0.; 2]; 4],
                bounds: pdf_editor_core::Bounds {
                    left: 0.,
                    bottom: 0.,
                    right: 100.,
                    top: 20.,
                },
                color: [0, 0, 0, 255],
                matrix: [1., 0., 0., 1., 0., 0.],
            };
            let line = Line {
                objects: vec![object],
                rect: Rect::from_min_size(Pos2::ZERO, vec2(100., 20.)),
                text: "AB".into(),
                paragraph: None,
                fallback: None,
            };
            let mut edit = EditState {
                page: 0,
                line,
                text: "AB".into(),
                focus: true,
                rejected_text: None,
            };
            let frame = |edit: &mut EditState, events: Vec<egui::Event>| {
                let mut result = None;
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900., 600.))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        result = show(
                            ui,
                            edit,
                            id,
                            (plane, vec2(100., 20.)),
                            FontId::proportional(20.),
                            Color32::BLACK,
                            &theme::palette(ui),
                        );
                    },
                );
                out.textures_delta.clear();
                (result, out.platform_output.commands)
            };
            assert_eq!(frame(&mut edit, vec![]).0, None);
            frame(
                &mut edit,
                vec![
                    key(Key::A, egui::Modifiers::COMMAND),
                    egui::Event::Text("BA".into()),
                ],
            );
            assert_eq!(edit.text, "BA");
            let pos = plane.point(pos2(0., 10.));
            let events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ];
            assert_eq!(frame(&mut edit, events).0, None);
            frame(&mut edit, vec![egui::Event::Text("X".into())]);
            assert_eq!(edit.text, "XBA");
            // Capture drags beyond either edge without intermediate events.
            for (start, end) in [
                (pos, plane.point(pos2(150., 10.))),
                (plane.point(pos2(90., 10.)), plane.point(pos2(-50., 10.))),
            ] {
                for events in [
                    vec![
                        egui::Event::PointerMoved(start),
                        egui::Event::PointerButton {
                            pos: start,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: Default::default(),
                        },
                    ],
                    vec![egui::Event::PointerMoved(end)],
                    vec![egui::Event::PointerButton {
                        pos: end,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    }],
                ] {
                    assert_eq!(frame(&mut edit, events).0, None);
                }
                let (_, commands) = frame(&mut edit, vec![egui::Event::Copy]);
                let copied = commands.iter().any(
                    |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="XBA"),
                );
                assert!(
                    copied,
                    "Dragging beyond the affine editor must select the full text"
                );
            }
            // A short selection gesture can still be classified as a click.
            // Releasing just outside must not apply the edit as a click-away.
            let start = plane.point(pos2(102., 10.));
            let end = plane.point(pos2(104., 10.));
            for (pos, pressed) in [(start, true), (end, false)] {
                assert_eq!(
                    frame(
                        &mut edit,
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: Default::default(),
                            },
                        ]
                    )
                    .0,
                    None,
                    "A selection release must not apply the edit"
                );
            }
            let (_, commands) = frame(
                &mut edit,
                vec![key(Key::A, egui::Modifiers::COMMAND), egui::Event::Copy],
            );
            assert!(commands.iter().any(
                |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="XBA")
            ));
            assert_eq!(
                frame(&mut edit, vec![key(Key::Escape, Default::default())]).0,
                Some(false)
            );
            for point in [pos2(0., 0.), pos2(100., 20.), pos2(45., 9.)] {
                assert!(plane.inverse(plane.point(point)).unwrap().distance(point) < 0.001);
            }
        }
    }
}
