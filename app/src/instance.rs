//! Per-user local mailbox. An OS-held lock releases automatically after a crash.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 65536;
static LAST_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    pub action: String,
    pub path: PathBuf,
}
pub struct Instance {
    _lock: File,
    folder: PathBuf,
    started: u128,
}
pub enum Start {
    Owner(Instance),
    Forwarded,
}

fn encode(request: &Incoming) -> io::Result<Vec<u8>> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if !matches!(request.path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            || request
                .path
                .components()
                .any(|c| matches!(c, Component::Normal(s) if s.to_string_lossy().contains(':')))
        {
            return Err(io::Error::other("Only local disk PDF paths are accepted"));
        }
    }
    if !matches!(
        request.action.as_str(),
        "open" | "merge" | "compress" | "export"
    ) || !request.path.is_absolute()
        || request
            .path
            .extension()
            .is_none_or(|s| !s.eq_ignore_ascii_case("pdf"))
    {
        return Err(io::Error::other(
            "Expected an absolute PDF path and known action",
        ));
    }
    let path = request
        .path
        .to_str()
        .ok_or_else(|| io::Error::other("Path is not Unicode"))?;
    if path.chars().any(|c| c.is_control()) {
        return Err(io::Error::other("Invalid path"));
    }
    let bytes = format!("{}\n{}", request.action, path).into_bytes();
    if bytes.len() > LIMIT {
        return Err(io::Error::other("Path is too long"));
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> io::Result<Incoming> {
    if bytes.len() > LIMIT {
        return Err(io::Error::other("Request too large"));
    }
    let value = std::str::from_utf8(bytes).map_err(io::Error::other)?;
    let (action, path) = value
        .split_once('\n')
        .ok_or_else(|| io::Error::other("Invalid request"))?;
    let request = Incoming {
        action: action.into(),
        path: path.into(),
    };
    encode(&request)?;
    Ok(request)
}
fn redirected(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
fn no_redirects(path: &Path) -> io::Result<()> {
    for part in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(part)
            && redirected(&meta)
        {
            return Err(io::Error::other(
                "Mailbox path must not use filesystem redirects",
            ));
        }
    }
    Ok(())
}
fn open_local(path: &Path, create: bool) -> io::Result<File> {
    no_redirects(path)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(create)
        .create(create)
        .truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path)?;
    if redirected(&file.metadata()?) {
        return Err(io::Error::other("Mailbox file must not be a link"));
    }
    Ok(file)
}
fn request_timestamp(path: &Path) -> Option<u128> {
    let (pid, stamp) = path.file_stem()?.to_str()?.split_once('-')?;
    pid.parse::<u32>().ok()?;
    stamp.parse().ok()
}
impl Instance {
    pub fn start(request: Option<&Incoming>) -> io::Result<Start> {
        let root = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::other("Local settings folder unavailable"))?;
        Self::at(&PathBuf::from(root).join("PdfEditor/Instance"), request)
    }
    fn at(folder: &Path, request: Option<&Incoming>) -> io::Result<Start> {
        if let Some(request) = request {
            encode(request)?;
        }
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        no_redirects(folder)?;
        fs::create_dir_all(folder)?;
        let lock = open_local(&folder.join("owner.lock"), true)?;
        match lock.try_lock() {
            Ok(()) => {
                let owner = Self {
                    _lock: lock,
                    folder: folder.into(),
                    started,
                };
                owner.purge_stale_temporary();
                Ok(Start::Owner(owner))
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                if let Some(request) = request {
                    Self::send(folder, request)?;
                }
                Ok(Start::Forwarded)
            }
            Err(std::fs::TryLockError::Error(e)) => Err(e),
        }
    }
    fn send(folder: &Path, request: &Incoming) -> io::Result<()> {
        if fs::read_dir(folder)?.take(129).count() > 128 {
            return Err(io::Error::other("Open request queue is full"));
        }
        let data = encode(request)?;
        let now = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos(),
        )
        .map_err(io::Error::other)?;
        // Windows clock resolution permits identical timestamps across threads.
        use std::sync::atomic::Ordering::Relaxed;
        let mut previous = LAST_NONCE.load(Relaxed);
        let nonce = loop {
            let next = now.max(previous.saturating_add(1));
            match LAST_NONCE.compare_exchange_weak(previous, next, Relaxed, Relaxed) {
                Ok(_) => break next,
                Err(actual) => previous = actual,
            }
        };
        let temp = folder.join(format!("{}-{nonce}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&data)?;
        drop(file);
        fs::rename(&temp, temp.with_extension("req"))
    }
    pub fn poll(&self) -> Vec<Incoming> {
        self.purge_stale_temporary();
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return vec![];
        };
        entries
            .take(256)
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().is_none_or(|s| s != "req") {
                    return None;
                }
                let result = (|| {
                    let timestamp = request_timestamp(&path).unwrap_or(0);
                    if timestamp < self.started {
                        return Err(io::Error::other("Stale request"));
                    }
                    let meta = fs::symlink_metadata(&path)?;
                    if !meta.is_file() || redirected(&meta) || meta.len() > LIMIT as u64 {
                        return Err(io::Error::other("Invalid mailbox file"));
                    }
                    let mut bytes = Vec::new();
                    open_local(&path, false)?
                        .take((LIMIT + 1) as u64)
                        .read_to_end(&mut bytes)?;
                    decode(&bytes)
                })();
                let _ = fs::remove_file(path);
                result.ok()
            })
            .take(32)
            .collect()
    }
    fn purge_stale_temporary(&self) {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return;
        };
        for entry in entries.take(256).flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|s| s == "tmp")
                && request_timestamp(&path).is_some_and(|t| t < self.started)
                && fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !redirected(&m))
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_messages_without_opening_documents() {
        let request = Incoming {
            action: "open".into(),
            path: std::env::temp_dir().join("sample.pdf"),
        };
        assert_eq!(decode(&encode(&request).unwrap()).unwrap(), request);
        assert!(decode(b"open\nrelative.pdf").is_err());
        assert!(decode(b"execute\nC:\\sample.pdf").is_err());
        assert!(decode(&vec![b'a'; LIMIT + 1]).is_err());
    }
    #[test]
    fn one_owner_forwards_and_releases_lock() {
        let root = std::env::temp_dir().join(format!(
            "pdf-instance-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let Start::Owner(owner) = Instance::at(&root, None).unwrap() else {
            panic!("owner")
        };
        let request = Incoming {
            action: "open".into(),
            path: root.join("generated.pdf"),
        };
        assert!(matches!(
            Instance::at(&root, Some(&request)).unwrap(),
            Start::Forwarded
        ));
        assert_eq!(owner.poll(), vec![request]);
        assert!(owner.poll().is_empty());
        drop(owner);
        assert!(matches!(
            Instance::at(&root, None).unwrap(),
            Start::Owner(_)
        ));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stale_and_oversized_requests_are_discarded() {
        let root = std::env::temp_dir().join(format!("pdf-stale-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("1-1.req"), b"open\nC:\\stale.pdf").unwrap();
        let Start::Owner(owner) = Instance::at(&root, None).unwrap() else {
            panic!("owner")
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        fs::write(
            root.join(format!("1-{timestamp}.req")),
            vec![b'a'; LIMIT + 1],
        )
        .unwrap();
        assert!(owner.poll().is_empty());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert!(root.join("owner.lock").exists());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn concurrent_secondaries_preserve_the_owner() {
        let root = std::env::temp_dir().join(format!("pdf-race-{}", std::process::id()));
        let Start::Owner(owner) = Instance::at(&root, None).unwrap() else {
            panic!("owner")
        };
        std::thread::scope(|scope| {
            for index in 0..8 {
                let folder = &root;
                scope.spawn(move || {
                    let request = Incoming {
                        action: "open".into(),
                        path: folder.join(format!("generated-{index}.pdf")),
                    };
                    assert!(matches!(
                        Instance::at(folder, Some(&request)).unwrap(),
                        Start::Forwarded
                    ));
                });
            }
        });
        assert_eq!(owner.poll().len(), 8);
        assert!(matches!(
            Instance::at(&root, None).unwrap(),
            Start::Forwarded
        ));
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stale_temp_cleanup_preserves_active_and_unrecognized_files() {
        let root = std::env::temp_dir().join(format!("pdf-temp-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("123-1.tmp"), b"partial").unwrap();
        fs::write(root.join("unrecognized.tmp"), b"keep").unwrap();
        let Start::Owner(owner) = Instance::at(&root, None).unwrap() else {
            panic!("owner")
        };
        assert!(!root.join("123-1.tmp").exists());
        let active = root.join(format!("123-{}.tmp", owner.started + 1));
        fs::write(&active, b"in progress").unwrap();
        fs::write(root.join("123-2.tmp"), b"old partial").unwrap();
        assert!(owner.poll().is_empty());
        assert!(!root.join("123-2.tmp").exists());
        assert!(active.exists());
        assert!(root.join("unrecognized.tmp").exists());
        assert!(root.join("owner.lock").exists());
        drop(owner);
        fs::remove_dir_all(root).unwrap();
    }
}
