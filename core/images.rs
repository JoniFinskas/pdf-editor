use crate::*;
use lopdf::{Dictionary, Object, Stream};

#[derive(Clone, Debug)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub page: u16,
    pub left: f32,
    pub bottom: f32,
    pub width: f32,
    pub height: f32,
}
impl Picture {
    pub fn open(path: &Path) -> Result<Self> {
        guarded(|| {
            let bytes = read_limited(path, MAX_FILE)?;
            let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                .with_guessed_format()
                .map_err(|_| err("Unsupported image"))?;
            if !matches!(
                reader.format(),
                Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
            ) {
                return Err(err("Choose a PNG or JPG image"));
            }
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(8192);
            limits.max_image_height = Some(8192);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            let rgba = reader
                .decode()
                .map_err(|_| err("Image could not be decoded"))?
                .into_rgba8();
            let result = Self {
                width: rgba.width(),
                height: rgba.height(),
                rgba: rgba.into_raw(),
            };
            result.validate()?;
            Ok(result)
        })
    }
    fn validate(&self) -> Result<()> {
        let pixels = u64::from(self.width) * u64::from(self.height);
        if pixels == 0 || pixels > 16_000_000 || pixels * 4 != self.rgba.len() as u64 {
            return Err(err("Image must contain at most 16 million pixels"));
        }
        Ok(())
    }
}

/// Rasterizes normalized signature strokes onto a transparent 1200 × 400 image.
pub fn signature_image(strokes: &[Vec<[f32; 2]>]) -> Result<Picture> {
    if strokes.iter().map(Vec::len).sum::<usize>() > 100_000
        || !strokes
            .iter()
            .flatten()
            .all(|p| p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
        || !strokes.iter().any(|s| s.len() > 1)
    {
        return Err(err("Draw a signature inside the signature pad"));
    }
    let mut out = Picture {
        width: 1200,
        height: 400,
        rgba: vec![0; 1200 * 400 * 4],
    };
    for stroke in strokes {
        for pair in stroke.windows(2) {
            let a = [pair[0][0] * 1199.0, pair[0][1] * 399.0];
            let b = [pair[1][0] * 1199.0, pair[1][1] * 399.0];
            let steps = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).max(1);
            for step in 0..=steps {
                let t = step as f32 / steps as f32;
                let x = (a[0] + (b[0] - a[0]) * t).round() as i32;
                let y = (a[1] + (b[1] - a[1]) * t).round() as i32;
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        if dx * dx + dy * dy > 4 {
                            continue;
                        }
                        let (x, y) = (x + dx, y + dy);
                        if (0..1200).contains(&x) && (0..400).contains(&y) {
                            let i = (y as usize * 1200 + x as usize) * 4;
                            out.rgba[i..i + 4].copy_from_slice(&[20, 20, 24, 255]);
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}
pub fn store_signature(folder: &Path, picture: &Picture) -> Result<PathBuf> {
    picture.validate()?;
    fs::create_dir_all(folder)?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut png,
        &picture.rgba,
        picture.width,
        picture.height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|_| err("Signature could not be encoded"))?;
    for i in 1..=1000 {
        let path = folder.join(format!("signature-{i}.png"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(png.get_ref())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(err("Signature library is full"))
}
pub fn signature_files(folder: &Path) -> Result<Vec<PathBuf>> {
    if !folder.exists() {
        return Ok(vec![]);
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(folder)?.take(1001) {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("png"))
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}
impl Engine {
    pub fn place_picture(&self, doc: &mut Document, image: &Picture, at: Placement) -> Result<()> {
        guarded(|| {
            image.validate()?;
            if ![at.left, at.bottom, at.width, at.height]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 100_000.0)
                || at.width <= 0.0
                || at.height <= 0.0
            {
                return Err(err("Invalid image position or size"));
            }
            let mut pdf = parse(&doc.bytes)?;
            let page = *pdf
                .get_pages()
                .get(&(u32::from(at.page) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
            let mut alpha = Vec::with_capacity(image.rgba.len() / 4);
            for pixel in image.rgba.as_chunks::<4>().0 {
                rgb.extend_from_slice(&pixel[..3]);
                alpha.push(pixel[3]);
            }
            let mut dict = Dictionary::new();
            dict.set("Type", "XObject");
            dict.set("Subtype", "Image");
            dict.set("Width", image.width);
            dict.set("Height", image.height);
            dict.set("BitsPerComponent", 8);
            dict.set("ColorSpace", "DeviceGray");
            let mut mask = Stream::new(dict.clone(), alpha);
            mask.compress()?;
            let mask = pdf.add_object(mask);
            dict.set("ColorSpace", "DeviceRGB");
            dict.set("SMask", mask);
            let mut stream = Stream::new(dict, rgb);
            stream.compress()?;
            let id = pdf.add_object(stream);
            let mut resources = content::resources(&pdf, page)?;
            let mut objects = resources
                .get(b"XObject")
                .ok()
                .and_then(|o| pdf.dereference(o).ok())
                .and_then(|(_, o)| o.as_dict().ok())
                .cloned()
                .unwrap_or_default();
            let mut name = format!("EditorImage{}", id.0);
            while objects.has(name.as_bytes()) {
                name.push('X');
            }
            objects.set(name.as_bytes(), id);
            resources.set("XObject", objects);
            pdf.get_object_mut(page)?
                .as_dict_mut()?
                .set("Resources", resources);
            let mut data = b"q\n".to_vec();
            data.extend(pdf.get_page_content_with_limit(page, MAX_STREAM)?);
            data.extend(b"\nQ\n");
            data.extend(
                format!(
                    "q /{} BMC {} 0 0 {} {} {} cm /{} Do EMC Q\n",
                    name, at.width, at.height, at.left, at.bottom, name
                )
                .as_bytes(),
            );
            let stream = pdf.add_object(Stream::new(Dictionary::new(), data));
            pdf.get_object_mut(page)?
                .as_dict_mut()?
                .set("Contents", Object::Reference(stream));
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drawn_signature_is_transparent_and_bounded() {
        let image = signature_image(&[vec![[0.1, 0.5], [0.9, 0.5]]]).unwrap();
        assert_eq!(image.rgba[3], 0);
        assert_eq!(image.rgba[(200 * 1200 + 600) * 4 + 3], 255);
        assert!(signature_image(&[vec![[f32::NAN, 0.0], [1.0, 1.0]]]).is_err());
        assert!(signature_image(&[]).is_err());
    }
}
