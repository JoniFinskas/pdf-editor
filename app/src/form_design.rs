use crate::{theme::Palette, worker::Request};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use pdf_editor_core::{Bounds, DesignFieldKind, DesignedField, FormDesignEdit, NewFormField};

pub struct Designer {
    pub fields: Vec<DesignedField>,
    pub page_sizes: Vec<(f32, f32)>,
    selected: Option<DesignedField>,
    page: u16,
    name: String,
    required: bool,
    kind: usize,
    options: String,
    bounds: Bounds,
}
impl Default for Designer {
    fn default() -> Self {
        Self {
            fields: vec![],
            page_sizes: vec![],
            selected: None,
            page: 1,
            name: "Field".into(),
            required: false,
            kind: 0,
            options: "First\nSecond".into(),
            bounds: Bounds {
                left: 36.,
                bottom: 100.,
                right: 220.,
                top: 128.,
            },
        }
    }
}
impl Designer {
    pub fn refresh(&mut self, fields: Vec<DesignedField>) {
        if let Some(selected) = self.selected.take() {
            self.selected = fields.iter().find(|field| field.id == selected.id).cloned();
            if let Some(field) = &self.selected {
                if self.name==selected.name || field.name!=selected.name {self.name=field.name.clone();}
                if self.required==selected.required || field.required!=selected.required {self.required=field.required;}
                if self.bounds==selected.bounds || field.bounds!=selected.bounds {self.bounds=field.bounds;}
                self.page = field.page + 1;
            }
        }
        self.fields = fields;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, p: &Palette, idle: bool) -> Option<Request> {
        let mut request = None;
        ui.add_enabled_ui(idle, |ui| {
            ui.horizontal(|ui| {
                if ui.button("New field").clicked() {
                    self.selected = None;
                    self.name = "Field".into();
                }
                if ui.button("Refresh").clicked() {
                    request = Some(Request::FormFields);
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("design-fields")
                .max_height(110.0)
                .show(ui, |ui| {
                    for (index, field) in self.fields.iter().enumerate() {
                        let selected = self.selected.as_ref().is_some_and(|s| s.id == field.id);
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "{} · {} · page {} · #{}",
                                    field.name,
                                    field.kind,
                                    field.page + 1,
                                    index + 1
                                ),
                            )
                            .clicked()
                        {
                            self.selected = Some(field.clone());
                            self.name = field.name.clone();
                            self.required = field.required;
                            self.bounds = field.bounds;
                            self.page = field.page + 1;
                        }
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut self.name);
                ui.checkbox(&mut self.required, "Required");
            });
            if self.selected.is_none() {
                ui.horizontal(|ui| {
                    ui.label("Page");
                    ui.add(egui::DragValue::new(&mut self.page).range(1..=10000));
                    let kinds = ["Text", "Checkbox", "Radio group", "Dropdown", "Date"];
                    egui::ComboBox::from_id_salt("field-type")
                        .selected_text(kinds[self.kind])
                        .show_ui(ui, |ui| {
                            for (i, label) in kinds.iter().enumerate() {
                                ui.selectable_value(&mut self.kind, i, *label);
                            }
                        });
                });
                if self.kind == 2 || self.kind == 3 {
                    ui.label("Options (one per line)");
                    ui.text_edit_multiline(&mut self.options);
                }
                if self.kind == 4 {
                    ui.label("Date format: YYYY-MM-DD");
                }
            }
            ui.label(
                "Drag to move; drag a corner to resize.",
            );
            ui.label("PDF points · bottom-left origin");
            ui.horizontal(|ui| {
                for (label, value) in [
                    ("Left", &mut self.bounds.left),
                    ("Bottom", &mut self.bounds.bottom),
                    ("Right", &mut self.bounds.right),
                    ("Top", &mut self.bounds.top),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(value).speed(1.0));
                }
            });
            let (canvas, _) = ui.allocate_exact_size(Vec2::new(300.0, 180.0), Sense::hover());
            ui.painter().rect_filled(canvas, 3.0, p.surface);
            let (page_width, page_height) = self
                .page_sizes
                .get(self.page.saturating_sub(1) as usize)
                .copied()
                .unwrap_or((612.0, 792.0));
            let extent_x = self.bounds.right.max(page_width);
            let extent_y = self.bounds.top.max(page_height);
            let scale = (canvas.width() / extent_x).min(canvas.height() / extent_y);
            let origin = Pos2::new(canvas.center().x - extent_x * scale / 2.0, canvas.bottom());
            let rect = Rect::from_min_max(
                Pos2::new(
                    origin.x + self.bounds.left * scale,
                    origin.y - self.bounds.top * scale,
                ),
                Pos2::new(
                    origin.x + self.bounds.right * scale,
                    origin.y - self.bounds.bottom * scale,
                ),
            );
            ui.painter()
                .rect_stroke(rect, 0.0, Stroke::new(1.5, p.accent), StrokeKind::Inside);
            let handle = Rect::from_center_size(rect.right_bottom(), Vec2::splat(12.0));
            let moving = ui.interact(
                Rect::from_min_max(rect.min, rect.max - Vec2::splat(6.0)),
                egui::Id::new(("form-designer-move", self.page)),
                Sense::drag(),
            );
            let sizing = ui.interact(
                handle,
                egui::Id::new(("form-designer-size", self.page)),
                Sense::drag(),
            );
            ui.painter()
                .rect_filled(handle, 1.0, Color32::from_rgb(75, 130, 230));
            if sizing.dragged() {
                let delta = sizing.drag_delta() / scale;
                self.bounds.right = (self.bounds.right + delta.x).max(self.bounds.left + 4.0);
                self.bounds.bottom = (self.bounds.bottom - delta.y).min(self.bounds.top - 4.0);
            } else if moving.dragged() {
                let delta = moving.drag_delta() / scale;
                self.bounds.left += delta.x;
                self.bounds.right += delta.x;
                self.bounds.bottom -= delta.y;
                self.bounds.top -= delta.y;
            }
            ui.horizontal(|ui| {
                if ui
                    .button(if self.selected.is_some() {
                        "Apply field"
                    } else {
                        "Add field"
                    })
                    .clicked()
                {
                    let edit = if let Some(field) = &self.selected {
                        FormDesignEdit::Update {
                            field: field.clone(),
                            name: self.name.clone(),
                            required: self.required,
                            bounds: self.bounds,
                        }
                    } else {
                        let options = self.options.lines().map(str::to_owned).collect();
                        let kind = match self.kind {
                            1 => DesignFieldKind::Checkbox,
                            2 => DesignFieldKind::Radio { options },
                            3 => DesignFieldKind::Dropdown { options },
                            4 => DesignFieldKind::Date,
                            _ => DesignFieldKind::Text,
                        };
                        FormDesignEdit::Add(NewFormField {
                            page: self.page.saturating_sub(1),
                            name: self.name.clone(),
                            required: self.required,
                            bounds: self.bounds,
                            kind,
                        })
                    };
                    request = Some(Request::DesignForm(edit));
                }
                if let Some(field) = &self.selected
                    && ui.button("Delete field").clicked()
                {
                    request = Some(Request::DesignForm(FormDesignEdit::Delete(field.clone())));
                    self.selected = None;
                }
            });
        });
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn designer_refresh_updates_selected_revision_and_removes_deleted_selection() {
        let mut d = Designer::default();
        let mut field = DesignedField {
            id: (12, 0),
            revision: 1,
            page: 0,
            name: "Name".into(),
            required: false,
            bounds: d.bounds,
            kind: "Text".into(),
        };
        d.selected = Some(field.clone());
        field.revision = 2;
        field.name = "Updated".into();
        d.refresh(vec![field]);
        assert_eq!(d.selected.as_ref().unwrap().revision, 2);
        assert_eq!(d.name, "Updated");
        d.refresh(vec![]);
        assert!(d.selected.is_none());
    }

    #[test]
    fn designer_pointer_moves_and_resizes_draft() {
        fn frame(d: &mut Designer, ctx: &egui::Context, events: Vec<egui::Event>) {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700., 650.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let p = crate::theme::palette(ui);
                        assert!(d.show(ui, &p, true).is_none());
                    });
                },
            );
            output.textures_delta.clear();
        }
        fn button(pos: Pos2, pressed: bool) -> egui::Event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }
        }
        let ctx = egui::Context::default();
        let mut d = Designer::default();
        for _ in 0..3 {
            frame(&mut d, &ctx, vec![]);
        }
        let pos = ctx
            .read_response(egui::Id::new(("form-designer-move", 1_u16)))
            .unwrap()
            .rect
            .center();
        let before = d.bounds;
        frame(
            &mut d,
            &ctx,
            vec![egui::Event::PointerMoved(pos), button(pos, true)],
        );
        let end = pos + Vec2::new(20., -20.);
        frame(&mut d, &ctx, vec![egui::Event::PointerMoved(end)]);
        frame(&mut d, &ctx, vec![button(end, false)]);
        assert!(d.bounds.left > before.left);
        assert!(d.bounds.bottom > before.bottom);
        assert!((d.bounds.right - d.bounds.left - (before.right - before.left)).abs() < 0.01);
        frame(&mut d, &ctx, vec![]);
        frame(&mut d, &ctx, vec![]);
        let pos = ctx
            .read_response(egui::Id::new(("form-designer-size", 1_u16)))
            .unwrap()
            .rect
            .center();
        let before = d.bounds;
        frame(
            &mut d,
            &ctx,
            vec![egui::Event::PointerMoved(pos), button(pos, true)],
        );
        let end = pos + Vec2::new(20., 20.);
        frame(&mut d, &ctx, vec![egui::Event::PointerMoved(end)]);
        frame(&mut d, &ctx, vec![button(end, false)]);
        assert!(d.bounds.right > before.right);
        assert!(d.bounds.bottom < before.bottom);
        assert_eq!(d.bounds.left, before.left);
        assert_eq!(d.bounds.top, before.top);
    }
}

