use crate::*;

/// Private raster spool, removed when dropped, including after failed printing.
pub struct PrintJob {
    directory: PathBuf,
    pages: usize,
}
impl PrintJob {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn page_count(&self) -> usize {
        self.pages
    }
}
impl Drop for PrintJob {
    fn drop(&mut self) {
        // Only this exclusively created job directory is owned by the spool.
        let _ = fs::remove_dir_all(&self.directory);
    }
}
impl Engine {
    pub fn prepare_print(&self, doc: &Document) -> Result<PrintJob> {
        guarded(|| {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| err("Invalid system clock"))?
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "pdf-editor-print-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&directory)?;
            let mut job = PrintJob {
                directory,
                pages: 0,
            };
            let mut total = 0usize;
            for (index, page) in self.pages(doc)?.iter().enumerate() {
                let scale =
                    (300.0_f32 / 72.0).min((15_000_000.0 / (page.width * page.height)).sqrt());
                let image = self.render(doc, index as u16, scale)?;
                let bitmap = bitmap_bytes(&image)?;
                total = total
                    .checked_add(bitmap.len())
                    .ok_or_else(|| err("Print job too large"))?;
                if total > 512 * 1024 * 1024 {
                    return Err(err(
                        "Print spool exceeds 512 MiB; print a smaller extracted document",
                    ));
                }
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(job.directory.join(format!("{index:05}.bmp")))?;
                file.write_all(&bitmap)?;
                job.pages += 1;
            }
            Ok(job)
        })
    }
}

fn bitmap_bytes(image: &Image) -> Result<Vec<u8>> {
    let row = (image.width as usize)
        .checked_mul(3)
        .ok_or_else(|| err("Invalid bitmap size"))?;
    let stride = (row + 3) & !3;
    let pixels = stride
        .checked_mul(image.height as usize)
        .ok_or_else(|| err("Invalid bitmap size"))?;
    if image.width == 0
        || image.height == 0
        || image.width as u64 * image.height as u64 > 16_000_000
        || image.rgba.len() != image.width as usize * image.height as usize * 4
    {
        return Err(err("Invalid raster data"));
    }
    let mut bytes = Vec::with_capacity(54 + pixels);
    bytes.extend(b"BM");
    bytes.extend(((54 + pixels) as u32).to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(54u32.to_le_bytes());
    bytes.extend(40u32.to_le_bytes());
    bytes.extend(image.width.to_le_bytes());
    bytes.extend(image.height.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(24u16.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend((pixels as u32).to_le_bytes());
    bytes.extend([0; 16]);
    for y in (0..image.height as usize).rev() {
        for x in 0..image.width as usize {
            let i = (y * image.width as usize + x) * 4;
            bytes.extend([image.rgba[i + 2], image.rgba[i + 1], image.rgba[i]]);
        }
        bytes.resize(bytes.len() + stride - row, 0);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bitmap_has_padding_bottom_up_rows_and_bgr_channels() {
        let image = Image {
            width: 1,
            height: 2,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let bytes = bitmap_bytes(&image).unwrap();
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(bytes.len(), 62);
        assert_eq!(&bytes[54..], [255, 0, 0, 0, 0, 0, 255, 0]);
        assert!(
            bitmap_bytes(&Image {
                width: 1,
                height: 2,
                rgba: vec![]
            })
            .is_err()
        );
    }
}
