use super::*;
use std::{io::{self, Read}, sync::atomic::{AtomicBool, Ordering}};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode { #[default] Off, AfterEdits, Minute, TwoMinutes, FiveMinutes, TenMinutes }
impl Mode {
    pub const ALL: [Self;6]=[Self::Off,Self::AfterEdits,Self::Minute,Self::TwoMinutes,Self::FiveMinutes,Self::TenMinutes];
    pub fn label(self)->&'static str {match self {Self::Off=>"Off",Self::AfterEdits=>"After edits (2 seconds)",Self::Minute=>"Every minute",Self::TwoMinutes=>"Every 2 minutes",Self::FiveMinutes=>"Every 5 minutes",Self::TenMinutes=>"Every 10 minutes"}}
    fn value(self)->&'static str {match self {Self::Off=>"off",Self::AfterEdits=>"after-edits",Self::Minute=>"1",Self::TwoMinutes=>"2",Self::FiveMinutes=>"5",Self::TenMinutes=>"10"}}
    fn delay(self)->Option<f64> {match self {Self::Off=>None,Self::AfterEdits=>Some(2.),Self::Minute=>Some(60.),Self::TwoMinutes=>Some(120.),Self::FiveMinutes=>Some(300.),Self::TenMinutes=>Some(600.)}}
    pub fn load(path:&std::path::Path)->io::Result<Self> {
        let file=match std::fs::File::open(path) {Ok(file)=>file,Err(e) if e.kind()==io::ErrorKind::NotFound=>return Ok(Self::Off),Err(e)=>return Err(e)};
        let mut text=String::new();file.take(129).read_to_string(&mut text)?;
        if text.len()<=128 && let Some(value)=text.trim().strip_prefix("working-copy-autosave=")
            && let Some(mode)=Self::ALL.into_iter().find(|mode|mode.value()==value) {return Ok(mode);}
        Err(io::Error::other("Invalid auto-save setting"))
    }
    pub fn save(self,path:&std::path::Path)->io::Result<()> {recovery::write_replacing(path,format!("working-copy-autosave={}\n",self.value()).as_bytes())}
}

static WRITING: AtomicBool=AtomicBool::new(false);
struct Lease;
impl Lease {
    fn acquire()->Option<Self> {WRITING.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).ok().map(|_|Self)}
}
impl Drop for Lease {fn drop(&mut self) {WRITING.store(false,Ordering::Release);}}

#[derive(Default)]
pub(super) struct State {
    pub mode: Mode,
    pub changed: bool,
    pub suspended: bool,
    pub in_flight: Option<u64>,
    pub paused: bool,
    warning: Option<u64>,
    generation: Option<u64>,
    due: Option<f64>,
    lease: Option<Lease>,
}
impl State {
    pub fn new(mode:Mode)->Self {Self {mode,..Default::default()}}
    fn schedule_reset(&mut self) {self.generation=None;self.due=None;}
    pub fn configure(&mut self,mode:Mode) {
        if self.mode!=mode {self.mode=mode;self.schedule_reset();self.paused=false;}
    }
    pub fn reset_document(&mut self) {self.schedule_reset();self.paused=false;self.finish();}
    fn finish(&mut self) {self.in_flight=None;self.lease=None;}
    pub fn stopped(&mut self) {self.finish();self.schedule_reset();self.paused=true;}
    fn observe(&mut self,generation:u64,now:f64)->Option<f64> {
        let delay=self.mode.delay()?;
        if self.generation!=Some(generation) {
            if self.mode==Mode::AfterEdits {self.due=None;}
            self.generation=Some(generation);
        }
        Some(*self.due.get_or_insert(now+delay))
    }
}

impl TabbedApp {
    pub(super) fn sync_working_auto_save(&mut self,ctx:&Context) {
        let changed=self.tabs.iter_mut().filter_map(|(_,tab)|std::mem::take(&mut tab.file_save.changed).then_some(tab.file_save.mode)).last();
        if let Some(mode)=changed {for (_,tab) in &mut self.tabs {tab.apply_file_save_mode(mode);}}
        let suspended=self.closing.is_some() || ctx.input(|i|i.viewport().close_requested());
        for (_,tab) in &mut self.tabs {tab.file_save.suspended=suspended;}
    }
}

