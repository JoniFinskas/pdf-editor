use crate::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageImageFormat {
    Png,
    Jpeg,
}
impl Engine {
    pub fn export_page_image(
        &self,
        doc: &Document,
        page: u16,
        dpi: u16,
        format: PageImageFormat,
        path: &Path,
    ) -> Result<()> {
        guarded(|| {
            if !(36..=600).contains(&dpi) {
                return Err(err("Choose an export resolution from 36 to 600 DPI"));
            }
            let image = self.render(doc, page, f32::from(dpi) / 72.0)?;
            let mut encoded = std::io::Cursor::new(vec![]);
            match format {
                PageImageFormat::Png => image::write_buffer_with_format(
                    &mut encoded,
                    &image.rgba,
                    image.width,
                    image.height,
                    image::ExtendedColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .map_err(|_| err("PNG encoding failed"))?,
                PageImageFormat::Jpeg => {
                    let rgb: Vec<u8> = image
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| p[..3].iter().copied())
                        .collect();
                    let mut encoder =
                        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90);
                    encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(dpi));
                    encoder
                        .encode(
                            &rgb,
                            image.width,
                            image.height,
                            image::ExtendedColorType::Rgb8,
                        )
                        .map_err(|_| err("JPEG encoding failed"))?;
                }
            }
            if encoded.get_ref().len() > MAX_FILE {
                return Err(err("Export image exceeds 64 MiB"));
            }
            operation::checkpoint("Writing image", 1, 1)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| {
                    err("Choose a new image filename; existing files are never overwritten")
                })?;
            if let Err(error) = file
                .write_all(encoded.get_ref())
                .and_then(|_| file.sync_all())
            {
                drop(file);
                let _ = fs::remove_file(path);
                return Err(error.into());
            }
            Ok(())
        })
    }
    pub fn export_page_images(
        &self,
        doc: &Document,
        pages: &[u16],
        dpi: u16,
        format: PageImageFormat,
        folder: &Path,
    ) -> Result<Vec<PathBuf>> {
        guarded(|| {
            if pages.is_empty() || pages.len() > 10000 {
                return Err(err("Select pages to export"));
            }
            let extension = if format == PageImageFormat::Png {
                "png"
            } else {
                "jpg"
            };
            let paths: Vec<_> = pages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    folder.join(format!(
                        "export-{}-page-{}.{}",
                        i + 1,
                        u32::from(*p) + 1,
                        extension
                    ))
                })
                .collect();
            if paths.iter().any(|p| p.exists()) {
                return Err(err("Export filenames already exist; choose another folder"));
            }
            let mut created = vec![];
            for (index, (page, path)) in pages.iter().zip(paths).enumerate() {
                let result = operation::checkpoint("Exporting images", index, pages.len())
                    .and_then(|_| self.export_page_image(doc, *page, dpi, format, &path));
                if let Err(e) = result {
                    for path in created {
                        let _ = fs::remove_file(path);
                    }
                    return Err(e);
                }
                created.push(path);
            }
            Ok(created)
        })
    }
}
