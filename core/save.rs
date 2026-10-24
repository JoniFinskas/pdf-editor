//! Publish only complete, flushed PDFs without replacing an existing destination.
use std::{
    fs,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_with(path, |file| file.write_all(bytes))
}

fn write_with(path: &Path, write: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
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
    let result = result.and_then(|_| publish(&temp, path));
    let _ = fs::remove_file(&temp);
    result
}

#[cfg(windows)]
fn publish(temp: &Path, path: &Path) -> io::Result<()> {
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
    // Same-directory move; WRITE_THROUGH, with no REPLACE_EXISTING flag.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 8) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn publish(temp: &Path, path: &Path) -> io::Result<()> {
    fs::hard_link(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_write_never_publishes_partial_output_or_replaces_existing_file() {
        let dir = std::env::temp_dir().join(format!("pdf-save-{}", std::process::id()));
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
        write_new(&output, b"complete generated content").unwrap();
        assert!(write_new(&output, b"replacement").is_err());
        assert_eq!(fs::read(&output).unwrap(), b"complete generated content");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
