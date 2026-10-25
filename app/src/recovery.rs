//! Auto-save as recovery copies.
//!
//! Unsaved edits are written to a private per-session folder under
//! `%LOCALAPPDATA%\PdfEditor\Recovery`, never to the original or a saved copy.
//! Each running editor holds an exclusive lock file in its folder, so a later
//! start only offers folders whose editor is no longer running.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const LOCK: &str = "session.lock";
const SOURCE: &str = "source.txt";
const PREFIX: &str = "snapshot-";

/// Auto-save interval in minutes, or None when auto-save is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoSave(pub Option<u32>);

pub const INTERVALS: [u32; 4] = [1, 2, 5, 10];

impl Default for AutoSave {
    fn default() -> Self {
        Self(Some(2))
    }
}

impl AutoSave {
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("PdfEditor/app-settings.txt"))
    }

    /// Reads `autosave=off` or `autosave=<minutes>`; anything else is the default.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) if text.len() <= 1024 => text,
            Ok(_) => return Err(io::Error::other("settings file is too large")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        for line in text.lines() {
            match line.trim().split_once('=') {
                Some(("autosave", "off")) => return Ok(Self(None)),
                Some(("autosave", minutes)) => {
                    if let Ok(m) = minutes.parse::<u32>()
                        && INTERVALS.contains(&m)
                    {
                        return Ok(Self(Some(m)));
                    }
                }
                _ => {}
            }
        }
        Ok(Self::default())
    }

    pub fn save(self, path: &Path) -> io::Result<()> {
        let value = self.0.map_or("off".to_string(), |m| m.to_string());
        write_replacing(path, format!("autosave={value}\n").as_bytes())
    }
}

/// Writes through a uniquely named temporary file, then renames it over `path`.
pub(crate) fn write_replacing(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid settings path"))?;
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
        std::process::id(),
        nanos()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}

pub fn root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("PdfEditor/Recovery"))
}

/// Resolve the existing parent so relative paths and directory aliases cannot
/// direct an output into storage reserved for automatic recovery cleanup.
pub(crate) fn contains_output(dir: &Path, output: &Path) -> bool {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (Ok(dir), Ok(parent)) = (dir.canonicalize(), parent.canonicalize()) else {
        return false;
    };
    #[cfg(windows)]
    {
        let mut components = parent.components();
        dir.components().all(|part| {
            components.next().is_some_and(|candidate| {
                part.as_os_str()
                    .as_encoded_bytes()
                    .eq_ignore_ascii_case(candidate.as_os_str().as_encoded_bytes())
            })
        })
    }
    #[cfg(not(windows))]
    parent.starts_with(dir)
}

fn numbered(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit())
}

fn snapshot_file(name: &str, extension: &str) -> bool {
    name.strip_prefix(PREFIX)
        .and_then(|s| s.strip_suffix(extension))
        .is_some_and(numbered)
}

fn owned_artifact(name: &str) -> bool {
    if name == LOCK
        || name == SOURCE
        || snapshot_file(name, ".pdf")
        || snapshot_file(name, ".history")
    {
        return true;
    }
    let pair = |value: &str, separator| {
        value
            .split_once(separator)
            .is_some_and(|(a, b)| numbered(a) && numbered(b))
    };
    name.strip_prefix(".pdf-editor-save-")
        .and_then(|s| s.strip_suffix(".tmp"))
        .is_some_and(|s| pair(s, '-'))
        || name
            .strip_prefix(".source.txt.")
            .and_then(|s| s.strip_suffix(".tmp"))
            .is_some_and(|s| pair(s, '.'))
}

/// Delete only the reserved runtime namespace. A saved file or unknown
/// subdirectory must never become collateral damage of recovery cleanup.
fn cleanup(dir: &Path) {
    let Ok(metadata) = fs::symlink_metadata(dir) else {
        return;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return;
        }
    }
    if dir.join(LOCK).exists() && open_lock(&dir.join(LOCK), false).is_err() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_str().is_some_and(owned_artifact) {
            let _ = fs::remove_file(entry.path());
        }
    }
    // Nonempty directories contain unrecognized entries and are retained.
    let _ = fs::remove_dir(dir);
}

