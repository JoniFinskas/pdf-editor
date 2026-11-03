use super::*;

impl TabbedApp {
    pub(super) fn tab_bar(&mut self, ui: &mut Ui, p: &Palette) -> (Option<u64>, Option<u64>, bool) {
        let ctx = ui.ctx().clone();
        let cancelled = self.closing.is_some() || ctx.input(|i| !i.focused)
            || (self.dragging_tab.is_some() && ctx.input_mut(|i|i.consume_key(egui::Modifiers::NONE,Key::Escape)));
        if cancelled { self.dragging_tab = None; }
        let mut select = None;
        let mut close = None;
        let mut new = false;
        let mut target = None;
        egui::Panel::top("document-tabs")
            .frame(Frame::new().fill(p.canvas).inner_margin(Margin {left:8,right:8,top:6,bottom:0}))
            .show_separator_line(false)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .scroll_source(egui::scroll_area::ScrollSource {drag:egui::scroll_area::DragScroll::Never,..Default::default()})
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        let visible = ui.clip_rect();
                        let row=ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for (index,(id,tab)) in self.tabs.iter().enumerate() {
                                ui.push_id(id, |ui| {
                                    let name = tab.doc.as_ref().map_or("New document",|d|d.name.as_str());
                                    let (response,closed) = document_tab(ui,p,name,self.active==index,Self::needs_warning(tab));
                                    if response.clicked() && self.dragging_tab.is_none() {select=Some(*id);}
                                    if closed && self.dragging_tab.is_none() {close=Some(*id);}
                                    if !cancelled && response.drag_started_by(egui::PointerButton::Primary)
                                        && let Some(origin)=ctx.input(|i|i.pointer.press_origin())
                                        && origin.x < response.rect.right()-30. {
                                        self.dragging_tab=Some((*id,response.rect.min-origin));
                                    }
                                    if let Some((dragged,offset))=self.dragging_tab {
                                        ctx.set_cursor_icon(CursorIcon::Grabbing);
                                        if let Some(pointer)=ctx.input(|i|i.pointer.hover_pos()) {
                                            if dragged==*id {
                                                let painter=ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip,Id::new("tab-drag-preview")));
                                                let ghost=Rect::from_min_size(pointer+offset,response.rect.size());
                                                painter.rect_filled(ghost,6.,p.chrome);
                                                painter.rect_stroke(ghost,6.,Stroke::new(1.5,p.accent),StrokeKind::Outside);
                                                painter.text(ghost.center(),Align2::CENTER_CENTER,shorten(name,24),FontId::proportional(13.5),p.text);
                                            }
                                            if visible.contains(pointer) && response.rect.contains(pointer) {
                                                let after=pointer.x>=response.rect.center().x;
                                                let x=if after {response.rect.right()+1.} else {response.rect.left()-1.};
                                                ui.painter().line_segment([pos2(x,response.rect.top()),pos2(x,response.rect.bottom())],Stroke::new(3.,p.accent));
                                                target=Some((dragged,index,after));
                                            }
                                        }
                                    } else if response.hovered() {ctx.set_cursor_icon(CursorIcon::Grab);}
                                });
                            }
                            if Btn::icon(Icon::Plus).id("new-tab").height(30.).tip("New tab").show(ui,p).clicked() {new=true;}
                        });
                        let visible=visible.intersect(row.response.rect);
                        if self.dragging_tab.is_some() && ctx.input(|i|i.pointer.primary_down())
                            && let Some(pointer)=ctx.input(|i|i.pointer.hover_pos()).filter(|p|visible.contains(*p)) {
                            let direction=if pointer.x<visible.left()+24. {1.} else if pointer.x>visible.right()-24. {-1.} else {0.};
                            if direction!=0. {ui.scroll_with_delta(vec2(direction*600.*ctx.input(|i|i.stable_dt.min(0.1)),0.));ctx.request_repaint();}
                        }
                    });
            });
        if ctx.input(|i|i.pointer.primary_released()) {
            if let Some((id,index,after))=target
                && let Some(from)=self.tabs.iter().position(|(key,_)|*key==id) {
                let to=reorder_destination(from,index,after,self.tabs.len());
                let active=self.tabs[self.active].0;
                let tab=self.tabs.remove(from);
                self.tabs.insert(to,tab);
                self.active=self.tabs.iter().position(|(key,_)|*key==active).unwrap();
            }
            self.dragging_tab=None;
        } else if !ctx.input(|i|i.pointer.primary_down()) {self.dragging_tab=None;}
        (select,close,new)
    }
}
