//! Page-space alignment for drag previews; never mutates the document.
use eframe::egui::{Rect, Vec2};

pub struct Snapped {
    pub bounds: Rect,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

pub fn align(
    proposed: Rect,
    handle: Option<usize>,
    targets: impl Iterator<Item = Rect>,
    tolerance: f32,
) -> Snapped {
    let mut result = Snapped {
        bounds: proposed,
        x: None,
        y: None,
    };
    let mut best_x = tolerance;
    let mut best_y = tolerance;
    let xs: Vec<_> = match handle {
        None => vec![proposed.left(), proposed.center().x, proposed.right()],
        Some(0 | 6 | 7) => vec![proposed.left()],
        Some(2..=4) => vec![proposed.right()],
        _ => vec![],
    };
    let ys: Vec<_> = match handle {
        None => vec![proposed.top(), proposed.center().y, proposed.bottom()],
        Some(0..=2) => vec![proposed.top()],
        Some(4..=6) => vec![proposed.bottom()],
        _ => vec![],
    };
    let mut offset = Vec2::ZERO;
    for target in targets.filter(|r| r.is_finite() && r.is_positive()) {
        for value in [target.left(), target.center().x, target.right()] {
            for x in &xs {
                let dx = value - x;
                if dx.abs() < best_x
                    && match handle {
                        Some(0 | 6 | 7) => value < proposed.right() - 1.0,
                        Some(2..=4) => value > proposed.left() + 1.0,
                        _ => true,
                    }
                {
                    best_x = dx.abs();
                    offset.x = dx;
                    result.x = Some(value);
                }
            }
        }
        for value in [target.top(), target.center().y, target.bottom()] {
            for y in &ys {
                let dy = value - y;
                if dy.abs() < best_y
                    && match handle {
                        Some(0..=2) => value < proposed.bottom() - 1.0,
                        Some(4..=6) => value > proposed.top() + 1.0,
                        _ => true,
                    }
                {
                    best_y = dy.abs();
                    offset.y = dy;
                    result.y = Some(value);
                }
            }
        }
    }
    match handle {
        None => result.bounds = proposed.translate(offset),
        Some(h) => {
            if matches!(h, 0 | 6 | 7) {
                result.bounds.min.x += offset.x;
            }
            if matches!(h, 2..=4) {
                result.bounds.max.x += offset.x;
            }
            if matches!(h, 0..=2) {
                result.bounds.min.y += offset.y;
            }
            if matches!(h, 4..=6) {
                result.bounds.max.y += offset.y;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};
    #[test]
    fn moving_snaps_nearest_edges_and_centers_without_changing_size() {
        let proposed = Rect::from_min_size(pos2(97.0, 48.0), vec2(20.0, 20.0));
        let target = Rect::from_min_max(pos2(100.0, 50.0), pos2(200.0, 150.0));
        let snapped = align(proposed, None, std::iter::once(target), 5.0);
        assert_eq!(snapped.bounds.min, pos2(100.0, 50.0));
        assert_eq!(snapped.bounds.size(), proposed.size());
        assert_eq!(snapped.x, Some(100.0));
        assert_eq!(snapped.y, Some(50.0));
        assert_eq!(
            align(proposed, None, std::iter::once(target), 1.0).bounds,
            proposed
        );
    }
    #[test]
    fn resizing_keeps_fixed_edges_and_rejects_inversions() {
        let proposed = Rect::from_min_max(pos2(20.0, 30.0), pos2(97.0, 99.0));
        let target = Rect::from_min_max(pos2(100.0, 100.0), pos2(200.0, 200.0));
        let snapped = align(proposed, Some(4), std::iter::once(target), 5.0);
        assert_eq!(snapped.bounds.min, proposed.min);
        assert_eq!(snapped.bounds.max, pos2(100.0, 100.0));
        for handle in 0..8 {
            let result = align(
                proposed,
                Some(handle),
                std::iter::once(Rect::from_min_max(pos2(96.5, 98.5), pos2(98.0, 100.0))),
                100.0,
            );
            assert!(result.bounds.width() >= 1.0 && result.bounds.height() >= 1.0);
        }
    }
}
