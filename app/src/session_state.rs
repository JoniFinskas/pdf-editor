//! Explicit, bounded local session persistence. No passwords are stored.
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"PDFSESS1";
const LIMIT: usize = 1024 * 1024;
const MAX_TABS: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub struct SessionTab {
    pub source: PathBuf,
    pub recovery: Option<PathBuf>,
    pub current_page: u16,
    pub zoom: f32,
    pub scroll: f32,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionState {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid session state")
}
fn valid_view(tab: &SessionTab) -> bool {
    tab.current_page < 10_000
        && tab.zoom.is_finite()
        && (0.05..=16.).contains(&tab.zoom)
        && tab.scroll.is_finite()
        && (0.0..=1_000_000_000.).contains(&tab.scroll)
}
fn put_path(out: &mut Vec<u8>, path: &Path) -> io::Result<()> {
    let text = path.to_str().ok_or_else(invalid)?;
    if text.is_empty() || text.len() > 4096 || text.contains('\0') {
        return Err(invalid());
    }
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> io::Result<&'a [u8]> {
        let value = self.0.get(..size).ok_or_else(invalid)?;
        self.0 = &self.0[size..];
        Ok(value)
    }
    fn number(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn path(&mut self) -> io::Result<PathBuf> {
        let size = self.number()? as usize;
        if size == 0 || size > 4096 {
            return Err(invalid());
        }
        let text = std::str::from_utf8(self.take(size)?).map_err(|_| invalid())?;
        if text.contains('\0') {
            return Err(invalid());
        }
        Ok(PathBuf::from(text))
    }
}
impl SessionState {
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if self.tabs.len() > MAX_TABS || (!self.tabs.is_empty() && self.active >= self.tabs.len()) {
            return Err(invalid());
        }
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&(self.tabs.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.active as u32).to_le_bytes());
        for tab in &self.tabs {
            if !valid_view(tab) {
                return Err(invalid());
            }
            put_path(&mut out, &tab.source)?;
            out.push(u8::from(tab.recovery.is_some()));
            if let Some(recovery) = &tab.recovery {
                put_path(&mut out, recovery)?;
            }
            out.extend_from_slice(&tab.current_page.to_le_bytes());
            out.extend_from_slice(&tab.zoom.to_le_bytes());
            out.extend_from_slice(&tab.scroll.to_le_bytes());
        }
        if out.len() > LIMIT {
            return Err(invalid());
        }
        crate::recovery::write_replacing(path, &out)
    }

    /// Missing documents are skipped; malformed state never partially restores.
    pub fn load(path: &Path) -> io::Result<Self> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut data = Vec::new();
        file.take(LIMIT as u64 + 1).read_to_end(&mut data)?;
        if data.len() > LIMIT {
            return Err(invalid());
        }
        let mut input = Reader(&data);
        if input.take(8)? != MAGIC {
            return Err(invalid());
        }
        let count = input.number()? as usize;
        let active = input.number()? as usize;
        if count > MAX_TABS || (count > 0 && active >= count) || (count == 0 && active != 0) {
            return Err(invalid());
        }
        let mut tabs = Vec::new();
        let mut restored_active = 0;
        for index in 0..count {
            let source = input.path()?;
            let recovery = match input.take(1)?[0] {
                0 => None,
                1 => Some(input.path()?),
                _ => return Err(invalid()),
            };
            let current_page = u16::from_le_bytes(input.take(2)?.try_into().unwrap());
            let zoom = f32::from_le_bytes(input.take(4)?.try_into().unwrap());
            let scroll = f32::from_le_bytes(input.take(4)?.try_into().unwrap());
            let mut tab = SessionTab {
                source,
                recovery,
                current_page,
                zoom,
                scroll,
            };
            if !valid_view(&tab) {
                return Err(invalid());
            }
            tab.recovery = tab.recovery.filter(|path| path.is_file());
            if tab.source.is_file() || tab.recovery.is_some() {
                if index <= active {
                    restored_active = tabs.len();
                }
                tabs.push(tab);
            }
        }
        if !input.0.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            tabs,
            active: restored_active,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_views_round_trip_and_missing_or_corrupt_entries_fail_safely() {
        let dir = std::env::temp_dir().join(format!("pdf-session-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("äöå-文書.pdf");
        fs::write(&source, b"generated").unwrap();
        let path = dir.join("session.bin");
        let state = SessionState {
            tabs: vec![SessionTab {
                source: source.clone(),
                recovery: None,
                current_page: 3,
                zoom: 1.25,
                scroll: 421.5,
            }],
            active: 0,
        };
        state.save(&path).unwrap();
        assert_eq!(SessionState::load(&path).unwrap(), state);
        fs::remove_file(source).unwrap();
        assert!(SessionState::load(&path).unwrap().tabs.is_empty());
        let mut bytes = fs::read(&path).unwrap();
        bytes.pop();
        fs::write(&path, bytes).unwrap();
        assert!(SessionState::load(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
