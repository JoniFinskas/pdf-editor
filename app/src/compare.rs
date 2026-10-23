//! Read-only comparison session with bounded background page processing.
use crate::{
    icons::Icon,
    theme::Palette,
    widgets::{self, Btn, ToolWindow},
};
use eframe::egui::{self, Color32, TextureHandle};
use pdf_editor_core::{ChangeKind, ComparePage, Engine};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
struct Session {
    tx: mpsc::SyncSender<(u16, bool)>,
    rx: mpsc::Receiver<Event>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
enum Event {
    Count(usize),
    Summary(usize, usize, usize),
    ScanError(usize, String),
    Page(u16, bool, pdf_editor_core::Result<ComparePage>),
    Error(String),
}
fn start(paths: [PathBuf; 2], passwords: [String; 2], ctx: egui::Context) -> Session {
    let (tx, commands) = mpsc::sync_channel::<(u16, bool)>(1);
    let (events, rx) = mpsc::sync_channel(2);
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let send = |event| {
            let delivered = events.send(event).is_ok();
            ctx.request_repaint();
            delivered
        };
        let setup = || -> pdf_editor_core::Result<_> {
            let exe = std::env::current_exe()?;
            let lib = exe
                .parent()
                .ok_or_else(|| pdf_editor_core::Error("Program folder unavailable".into()))?
                .join("pdfium.dll");
            let engine = Engine::new(&lib)?;
            let left = engine.open_with_password(&paths[0], &passwords[0])?;
            let right = engine.open_with_password(&paths[1], &passwords[1])?;
            let count = engine.pages(&left)?.len().max(engine.pages(&right)?.len());
            Ok((engine, left, right, count))
        };
        let (engine, left, right, count) = match setup() {
            Ok(v) => v,
            Err(e) => {
                send(Event::Error(e.to_string()));
                return;
            }
        };
        if !send(Event::Count(count)) {
            return;
        }
        let mut scan = 0;
        while !stop.load(Ordering::Relaxed) {
            let request = if scan < count {
                commands.try_recv().ok()
            } else {
                commands
                    .recv_timeout(std::time::Duration::from_millis(100))
                    .ok()
            };
            if let Some((page, overlay)) = request {
                if !send(Event::Page(
                    page,
                    overlay,
                    engine.compare_page(&left, &right, page, overlay),
                )) {
                    break;
                }
            } else if scan < count {
                let event = match engine.compare_page(&left, &right, scan as u16, false) {
                    Ok(page) => Event::Summary(scan, page.changes.len(), page.changed_pixels),
                    Err(e) => Event::ScanError(scan, e.to_string()),
                };
                if !send(event) {
                    break;
                }
                scan += 1;
            }
        }
    });
    Session { tx, rx, cancel }
}
#[derive(Default)]
pub struct Compare {
    pub open: bool,
    paths: [Option<PathBuf>; 2],
    passwords: [String; 2],
    session: Option<Session>,
    summaries: Vec<Option<(usize, usize)>>,
    errors: Vec<(usize, String)>,
    selected: u16,
    overlay: bool,
    zoom: f32,
    waiting: bool,
    requested: Option<(u16, bool)>,
    textures: [Option<TextureHandle>; 2],
    marks: [Vec<(ChangeKind, [f32; 4])>; 2],
    error: Option<String>,
}
impl Compare {
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette) {
        if !self.open {
            self.session = None;
            self.textures = [None, None];
            self.requested = None;
            self.waiting = false;
            self.summaries.clear();
            self.errors.clear();
            self.passwords = Default::default();
            return;
        }
        if self.zoom == 0. {
            self.zoom = 1.;
        }
        if let Some(session) = &self.session {
            while let Ok(event) = session.rx.try_recv() {
                match event {
                    Event::Count(n) => self.summaries = vec![None; n],
                    Event::Summary(i, t, v) => {
                        if let Some(s) = self.summaries.get_mut(i) {
                            *s = Some((t, v));
                        }
                    }
                    Event::ScanError(i, e) => {
                        if let Some(s) = self.summaries.get_mut(i) {
                            *s = Some((0, 0));
                        }
                        self.errors.push((i, e));
                    }
                    Event::Error(e) => {
                        self.error = Some(e);
                        self.waiting = false;
                    }
                    Event::Page(page, overlay, result) => {
                        self.waiting = false;
                        if page != self.selected || overlay != self.overlay {
                            self.requested = None;
                            continue;
                        }
                        match result {
                            Err(e) => {
                                self.error = Some(e.to_string());
                                self.textures = [None, None];
                            }
                            Ok(page) => {
                                self.error = None;
                                self.marks = [page.left_marks, page.right_marks];
                                self.textures = [page.left, page.right].map(|image| {
                                    image.map(|im| {
                                        ctx.load_texture(
                                            "compare-page",
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [im.width as usize, im.height as usize],
                                                &im.rgba,
                                            ),
                                            egui::TextureOptions::LINEAR,
                                        )
                                    })
                                });
                            }
                        }
                    }
                }
            }
        }
        let mut open = self.open;
        ToolWindow::new("Compare PDFs",Icon::Layers).width((ctx.content_rect().width()-64.).clamp(300.,1050.)).show(ctx,p,&mut open,|ui|{
            widgets::hint(ui,p,"Compare saved files, page by page. Originals and open edits stay unchanged. Pages align by index; inserted pages are not automatically realigned.");
            ui.horizontal_wrapped(|ui|{
                for i in 0..2 {
                    if Btn::new(if i==0{"Choose left PDF"}else{"Choose right PDF"}).show(ui,p).clicked()
                        && let Some(path)=rfd::FileDialog::new().add_filter("PDF",&["pdf"]).pick_file() {
                        self.paths[i]=Some(path);self.session=None;self.textures=[None,None];self.summaries.clear();self.errors.clear();self.requested=None;self.waiting=false;
                    }
                    ui.label(self.paths[i].as_ref().and_then(|p|p.file_name()).map(|n|n.to_string_lossy().into_owned()).unwrap_or_else(||"No file".into()));
                }
            });
            ui.horizontal_wrapped(|ui|{
                for i in 0..2 {ui.label(if i==0{"Left password"}else{"Right password"});ui.add(egui::TextEdit::singleline(&mut self.passwords[i]).password(true).desired_width(110.));}
                if Btn::new("Compare").enabled(self.paths.iter().all(Option::is_some)).show(ui,p).clicked()
                    && let [Some(a),Some(b)]=&self.paths {
                        self.session=Some(start([a.clone(),b.clone()],std::mem::take(&mut self.passwords),ctx.clone()));
                        self.summaries.clear();self.errors.clear();self.error=None;self.selected=0;self.waiting=false;self.requested=None;self.textures=[None,None];
                }
            });
            if let Some(error)=&self.error {ui.colored_label(p.error_fg,error);}
            ui.horizontal_wrapped(|ui|{
                ui.checkbox(&mut self.overlay,"Visual pixel overlay");
                ui.add(egui::Slider::new(&mut self.zoom,0.25..=3.).text("Shared zoom"));
                let count=self.summaries.len();
                if Btn::new("Previous").enabled(self.selected>0).show(ui,p).clicked(){self.selected-=1;}
                if Btn::new("Next").enabled((self.selected as usize)+1<count).show(ui,p).clicked(){self.selected+=1;}
                ui.label(format!("Page {} / {}",usize::from(self.selected)+1,count));
            });
            widgets::hint(ui,p,"Green: added · Red: removed · Amber: changed · Magenta: changed pixels. Both pages share scrolling and zoom. Raster preview is capped at one megapixel per page.");
            ui.horizontal_top(|ui|{
                ui.vertical(|ui|{
                    ui.set_width(160.);
                    ui.label(format!("Scanned {} / {}",self.summaries.iter().filter(|s|s.is_some()).count(),self.summaries.len()));
                    egui::ScrollArea::vertical().id_salt("comparison-changes").max_height(450.).show(ui,|ui|{
                        for (i,summary) in self.summaries.iter().enumerate() {
                            if let Some((text,pixels))=summary && (*text>0 || *pixels>0)
                                && ui.selectable_label(self.selected as usize==i,format!("Page {}\n{text} text · {pixels} pixels",i+1)).clicked() {self.selected=i as u16;}
                        }
                        for (i,error) in &self.errors {if ui.selectable_label(self.selected as usize==*i,format!("Page {}: {error}",i+1)).clicked(){self.selected=*i as u16;}}
                    });
                });
                egui::ScrollArea::both().id_salt(("compare-pages",self.selected)).max_height(450.).show(ui,|ui|{
                    ui.horizontal_top(|ui|{
                        for side in 0..2 {
                            if let Some(texture)=&self.textures[side] {
                                let size=texture.size_vec2()*self.zoom;
                                let response=ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                                if !self.overlay {for (kind,b) in &self.marks[side] {
                                    let color=match kind{ChangeKind::Added=>Color32::from_rgba_unmultiplied(30,180,80,75),ChangeKind::Removed=>Color32::from_rgba_unmultiplied(230,40,40,75),ChangeKind::Changed=>Color32::from_rgba_unmultiplied(240,160,20,90)};
                                    let rect=egui::Rect::from_min_max(response.rect.min+egui::vec2(b[0],b[1])*self.zoom,response.rect.min+egui::vec2(b[2],b[3])*self.zoom);
                                    ui.painter().rect_filled(rect,0.,color);
                                }}
                            } else {ui.allocate_ui(egui::vec2(240.,300.),|ui|{ui.label(if self.waiting{"Loading…"}else{"No page"});});}
                        }
                    });
                });
            });
        });
        self.open = open;
        let key = (self.selected, self.overlay);
        if !self.waiting
            && !self.summaries.is_empty()
            && self.requested != Some(key)
            && let Some(session) = &self.session
            && session.tx.try_send(key).is_ok()
        {
            self.waiting = true;
            self.requested = Some(key);
            self.textures = [None, None];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_window_draws_without_loading_a_document() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut compare = Compare {
            open: true,
            ..Default::default()
        };
        for dark in [false, true] {
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1180., 820.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    compare.show(&ctx, &crate::theme::palette(ui));
                },
            );
            output.textures_delta.clear();
            assert!(compare.session.is_none());
            assert!(compare.paths.iter().all(Option::is_none));
        }
    }
}
