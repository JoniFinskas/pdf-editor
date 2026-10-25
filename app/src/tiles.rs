//! Display-resolution viewport rasters; one bounded in-flight batch per page.
use super::*;
const SIDE: u32 = 512;
type Region = [u32; 4];
#[derive(Default)]
pub(super) struct PageTiles {
    key: Option<(f32, u64)>,
    wanted: Vec<Region>,
    pending: bool,
    failed: bool,
    cached: Vec<(Region, TextureHandle)>,
}
impl PageTiles {
    #[cfg(test)]
    pub(super) fn ready(&self) -> bool {
        !self.wanted.is_empty()
            && self
                .wanted
                .iter()
                .all(|region| self.cached.iter().any(|(old, _)| old == region))
    }
    pub(super) fn accepts(&self, scale: f32, generation: u64) -> bool {
        self.key == Some((scale, generation))
    }
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
    pub(super) fn bytes(&self) -> usize {
        self.cached
            .iter()
            .map(|(_, texture)| texture.byte_size())
            .sum()
    }
    pub(super) fn discard_cache(&mut self) {
        self.cached.clear();
    }
    pub(super) fn plan(
        &mut self,
        info: &PageInfo,
        display: Rect,
        visible: Rect,
        scale: f32,
        generation: u64,
    ) -> Option<Vec<Region>> {
        let scale = (scale * 64.).floor() / 64.;
        if self.key != Some((scale, generation)) {
            self.clear();
            self.key = Some((scale, generation));
        }
        let clip = display.intersect(visible);
        if !clip.is_positive() {
            self.clear();
            return None;
        }
        let full = [
            (info.width * scale).ceil() as u32,
            (info.height * scale).ceil() as u32,
        ];
        let ratio = vec2(
            full[0] as f32 / display.width(),
            full[1] as f32 / display.height(),
        );
        let minimum = (clip.min - display.min) * ratio;
        let maximum = (clip.max - display.min) * ratio;
        let mut wanted = Vec::new();
        for y in ((minimum.y.max(0.) as u32 / SIDE) * SIDE..(maximum.y.ceil() as u32).min(full[1]))
            .step_by(SIDE as usize)
        {
            for x in ((minimum.x.max(0.) as u32 / SIDE) * SIDE
                ..(maximum.x.ceil() as u32).min(full[0]))
                .step_by(SIDE as usize)
            {
                if wanted.len() < 64 {
                    wanted.push([x, y, SIDE.min(full[0] - x), SIDE.min(full[1] - y)]);
                }
            }
        }
        if self.wanted != wanted {
            self.failed = false;
        }
        self.wanted = wanted;
        self.cached.retain(|(rect, _)| self.wanted.contains(rect));
        if self.pending || self.failed {
            return None;
        }
        let missing: Vec<_> = self
            .wanted
            .iter()
            .filter(|rect| !self.cached.iter().any(|(old, _)| old == *rect))
            .take(16)
            .copied()
            .collect();
        if missing.is_empty() {
            return None;
        }
        self.pending = true;
        Some(missing)
    }
    pub(super) fn receive(
        &mut self,
        ctx: &Context,
        scale: f32,
        generation: u64,
        images: Result<Vec<(Region, pdf_editor_core::Image)>, String>,
    ) -> Option<String> {
        if self.key != Some((scale, generation)) {
            return None;
        }
        self.pending = false;
        match images {
            Err(error) => {
                self.failed = true;
                Some(error)
            }
            Ok(images) => {
                for (rect, image) in images {
                    if !self.wanted.contains(&rect)
                        || image.width != rect[2]
                        || image.height != rect[3]
                        || image.rgba.len() != rect[2] as usize * rect[3] as usize * 4
                    {
                        continue;
                    }
                    let texture = ctx.load_texture(
                        "viewport-tile",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [rect[2] as usize, rect[3] as usize],
                            &image.rgba,
                        ),
                        TextureOptions::LINEAR,
                    );
                    self.cached.retain(|(old, _)| *old != rect);
                    self.cached.push((rect, texture));
                }
                None
            }
        }
    }
    pub(super) fn paint(
        &self,
        painter: &egui::Painter,
        page: Rect,
        info: &PageInfo,
        generation: u64,
    ) {
        let Some((scale, version)) = self.key else {
            return;
        };
        if version != generation {
            return;
        }
        let full = vec2((info.width * scale).ceil(), (info.height * scale).ceil());
        for ([x, y, w, h], texture) in &self.cached {
            let min = page.min + vec2(*x as f32, *y as f32) / full * page.size();
            let rect = Rect::from_min_size(min, vec2(*w as f32, *h as f32) / full * page.size());
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
                Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_are_bounded_and_late_tiles_do_not_replace_new_generation_or_zoom() {
        let mut tiles = PageTiles::default();
        let ctx = Context::default();
        let info = PageInfo {
            width: 10000.,
            height: 12000.,
        };
        let page = Rect::from_min_size(Pos2::ZERO, vec2(100000., 120000.));
        let visible = Rect::from_min_size(pos2(45000., 62000.), vec2(1600., 1200.));
        let first = tiles.plan(&info, page, visible, 10., 1).unwrap();
        assert!(!first.is_empty() && first.len() <= 16);
        assert!(first.iter().all(|r| r[2] <= 512 && r[3] <= 512));
        assert!(tiles.plan(&info, page, visible, 10., 1).is_none());
        tiles.plan(&info, page, visible, 12., 2).unwrap();
        tiles.receive(&ctx, 10., 1, Ok(vec![]));
        assert!(tiles.pending);
        assert_eq!(tiles.key, Some((12., 2)));
        tiles.receive(&ctx, 12., 2, Err("generated failure".into()));
        assert!(!tiles.pending);
        assert!(tiles.plan(&info, page, visible, 12., 2).is_none());
        assert!(
            tiles
                .plan(&info, page, visible.translate(vec2(1024., 0.)), 12., 2)
                .is_some()
        );
        tiles.clear();
        assert_eq!(tiles.bytes(), 0);
    }
}
