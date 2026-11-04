use super::*;

enum DragEnd { Cancel, Release(Pos2,bool) }

impl ItemDrag {
    pub(super) fn update(&mut self,pointer:Option<Pos2>,snap:bool)->(Rect,Option<crate::snapping::Snapped>) {
        if let Some(pointer) = pointer.filter(|pos|pos.is_finite()) {
            self.delta = ((pointer - self.page_rect.min) / self.scale).to_pos2() - self.anchor;
        }
        let mut preview_bounds = self.resize_handle.map_or_else(
            || self.bounds.translate(self.delta),
            |handle| self.font_resize.as_ref().map_or_else(||resized_rect(self.bounds,handle,self.delta),|font|font.drag(self.bounds,handle,self.delta)),
        );
        let mut guides = None;
        if snap
        {
            let page_bounds = self.page_bounds;
            let targets = self.snap_targets.iter().copied();
            let mut snapped = crate::snapping::align_text(preview_bounds,self.resize_handle,page_bounds,targets,6.0/self.scale,&mut self.snap_state,&self.snap_baselines);
            if let (Some(font),Some(handle))=(&self.font_resize,self.resize_handle) {
                let axis=match (snapped.x,snapped.y) {
                    (Some(_),Some(_))=>Some(usize::from((snapped.bounds.height()-preview_bounds.height()).abs()<(snapped.bounds.width()-preview_bounds.width()).abs())),
                    (Some(_),None)=>Some(0),(None,Some(_))=>Some(1),_=>None,
                };
                if let Some(axis)=axis {snapped.bounds=font.bounds(self.bounds,handle,snapped.bounds.size()[axis]/self.bounds.size()[axis]);}
                let active=resize_handles(snapped.bounds)[handle];
                if snapped.x.is_some_and(|g|(active.x-g.position).abs()>0.1) {snapped.x=None;}
                if snapped.y.is_some_and(|g|(active.y-g.position).abs()>0.1) {snapped.y=None;}
            }
            preview_bounds = snapped.bounds;
            guides = Some(snapped);
        } else {
            self.snap_state = Default::default();
        }
        self.preview = Some(preview_bounds);
        (preview_bounds,guides)
    }
}

impl EditorApp {
    pub(super) fn cancel_item_drag(&mut self)->bool {
        let Some(doc)=&mut self.doc else {return false;};
        doc.panning=false;
        let active=doc.dragging_item.take().is_some();
        if active {self.drag_preview.cancel();}
        active
    }

    pub(super) fn update_item_drag(&mut self,ctx:&Context) {
        if self.doc.as_ref().is_none_or(|doc|doc.dragging_item.is_none()) {
            if !ctx.input(|i|i.pointer.primary_down()) && let Some(doc)=&mut self.doc {doc.panning=false;}
            return;
        }
        let blocked=self.busy>0 || self.settings_open || self.confirm.is_some() || self.file_save.suspended
            || self.picker.pending() || self.signatures.placing || self.drawing.active || self.tables.selecting
            || self.text_tool.adding || self.qol.text_mode || self.qol.box_select || self.markup.links.selecting;
        let end=ctx.input_mut(|i| {
            if blocked || !i.focused || i.events.iter().any(|e|matches!(e,egui::Event::WindowFocused(false)))
                || i.consume_key(egui::Modifiers::NONE,Key::Escape) {return Some(DragEnd::Cancel);}
            // The first terminal event belongs to the existing gesture. A later
            // press in the same frame must not move it to the next click.
            i.events.iter().find_map(|event|match event {
                egui::Event::PointerButton {pos,pressed:false,button:egui::PointerButton::Primary,modifiers}=>Some(DragEnd::Release(*pos,modifiers.alt)),
                egui::Event::PointerButton {pressed:true,..}|egui::Event::PointerGone=>Some(DragEnd::Cancel),
                _=>None,
            }).or_else(||(!i.pointer.primary_down()).then_some(DragEnd::Cancel))
        });
        match end {
            Some(DragEnd::Cancel)=>{self.cancel_item_drag();}
            Some(DragEnd::Release(pos,alt))=>{
                let Some(mut drag)=self.doc.as_mut().and_then(|doc|doc.dragging_item.take()) else {return;};
                self.doc.as_mut().unwrap().panning=false;
                let (bounds,_)=drag.update(Some(pos),self.snap_enabled && !alt);
                let changed=(bounds.min-drag.bounds.min).abs().max_elem()>0.01 || (bounds.max-drag.bounds.max).abs().max_elem()>0.01;
                let action=if !changed {None} else if drag.resize_handle.is_some() {
                    raster_bounds(&drag.transform,bounds).map(|bounds|if drag.font_resize.is_some() {
                        pdf_editor_core::ObjectAction::ResizeText {bounds}
                    } else {pdf_editor_core::ObjectAction::Resize {bounds}})
                } else {
                    raster_delta(&drag.transform,bounds.min-drag.bounds.min).map(|(dx,dy)|pdf_editor_core::ObjectAction::Move {dx,dy})
                };
                if let Some(action)=action {
                    self.drag_preview.place(drag.bounds,bounds);self.drag_preview.commit();
                    self.busy+=1;self.send(Request::EditItems(drag.items,action));
                } else {self.drag_preview.cancel();}
            }
            None=>{}
        }
    }
}
