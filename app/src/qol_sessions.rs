//! Optional local tab continuity and bounded recently closed tabs.
use super::*;
use crate::session_state::{SessionState, SessionTab};
use std::{
    fs,
    io::{self, Read},
    path::Path,
};

const CLOSED_LIMIT: usize = 20;
const TAB_LIMIT: usize = 32;

#[derive(Default)]
pub(super) struct Sessions {
    enabled: bool,
    preference: Option<PathBuf>,
    state: Option<PathBuf>,
    closed: Vec<SessionTab>,
    last_saved: Option<SessionState>,
    next_save: f64,
    error: Option<String>,
}

impl Sessions {
    /// Uses only a caller-supplied existing settings location.
    pub(super) fn new(config: Option<&Path>) -> Self {
        let Some(config) = config else {
            return Self::default();
        };
        let preference = config.with_file_name("session-settings.txt");
        let state = config.with_file_name("last-session.bin");
        let result = read_preference(&preference);
        Self {
            enabled: result.as_ref().copied().unwrap_or(false),
            preference: Some(preference),
            state: Some(state),
            error: result
                .err()
                .map(|error| format!("Session settings could not be read: {error}")),
            ..Default::default()
        }
    }

    fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        self.enabled = enabled;
        self.last_saved = None;
        self.next_save = 0.0;
        let path = self
            .preference
            .as_deref()
            .ok_or_else(|| io::Error::other("The settings folder is unavailable"))?;
        recovery::write_replacing(
            path,
            if enabled {
                b"remember-session=on\n"
            } else {
                b"remember-session=off\n"
            },
        )?;
        if !enabled && let Some(path) = &self.state {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn push_closed(&mut self, tab: SessionTab) {
        if self.closed.len() == CLOSED_LIMIT {
            self.closed.remove(0);
        }
        self.closed.push(tab);
    }
}

fn read_preference(path: &Path) -> io::Result<bool> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.take(129).read_to_string(&mut text)?;
    if text.len() > 128 {
        return Err(io::Error::other("Session settings exceed limits"));
    }
    match text.trim() {
        "remember-session=on" => Ok(true),
        "remember-session=off" => Ok(false),
        _ => Err(io::Error::other("Invalid session preference")),
    }
}

pub(super) fn capture_tab(tab: &EditorApp, recovery: bool) -> Option<SessionTab> {
    let doc = tab.doc.as_ref()?;
    if doc.protected || tab.qol.original_protected || tab.qol.copy_encrypted {
        return None;
    }
    let source = tab
        .qol
        .working_copy
        .clone()
        .unwrap_or_else(|| doc.source.clone());
    let recovery = if recovery && doc.dirty() {
        tab.session
            .as_ref()
            .and_then(Session::current_path)
            .map(Path::to_path_buf)
            .or_else(|| doc.restored_from.as_ref().map(|orphan| orphan.copy.clone()))
    } else {
        None
    };
    let source=if source.as_os_str().is_empty() {recovery.clone()?}else{source};
    Some(SessionTab {
        source,
        recovery,
        current_page: doc.current_page.min(9999) as u16,
        zoom: doc.zoom,
        scroll: doc.offset.y.max(0.0),
    })
}

fn visible_state(
    tabs: impl IntoIterator<Item = Option<SessionTab>>,
    active: usize,
) -> SessionState {
    let mut state = SessionState::default();
    for (index, tab) in tabs.into_iter().enumerate() {
        if let Some(tab) = tab {
            if index <= active {
                state.active = state.tabs.len();
            }
            state.tabs.push(tab);
        }
    }
    state
}

