use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, Look, ToolWindow},
    worker::{PageTransform, Request},
};
use eframe::egui;
use pdf_editor_core::{Bounds, TableFormat, TableGrid};
#[derive(Default)]
pub struct Tables {
    pub open: bool,
    dialog: crate::dialogs::ToolDialog<Request>,
    pub selecting: bool,
    start: Option<(u16, egui::Pos2)>,
    pub grid: Option<TableGrid>,
}
impl Tables {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, idle: bool) -> Option<Request> {
        let request = self.dialog.poll(ctx, self.open, idle);
        let idle = idle && !self.dialog.busy() && request.is_none();
        let mut select = false;
        ToolWindow::new("Tables to Excel or CSV",Icon::Export).width(620.).show(ctx,p,&mut self.open,|ui|{
   self.dialog.feedback(ui,p);
   widgets::hint(ui,p,"Select a table on the page. Check the detected grid before exporting; complex or merged cells may need corrections.");
   if Btn::new("Select area on page").enabled(idle).show(ui,p).clicked(){select=true;}
   if let Some(grid)=&mut self.grid {
    egui::ScrollArea::both().max_height(340.).show(ui,|ui|{egui::Grid::new("table-preview").striped(true).show(ui,|ui|{for row in &mut grid.cells {for cell in row {ui.add(widgets::text_field(cell,110.));}ui.end_row();}});});
    ui.horizontal(|ui|{for (format,label,ext) in [(TableFormat::Xlsx,"Export Excel…","xlsx"),(TableFormat::Csv,"Export CSV…","csv")] {if Btn::new(label).look(Look::Primary).enabled(idle).show(ui,p).clicked() {let grid=grid.clone(); self.dialog.launch(move || rfd::FileDialog::new().add_filter(label,&[ext]).set_file_name(format!("table.{ext}")).save_file().map(|path| Request::ExportTable(grid,format,path)));}}});
   }
  });
        if select {
            self.selecting = true;
            self.open = false;
            self.grid = None;
        }
        if !self.open { self.dialog.cancel(); return None; }
        request
    }
    pub fn on_page(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        page: u16,
        rect: egui::Rect,
        scale: f32,
        transform: PageTransform,
    ) -> Option<Request> {
        if !self.selecting {
            return None;
        }
        response.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        if response.drag_started()
            && let Some(origin) = response.ctx.input(|i| i.pointer.press_origin())
        {
            self.start = Some((page, origin));
        }
        if let Some((start_page, start)) = self.start
            && start_page == page
            && let Some(end) = response.interact_pointer_pos()
        {
            let selected = egui::Rect::from_two_pos(start, end).intersect(rect);
            painter.rect_stroke(
                selected,
                0.,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(70, 130, 240)),
                egui::StrokeKind::Inside,
            );
            if response.drag_stopped() {
                self.start = None;
                self.selecting = false;
                self.open = true;
                if selected.width() > 4. && selected.height() > 4. {
                    let a = (selected.min - rect.min) / scale;
                    let b = (selected.max - rect.min) / scale;
                    if let (Some(a), Some(b)) =
                        (transform.inverse(a.x, a.y), transform.inverse(b.x, b.y))
                    {
                        return Some(Request::DetectTable(
                            page,
                            Bounds {
                                left: a.0.min(b.0),
                                right: a.0.max(b.0),
                                bottom: a.1.min(b.1),
                                top: a.1.max(b.1),
                            },
                        ));
                    }
                }
            }
        }
        if response.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selecting = false;
            self.start = None;
            self.open = true;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_dispatches_pdf_bounds_once() {
        for (matrix, scale, expected) in [
            (
                [1., 0., 0., -1., 0., 300.],
                1.,
                Bounds {
                    left: 50.,
                    right: 200.,
                    bottom: 100.,
                    top: 250.,
                },
            ),
            (
                [0., 1., -1., 0., 300., 0.],
                2.,
                Bounds {
                    left: 25.,
                    right: 100.,
                    bottom: 200.,
                    top: 275.,
                },
            ),
        ] {
            let ctx = egui::Context::default();
            let mut tool = Tables {
                selecting: true,
                ..Default::default()
            };
            let mut requests = vec![];
            let mut frame = |events: Vec<egui::Event>| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(400., 400.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let rect =
                            egui::Rect::from_min_max(egui::pos2(20., 20.), egui::pos2(320., 320.));
                        let response = ui.interact(
                            rect,
                            egui::Id::new("selection-test"),
                            egui::Sense::click_and_drag(),
                        );
                        if let Some(request) = tool.on_page(
                            &response,
                            ui.painter(),
                            2,
                            rect,
                            scale,
                            PageTransform::for_test(matrix),
                        ) {
                            requests.push(request);
                        }
                    },
                );
                output.textures_delta.clear();
            };
            frame(vec![]);
            frame(vec![]);
            frame(vec![
                egui::Event::PointerMoved(egui::pos2(70., 70.)),
                egui::Event::PointerButton {
                    pos: egui::pos2(70., 70.),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            frame(vec![egui::Event::PointerMoved(egui::pos2(220., 220.))]);
            frame(vec![egui::Event::PointerButton {
                pos: egui::pos2(220., 220.),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(requests.len(), 1);
            let Request::DetectTable(page, area) = &requests[0] else {
                panic!("wrong request")
            };
            assert_eq!(*page, 2);
            assert_eq!(*area, expected);
            assert!(!tool.selecting);
        }
    }
}
