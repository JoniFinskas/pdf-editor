//! Bounded local provenance for copies successfully saved by the editor.
//! Filenames can suggest a candidate, but never create recorded provenance.
use std::{
    collections::HashSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const MAGIC: &[u8; 8] = b"PDFORIG1";
const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PATH: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    source: PathBuf,
    copy: PathBuf,
    length: u64,
    modified_seconds: u64,
    modified_nanos: u32,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid comparison provenance")
}

fn unavailable(error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        "Comparison provenance or files are unavailable",
    )
}

fn store_path(config: &Path) -> PathBuf {
    config.with_file_name("comparison-origins.bin")
}

fn regular(path: &Path) -> io::Result<Option<(PathBuf, fs::Metadata)>> {
    let canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(unavailable(error)),
    };
    let metadata = match fs::metadata(&canonical) {
        Ok(metadata) => metadata,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(unavailable(error)),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    Ok(Some((canonical, metadata)))
}

fn stamp(metadata: &fs::Metadata) -> io::Result<(u64, u32)> {
    let modified = metadata
        .modified()
        .map_err(unavailable)?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid())?;
    Ok((modified.as_secs(), modified.subsec_nanos()))
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> io::Result<&'a [u8]> {
        let value = self.0.get(..size).ok_or_else(invalid)?;
        self.0 = &self.0[size..];
        Ok(value)
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| invalid())?,
        ))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| invalid())?,
        ))
    }
    fn path(&mut self) -> io::Result<PathBuf> {
        let length = self.u32()? as usize;
        if length == 0 || length > MAX_PATH {
            return Err(invalid());
        }
        let value = std::str::from_utf8(self.take(length)?).map_err(|_| invalid())?;
        let path = PathBuf::from(value);
        if value.contains('\0') || !path.is_absolute() {
            return Err(invalid());
        }
        Ok(path)
    }
}

fn read(config: &Path) -> io::Result<Vec<Entry>> {
    let file = match fs::File::open(store_path(config)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(unavailable(error)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unavailable)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid());
    }
    let mut input = Reader(&bytes);
    if input.take(8)? != MAGIC {
        return Err(invalid());
    }
    let count = input.u32()? as usize;
    if count > MAX_ENTRIES {
        return Err(invalid());
    }
    let mut entries = Vec::with_capacity(count);
    let mut seen = HashSet::new();
    for _ in 0..count {
        let source = input.path()?;
        let copy = input.path()?;
        let length = input.u64()?;
        let modified_seconds = input.u64()?;
        let modified_nanos = input.u32()?;
        if source == copy || !seen.insert(copy.clone()) || modified_nanos >= 1_000_000_000 {
            return Err(invalid());
        }
        entries.push(Entry {
            source,
            copy,
            length,
            modified_seconds,
            modified_nanos,
        });
    }
    if !input.0.is_empty() {
        return Err(invalid());
    }
    Ok(entries)
}

fn put_path(bytes: &mut Vec<u8>, path: &Path) -> io::Result<()> {
    let text = path.to_str().ok_or_else(invalid)?;
    if text.is_empty() || text.len() > MAX_PATH || text.contains('\0') || !path.is_absolute() {
        return Err(invalid());
    }
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    Ok(())
}

fn encode(entries: &[Entry]) -> io::Result<Vec<u8>> {
    if entries.len() > MAX_ENTRIES {
        return Err(invalid());
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        put_path(&mut bytes, &entry.source)?;
        put_path(&mut bytes, &entry.copy)?;
        bytes.extend_from_slice(&entry.length.to_le_bytes());
        bytes.extend_from_slice(&entry.modified_seconds.to_le_bytes());
        bytes.extend_from_slice(&entry.modified_nanos.to_le_bytes());
    }
    Ok(bytes)
}

