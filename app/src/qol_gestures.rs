use super::qol::Qol;
use super::*;
use pdf_editor_core::{PageItem, TextCharacter};

#[derive(Clone, Copy)]
enum TextUnit {
    Character,
    Word,
    Line,
}

#[derive(Clone, Copy)]
struct PendingHit {
    page: u16,
    at: Pos2,
    unit: TextUnit,
    last: bool,
    transform: Option<PageTransform>,
}

#[derive(Clone, Default)]
struct PendingText {
    anchor: Option<PendingHit>,
    end: Option<PendingHit>,
}

#[derive(Clone)]
struct MarqueeStart {
    page: u16,
    extend: bool,
    previous: Vec<PageItem>,
}

fn text_range(chars: &[TextCharacter], position: usize, unit: TextUnit) -> (usize, usize) {
    let mut first = position;
    let mut last = position;
    let word = |s: &str| {
        s.chars().any(|c| {
            c.is_alphanumeric()
                || c == '_'
                || matches!(c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0xfe20..=0xfe2f)
        })
    };
    match unit {
        TextUnit::Character => {}
        TextUnit::Word if word(&chars[position].text) => {
            while first > 0 && word(&chars[first - 1].text) {
                first -= 1;
            }
            while last + 1 < chars.len() && word(&chars[last + 1].text) {
                last += 1;
            }
        }
        TextUnit::Word => {}
        TextUnit::Line => {
            while first > 0 && !chars[first - 1].text.contains(['\r', '\n']) {
                first -= 1;
            }
            while last + 1 < chars.len() && !chars[last + 1].text.contains(['\r', '\n']) {
                last += 1;
            }
        }
    }
    (chars[first].index, chars[last].index)
}

