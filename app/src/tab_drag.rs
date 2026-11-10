use super::*;

const TAB_HEIGHT: f32 = 34.0;
const TAB_SPACE: f32 = 2.0;
const TAB_CLOSE: f32 = 22.0;

#[derive(Clone, Copy)]
pub(super) struct TabDrag {
    id: u64,
    offset: f32,
    destination: usize,
}

#[derive(Clone, Copy)]
struct TabMotion {
    from: f32,
    to: f32,
    current: f32,
    started: f64,
}

struct TabAppearance {
    active: bool,
    edited: bool,
    blocked: bool,
    ghost: bool,
}

fn tab_x(ctx: &Context, id: u64, target: f32, direct: bool) -> f32 {
    let key = Id::new(("document-tab-x", id));
    let now = ctx.input(|i| i.time);
    let duration = if direct {
        0.
    } else {
        ctx.global_style().animation_time.min(0.12)
    };
    let mut motion = ctx
        .data_mut(|d| d.get_temp::<TabMotion>(key))
        .unwrap_or(TabMotion {
            from: target,
            to: target,
            current: target,
            started: now,
        });
    if duration <= 0. {
        motion = TabMotion {
            from: target,
            to: target,
            current: target,
            started: now,
        };
    } else {
        if motion.to != target {
            motion.from = motion.current;
            motion.to = target;
            motion.started = now;
        }
        let t = ((now - motion.started) as f32 / duration).clamp(0., 1.);
        motion.current = egui::lerp(motion.from..=motion.to, egui::emath::easing::cubic_out(t));
        if t < 1. {
            ctx.request_repaint();
        }
    }
    ctx.data_mut(|d| d.insert_temp(key, motion));
    motion.current
}