#[derive(Debug, PartialEq, Eq)]
enum TabCommand {
    Previous,
    Next,
    New,
    Close,
    Reopen,
}
fn tab_command(ctx: &Context) -> Option<TabCommand> {
    let ctrl = egui::Modifiers::COMMAND;
    let shift = ctrl | egui::Modifiers::SHIFT;
    for (modifiers, key, command) in [
        (shift, Key::T, TabCommand::Reopen),
        (shift, Key::Tab, TabCommand::Previous),
        (ctrl, Key::Tab, TabCommand::Next),
        (ctrl, Key::T, TabCommand::New),
        (ctrl, Key::W, TabCommand::Close),
    ] {
        if ctx
            .input_mut(|input| input.consume_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
        {
            return Some(command);
        }
    }
    None
}

impl TabbedApp {
    /// Call once after construction; explicit launch paths take precedence over restore.
    pub(super) fn sessions_start(&mut self, ctx: &Context, restore: bool) {
        for (_, tab) in &mut self.tabs {
            tab.qol.remember_session = self.sessions.enabled;
        }
        if let Some(error) = self.sessions.error.take() {
            self.tabs[self.active].1.banner(BannerKind::Warning, error);
        }
        if !restore || !self.sessions.enabled {
            return;
        }
        let Some(path) = self.sessions.state.as_deref() else {
            return;
        };
        match SessionState::load(path) {
            Ok(state) => {
                let selected = state.active;
                let mut active = self.active;
                for (index, tab) in state.tabs.into_iter().take(TAB_LIMIT).enumerate() {
                    if self.sessions_open(ctx, tab) && index == selected {
                        active = self.active;
                    }
                }
                self.active = active.min(self.tabs.len().saturating_sub(1));
            }
            Err(error) => self.tabs[self.active].1.banner(
                BannerKind::Warning,
                format!("The previous session could not be restored: {error}"),
            ),
        }
    }

    fn sessions_open(&mut self, ctx: &Context, entry: SessionTab) -> bool {
        let reuse = self.tabs.len() == 1
            && self.tabs[0].1.doc.is_none()
            && !self.tabs[0].1.opening
            && self.tabs[0].1.busy == 0;
        if !reuse && self.tabs.len() >= TAB_LIMIT {
            self.tabs[self.active].1.banner(
                BannerKind::Warning,
                "Close a tab before reopening more documents (32-tab limit).",
            );
            return false;
        }
        if !entry.source.is_file() && !entry.recovery.as_ref().is_some_and(|path| path.is_file()) {
            self.tabs[self.active].1.banner(
                BannerKind::Warning,
                "The closed document is no longer available at its saved location.",
            );
            return false;
        }
        // Only retain deletion ownership from an already discovered recovery orphan.
        let orphan = entry.recovery.as_ref().and_then(|path| {
            self.tabs.iter_mut().find_map(|(_, tab)| {
                let index = tab.orphans.iter().position(|orphan| &orphan.copy == path)?;
                Some(tab.orphans.remove(index))
            })
        });
        if !reuse {
            self.add(ctx, None);
        } else {
            self.active = 0;
        }
        let tab = &mut self.tabs[self.active].1;
        if tab.setup_error.is_some() {
            return false;
        }
        tab.qol.remember_session = self.sessions.enabled;
        tab.qol.pending_view = Some(entry.clone());
        tab.title.clear();
        if let Some(path) = entry.recovery {
            tab.restoring = orphan;
            tab.opening = true;
            tab.busy += 1;
            tab.send(Request::OpenRecovery {
                path,
                source: entry.source,
            });
        } else {
            tab.open_path(entry.source);
        }
        true
    }

    pub(super) fn sessions_shortcuts(&mut self, ctx: &Context) {
        if self.tabs.is_empty()
            || self.closing.is_some()
            || self.tabs[self.active].1.confirm.is_some()
        {
            return;
        }
        match tab_command(ctx) {
            Some(command @ (TabCommand::Previous | TabCommand::Next)) => {
                let backwards = command == TabCommand::Previous;
                self.tabs[self.active].1.commit_edit();
                self.active = if backwards {
                    (self.active + self.tabs.len() - 1) % self.tabs.len()
                } else {
                    (self.active + 1) % self.tabs.len()
                };
                self.tabs[self.active].1.title.clear();
            }
            Some(TabCommand::New) => {
                self.tabs[self.active].1.commit_edit();
                self.add(ctx, None);
            }
            Some(TabCommand::Close) => {
                let (id, tab) = &self.tabs[self.active];
                let id = *id;
                if Self::needs_warning(tab) {
                    self.closing = Some(Some(id));
                } else {
                    self.remove(id, ctx);
                }
            }
            Some(TabCommand::Reopen) => {
                if self.tabs.len() >= TAB_LIMIT {
                    self.tabs[self.active].1.banner(
                        BannerKind::Warning,
                        "Close a tab before reopening more documents (32-tab limit).",
                    );
                } else if let Some(entry) = self.sessions.closed.pop() {
                    self.tabs[self.active].1.commit_edit();
                    self.sessions_open(ctx, entry);
                }
            }
            None => {}
        }
    }

    /// Call only when a close has actually been accepted, before removing the tab.
    pub(super) fn sessions_remember_closed(&mut self, id: u64) {
        if let Some(entry) = self
            .tabs
            .iter()
            .find(|(key, _)| *key == id)
            .and_then(|(_, tab)| capture_tab(tab, false))
        {
            self.sessions.push_closed(entry);
        }
    }

    pub(super) fn sessions_tick(&mut self, ctx: &Context) {
        let changed = self.tabs.iter_mut().find_map(|(_, tab)| {
            if std::mem::take(&mut tab.qol.remember_changed) {
                Some(tab.qol.remember_session)
            } else {
                None
            }
        });
        if let Some(enabled) = changed
            && let Err(error) = self.sessions.set_enabled(enabled) {
                self.tabs[self.active].1.banner(
                    BannerKind::Warning,
                    format!("Session settings could not be saved: {error}"),
                );
        }
        for (_, tab) in &mut self.tabs {
            tab.qol.remember_session = self.sessions.enabled;
        }
        if !self.sessions.enabled {
            return;
        }
        let now = ctx.input(|input| input.time);
        if now >= self.sessions.next_save {
            self.sessions.next_save = now + 2.0;
            self.sessions_flush();
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }

    /// Call before tab recovery folders are closed on normal application exit.
    pub(super) fn sessions_flush(&mut self) {
        if !self.sessions.enabled || self.tabs.iter().any(|(_, tab)| tab.opening) {
            return;
        }
        let state = visible_state(
            self.tabs.iter().map(|(_, tab)| capture_tab(tab, true)),
            self.active,
        );
        if self.sessions.last_saved.as_ref() == Some(&state) {
            return;
        }
        let Some(path) = self.sessions.state.as_deref() else {
            return;
        };
        match state.save(path) {
            Ok(()) => self.sessions.last_saved = Some(state),
            Err(error) => self.tabs[self.active].1.banner(
                BannerKind::Warning,
                format!("Open tabs could not be remembered: {error}"),
            ),
        }
    }
}

impl EditorApp {
    /// Call after a successful Opened reply has installed the new DocView.
    pub(super) fn sessions_apply_view(&mut self) {
        let Some(entry) = self.qol.pending_view.take() else {
            return;
        };
        let Some(doc) = &mut self.doc else { return };
        doc.zoom = entry.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        doc.current_page = usize::from(entry.current_page).min(doc.pages.len().saturating_sub(1));
        doc.explicit_page = Some(doc.current_page);
        doc.offset = vec2(0.0, entry.scroll.max(0.0));
        doc.scroll_to = Some(doc.offset);
        if entry.recovery.is_some() {
            doc.changes = pdf_editor_core::ChangeState::recovered();
            doc.next_copy = Some(0.0);
        }
        self.tools_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name: &str) -> SessionTab {
        SessionTab {
            source: PathBuf::from(name),
            recovery: None,
            current_page: 2,
            zoom: 1.25,
            scroll: 300.0,
        }
    }
    #[test]
    fn omitted_tabs_keep_a_valid_active_index_and_closed_history_is_bounded() {
        let state = visible_state([Some(entry("one.pdf")), None, Some(entry("three.pdf"))], 1);
        assert_eq!(state.active, 0);
        assert_eq!(state.tabs.len(), 2);
        assert_eq!(visible_state([None], 0), SessionState::default());
        let mut sessions = Sessions::default();
        for index in 0..CLOSED_LIMIT + 3 {
            sessions.push_closed(entry(&format!("{index}.pdf")));
        }
        assert_eq!(sessions.closed.len(), CLOSED_LIMIT);
        assert_eq!(
            sessions.closed.pop().unwrap().source,
            PathBuf::from("22.pdf")
        );
        assert_eq!(
            sessions.closed.first().unwrap().source,
            PathBuf::from("3.pdf")
        );
    }
    #[test]
    fn opt_in_preference_round_trips_and_opt_out_removes_saved_tabs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/test-output/session-preference");
        fs::create_dir_all(&root).unwrap();
        let config = root.join("settings.txt");
        let mut sessions = Sessions::new(Some(&config));
        sessions.set_enabled(false).unwrap();
        assert!(!Sessions::new(Some(&config)).enabled);
        sessions.set_enabled(true).unwrap();
        assert!(Sessions::new(Some(&config)).enabled);
        let state = sessions.state.as_ref().unwrap();
        SessionState::default().save(state).unwrap();
        sessions.set_enabled(false).unwrap();
        assert!(!sessions.state.as_ref().unwrap().exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn tab_shortcuts_distinguish_shifted_commands() {
        for (key, shift, expected) in [
            (Key::Tab, false, TabCommand::Next),
            (Key::Tab, true, TabCommand::Previous),
            (Key::T, false, TabCommand::New),
            (Key::T, true, TabCommand::Reopen),
            (Key::W, false, TabCommand::Close),
        ] {
            let ctx = Context::default();
            let modifiers = egui::Modifiers::COMMAND
                | if shift {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                };
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            };
            let mut command = None;
            let mut output = ctx.run_ui(input, |ui| command = tab_command(ui.ctx()));
            output.textures_delta.clear();
            assert_eq!(command, Some(expected));
        }
    }
}