fn character_at(chars: &[TextCharacter], transform: &PageTransform, at: Pos2) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            !c.text.contains(['\r', '\n'])
                && c.bounds.right > c.bounds.left
                && c.bounds.top > c.bounds.bottom
        })
        .min_by(|(_, a), (_, b)| {
            let distance = |c: &TextCharacter| {
                let bounds = bounds_rect(transform, c.bounds);
                (bounds.clamp(at) - at).length_sq()
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(position, _)| position)
}

fn contained_items(objects: &PageObjects, page: u16, bounds: Rect) -> Vec<PageItem> {
    let revision = objects
        .items
        .iter()
        .find(|i| i.page == page)
        .map(|i| i.revision);
    objects
        .items
        .iter()
        .filter(|i| {
            i.page == page
                && Some(i.revision) == revision
                && bounds.contains_rect(bounds_rect(&objects.transform, i.bounds))
        })
        .cloned()
        .collect()
}

impl Qol {
    fn request_characters(&mut self, page: u16, requests: &mut Vec<Request>) {
        if !self.characters.contains_key(&page) && self.pending_characters.insert(page) {
            requests.push(Request::Characters(page));
        }
    }

    fn resolve_hit(&self, hit: PendingHit, objects: Option<&PageObjects>) -> Option<(u16, usize)> {
        let chars = self.characters.get(&hit.page)?;
        let transform = hit.transform.or_else(|| objects.map(|o| o.transform))?;
        let position = character_at(chars, &transform, hit.at)?;
        let (first, last) = text_range(chars, position, hit.unit);
        Some((hit.page, if hit.last { last } else { first }))
    }

    /// Call once for each visible page, before its normal editing gestures.
    /// A true result reserves the page gesture for text or rectangle selection.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_canvas(
        &mut self,
        ctx: &Context,
        response: &egui::Response,
        painter: &egui::Painter,
        page: u16,
        rect: Rect,
        scale: f32,
        objects: Option<&PageObjects>,
        selected_item: &mut Option<PageItem>,
        requests: &mut Vec<Request>,
        accent: Color32,
    ) -> bool {
        if !scale.is_finite() || scale <= 0.0 {
            return self.text_mode || self.box_select;
        }
        let (pointer, pressed, released, down, shift) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.primary_down(),
                i.modifiers.shift,
            )
        });
        let inside = pointer.is_some_and(|at| rect.contains(at));
        let at = pointer.map(|at| ((at - rect.min) / scale).to_pos2());
        let state_id = Id::new(("qol-text-gesture", self as *const Self as usize));

        if self.text_mode {
            self.marquee = None;
            self.request_characters(page, requests);
            if response.hovered() || self.text_dragging && inside {
                ctx.set_cursor_icon(CursorIcon::Text);
            }
            let mut pending =
                ctx.data_mut(|data| data.get_temp::<PendingText>(state_id).unwrap_or_default());

            // Missing text can arrive after the pointer is released. Sentinel
            // endpoints keep the exact pending hit separate from a valid index.
            {
                if let Some(hit) = pending.anchor
                    && self.anchor == Some((hit.page, usize::MAX))
                {
                    if self.characters.get(&hit.page).is_some_and(Vec::is_empty) {
                        self.anchor = None;
                        pending.anchor = None;
                    } else if let Some(anchor) =
                        self.resolve_hit(hit, objects.filter(|_| hit.page == page))
                    {
                        self.anchor = Some(anchor);
                        pending.anchor = None;
                    }
                }
                if let Some(hit) = pending.end
                    && self.end == Some((hit.page, usize::MAX))
                {
                    if self.characters.get(&hit.page).is_some_and(Vec::is_empty) {
                        self.end = None;
                        pending.end = None;
                    } else if let Some(end) =
                        self.resolve_hit(hit, objects.filter(|_| hit.page == page))
                    {
                        self.end = Some(end);
                        pending.end = None;
                    }
                }
            }

            if pressed && inside && response.contains_pointer() {
                self.objects.clear();
                *selected_item = None;
                self.text_dragging = true;
                let hit = PendingHit {
                    page,
                    at: at.unwrap(),
                    unit: TextUnit::Character,
                    last: false,
                    transform: objects.map(|o| o.transform),
                };
                self.anchor = self.resolve_hit(hit, objects).or(Some((page, usize::MAX)));
                self.end = self.anchor;
                pending = PendingText {
                    anchor: (self.anchor == Some((page, usize::MAX))).then_some(hit),
                    end: (self.end == Some((page, usize::MAX))).then_some(hit),
                };
            }

            if self.text_dragging && inside && (down || released) {
                let hit = PendingHit {
                    page,
                    at: at.unwrap(),
                    unit: TextUnit::Character,
                    last: true,
                    transform: objects.map(|o| o.transform),
                };
                self.end = self.resolve_hit(hit, objects).or(Some((page, usize::MAX)));
                pending.end = (self.end == Some((page, usize::MAX))).then_some(hit);
                if released {
                    self.text_dragging = false;
                }
            }

            if inside && response.clicked_by(egui::PointerButton::Primary) {
                let unit = if response.triple_clicked() {
                    TextUnit::Line
                } else if response.double_clicked() {
                    TextUnit::Word
                } else {
                    TextUnit::Character
                };
                let first = PendingHit {
                    page,
                    at: at.unwrap(),
                    unit,
                    last: false,
                    transform: objects.map(|o| o.transform),
                };
                let last = PendingHit {
                    last: true,
                    ..first
                };
                self.anchor = self
                    .resolve_hit(first, objects)
                    .or(Some((page, usize::MAX)));
                self.end = self.resolve_hit(last, objects).or(Some((page, usize::MAX)));
                pending.anchor = (self.anchor == Some((page, usize::MAX))).then_some(first);
                pending.end = (self.end == Some((page, usize::MAX))).then_some(last);
                self.text_dragging = false;
            }
            if !down && !released {
                self.text_dragging = false;
            }
            if let (Some(anchor), Some(end)) = (self.anchor, self.end) {
                for missing in anchor.0.min(end.0)..=anchor.0.max(end.0) {
                    self.request_characters(missing, requests);
                }
                if let (Some(objects), Some(chars)) = (objects, self.characters.get(&page)) {
                    let (first, last) = (anchor.min(end), anchor.max(end));
                    if (first.0..=last.0).contains(&page) {
                        let from = if first.0 == page { first.1 } else { 0 };
                        let to = if last.0 == page { last.1 } else { usize::MAX };
                        if from != usize::MAX && (to != usize::MAX || pending.end.is_none()) {
                            for bounds in pdf_editor_core::selected_text(chars, from, to).bounds {
                                let bounds =
                                    to_screen(bounds_rect(&objects.transform, bounds), rect, scale);
                                painter.rect_filled(
                                    bounds.expand(0.5),
                                    0,
                                    accent.gamma_multiply(0.25),
                                );
                            }
                        }
                    }
                }
            }
            ctx.data_mut(|data| data.insert_temp(state_id, pending));
            return true;
        }

        self.text_dragging = false;
        ctx.data_mut(|data| data.remove::<PendingText>(state_id));
        let Some(objects) = objects else {
            return self.box_select;
        };
        for item in &self.objects {
            if item.page == page
                && objects
                    .items
                    .iter()
                    .any(|current| current.index == item.index && current.revision == item.revision)
            {
                let bounds = to_screen(bounds_rect(&objects.transform, item.bounds), rect, scale);
                painter.rect_stroke(bounds, 1, Stroke::new(1.0, accent), StrokeKind::Outside);
            }
        }
        let marquee_id = state_id.with("marquee");
        let empty = at.is_some_and(|at| {
            !objects
                .items
                .iter()
                .any(|item| bounds_rect(&objects.transform, item.bounds).contains(at))
        });
        if pressed && inside && response.contains_pointer() && (self.box_select || shift && empty) {
            self.marquee = Some((page, at.unwrap()));
            let start = MarqueeStart {
                page,
                extend: shift,
                previous: self.objects.clone(),
            };
            if !shift {
                self.objects.clear();
                *selected_item = None;
            }
            ctx.data_mut(|data| data.insert_temp(marquee_id, start));
        }
        if let Some((origin_page, origin)) = self.marquee {
            if origin_page != page {
                return self.box_select;
            }
            let start = ctx.data_mut(|data| data.get_temp::<MarqueeStart>(marquee_id));
            let current = at.unwrap_or(origin);
            let page_bounds = Rect::from_min_size(Pos2::ZERO, rect.size() / scale);
            let bounds = Rect::from_two_pos(origin, page_bounds.clamp(current));
            let screen = to_screen(bounds, rect, scale);
            painter.rect_filled(screen, 0, accent.gamma_multiply(0.10));
            painter.rect_stroke(screen, 0, Stroke::new(1.0, accent), StrokeKind::Inside);
            ctx.set_cursor_icon(CursorIcon::Crosshair);
            if released || !down && !pressed {
                let picked = contained_items(objects, page, bounds);
                let revision = objects.items.first().map(|i| i.revision);
                self.objects = start
                    .filter(|start| start.page == page && start.extend)
                    .map_or_else(Vec::new, |start| {
                        start
                            .previous
                            .into_iter()
                            .filter(|item| {
                                item.page == page
                                    && Some(item.revision) == revision
                                    && objects
                                        .items
                                        .iter()
                                        .any(|current| current.index == item.index)
                            })
                            .collect()
                    });
                for item in picked {
                    if !self.objects.iter().any(|old| old.index == item.index) {
                        self.objects.push(item);
                    }
                }
                self.objects.sort_by_key(|item| item.index);
                *selected_item = self.primary();
                self.marquee = None;
                ctx.data_mut(|data| data.remove::<MarqueeStart>(marquee_id));
            }
            return true;
        }
        if self.box_select && response.hovered() {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
        }
        self.box_select
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn characters(page: u16, text: &str) -> Vec<TextCharacter> {
        text.chars()
            .enumerate()
            .map(|(index, c)| TextCharacter {
                page,
                index,
                text: c.to_string(),
                bounds: pdf_editor_core::Bounds {
                    left: index as f32 * 10.0,
                    bottom: 10.0,
                    right: index as f32 * 10.0 + 8.0,
                    top: 20.0,
                },
            })
            .collect()
    }

    #[test]
    fn word_and_line_ranges_follow_native_indices_and_unicode() {
        let chars = characters(0, "Hi äiti!\r\nNext line");
        assert_eq!(text_range(&chars, 5, TextUnit::Word), (3, 6));
        assert_eq!(text_range(&chars, 7, TextUnit::Word), (7, 7));
        assert_eq!(text_range(&chars, 5, TextUnit::Line), (0, 7));
        assert_eq!(text_range(&chars, 12, TextUnit::Line), (10, 18));
        let chars = characters(0, "cafe\u{301}");
        assert_eq!(text_range(&chars, 2, TextUnit::Word), (0, 4));
    }

    #[test]
    fn text_hit_testing_and_containment_respect_rotated_coordinates() {
        let chars = characters(0, "AB");
        let transform = PageTransform::for_test([0.0, 1.0, -1.0, 0.0, 100.0, 0.0]);
        assert_eq!(character_at(&chars, &transform, pos2(85.0, 14.0)), Some(1));
        let items = vec![
            PageItem {
                page: 0,
                index: 0,
                revision: 2,
                kind: "Text".into(),
                bounds: chars[0].bounds,
            },
            PageItem {
                page: 0,
                index: 1,
                revision: 2,
                kind: "Image".into(),
                bounds: chars[1].bounds,
            },
        ];
        let objects = PageObjects {
            items,
            transform,
            generation: 0,
            lines: vec![],
        };
        assert_eq!(
            contained_items(
                &objects,
                0,
                Rect::from_min_max(pos2(80.0, 0.0), pos2(90.0, 8.0))
            )
            .len(),
            1
        );
        assert!(
            contained_items(
                &objects,
                0,
                Rect::from_min_max(pos2(82.0, 0.0), pos2(88.0, 8.0))
            )
            .is_empty()
        );
    }

    #[test]
    fn text_requests_are_lazy_and_cross_page_selection_is_inclusive() {
        let mut q = Qol::default();
        let mut requests = vec![];
        q.request_characters(0, &mut requests);
        q.request_characters(0, &mut requests);
        assert_eq!(requests.len(), 1);
        q.characters.insert(0, characters(0, "first"));
        q.characters.insert(1, characters(1, "second"));
        q.anchor = Some((1, 2));
        q.end = Some((0, 2));
        assert_eq!(q.selection().unwrap().0, "rst\nsec");
        q.request_characters(1, &mut requests);
        assert_eq!(requests.len(), 1);
    }

    fn frame(q: &mut Qol, ctx: &Context, events: Vec<egui::Event>, requests: &mut Vec<Request>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 320.0))),
                events,
                ..Default::default()
            },
            |ui| {
                let mut selected = None;
                for page in 0..2 {
                    let rect = Rect::from_min_size(
                        pos2(20.0, 20.0 + f32::from(page) * 140.0),
                        vec2(300.0, 100.0),
                    );
                    let response =
                        ui.interact(rect, Id::new(("test-page", page)), Sense::click_and_drag());
                    let objects = PageObjects {
                        items: vec![],
                        transform: PageTransform::identity(),
                        generation: 0,
                        lines: vec![],
                    };
                    assert!(q.on_canvas(
                        ui.ctx(),
                        &response,
                        ui.painter(),
                        page,
                        rect,
                        1.0,
                        Some(&objects),
                        &mut selected,
                        requests,
                        Color32::BLUE
                    ));
                }
            },
        );
        output.textures_delta.clear();
    }

    fn button(at: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn rendered_text_drag_keeps_anchor_when_pointer_crosses_pages() {
        let ctx = Context::default();
        let mut q = Qol {
            text_mode: true,
            ..Default::default()
        };
        q.characters.insert(0, characters(0, "first"));
        q.characters.insert(1, characters(1, "second"));
        let mut requests = vec![];
        frame(&mut q, &ctx, vec![], &mut requests);
        let start = pos2(35.0, 35.0);
        frame(
            &mut q,
            &ctx,
            vec![egui::Event::PointerMoved(start), button(start, true)],
            &mut requests,
        );
        let end = pos2(45.0, 175.0);
        frame(
            &mut q,
            &ctx,
            vec![egui::Event::PointerMoved(end)],
            &mut requests,
        );
        frame(&mut q, &ctx, vec![button(end, false)], &mut requests);
        assert_eq!(q.anchor, Some((0, 1)));
        assert_eq!(q.end, Some((1, 2)));
        assert_eq!(q.selection().unwrap().0, "irst\nsec");
        assert!(!q.text_dragging);
    }

    #[test]
    fn rendered_click_before_character_reply_resolves_original_hit() {
        let ctx = Context::default();
        let mut q = Qol {
            text_mode: true,
            ..Default::default()
        };
        let mut requests = vec![];
        frame(&mut q, &ctx, vec![], &mut requests);
        let at = pos2(35.0, 35.0);
        frame(
            &mut q,
            &ctx,
            vec![egui::Event::PointerMoved(at), button(at, true)],
            &mut requests,
        );
        frame(&mut q, &ctx, vec![button(at, false)], &mut requests);
        assert_eq!(q.anchor, Some((0, usize::MAX)));
        q.characters.insert(0, characters(0, "first"));
        frame(&mut q, &ctx, vec![], &mut requests);
        assert_eq!(q.anchor, Some((0, 1)));
        assert_eq!(q.end, Some((0, 1)));
        assert_eq!(q.selection().unwrap().0, "i");
        assert_eq!(requests.len(), 2);
    }
}