/// Opens the session lock so no other process can open it while held.
fn open_lock(path: &Path, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create {
        options.create_new(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(path)
}

/// This editor's recovery files, cleaned up on a normal exit.
pub struct Session {
    dir: PathBuf,
    _lock: File,
    counter: u64,
    current: Option<PathBuf>,
}

impl Session {
    pub fn start() -> io::Result<Self> {
        let root = root().ok_or_else(|| io::Error::other("LOCALAPPDATA is not available"))?;
        Self::start_in(&root)
    }

    pub(crate) fn start_in(root: &Path) -> io::Result<Self> {
        let dir = root.join(format!("{}-{}", std::process::id(), nanos()));
        fs::create_dir_all(&dir)?;
        let lock = open_lock(&dir.join(LOCK), true)?;
        Ok(Self {
            dir,
            _lock: lock,
            counter: 0,
            current: None,
        })
    }

    /// A fresh path for the next copy; the core refuses existing files.
    pub fn next_path(&mut self) -> PathBuf {
        self.counter += 1;
        self.dir.join(format!("{PREFIX}{}.pdf", self.counter))
    }

    /// The last complete copy owned by this session.
    pub(crate) fn current_path(&self) -> Option<&Path> {
        self.current.as_deref()
    }

    pub(crate) fn contains_output(&self, output: &Path) -> bool {
        contains_output(&self.dir, output)
    }

    /// Records a written copy of `source` and removes the previous one.
    pub fn written(&mut self, copy: PathBuf, source: &Path) -> io::Result<()> {
        write_replacing(&self.dir.join(SOURCE), source.to_string_lossy().as_bytes())?;
        if let Some(previous) = self.current.replace(copy)
            && Some(&previous) != self.current.as_ref()
        {
            let _ = fs::remove_file(previous.with_extension("history"));
            let _ = fs::remove_file(previous);
        }
        Ok(())
    }

    /// Removes the copy, for example after a save or a discard.
    pub fn clear(&mut self) {
        if let Some(copy) = self.current.take() {
            let _ = fs::remove_file(copy.with_extension("history"));
            let _ = fs::remove_file(copy);
        }
        let _ = fs::remove_file(self.dir.join(SOURCE));
    }

    /// Removes owned recovery artifacts on normal exit. Unknown entries remain.
    pub fn close(self) {
        let Self { dir, _lock, .. } = self;
        drop(_lock);
        cleanup(&dir);
    }
}

/// A copy left by an editor that did not exit normally.
pub struct Orphan {
    pub dir: PathBuf,
    pub copy: PathBuf,
    pub source: Option<PathBuf>,
    pub modified: SystemTime,
}

impl Orphan {
    pub fn name(&self) -> String {
        self.source
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled.pdf".into())
    }

    pub fn discard(&self) {
        cleanup(&self.dir);
    }

    pub(crate) fn contains_output(&self, output: &Path) -> bool {
        contains_output(&self.dir, output)
    }
}

/// Finds copies whose editor is no longer running, newest first. Folders of
/// finished sessions without a copy are removed.
pub fn orphans() -> Vec<Orphan> {
    let Some(root) = root() else { return vec![] };
    orphans_in(&root)
}

pub(crate) fn orphans_in(root: &Path) -> Vec<Orphan> {
    let Ok(entries) = fs::read_dir(root) else {
        return vec![];
    };
    let mut found = vec![];
    for dir in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let lock = dir.join(LOCK);
        if lock.exists() && open_lock(&lock, false).is_err() {
            continue; // Another editor is running with this folder.
        }
        let copy = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| snapshot_file(n, ".pdf"))
            })
            .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
            .max();
        match copy {
            Some((modified, copy)) => {
                let source = fs::read_to_string(dir.join(SOURCE))
                    .ok()
                    .filter(|s| s.len() < 4096 && !s.trim().is_empty())
                    .map(|s| PathBuf::from(s.trim()));
                found.push(Orphan {
                    dir,
                    copy,
                    source,
                    modified,
                });
            }
            None => {
                cleanup(&dir);
            }
        }
    }
    found.sort_by_key(|o| std::cmp::Reverse(o.modified));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_recognizes_only_reserved_runtime_artifacts() {
        for name in [
            "session.lock",
            "source.txt",
            "snapshot-1.pdf",
            "snapshot-12.history",
            ".pdf-editor-save-123-4.tmp",
            ".source.txt.123.456.tmp",
        ] {
            assert!(owned_artifact(name), "{name}");
        }
        for name in [
            "snapshot-copy.pdf",
            "snapshot-.pdf",
            "snapshot-1_edited.pdf",
            "saved.pdf",
            "snapshot-2.history.backup",
            ".pdf-editor-save-notes.tmp",
            ".source.txt.backup.tmp",
            "source.txt.backup",
            "keep",
        ] {
            assert!(!owned_artifact(name), "{name}");
        }
    }

    #[test]
    fn output_boundaries_resolve_parents_and_do_not_match_sibling_prefixes() {
        let root = std::env::temp_dir().join(format!("pdf-recovery-boundary-{}", nanos()));
        let private = root.join("session");
        let nested = private.join("nested");
        let sibling = root.join("session-other");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(&sibling).unwrap();
        assert!(contains_output(&private, &private.join("new.pdf")));
        assert!(contains_output(&private, &nested.join("../new.pdf")));
        assert!(contains_output(&private, &nested.join("new.pdf")));
        assert!(!contains_output(&private, &sibling.join("new.pdf")));
        assert!(!contains_output(&private, &root.join("new.pdf")));
        #[cfg(windows)]
        assert!(contains_output(
            &private,
            &PathBuf::from(nested.to_string_lossy().to_uppercase()).join("new.pdf")
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_cannot_discard_a_session_with_a_held_lock() {
        let root = std::env::temp_dir().join(format!("pdf-recovery-active-{}", nanos()));
        let mut session = Session::start_in(&root).unwrap();
        let snapshot = session.next_path();
        fs::write(&snapshot, b"generated snapshot").unwrap();
        session
            .written(snapshot.clone(), Path::new("generated.pdf"))
            .unwrap();
        cleanup(snapshot.parent().unwrap());
        assert!(snapshot.exists());
        assert!(orphans_in(&root).is_empty());
        session.close();
        assert!(!snapshot.exists());
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn interrupted_snapshot_keeps_the_previous_complete_recovery_copy() {
        let root = std::env::temp_dir().join(format!("pdf-recovery-interrupted-{}", nanos()));
        let mut session = Session::start_in(&root).unwrap();
        let copy = session.next_path();
        fs::write(&copy, b"generated complete snapshot").unwrap();
        session
            .written(copy.clone(), Path::new("generated.pdf"))
            .unwrap();
        fs::write(session.dir.join(".pdf-editor-save-123-0.tmp"), b"partial").unwrap();
        drop(session);
        let found = orphans_in(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].copy, copy);
        assert_eq!(
            fs::read(&found[0].copy).unwrap(),
            b"generated complete snapshot"
        );
        found[0].discard();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn autosave_setting_round_trips_and_ignores_garbage() {
        let dir = std::env::temp_dir().join(format!("pdf-editor-autosave-{}", nanos()));
        let path = dir.join("app-settings.txt");
        assert_eq!(AutoSave::load(&path).unwrap(), AutoSave::default());
        AutoSave(None).save(&path).unwrap();
        assert_eq!(AutoSave::load(&path).unwrap(), AutoSave(None));
        AutoSave(Some(5)).save(&path).unwrap();
        assert_eq!(AutoSave::load(&path).unwrap(), AutoSave(Some(5)));
        fs::write(&path, "autosave=7\n").unwrap();
        assert_eq!(AutoSave::load(&path).unwrap(), AutoSave::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn held_lock_hides_a_running_session() {
        let dir = std::env::temp_dir().join(format!("pdf-editor-lock-{}", nanos()));
        fs::create_dir_all(&dir).unwrap();
        let lock = open_lock(&dir.join(LOCK), true).unwrap();
        assert!(open_lock(&dir.join(LOCK), false).is_err());
        drop(lock);
        assert!(open_lock(&dir.join(LOCK), false).is_ok());
        let _ = fs::remove_dir_all(dir);
    }
}