impl TabbedApp {
    pub(super) fn tab_bar(&mut self, ui: &mut Ui, p: &Palette) -> (Option<u64>, Option<u64>, bool) {
        let ctx = ui.ctx().clone();
        // Resolve the existing gesture from its first terminal event, even if
        // a release and another click arrive together in one input frame.
        let mut release = None;
        let mut cancelled = self.closing.is_some() || ctx.input(|i| !i.focused);
        if self.dragging_tab.is_some() {
            cancelled |= ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            ctx.input(|i| {
                for event in &i.events {
                    match event {
                        egui::Event::PointerButton {
                            pos,
                            pressed: false,
                            button: egui::PointerButton::Primary,
                            ..
                        } => {
                            release = Some(*pos);
                            break;
                        }
                        egui::Event::PointerButton { pressed: true, .. }
                        | egui::Event::PointerGone
                        | egui::Event::WindowFocused(false) => {
                            cancelled = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if release.is_none() && !i.pointer.primary_down() {
                    cancelled = true;
                }
            });
        }
        if cancelled {
            self.dragging_tab = None;
        }
        let pointer = release
            .or_else(|| ctx.input(|i| i.pointer.hover_pos()))
            .filter(|pos| pos.is_finite());
        let mut select = None;
        let mut close = None;
        let mut new = false;
        let mut drop_valid = false;
        egui::Panel::top("document-tabs")
            .frame(Frame::new().fill(p.canvas).inner_margin(Margin {
                left: 8,
                right: 8,
                top: 6,
                bottom: 0,
            }))
            .show_separator_line(false)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .scroll_source(egui::scroll_area::ScrollSource {
                        drag: egui::scroll_area::DragScroll::Never,
                        ..Default::default()
                    })
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        let visible = ui.clip_rect();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = TAB_SPACE;
                            let labels: Vec<_> = self
                                .tabs
                                .iter()
                                .map(|(_, tab)| {
                                    let name = tab
                                        .doc
                                        .as_ref()
                                        .map_or("New document", |d| d.name.as_str());
                                    let mut job = egui::text::LayoutJob::single_section(
                                        name.to_owned(),
                                        egui::TextFormat::simple(
                                            FontId::proportional(13.5),
                                            Color32::PLACEHOLDER,
                                        ),
                                    );
                                    job.wrap = egui::text::TextWrapping::truncate_at_width(180.);
                                    ui.painter().layout_job(job)
                                })
                                .collect();
                            let widths: Vec<_> =
                                labels.iter().map(|label| 74. + label.size().x).collect();
                            let total = widths.iter().sum::<f32>()
                                + TAB_SPACE * self.tabs.len().saturating_sub(1) as f32;
                            let (strip, _) =
                                ui.allocate_exact_size(vec2(total, TAB_HEIGHT), Sense::hover());
                            let visible = visible.intersect(Rect::from_min_max(
                                pos2(visible.left(), strip.top()),
                                pos2(visible.right(), strip.bottom()),
                            ));
                            drop_valid = pointer.is_some_and(|pos| visible.contains(pos));
                            let mut positions = Vec::with_capacity(widths.len());
                            let mut x = 0.;
                            for width in &widths {
                                positions.push(x);
                                x += width + TAB_SPACE;
                            }
                            // Interact at the painted positions. During a drag the
                            // other tabs and their close buttons cannot take it over.
                            for (index, (id, tab)) in self.tabs.iter().enumerate() {
                                let x = if self.dragging_tab.is_none() {
                                    tab_x(&ctx, *id, positions[index], false)
                                } else {
                                    ctx.data_mut(|d| {
                                        d.get_temp::<TabMotion>(Id::new(("document-tab-x", id)))
                                    })
                                    .map_or(positions[index], |m| m.current)
                                };
                                let rect = Rect::from_min_size(
                                    strip.min + vec2(x, 0.),
                                    vec2(widths[index], TAB_HEIGHT),
                                );
                                let response = ui.interact(
                                    rect,
                                    ui.id().with(("document-tab", id)),
                                    Sense::click_and_drag(),
                                );
                                response.widget_info(|| {
                                    WidgetInfo::selected(
                                        WidgetType::Button,
                                        true,
                                        self.active == index,
                                        tab.doc
                                            .as_ref()
                                            .map_or("New document", |d| d.name.as_str()),
                                    )
                                });
                                if self.dragging_tab.is_none() && !cancelled {
                                    if response.clicked() {
                                        select = Some(*id);
                                    }
                                    if response.drag_started_by(egui::PointerButton::Primary)
                                        && let Some(origin) =
                                            ctx.input(|i| i.pointer.press_origin())
                                        && origin.x < rect.right() - 30.
                                    {
                                        self.dragging_tab = Some(TabDrag {
                                            id: *id,
                                            offset: rect.left() - origin.x,
                                            destination: index,
                                        });
                                    } else if response.hovered() {
                                        ctx.set_cursor_icon(CursorIcon::Grab);
                                    }
                                }
                            }
                            let from = self
                                .dragging_tab
                                .and_then(|d| self.tabs.iter().position(|(id, _)| *id == d.id));
                            let mut gap = None;
                            if let (Some(from), Some(drag), Some(pointer)) =
                                (from, self.dragging_tab.as_mut(), pointer)
                            {
                                ctx.set_cursor_icon(CursorIcon::Grabbing);
                                // Slot boundaries depend only on document widths,
                                // never on animated neighbors or the grabbed edge.
                                let mut slots = vec![0.];
                                for (index, width) in widths.iter().enumerate() {
                                    if index != from {
                                        slots.push(slots.last().unwrap() + width + TAB_SPACE);
                                    }
                                }
                                drag.destination = drag.destination.min(slots.len() - 1);
                                let left = pointer.x + drag.offset - strip.left();
                                if drop_valid {
                                    while drag.destination + 1 < slots.len()
                                        && left
                                            > (slots[drag.destination]
                                                + slots[drag.destination + 1])
                                                * 0.5
                                                + 4.
                                    {
                                        drag.destination += 1;
                                    }
                                    while drag.destination > 0
                                        && left
                                            < (slots[drag.destination - 1]
                                                + slots[drag.destination])
                                                * 0.5
                                                - 4.
                                    {
                                        drag.destination -= 1;
                                    }
                                }
                                let mut x = 0.;
                                let mut slot = 0;
                                for index in 0..widths.len() {
                                    if index == from {
                                        continue;
                                    }
                                    if slot == drag.destination {
                                        gap = Some(x);
                                        x += widths[from] + TAB_SPACE;
                                    }
                                    positions[index] = x;
                                    x += widths[index] + TAB_SPACE;
                                    slot += 1;
                                }
                                let gap = gap.get_or_insert(x);
                                if drop_valid {
                                    let rect = Rect::from_min_size(
                                        strip.min + vec2(*gap, 0.),
                                        vec2(widths[from], TAB_HEIGHT),
                                    );
                                    ui.painter().rect(
                                        rect,
                                        6.,
                                        p.accent.gamma_multiply(0.10),
                                        Stroke::new(1., p.accent.gamma_multiply(0.6)),
                                        StrokeKind::Inside,
                                    );
                                }
                                positions[from] = left;
                            }
                            for (index, (id, tab)) in self.tabs.iter().enumerate() {
                                if from == Some(index) {
                                    continue;
                                }
                                let x = tab_x(&ctx, *id, positions[index], false);
                                let rect = Rect::from_min_size(
                                    strip.min + vec2(x, 0.),
                                    vec2(widths[index], TAB_HEIGHT),
                                );
                                let closed = ui
                                    .push_id(id, |ui| {
                                        paint_document_tab(
                                            ui,
                                            p,
                                            rect,
                                            labels[index].clone(),
                                            TabAppearance {
                                                active: self.active == index,
                                                edited: Self::needs_warning(tab),
                                                blocked: self.dragging_tab.is_some()
                                                    || cancelled
                                                    || release.is_some(),
                                                ghost: false,
                                            },
                                        )
                                    })
                                    .inner;
                                if closed {
                                    close = Some(*id);
                                }
                            }
                            if let Some(from) = from {
                                let (id, tab) = &self.tabs[from];
                                tab_x(&ctx, *id, positions[from], true);
                                let rect = Rect::from_min_size(
                                    strip.min + vec2(positions[from], 0.),
                                    vec2(widths[from], TAB_HEIGHT),
                                );
                                // Paint last, within the tab strip, with the exact
                                // same label, icon and dirty state as the real tab.
                                paint_document_tab(
                                    ui,
                                    p,
                                    rect,
                                    labels[from].clone(),
                                    TabAppearance {
                                        active: self.active == from,
                                        edited: Self::needs_warning(tab),
                                        blocked: true,
                                        ghost: true,
                                    },
                                );
                            }
                            if Btn::icon(Icon::Plus)
                                .id("new-tab")
                                .height(30.)
                                .tip("New tab")
                                .enabled(self.dragging_tab.is_none() && !cancelled)
                                .show(ui, p)
                                .clicked()
                            {
                                new = true;
                            }
                            // Reveal a selection change once so manual scrolling stays independent.
                            if self.dragging_tab.is_none()
                                && let Some((active_id, _)) = self.tabs.get(self.active)
                                && ctx.data_mut(|data| {
                                    data.get_temp_mut_or_default::<Option<u64>>(
                                        ui.id().with("active-document-tab"),
                                    )
                                    .replace(*active_id)
                                        != Some(*active_id)
                                })
                            {
                                let rect = Rect::from_min_size(
                                    strip.min + vec2(positions[self.active], 0.),
                                    vec2(widths[self.active], TAB_HEIGHT),
                                );
                                ui.scroll_to_rect(rect, None);
                            }
                            if self.dragging_tab.is_some()
                                && release.is_none()
                                && drop_valid
                                && total > visible.width()
                                && let Some(pointer) = pointer
                            {
                                let direction = if pointer.x < visible.left() + 24. {
                                    1.
                                } else if pointer.x > visible.right() - 24. {
                                    -1.
                                } else {
                                    0.
                                };
                                if direction != 0. {
                                    ui.scroll_with_delta(vec2(
                                        direction * 600. * ctx.input(|i| i.stable_dt.min(0.1)),
                                        0.,
                                    ));
                                    ctx.request_repaint();
                                }
                            }
                        });
                    });
            });
        if release.is_some()
            && let Some(drag) = self.dragging_tab.take().filter(|_| drop_valid)
            && let Some(from) = self.tabs.iter().position(|(id, _)| *id == drag.id)
        {
            let active = self.tabs[self.active].0;
            let tab = self.tabs.remove(from);
            self.tabs.insert(drag.destination, tab);
            self.active = self.tabs.iter().position(|(id, _)| *id == active).unwrap();
            ctx.request_repaint();
        }
        (select, close, new)
    }
}

