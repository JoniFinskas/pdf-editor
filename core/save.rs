//! Publish only complete, flushed PDFs without replacing an existing destination.
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_with(path, |file| file.write_all(bytes))
}

pub(crate) fn write_verified_new(
    path: &Path,
    bytes: &[u8],
    verify: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    transaction_verified(path, |file| file.write_all(bytes), None, verify)
}

fn write_with(path: &Path, write: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
    transaction(path, write, None)
}

pub(crate) fn write_replacing(path: &Path, bytes: &[u8], expected: &[u8]) -> io::Result<()> {
    transaction(path, |file| file.write_all(bytes), Some(expected))
}

fn verify_existing(path: &Path, expected: &[u8]) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("Working copy is not a regular file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("Working copy is a redirected file"));
        }
    }
    let mut actual = Vec::new();
    fs::File::open(path)?
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut actual)?;
    if actual != expected {
        return Err(io::Error::other("Working copy changed externally"));
    }
    Ok(())
}

fn transaction(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
    expected: Option<&[u8]>,
) -> io::Result<()> {
    transaction_verified(path, write, expected, |_| Ok(()))
}

fn transaction_verified(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
    expected: Option<&[u8]>,
    verify: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    if let Some(expected) = expected {
        verify_existing(path, expected)?;
    }
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (temp, mut file) = loop {
        let temp = parent.join(format!(
            ".pdf-editor-save-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = write(&mut file).and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| verify(&temp)).and_then(|_| {
        if let Some(expected) = expected {
            verify_existing(path, expected)?;
        }
        publish(&temp, path, expected.is_some())
    });
    let _ = fs::remove_file(&temp);
    result
}

#[cfg(windows)]
fn publish(temp: &Path, path: &Path, replace: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let wide = |p: &Path| -> io::Result<Vec<u16>> {
        let mut value: Vec<_> = p.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid output filename",
            ));
        }
        value.push(0);
        Ok(value)
    };
    let from = wide(temp)?;
    let to = wide(path)?;
    // Same-directory atomic publication. Only a checked session-owned working
    // copy permits replacement; Save As retains exclusive-create semantics.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 8 | u32::from(replace)) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn publish(temp: &Path, path: &Path, replace: bool) -> io::Result<()> {
    if replace {
        fs::rename(temp, path)
    } else {
        fs::hard_link(temp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_write_never_publishes_partial_output_or_replaces_existing_file() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(format!("target/pdf-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let output = dir.join("generated.pdf");
        assert!(
            write_with(&output, |file| {
                file.write_all(b"partial")?;
                Err(io::Error::other("simulated interrupted write"))
            })
            .is_err()
        );
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        assert!(
            write_verified_new(&output, b"unverified", |_| Err(io::Error::other(
                "verification rejected"
            )))
            .is_err()
        );
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        write_new(&output, b"complete generated content").unwrap();
        assert!(write_new(&output, b"replacement").is_err());
        assert_eq!(fs::read(&output).unwrap(), b"complete generated content");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