/// Records a known source only after a successful save. The caller excludes
/// protected/encrypted documents. No document bytes or passwords are retained.
pub(crate) fn remember(config: &Path, source: &Path, copy: &Path) -> io::Result<()> {
    let (source, _) = regular(source)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Comparison source must be an existing regular file",
        )
    })?;
    let (copy, metadata) = regular(copy)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Comparison copy must be an existing regular file",
        )
    })?;
    if source == copy {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Comparison source and copy must be distinct",
        ));
    }
    let (modified_seconds, modified_nanos) = stamp(&metadata)?;
    let mut entries = read(config)?;
    entries.retain(|entry| entry.copy != copy);
    entries.push(Entry {
        source,
        copy,
        length: metadata.len(),
        modified_seconds,
        modified_nanos,
    });
    if entries.len() > MAX_ENTRIES {
        entries.remove(0);
    }
    loop {
        let bytes = encode(&entries)?;
        if bytes.len() <= MAX_BYTES {
            return crate::recovery::write_replacing(&store_path(config), &bytes)
                .map_err(unavailable);
        }
        // Retain the newest mapping when long paths exhaust the byte budget.
        if entries.len() <= 1 {
            return Err(invalid());
        }
        entries.remove(0);
    }
}

/// Resolves only recorded provenance for an unchanged on-disk copy. File size
/// and modification time detect stale records; they are not a security digest.
pub(crate) fn resolve(config: &Path, copy: &Path) -> io::Result<Option<PathBuf>> {
    // Validate the entire store before returning any record.
    let entries = read(config)?;
    let Some((copy, metadata)) = regular(copy)? else {
        return Ok(None);
    };
    let Some(entry) = entries.iter().find(|entry| entry.copy == copy) else {
        return Ok(None);
    };
    if metadata.len() != entry.length
        || stamp(&metadata)? != (entry.modified_seconds, entry.modified_nanos)
    {
        return Ok(None);
    }
    let Some((source, _)) = regular(&entry.source)? else {
        return Ok(None);
    };
    if source != entry.source || source == copy {
        return Ok(None);
    }
    Ok(Some(source))
}