impl EditorApp {
    pub(super) fn clear_auto_save_warning(&mut self) {
        if let Some(id)=self.file_save.warning.take() {self.banners.retain(|b|b.id!=id);}
    }
    fn apply_file_save_mode(&mut self,mode:Mode) {self.clear_auto_save_warning();self.file_save.configure(mode);}
    pub(super) fn set_file_save_mode(&mut self,mode:Mode) {
        self.apply_file_save_mode(mode);self.file_save.changed=true;
        let result=self.config_path.as_ref().ok_or_else(||io::Error::other("The settings folder is unavailable"))
            .and_then(|path|mode.save(&path.with_file_name("working-copy-autosave.txt")));
        if let Err(error)=result {self.banner(BannerKind::Error,format!("Auto-save setting could not be saved: {error}"));}
    }
    pub(super) fn working_auto_save_settings(&mut self,ui:&mut Ui,p:&Palette) {
        section_title(ui,"Auto-save");
        let mut mode=self.file_save.mode;
        widgets::setting_row(ui,p,"Working copy","Saves applied edits to your working copy. Originals stay unchanged.",|ui| {
            egui::ComboBox::from_id_salt("working-copy-autosave").width(240.).selected_text(mode.label()).show_ui(ui,|ui| {
                for value in Mode::ALL {ui.selectable_value(&mut mode,value,value.label());}
            });
        });
        if mode!=self.file_save.mode {self.set_file_save_mode(mode);}
        if mode!=Mode::Off {
            let status=if self.file_save.paused {Some("Auto-save paused. Save manually or use Save As.")}
                else if self.file_save.in_flight.is_some() {Some("Saving working copy...")}
                else if self.qol.working_copy.is_none() {Some("Use Save As once to create a working copy.")}
                else {None};
            if let Some(status)=status {ui.label(RichText::new(status).small().color(p.muted));}
        }
        ui.add_space(20.);
    }
    pub(super) fn working_auto_save_tick(&mut self,ctx:&Context) {
        if self.file_save.mode==Mode::Off || self.file_save.paused || self.file_save.in_flight.is_some() {return;}
        let Some(doc)=&self.doc else {return;};
        if !doc.dirty() || self.qol.working_copy.is_none() {self.file_save.schedule_reset();return;}
        let generation=doc.generation;
        let now=ctx.input(|i|i.time);
        let Some(due)=self.file_save.observe(generation,now) else {return;};
        if now<due {ctx.request_repaint_after(std::time::Duration::from_secs_f64(due-now));return;}
        if !self.engine_alive || self.opening || self.busy>0 || self.pending_action.is_some() || self.confirm.is_some()
            || self.file_save.suspended || self.forms.has_drafts() || self.picker.pending() || doc.copying || doc.applying.is_some() || doc.pending_edit.is_some()
            || doc.dragging_item.is_some() || doc.edit.as_ref().is_some_and(|edit|edit.changed())
            || ctx.input(|i|i.pointer.any_down() || i.pointer.any_pressed() || i.pointer.any_released() || i.viewport().close_requested()) {return;}
        if self.qol.working_copy.as_ref().is_some_and(|path|self.recovery_destination(path)) {self.file_save.paused=true;return;}
        let Some(lease)=Lease::acquire() else {ctx.request_repaint_after(std::time::Duration::from_millis(250));return;};
        self.file_save.lease=Some(lease);self.file_save.in_flight=Some(generation);
        self.send(Request::AutoSaveWorkingCopy {generation});
    }
    pub(super) fn working_copy_saved(&mut self,path:&std::path::Path,encrypted:bool) {
        if !encrypted && !self.qol.original_protected && let (Some(config),Some(doc))=(&self.config_path,&self.doc) {
            let source=self.compare_origin.as_ref().unwrap_or(&doc.source);
            if !source.as_os_str().is_empty() && crate::comparison_origin::remember(config,source,path).is_err() {
                self.banner(BannerKind::Warning,"Saved. The original-file link could not be remembered.");
            }
        }
        self.qol.working_copy=Some(path.to_owned());self.qol.copy_encrypted=encrypted;
        if let Some(doc)=&mut self.doc {
            doc.protected=self.qol.original_protected||encrypted;
            doc.name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        }
    }
    pub(super) fn saved_current_document(&mut self) {
        self.clear_auto_save_warning();self.file_save.paused=false;self.file_save.schedule_reset();
        if let Some(doc)=&mut self.doc {
            doc.changes.mark_saved();doc.copied_generation=None;doc.next_copy=None;
            if let Some(orphan)=doc.restored_from.take() {orphan.discard();}
        }
        if let Some(session)=&mut self.session {session.clear();}
    }
    pub(super) fn auto_saved(&mut self,generation:u64,result:Result<Option<(PathBuf,bool)>,String>) {
        if self.file_save.in_flight!=Some(generation) {return;}
        self.file_save.finish();self.file_save.schedule_reset();
        match result {
            Ok(Some((path,encrypted)))=>{
                self.working_copy_saved(&path,encrypted);
                if self.doc.as_ref().is_some_and(|doc|doc.generation==generation) {self.saved_current_document();}
                else if let Some(doc)=&mut self.doc {
                    // The file now represents an earlier version; keep later edits dirty.
                    doc.changes=pdf_editor_core::ChangeState::recovered();
                }
            }
            Ok(None)=>{},
            Err(error)=>{
                self.file_save.paused=true;
                self.banner(BannerKind::Warning,format!("Auto-save paused: {error}"));
                self.file_save.warning=self.banners.last().map(|b|b.id);
            }
        }
    }
}
