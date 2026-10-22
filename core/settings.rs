use crate::{Result, err, read_limited};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Substitution {
    #[default]
    Line,
    Document,
}
impl Substitution {
    pub fn config_path() -> Result<PathBuf> {
        std::env::var_os("APPDATA")
            .map(|p| PathBuf::from(p).join("PdfEditor/settings.txt"))
            .ok_or_else(|| err("APPDATA is not available"))
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::Line);
        }
        match read_limited(path, 64)?.as_slice() {
            b"line\n" => Ok(Self::Line),
            b"document\n" => Ok(Self::Document),
            _ => Err(err("Invalid substitution setting")),
        }
    }
    pub fn save(self, path: &Path) -> Result<()> {
        let parent = path.parent().ok_or_else(|| err("Invalid settings path"))?;
        fs::create_dir_all(parent)?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temp = path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let outcome = (|| -> Result<()> {
            file.write_all(match self {
                Self::Line => b"line\n",
                Self::Document => b"document\n",
            })?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)?;
            Ok(())
        })();
        if outcome.is_err() {
            let _ = fs::remove_file(&temp);
        }
        outcome
    }
}
