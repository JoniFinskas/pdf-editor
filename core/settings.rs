use crate::{Result, err, read_limited};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Substitution {
    Line,
    #[default]
    Document,
}
impl Substitution {
    pub fn config_path() -> Result<PathBuf> {
        std::env::var_os("APPDATA")
            .map(|p| PathBuf::from(p).join("PdfEditor/settings.txt"))
            .ok_or_else(|| err("APPDATA is not available"))
    }
    pub fn load(path: &Path) -> Result<Self> {
        Ok(Settings::load(path)?.substitution)
    }
    pub fn save(self, path: &Path) -> Result<()> {
        let mut settings = Settings::load(path)?;
        settings.substitution = self;
        settings.save(path)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub substitution: Substitution,
    pub recent_files: Vec<PathBuf>,
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = read_limited(path, 64 * 1024)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| err("Invalid settings encoding"))?;
        let mut lines = text.lines();
        let substitution = match lines.next() {
            Some("line") => Substitution::Line,
            Some("document") => Substitution::Document,
            _ => return Err(err("Invalid substitution setting")),
        };
        let mut recent_files = Vec::new();
        for line in lines {
            let hex = line
                .strip_prefix("recent=")
                .ok_or_else(|| err("Invalid settings entry"))?;
            if hex.len() % 2 != 0
                || hex.len() > 16_384
                || !hex.is_ascii()
                || recent_files.len() >= 10
            {
                return Err(err("Invalid recent-file entry"));
            }
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| {
                    u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err("Invalid path encoding"))
                })
                .collect::<Result<Vec<_>>>()?;
            let path = String::from_utf8(bytes).map_err(|_| err("Invalid path encoding"))?;
            if path.is_empty() || path.contains('\0') {
                return Err(err("Invalid recent path"));
            }
            recent_files.push(PathBuf::from(path));
        }
        Ok(Self {
            substitution,
            recent_files,
        })
    }
    pub fn remember(&mut self, path: &Path) {
        self.recent_files.retain(|p| {
            !p.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&path.as_os_str().to_string_lossy())
        });
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(10);
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut data = match self.substitution {
            Substitution::Line => "line\n",
            Substitution::Document => "document\n",
        }
        .to_string();
        for path in self.recent_files.iter().take(10) {
            let value = path
                .to_str()
                .ok_or_else(|| err("Recent path is not valid Unicode"))?;
            if value.is_empty() || value.len() > 8192 || value.contains('\0') {
                return Err(err("Invalid recent path"));
            }
            data.push_str("recent=");
            for byte in value.bytes() {
                use std::fmt::Write;
                write!(&mut data, "{byte:02x}").map_err(|_| err("Cannot encode settings"))?;
            }
            data.push('\n');
        }
        if data.len() > 64 * 1024 {
            return Err(err("Settings exceed supported size"));
        }
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
            file.write_all(data.as_bytes())?;
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