/// Offers one exact sibling based on the default save suffix. This candidate
/// is not verified ancestry and must be identified as a filename suggestion.
pub(crate) fn suggest(copy: &Path) -> Option<PathBuf> {
    let extension = copy.extension()?;
    if !extension.eq_ignore_ascii_case("pdf") {
        return None;
    }
    let stem = copy.file_stem()?.to_str()?.strip_suffix("_edited")?;
    if stem.is_empty() {
        return None;
    }
    let mut name = std::ffi::OsString::from(stem);
    name.push(".");
    name.push(extension);
    let (candidate, _) = regular(&copy.with_file_name(name)).ok()??;
    let (copy, _) = regular(copy).ok()??;
    (candidate != copy).then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    struct Folder(PathBuf);
    impl Folder {
        fn new(label: &str) -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let folder = root
                .join("target/test-output/comparison-origin")
                .join(format!(
                    "{label}-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
            fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }
        fn config(&self) -> PathBuf {
            self.0.join("settings.txt")
        }
        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"generated marker").unwrap();
            path
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn arbitrary_names_and_unicode_round_trip_without_changing_document_files() {
        let folder = Folder::new("round-trip");
        let source = folder.file("äöå-文書.pdf");
        let copy = folder.file("chosen-name.pdf");
        let config = folder.config();
        assert_eq!(resolve(&config, &copy).unwrap(), None);
        remember(&config, &source, &copy).unwrap();
        assert_eq!(
            resolve(&config, &copy).unwrap(),
            Some(source.canonicalize().unwrap())
        );
        assert_eq!(fs::read(&source).unwrap(), b"generated marker");
        assert_eq!(fs::read(&copy).unwrap(), b"generated marker");
        assert!(remember(&config, &source, &source).is_err());
        assert!(remember(&config, &folder.0, &copy).is_err());
        fs::remove_file(&source).unwrap();
        assert_eq!(resolve(&config, &copy).unwrap(), None);
    }

    #[test]
    fn changed_copy_length_or_timestamp_invalidates_the_record_until_remembered_again() {
        let folder = Folder::new("stale");
        let source = folder.file("source.pdf");
        let copy = folder.file("copy.pdf");
        let config = folder.config();
        let set_time = |seconds| {
            fs::OpenOptions::new()
                .write(true)
                .open(&copy)
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(seconds)),
                )
                .unwrap()
        };
        set_time(1_200_000_000);
        remember(&config, &source, &copy).unwrap();
        set_time(1_200_000_002);
        assert_eq!(resolve(&config, &copy).unwrap(), None);
        remember(&config, &source, &copy).unwrap();
        assert!(resolve(&config, &copy).unwrap().is_some());
        fs::write(&copy, b"different generated length").unwrap();
        assert_eq!(resolve(&config, &copy).unwrap(), None);
        remember(&config, &source, &copy).unwrap();
        assert!(resolve(&config, &copy).unwrap().is_some());
        fs::remove_file(&copy).unwrap();
        assert_eq!(resolve(&config, &copy).unwrap(), None);
    }

    #[test]
    fn malformed_store_never_returns_partial_records_or_gets_overwritten() {
        let folder = Folder::new("corrupt");
        let source = folder.file("source.pdf");
        let copy = folder.file("copy.pdf");
        let config = folder.config();
        remember(&config, &source, &copy).unwrap();
        let complete = fs::read(store_path(&config)).unwrap();
        let mut trailing = complete.clone();
        trailing.push(0);
        let mut unknown = complete.clone();
        unknown[7] = b'9';
        let mut count = MAGIC.to_vec();
        count.extend_from_slice(&129u32.to_le_bytes());
        for invalid in [
            complete[..complete.len() - 1].to_vec(),
            trailing,
            unknown,
            count,
            vec![0; MAX_BYTES + 1],
        ] {
            fs::write(store_path(&config), &invalid).unwrap();
            assert!(resolve(&config, &copy).is_err());
            assert!(remember(&config, &source, &copy).is_err());
            assert_eq!(fs::read(store_path(&config)).unwrap(), invalid);
        }
    }

    #[test]
    fn entry_limit_evicts_the_oldest_mapping() {
        let folder = Folder::new("bounded");
        let source = folder.file("source.pdf");
        let config = folder.config();
        let first = folder.file("copy-0.pdf");
        remember(&config, &source, &first).unwrap();
        for index in 1..=MAX_ENTRIES {
            let copy = folder.file(&format!("copy-{index}.pdf"));
            remember(&config, &source, &copy).unwrap();
        }
        assert_eq!(read(&config).unwrap().len(), MAX_ENTRIES);
        assert_eq!(resolve(&config, &first).unwrap(), None);
        let newest = folder.0.join(format!("copy-{MAX_ENTRIES}.pdf"));
        assert_eq!(
            resolve(&config, &newest).unwrap(),
            Some(source.canonicalize().unwrap())
        );
        assert!(fs::metadata(store_path(&config)).unwrap().len() <= MAX_BYTES as u64);
    }

    #[test]
    fn filename_suggestions_remove_only_one_exact_terminal_suffix_and_are_not_persisted() {
        let folder = Folder::new("suggestions");
        let source = folder.file("äöå.pdf");
        let copy = folder.file("äöå_edited.pdf");
        assert_eq!(suggest(&copy), Some(source.canonicalize().unwrap()));
        assert_eq!(resolve(&folder.config(), &copy).unwrap(), None);
        assert!(!store_path(&folder.config()).exists());
        assert_eq!(suggest(&folder.file("äöå_edited_backup.pdf")), None);
        assert_eq!(suggest(&folder.file("äöå_EDITED.pdf")), None);
        assert_eq!(suggest(&folder.file("_edited.pdf")), None);
        let twice = folder.file("base_edited_edited.pdf");
        folder.file("base.pdf");
        assert_eq!(suggest(&twice), None);
        let once = folder.file("base_edited.pdf");
        assert_eq!(suggest(&twice), Some(once.canonicalize().unwrap()));
        fs::remove_file(source).unwrap();
        assert_eq!(suggest(&copy), None);
    }
}
