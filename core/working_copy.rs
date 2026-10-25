use crate::*;

/// Authority to update only a copy created explicitly by this editing session.
/// Passwords stay in memory and are never part of recovery or session data.
pub struct WorkingCopy {
    path: PathBuf,
    original: PathBuf,
    expected: Vec<u8>,
    password: Vec<u8>,
}
impl WorkingCopy {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn encrypted(&self) -> bool {
        !self.password.is_empty()
    }
}
impl Drop for WorkingCopy {
    fn drop(&mut self) {
        self.password.fill(0);
    }
}

impl Document {
    pub fn create_working_copy(&self, path: &Path, password: &str) -> Result<WorkingCopy> {
        self.save_as_with_password(path, password)?;
        Ok(WorkingCopy {
            path: path.canonicalize()?,
            original: self.original.clone(),
            expected: read_limited(path, MAX_FILE)?,
            password: password.as_bytes().to_vec(),
        })
    }

    pub fn update_working_copy(&self, copy: &mut WorkingCopy) -> Result<()> {
        guarded(|| {
            if copy.original != self.original || copy.path == self.original {
                return Err(err(
                    "This working copy does not belong to the open document",
                ));
            }
            let bytes = self.output_bytes(
                std::str::from_utf8(&copy.password)
                    .map_err(|_| err("Working-copy encryption settings are invalid"))?,
            )?;
            save::write_replacing(&copy.path, &bytes, &copy.expected)
                .map_err(|_| err("The working copy changed, is missing or cannot be replaced. Use Save As to create a new copy."))?;
            copy.expected = bytes;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn working_copy_updates_refuse_external_changes_and_keep_original_bytes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let dir = root.join(format!("target/working-copy-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("generated-source.pdf");
        let bytes = fs::read(root.join("tests/fixtures/generated/unicode-cid.pdf")).unwrap();
        fs::write(&source, &bytes).unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let mut doc = engine.open(&source).unwrap();
        let path = dir.join("generated-copy.pdf");
        let mut copy = doc.create_working_copy(&path, "").unwrap();
        assert!(!copy.encrypted());
        for text in ["AB", "BA"] {
            let selected = engine.text_objects(&doc, 0).unwrap().remove(0);
            engine.edit(&mut doc, &selected, text).unwrap();
            doc.update_working_copy(&mut copy).unwrap();
            let saved = engine.open(&path).unwrap();
            assert_eq!(engine.text_objects(&saved, 0).unwrap()[0].text, text);
            assert_eq!(fs::read(&source).unwrap(), bytes);
        }
        fs::write(&path, b"external generated change").unwrap();
        assert!(doc.update_working_copy(&mut copy).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external generated change");
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert!(doc.create_working_copy(&path, "").is_err());
        for path in [source, path] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn working_copy_updates_preserve_encryption() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let dir = root.join(format!(
            "target/encrypted-working-copy-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("generated-source.pdf");
        fs::copy(
            root.join("tests/fixtures/generated/unicode-cid.pdf"),
            &source,
        )
        .unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let mut doc = engine.open(&source).unwrap();
        let path = dir.join("encrypted-copy.pdf");
        let mut copy = doc.create_working_copy(&path, "fixture-password").unwrap();
        assert!(copy.encrypted());
        let selected = engine.text_objects(&doc, 0).unwrap().remove(0);
        engine.edit(&mut doc, &selected, "AB").unwrap();
        doc.update_working_copy(&mut copy).unwrap();
        assert!(engine.open(&path).is_err());
        assert!(engine.open_with_password(&path, "wrong").is_err());
        let reopened = engine
            .open_with_password(&path, "fixture-password")
            .unwrap();
        assert_eq!(engine.text_objects(&reopened, 0).unwrap()[0].text, "AB");
        for path in [source, path] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(dir).unwrap();
    }
}
