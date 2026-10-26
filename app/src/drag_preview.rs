use super::*;

#[derive(PartialEq)]
struct Key {
    generation: u64,
    page: usize,
    indices: Vec<usize>,
    scale: u32,
}
struct Layers {
    background: TextureHandle,
    foreground: TextureHandle,
    source: Rect,
}
#[derive(Default)]
pub(super) struct Preview {
    serial: u64,
    key: Option<Key>,
    layers: Option<Layers>,
    placement: Option<(Rect, Rect)>,
    committing: bool,
    handoff: Option<u64>,
}
impl Preview {
    #[cfg(test)]
    pub fn ready(&self) -> bool {
        self.layers.is_some()
    }
    pub fn plan(
        &mut self,
        generation: u64,
        page: usize,
        items: &[pdf_editor_core::PageItem],
        scale: f32,
    ) -> Option<u64> {
        let mut indices = items.iter().map(|item| item.index).collect::<Vec<_>>();
        indices.sort_unstable();
        let key = Key {
            generation,
            page,
            indices,
            scale: scale.to_bits(),
        };
        if self.key.as_ref() == Some(&key) {
            return None;
        }
        self.serial = self.serial.wrapping_add(1);
        self.key = Some(key);
        self.layers = None;
        self.placement = None;
        self.committing = false;
        self.handoff = None;
        Some(self.serial)
    }
    pub fn receive(
        &mut self,
        ctx: &Context,
        id: u64,
        generation: u64,
        result: pdf_editor_core::Result<pdf_editor_core::ObjectPreview>,
    ) {
        if self.serial != id
            || self
                .key
                .as_ref()
                .is_none_or(|key| key.generation != generation)
        {
            return;
        }
        let Ok(preview) = result else {
            return;
        };
        let bg = &preview.background;
        let fg = &preview.foreground;
        let [x, y, w, h] = preview.rect;
        if bg.width == 0
            || bg.height == 0
            || w == 0
            || h == 0
            || fg.width != w
            || fg.height != h
            || x.checked_add(w).is_none_or(|right| right > bg.width)
            || y.checked_add(h).is_none_or(|bottom| bottom > bg.height)
            || [bg, fg].iter().any(|image| {
                image.width as u64 * image.height as u64 > 2_000_000
                    || image.rgba.len() as u64 != image.width as u64 * image.height as u64 * 4
            })
        {
            return;
        }
        let source = Rect::from_min_max(
            pos2(x as f32 / bg.width as f32, y as f32 / bg.height as f32),
            pos2(
                (x + w) as f32 / bg.width as f32,
                (y + h) as f32 / bg.height as f32,
            ),
        );
        let texture = |name, image: pdf_editor_core::Image| {
            ctx.load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width as usize, image.height as usize],
                    &image.rgba,
                ),
                TextureOptions::LINEAR,
            )
        };
        self.layers = Some(Layers {
            source,
            background: texture("drag-background", preview.background),
            foreground: texture("drag-foreground", preview.foreground),
        });
    }
    pub fn place(&mut self, original: Rect, current: Rect) {
        self.placement = Some((original, current));
    }
    pub fn placement(&self) -> Option<Rect> {
        self.placement.map(|(_, current)| current)
    }
    pub fn commit(&mut self) {
        self.committing = true;
    }
    pub fn committing(&self) -> bool {
        self.committing
    }
    pub fn cancel(&mut self) {
        self.placement = None;
        self.committing = false;
        self.handoff = None;
    }
    pub fn changed(&mut self, generation: u64) {
        if self.committing {
            self.handoff = Some(generation);
        } else {
            *self = Self::default();
        }
    }
    pub fn rendered(&mut self, page: usize, generation: u64) {
        if self.handoff == Some(generation) && self.key.as_ref().is_some_and(|key| key.page == page)
        {
            *self = Self::default();
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        painter: &egui::Painter,
        page: Rect,
        scale: f32,
        index: usize,
        generation: u64,
        fallback: Option<&PageTexture>,
        accent: Color32,
    ) {
        if self.key.as_ref().is_none_or(|key| {
            key.page != index || (key.generation != generation && self.handoff != Some(generation))
        }) {
            return;
        }
        let Some((original, current)) = self.placement else {
            return;
        };
        let original = to_screen(original, page, scale);
        let current = to_screen(current, page, scale);
        if !original.is_positive() || !current.is_positive() {
            return;
        }
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(page));
        let unit = Rect::from_min_max(Pos2::ZERO, pos2(1., 1.));
        if let Some(layers) = &self.layers {
            painter.image(layers.background.id(), page, unit, Color32::WHITE);
            let source =
                egui::emath::RectTransform::from_to(unit, page).transform_rect(layers.source);
            let target =
                egui::emath::RectTransform::from_to(original, current).transform_rect(source);
            painter.image(layers.foreground.id(), target, unit, Color32::WHITE);
        } else if let Some(texture) = fallback {
            let source = original.intersect(page);
            if source.is_positive() {
                let uv = egui::emath::RectTransform::from_to(page, unit).transform_rect(source);
                let target =
                    egui::emath::RectTransform::from_to(original, current).transform_rect(source);
                painter.image(texture.handle.id(), target, uv, Color32::WHITE);
            }
        }
        if self.committing {
            painter.rect_stroke(current, 2., Stroke::new(1.5, accent), StrokeKind::Outside);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item() -> pdf_editor_core::PageItem {
        pdf_editor_core::PageItem {
            page: 0,
            index: 0,
            revision: 1,
            kind: "Text".into(),
            bounds: pdf_editor_core::Bounds {
                left: 0.,
                bottom: 0.,
                right: 1.,
                top: 1.,
            },
        }
    }
    fn layers() -> pdf_editor_core::ObjectPreview {
        pdf_editor_core::ObjectPreview {
            background: pdf_editor_core::Image {
                width: 10,
                height: 10,
                rgba: vec![255; 400],
            },
            foreground: pdf_editor_core::Image {
                width: 4,
                height: 2,
                rgba: vec![90; 32],
            },
            rect: [2, 3, 4, 2],
        }
    }
    #[test]
    fn stale_preview_replies_and_handoff_rasters_cannot_replace_current_layers() {
        let ctx = Context::default();
        let mut preview = Preview::default();
        let id = preview.plan(1, 0, &[item()], 1.).unwrap();
        assert!(preview.plan(1, 0, &[item()], 1.).is_none());
        let newer = preview.plan(1, 0, &[item()], 2.).unwrap();
        preview.receive(&ctx, id, 1, Ok(layers()));
        assert!(!preview.ready());
        preview.receive(&ctx, newer, 0, Ok(layers()));
        assert!(!preview.ready());
        preview.receive(&ctx, newer, 1, Ok(layers()));
        assert!(preview.ready());
        preview.place(
            Rect::from_min_size(pos2(20., 30.), vec2(40., 20.)),
            Rect::from_min_size(pos2(70., 80.), vec2(40., 20.)),
        );
        preview.commit();
        preview.changed(2);
        preview.rendered(1, 2);
        assert!(preview.committing());
        preview.rendered(0, 1);
        assert!(preview.committing());
        preview.rendered(0, 2);
        assert!(!preview.committing());
        assert!(!preview.ready());
    }
    #[test]
    fn foreground_mesh_uses_current_frame_position_and_original_uvs() {
        let ctx = Context::default();
        let mut preview = Preview::default();
        let id = preview.plan(1, 0, &[item()], 1.).unwrap();
        preview.receive(&ctx, id, 1, Ok(layers()));
        let foreground = preview.layers.as_ref().unwrap().foreground.id();
        preview.place(
            Rect::from_min_size(pos2(20., 30.), vec2(40., 20.)),
            Rect::from_min_size(pos2(70., 80.), vec2(40., 20.)),
        );
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200., 200.))),
                ..Default::default()
            },
            |ui| {
                preview.paint(
                    ui.painter(),
                    Rect::from_min_size(Pos2::ZERO, vec2(100., 100.)),
                    1.,
                    0,
                    1,
                    None,
                    Color32::BLUE,
                );
            },
        );
        out.textures_delta.clear();
        fn mesh(shape: &Shape, id: egui::TextureId) -> Option<&egui::epaint::Mesh> {
            match shape {
                Shape::Mesh(mesh) if mesh.texture_id == id => Some(mesh),
                Shape::Vec(shapes) => shapes.iter().find_map(|shape| mesh(shape, id)),
                _ => None,
            }
        }
        let mesh = out
            .shapes
            .iter()
            .find_map(|s| mesh(&s.shape, foreground))
            .expect("Foreground image was not painted");
        let bounds = mesh.vertices.iter().fold(Rect::NOTHING, |bounds, v| {
            bounds.union(Rect::from_min_max(v.pos, v.pos))
        });
        assert!(bounds.min.distance(pos2(70., 80.)) < 0.001);
        assert!(bounds.max.distance(pos2(110., 100.)) < 0.001);
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.uv.x >= 0. && v.uv.x <= 1. && v.uv.y >= 0. && v.uv.y <= 1.)
        );
    }
}