#[cfg(test)]
mod refresh_tests {
    use super::*;
    #[test]
    fn form_designer_refresh_preserves_drafts_and_uses_fresh_apply_handles() {
        let field=DesignedField {id:(7,0),revision:1,page:0,name:"Generated field".into(),required:false,
            bounds:Bounds {left:36.,right:220.,bottom:100.,top:128.},kind:"Text".into()};
        let mut designer=Designer {selected:Some(field.clone()),name:"Draft name".into(),required:true,
            bounds:Bounds {right:240.,..field.bounds},..Default::default()};
        let mut current=field.clone();current.revision=2;designer.refresh(vec![current.clone()]);
        assert_eq!(designer.name,"Draft name");assert!(designer.required);assert_eq!(designer.bounds.right,240.);
        let ctx=egui::Context::default();crate::theme::install(&ctx);let mut request=None;
        let mut frame=|designer:&mut Designer,events| {
            let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(700.,750.))),events,..Default::default()},|ui| {
                let p=crate::theme::palette(ui);egui::CentralPanel::default().show(ui,|ui|request=designer.show(ui,&p,true));
            });out.textures_delta.clear();out
        };
        frame(&mut designer,vec![]);let out=frame(&mut designer,vec![]);
        let at=out.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t) if t.galley.text()=="Apply field"=>Some(t.pos+t.galley.size()*0.5),_=>None}).unwrap();
        let button=|pressed|egui::Event::PointerButton {pos:at,pressed,button:egui::PointerButton::Primary,modifiers:Default::default()};
        frame(&mut designer,vec![egui::Event::PointerMoved(at),button(true)]);frame(&mut designer,vec![button(false)]);
        assert!(matches!(request,Some(Request::DesignForm(FormDesignEdit::Update {field,name,required:true,bounds})) if field.revision==2 && name=="Draft name" && bounds.right==240.));
        current.name="Changed in document".into();current.required=true;current.bounds.right=260.;current.revision=3;
        designer.refresh(vec![current]);assert_eq!(designer.name,"Changed in document");assert_eq!(designer.bounds.right,260.);
        designer.refresh(vec![]);assert!(designer.selected.is_none());
    }
}
