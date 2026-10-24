//! Stable native sizing and bounded PDF refreshes across display transitions.
use eframe::egui::{Context, Pos2, Vec2};

#[derive(Default)]
pub(crate) struct Surface {
    previous: Option<(Vec2, f32, Option<Pos2>)>,
    changed_at: Option<f64>,
}
impl Surface {
    pub fn ready(&mut self, ctx: &Context) -> bool {
        let state = (
            ctx.content_rect().size(),
            ctx.pixels_per_point(),
            ctx.input(|i| i.viewport().outer_rect.map(|r| r.min)),
        );
        let now = ctx.input(|i| i.time);
        if self.previous.is_some_and(|previous| previous != state) {
            self.changed_at = Some(now);
        }
        self.previous = Some(state);
        let remaining = self.changed_at.map_or(0.0, |at| 0.18 - (now - at));
        if remaining > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
            false
        } else {
            true
        }
    }
}

struct NativeSizing {
    logical: [f64; 2],
    scale: f64,
    target: Option<([u32; 2], f64, u8)>,
    managed: bool,
}

fn minimum_size(scale: f64, limit: [u32; 2]) -> [u32; 2] {
    std::array::from_fn(|i| {
        (([640.0, 420.0][i] * scale).round().max(1.0) as u32).min(limit[i].max(1))
    })
}
impl NativeSizing {
    fn new(size: [u32; 2], scale: f64) -> Self {
        Self {
            logical: size.map(|v| f64::from(v) / scale),
            scale,
            target: None,
            managed: false,
        }
    }
    fn update(
        &mut self,
        size: [u32; 2],
        scale: f64,
        limit: [u32; 2],
        now: f64,
        managed: bool,
    ) -> Option<[u32; 2]> {
        if !scale.is_finite() || scale <= 0.0 || size.contains(&0) {
            return None;
        }
        if managed {
            self.managed = true;
            self.scale = scale;
            self.target = None;
            return None;
        }
        if self.managed {
            *self = Self::new(size, scale);
            return None;
        }
        if (scale - self.scale).abs() > 0.001 {
            self.scale = scale;
            let wanted = std::array::from_fn(|i| {
                (self.logical[i] * scale)
                    .round()
                    .clamp(1.0, f64::from(limit[i].max(1))) as u32
            });
            self.target = Some((wanted, now + 0.5, 0));
        }
        if let Some((wanted, until, attempts)) = self.target {
            if now < until {
                if size != wanted && attempts < 2 {
                    self.target = Some((wanted, until, attempts + 1));
                    return Some(wanted);
                }
                return None;
            }
            self.target = None;
        }
        self.logical = size.map(|v| f64::from(v) / scale);
        None
    }
}

pub(crate) fn native_guard(cc: &eframe::CreationContext<'_>) -> Option<Box<dyn FnMut()>> {
    let window = cc.winit_window()?.clone();
    let ctx = cc.egui_ctx.clone();
    let size = window.inner_size();
    let mut sizing = NativeSizing::new([size.width, size.height], window.scale_factor());
    let start = std::time::Instant::now();
    Some(Box::new(move || {
        let size = window.inner_size();
        let scale = window.scale_factor();
        let scale_changed = (scale - sizing.scale).abs() > 0.001;
        let limit = if scale_changed {
            window.current_monitor().map_or([u32::MAX; 2], |m| {
                let size = m.size();
                [
                    size.width.saturating_sub(80).max(1),
                    size.height.saturating_sub(100).max(1),
                ]
            })
        } else {
            [u32::MAX; 2]
        };
        if scale_changed {
            let minimum = minimum_size(scale, limit);
            let mut physical = size;
            physical.width = minimum[0];
            physical.height = minimum[1];
            window.set_min_inner_size(Some(physical));
        }
        let managed = window.is_maximized()
            || window.fullscreen().is_some()
            || window.is_minimized() == Some(true);
        if let Some(wanted) = sizing.update(
            [size.width, size.height],
            scale,
            limit,
            start.elapsed().as_secs_f64(),
            managed,
        ) {
            let mut requested = size;
            requested.width = wanted[0];
            requested.height = wanted[1];
            let _ = window.request_inner_size(requested);
            ctx.request_repaint();
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_motion_defers_refresh_until_settled() {
        let ctx = Context::default();
        let mut surface = Surface::default();
        let mut frame = |time, x| {
            let mut ready = false;
            let mut input = eframe::egui::RawInput {
                time: Some(time),
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1000.0, 700.0),
                )),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&eframe::egui::ViewportId::ROOT)
                .unwrap()
                .outer_rect = Some(eframe::egui::Rect::from_min_size(
                Pos2::new(x, 0.0),
                Vec2::new(1000.0, 700.0),
            ));
            let mut output = ctx.run_ui(input, |_| ready = surface.ready(&ctx));
            output.textures_delta.clear();
            ready
        };
        assert!(frame(0.0, 0.0));
        for i in 1..100 {
            assert!(!frame(i as f64 * 0.01, i as f32));
        }
        assert!(frame(1.3, 99.0));
    }
    #[test]
    fn ignored_native_size_requests_cannot_cause_an_unbounded_retry_loop() {
        let mut state = NativeSizing::new([1000, 700], 1.0);
        let requests = (0..100)
            .filter_map(|i| state.update([3000, 2000], 2.0, [1400, 900], i as f64 * 0.001, false))
            .count();
        assert_eq!(requests, 2);
    }
    #[test]
    fn repeated_display_crossings_do_not_compound_window_size() {
        let mut state = NativeSizing::new([1000, 700], 1.0);
        for step in 0..100 {
            let scale = if step % 2 == 0 { 1.5 } else { 1.0 };
            let wanted = [(1000.0 * scale) as u32, (700.0 * scale) as u32];
            let time = step as f64 * 0.1;
            assert_eq!(
                state.update([3000, 2000], scale, [4000, 3000], time, false),
                Some(wanted)
            );
            assert_eq!(
                state.update(wanted, scale, [4000, 3000], time + 0.01, false),
                None
            );
        }
        assert_eq!(state.logical, [1000.0, 700.0]);
    }
    #[test]
    fn manual_resize_maximize_and_small_displays_remain_supported() {
        assert_eq!(minimum_size(1.0, [1920, 1080]), [640, 420]);
        assert_eq!(minimum_size(2.0, [1000, 700]), [1000, 700]);
        let mut state = NativeSizing::new([1000, 700], 1.0);
        assert_eq!(
            state.update([900, 600], 1.0, [2000, 1400], 1.0, false),
            None
        );
        assert_eq!(
            state.update([1800, 1200], 2.0, [1400, 900], 2.0, false),
            Some([1400, 900])
        );
        assert_eq!(
            state.update([1920, 1080], 1.0, [1840, 980], 3.0, true),
            None
        );
        assert_eq!(state.update([0, 0], 1.0, [1840, 980], 4.0, false), None);
        assert_eq!(state.update([900, 600], 1.5, [1840, 980], 5.0, false), None);
        assert_eq!(state.logical, [600.0, 400.0]);
    }
}