fn paint_document_tab(
    ui: &mut Ui,
    p: &Palette,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    appearance: TabAppearance,
) -> bool {
    let TabAppearance {
        active,
        edited,
        blocked,
        ghost,
    } = appearance;
    let hot = !blocked && ui.rect_contains_pointer(rect);
    let painter = ui.painter();
    let top = CornerRadius {
        nw: 8,
        ne: 8,
        sw: 0,
        se: 0,
    };
    if active || ghost {
        painter.rect_filled(rect, top, p.chrome);
        if active {
            painter.rect_filled(
                Rect::from_min_size(rect.left_top(), vec2(rect.width(), 2.)),
                top,
                p.accent,
            );
        }
    } else if hot {
        painter.rect_filled(rect, top, p.hover);
    }
    if ghost {
        painter.rect_stroke(rect, top, Stroke::new(1.5, p.accent), StrokeKind::Inside);
    }
    let icon = Rect::from_min_size(
        pos2(rect.left() + 12., rect.center().y - 8.),
        vec2(16., 16.),
    );
    crate::icons::paint(
        painter,
        icon,
        Icon::File,
        if active { p.accent_strong } else { p.muted },
    );
    painter.galley(
        pos2(icon.right() + 8., rect.center().y - galley.size().y / 2.),
        galley,
        if active { p.text } else { p.muted },
    );
    let close_rect = Rect::from_min_size(
        pos2(
            rect.right() - 8. - TAB_CLOSE,
            rect.center().y - TAB_CLOSE / 2.,
        ),
        vec2(TAB_CLOSE, TAB_CLOSE),
    );
    if edited && !hot {
        painter.circle_filled(close_rect.center(), 4., p.accent_strong);
        false
    } else if blocked {
        crate::icons::paint(painter, close_rect.shrink(4.), Icon::Close, p.muted);
        false
    } else {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(close_rect));
        Btn::icon(Icon::Close)
            .height(TAB_CLOSE)
            .tip("Close tab")
            .show(&mut child, p)
            .clicked()
    }
}
