//! Bounded recovery snapshots containing current bytes and undo/redo branches.
use crate::*;

const MAGIC: &[u8; 8] = b"PDFHIST1";
const LIMIT: usize = MAX_HISTORY + MAX_FILE + 65536;
const MAX_STATES: usize = 4096;

impl Document {
    /// Recovery is intentionally unavailable for password-protected documents.
    pub fn recovery_bytes(&self) -> Result<Vec<u8>> {
        if self.protected {
            return Err(err("Recovery is disabled for password-protected documents"));
        }
        let states = self
            .undo
            .len()
            .checked_add(self.redo.len())
            .ok_or_else(|| err("Recovery history exceeds limits"))?;
        let size = self
            .undo
            .iter()
            .chain(&self.redo)
            .chain(std::iter::once(&self.bytes))
            .try_fold(0usize, |sum, bytes| {
                sum.checked_add(bytes.len())
                    .ok_or_else(|| err("Recovery history exceeds limits"))
            })?;
        if states > MAX_STATES || size > MAX_HISTORY + MAX_FILE {
            return Err(err("Recovery history exceeds limits"));
        }
        let mut out = Vec::with_capacity(size + (states + 1) * 8 + 32);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.revision.to_le_bytes());
        out.extend_from_slice(&(self.undo.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.redo.len() as u32).to_le_bytes());
        for bytes in std::iter::once(&self.bytes)
            .chain(&self.undo)
            .chain(&self.redo)
        {
            out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(bytes);
        }
        if out.len() > LIMIT {
            return Err(err("Recovery history exceeds limits"));
        }
        Ok(out)
    }

    /// Publishes the PDF last, so discovery only sees complete recovery pairs.
    pub fn save_recovery(&self, path: &Path) -> Result<()> {
        let bytes = self.recovery_bytes()?;
        let sidecar = path.with_extension("history");
        operation::checkpoint("Saving recovery", 0, 1)?;
        save::write_new(&sidecar, &bytes)?;
        if let Err(error) = self.save_as(path) {
            let _ = fs::remove_file(sidecar);
            return Err(error);
        }
        Ok(())
    }
}

impl Engine {
    /// Restores local history with an explicitly supplied source identity.
    pub fn restore_recovery(&self, source: &Path, sidecar: &[u8]) -> Result<Document> {
        guarded(|| {
            if sidecar.len() > LIMIT || sidecar.len() < 24 || &sidecar[..8] != MAGIC {
                return Err(err("Invalid recovery history"));
            }
            let revision = u64::from_le_bytes(sidecar[8..16].try_into().unwrap());
            let undo_count = u32::from_le_bytes(sidecar[16..20].try_into().unwrap()) as usize;
            let redo_count = u32::from_le_bytes(sidecar[20..24].try_into().unwrap()) as usize;
            if undo_count.saturating_add(redo_count) > MAX_STATES {
                return Err(err("Recovery history exceeds limits"));
            }
            let mut cursor = 24usize;
            let mut states = Vec::new();
            let mut history_size = 0usize;
            for index in 0..=undo_count + redo_count {
                operation::checkpoint("Restoring history", index, undo_count + redo_count + 1)?;
                let header = sidecar
                    .get(cursor..cursor.saturating_add(8))
                    .ok_or_else(|| err("Truncated recovery history"))?;
                let size = usize::try_from(u64::from_le_bytes(header.try_into().unwrap()))
                    .map_err(|_| err("Invalid recovery length"))?;
                cursor += 8;
                if size > MAX_FILE {
                    return Err(err("Recovery snapshot exceeds limits"));
                }
                let end = cursor
                    .checked_add(size)
                    .ok_or_else(|| err("Invalid recovery length"))?;
                let bytes = sidecar
                    .get(cursor..end)
                    .ok_or_else(|| err("Truncated recovery history"))?;
                parse(bytes)?;
                let native = self.pdfium.load_pdf_from_byte_slice(bytes, None)?;
                loaded_page_sizes(native.pages())?;
                history_size = history_size.saturating_add(size);
                if history_size > MAX_HISTORY + MAX_FILE {
                    return Err(err("Recovery history exceeds limits"));
                }
                states.push(bytes.to_vec());
                cursor = end;
            }
            if cursor != sidecar.len() {
                return Err(err("Unexpected recovery data"));
            }
            let bytes = states.remove(0);
            let redo = states.split_off(undo_count);
            Ok(Document {
                bytes,
                original: source.to_path_buf(),
                undo: states,
                redo,
                revision,
                protected: false,
                substitution: Substitution::default(),
            })
        })
    }

