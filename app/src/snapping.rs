use eframe::egui::{Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    PageCenter,
    Center,
    Edge,
    Gap,
}
#[derive(Clone, Copy, Debug)]
pub struct Guide {
    pub position: f32,
    pub from: f32,
    pub to: f32,
    pub kind: Kind,
    anchor: usize,
}
#[derive(Default)]
pub struct State {
    x: Option<Guide>,
    y: Option<Guide>,
}
pub struct Snapped {
    pub bounds: Rect,
    pub x: Option<Guide>,
    pub y: Option<Guide>,
}
fn values(r: Rect, axis: usize) -> [f32; 3] {
    [r.min[axis], r.center()[axis], r.max[axis]]
}
fn active(handle: Option<usize>, axis: usize, anchor: usize) -> bool {
    matches!((handle, axis, anchor),
        (None, _, _) | (Some(0 | 6 | 7), 0, 0)
        | (Some(2..=4), 0, 2)
        | (Some(0..=2), 1, 0)
        | (Some(4..=6), 1, 2))
}
fn axis_snap(
    r: Rect,
    handle: Option<usize>,
    page: Rect,
    targets: impl Iterator<Item = Rect>,
    tolerance: f32,
    axis: usize,
    previous: Option<Guide>,
) -> Option<Guide> {
    let mut best: Option<(f32, Guide)> = None;
    let mut held = None;
    let other = 1 - axis;
    let anchors = values(r, axis);
    let mut consider = |g: Guide| {
        if !active(handle, axis, g.anchor) {
            return;
        }
        if handle.is_some()
            && ((g.anchor == 0 && g.position > r.max[axis] - 1.0)
                || (g.anchor == 2 && g.position < r.min[axis] + 1.0))
        {
            return;
        }
        let distance = (g.position - anchors[g.anchor]).abs();
        if previous.is_some_and(|p| {
            p.anchor == g.anchor && p.kind == g.kind && (p.position - g.position).abs() < 0.001
        }) && distance <= tolerance * 1.7
        {
            held = Some(g);
        }
        if distance > tolerance {
            return;
        }
        if best.is_none_or(|(d, b)| {
            if (distance - d).abs() <= tolerance * 0.05 {
                (g.kind, g.position.to_bits(), g.anchor) < (b.kind, b.position.to_bits(), b.anchor)
            } else {
                distance < d
            }
        }) {
            best = Some((distance, g));
        }
    };
    for (target_anchor, position) in values(page, axis).into_iter().enumerate() {
        for anchor in 0..3 {
            if handle.is_none() && anchor != target_anchor {
                continue;
            }
            consider(Guide {
                position,
                from: page.min[other],
                to: page.max[other],
                kind: if target_anchor == 1 {
                    Kind::PageCenter
                } else {
                    Kind::Edge
                },
                anchor,
            });
        }
    }
    let mut before: Option<f32> = None;
    let mut after: Option<f32> = None;
    for target in targets.filter(|r| r.is_finite() && r.is_positive()) {
        for (target_anchor, position) in values(target, axis).into_iter().enumerate() {
            for anchor in 0..3 {
                if handle.is_none() && (anchor == 1) != (target_anchor == 1) {
                    continue;
                }
                consider(Guide {
                    position,
                    from: r.min[other].min(target.min[other]),
                    to: r.max[other].max(target.max[other]),
                    kind: if target_anchor == 1 {
                        Kind::Center
                    } else {
                        Kind::Edge
                    },
                    anchor,
                });
            }
        }
        if target.min[other] < r.max[other] && target.max[other] > r.min[other] {
            if target.max[axis] <= r.min[axis] {
                before = Some(before.map_or(target.max[axis], |v| v.max(target.max[axis])));
            }
            if target.min[axis] >= r.max[axis] {
                after = Some(after.map_or(target.min[axis], |v| v.min(target.min[axis])));
            }
        }
    }
    if handle.is_none()
        && let (Some(from), Some(to)) = (before, after)
    {
        consider(Guide {
            position: (from + to) * 0.5,
            from,
            to,
            kind: Kind::Gap,
            anchor: 1,
        });
    }
    held.or(best.map(|(_, g)| g))
}
pub fn align(
    proposed: Rect,
    handle: Option<usize>,
    page: Rect,
    targets: impl Iterator<Item = Rect> + Clone,
    tolerance: f32,
    state: &mut State,
) -> Snapped {
    if !proposed.is_finite()
        || !proposed.is_positive()
        || !tolerance.is_finite()
        || tolerance <= 0.0
    {
        *state = State::default();
        return Snapped {
            bounds: proposed,
            x: None,
            y: None,
        };
    }
    let x = axis_snap(proposed, handle, page, targets.clone(), tolerance, 0, state.x);
    let y = axis_snap(proposed, handle, page, targets, tolerance, 1, state.y);
    let offset = Vec2::new(
        x.map_or(0.0, |g| g.position - values(proposed, 0)[g.anchor]),
        y.map_or(0.0, |g| g.position - values(proposed, 1)[g.anchor]),
    );
    let mut bounds = proposed;
    if handle.is_none() {
        bounds = bounds.translate(offset);
    } else {
        for axis in 0..2 {
            if active(handle, axis, 0) {
                bounds.min[axis] += offset[axis];
            }
            if active(handle, axis, 2) {
                bounds.max[axis] += offset[axis];
            }
        }
    }
    state.x = x;
    state.y = y;
    Snapped { bounds, x, y }
}
pub fn paint(
    painter: &eframe::egui::Painter,
    snapped: &Snapped,
    page: Rect,
    scale: f32,
    color: eframe::egui::Color32,
) {
    use eframe::egui::Stroke;
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(page));
    for (axis, guide) in [snapped.x, snapped.y].into_iter().enumerate() {
        let Some(g) = guide else {
            continue;
        };
        let other = 1 - axis;
        let point = |v: f32, w: f32| {
            let mut p = page.min;
            p[axis] += v * scale;
            p[other] += w * scale;
            p
        };
        let stroke = Stroke::new(1.2, color);
        if g.kind == Kind::Gap {
            let mid = snapped.bounds.center()[other];
            for (a, b) in [
                (g.from, snapped.bounds.min[axis]),
                (snapped.bounds.max[axis], g.to),
            ] {
                painter.line_segment([point(a, mid), point(b, mid)], stroke);
                for v in [a, b] {
                    let p = point(v, mid);
                    let mut d = Vec2::ZERO;
                    d[other] = 4.0;
                    painter.line_segment([p - d, p + d], stroke);
                }
            }
        } else {
            let from = g.from.min(snapped.bounds.min[other]);
            let to = g.to.max(snapped.bounds.max[other]);
            painter.line_segment([point(g.position, from), point(g.position, to)], stroke);
            for v in [from, to] {
                painter.circle_filled(point(g.position, v), 2.5, color);
            }
            if g.kind == Kind::PageCenter {
                painter.circle_stroke(page.center(), 5.0, stroke);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};
    fn page() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0))
    }
    #[test]
    fn page_center_matches_center_not_an_edge() {
        let mut state = State::default();
        let r = Rect::from_min_size(pos2(278.0, 388.0), vec2(40.0, 20.0));
        let s = align(r, None, page(), std::iter::empty(), 6.0, &mut state);
        assert_eq!(s.bounds.center(), pos2(300.0, 400.0));
        assert_eq!(s.x.unwrap().kind, Kind::PageCenter);
        let r = Rect::from_min_size(pos2(298.0, 200.0), vec2(40.0, 20.0));
        assert!(
            align(r, None, page(), std::iter::empty(), 6.0, &mut state)
                .x
                .is_none()
        );
    }
    #[test]
    fn lock_resists_jitter_then_releases() {
        let targets = [
            Rect::from_min_size(pos2(100.0, 100.0), vec2(20.0, 20.0)),
            Rect::from_min_size(pos2(104.0, 150.0), vec2(20.0, 20.0)),
        ];
        let mut state = State::default();
        for x in [99.0, 101.0, 103.0, 105.0] {
            let r = Rect::from_min_size(pos2(x, 250.0), vec2(20.0, 20.0));
            assert_eq!(
                align(r, None, page(), targets.into_iter(), 6.0, &mut state)
                    .bounds
                    .left(),
                100.0
            );
        }
        let r = Rect::from_min_size(pos2(150.0, 250.0), vec2(20.0, 20.0));
        assert!(
            align(r, None, page(), targets.into_iter(), 6.0, &mut state)
                .x
                .is_none()
        );
    }
    #[test]
    fn equal_spacing_in_both_axes() {
        for axis in 0..2 {
            let mut a = Rect::from_min_size(pos2(100.0, 100.0), vec2(20.0, 20.0));
            let mut b = a;
            b.min[axis] = 200.0;
            b.max[axis] = 220.0;
            a.min[axis] = 100.0;
            let mut r = a;
            r.min[axis] = 147.0;
            r.max[axis] = 167.0;
            let s = align(
                r,
                None,
                page(),
                [a, b].into_iter(),
                6.0,
                &mut State::default(),
            );
            assert_eq!(
                s.bounds.min[axis] - a.max[axis],
                b.min[axis] - s.bounds.max[axis]
            );
            assert_eq!([s.x, s.y][axis].unwrap().kind, Kind::Gap);
        }
    }
    #[test]
    fn resize_preserves_fixed_edges_and_never_inverts() {
        let r = Rect::from_min_max(pos2(20.0, 30.0), pos2(97.0, 99.0));
        let target = Rect::from_min_max(pos2(100.0, 100.0), pos2(200.0, 200.0));
        let s = align(
            r,
            Some(4),
            page(),
            [target].into_iter(),
            5.0,
            &mut State::default(),
        );
        assert_eq!(s.bounds.min, r.min);
        assert_eq!(s.bounds.max, pos2(100.0, 100.0));
        for h in 0..8 {
            let s = align(
                r,
                Some(h),
                page(),
                [target].into_iter(),
                100.0,
                &mut State::default(),
            );
            assert!(s.bounds.width() >= 1.0 && s.bounds.height() >= 1.0);
            for axis in 0..2 {
                if !active(Some(h), axis, 0) {
                    assert_eq!(s.bounds.min[axis], r.min[axis]);
                }
                if !active(Some(h), axis, 2) {
                    assert_eq!(s.bounds.max[axis], r.max[axis]);
                }
            }
        }
    }
    #[test]
    fn target_order_and_zoom_do_not_change_alignment() {
        let targets = [
            Rect::from_min_size(pos2(100.0, 100.0), vec2(20.0, 20.0)),
            Rect::from_min_size(pos2(104.0, 150.0), vec2(20.0, 20.0)),
        ];
        for scale in [0.5, 1.0, 2.0, 4.0] {
            let r = Rect::from_min_size(pos2(100.0 - 2.0 / scale, 300.0), vec2(20.0, 20.0));
            let a = align(
                r,
                None,
                page(),
                targets.into_iter(),
                6.0 / scale,
                &mut State::default(),
            );
            let b = align(
                r,
                None,
                page(),
                targets.into_iter().rev(),
                6.0 / scale,
                &mut State::default(),
            );
            assert_eq!(a.bounds, b.bounds);
            assert_eq!(a.bounds.size(), r.size());
            assert!(a.x.is_some());
        }
    }
}