    /// Older recovery PDFs remain readable without a history sidecar.
    pub fn open_recovery(&self, path: &Path, source: &Path) -> Result<Document> {
        let sidecar = path.with_extension("history");
        if sidecar.exists() {
            let doc = self.restore_recovery(source, &read_limited(&sidecar, LIMIT)?)?;
            if doc.bytes != read_limited(path, MAX_FILE)? {
                return Err(err("Recovery PDF and history do not match"));
            }
            Ok(doc)
        } else {
            let mut doc = self.open(path)?;
            if doc.protected {
                return Err(err("Protected documents cannot be recovered"));
            }
            doc.original = source.to_path_buf();
            Ok(doc)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_after_a_merge_step_preserves_current_and_redo_snapshots() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let source = root.join("tests/fixtures/generated/unicode-cid.pdf");
        let mut doc = engine.open(&source).unwrap();
        let selected = engine.text_objects(&doc, 0).unwrap().remove(0);
        engine.edit(&mut doc, &selected, "AB").unwrap();
        doc.undo();
        let before = doc.recovery_bytes().unwrap();
        let control = OperationControl::default();
        let finished = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            let observer = scope.spawn(|| {
                while !finished.load(Ordering::Acquire) {
                    if control
                        .progress()
                        .is_some_and(|p| p.label == "Merging PDFs" && p.completed >= 1)
                    {
                        control.cancel();
                        return;
                    }
                    std::thread::yield_now();
                }
            });
            let result =
                engine.with_operation(&control, || engine.merge_pdfs(&mut doc, &vec![source; 100]));
            finished.store(true, Ordering::Release);
            observer.join().unwrap();
            result
        });
        assert_eq!(result.unwrap_err().0, "Operation cancelled");
        assert_eq!(doc.recovery_bytes().unwrap(), before);
        engine
            .edit_page_batch(&mut doc, &PageBatch::Rotate(vec![0]))
            .unwrap();
        assert!(doc.can_undo());
    }
    #[test]
    fn cancellation_after_an_export_removes_already_created_outputs() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let doc = engine
            .open(&root.join("tests/fixtures/generated/unicode-cid.pdf"))
            .unwrap();
        let folder = root.join(format!(
            "target/cancel-running-export-{}",
            std::process::id()
        ));
        fs::create_dir_all(&folder).unwrap();
        let control = OperationControl::default();
        let finished = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            let observer = scope.spawn(|| {
                while !finished.load(Ordering::Acquire) {
                    if fs::read_dir(&folder).unwrap().next().is_some() {
                        control.cancel();
                        return;
                    }
                    std::thread::yield_now();
                }
            });
            let result = engine.with_operation(&control, || {
                engine.export_page_images(&doc, &[0; 100], 72, PageImageFormat::Png, &folder)
            });
            finished.store(true, Ordering::Release);
            observer.join().unwrap();
            result
        });
        assert_eq!(result.unwrap_err().0, "Operation cancelled");
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
        fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn recovery_preserves_both_history_branches_and_rejects_corruption() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let path =
            std::env::temp_dir().join(format!("history-generated-{}.pdf", std::process::id()));
        fs::copy(root.join("tests/fixtures/generated/unicode-cid.pdf"), &path).unwrap();
        let mut doc = engine.open(&path).unwrap();
        let first = doc.bytes.clone();
        let object = engine.text_objects(&doc, 0).unwrap().remove(0);
        engine.edit(&mut doc, &object, "Second").unwrap();
        let second = doc.bytes.clone();
        let object = engine.text_objects(&doc, 0).unwrap().remove(0);
        engine.edit(&mut doc, &object, "Third").unwrap();
        let third = doc.bytes.clone();
        doc.undo();
        let data = doc.recovery_bytes().unwrap();
        let mut restored = engine.restore_recovery(&path, &data).unwrap();
        assert_eq!(restored.bytes, second);
        assert_eq!(restored.revision, doc.revision);
        assert!(restored.undo());
        assert_eq!(restored.bytes, first);
        assert!(restored.redo());
        assert_eq!(restored.bytes, second);
        assert!(restored.redo());
        assert_eq!(restored.bytes, third);
        assert!(
            engine
                .restore_recovery(&path, &data[..data.len() - 1])
                .is_err()
        );
        let mut invalid = data;
        invalid[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(engine.restore_recovery(&path, &invalid).is_err());
        doc.protected = true;
        assert!(doc.recovery_bytes().is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn cancelled_mutation_keeps_bytes_and_history_and_creates_no_export() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let source = root.join("tests/fixtures/generated/unicode-cid.pdf");
        let mut doc = engine.open(&source).unwrap();
        let before = doc.recovery_bytes().unwrap();
        let control = OperationControl::default();
        control.cancel();
        assert!(
            engine
                .with_operation(&control, || engine
                    .merge_pdfs(&mut doc, std::slice::from_ref(&source)))
                .is_err()
        );
        assert_eq!(doc.recovery_bytes().unwrap(), before);
        let folder = std::env::temp_dir().join(format!("cancel-export-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        assert!(
            engine
                .with_operation(&control, || engine.export_page_images(
                    &doc,
                    &[0],
                    72,
                    PageImageFormat::Png,
                    &folder
                ))
                .is_err()
        );
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
        fs::remove_dir(folder).unwrap();

        let control = OperationControl::default();
        let mut index = 0;
        let pictures = std::iter::from_fn(|| {
            index += 1;
            if index == 2 {
                control.cancel();
            }
            if index > 2 {
                None
            } else {
                Some(Ok(Picture {
                    width: 1,
                    height: 1,
                    rgba: vec![0, 0, 0, 255],
                }))
            }
        });
        // Cancellation after encoding the first generated image still discards the candidate.
        assert!(
            operation::with_control(&control, || engine
                .pictures_to_pdf(pictures, ImagePageSize::A4))
            .is_err()
        );
        assert_eq!(doc.recovery_bytes().unwrap(), before);
    }
}
