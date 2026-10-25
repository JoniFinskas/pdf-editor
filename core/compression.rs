use crate::*;
use lopdf::{Object, Stream};
#[derive(Clone, Debug)]
pub struct CompressionReport {
    pub before: usize,
    pub after: usize,
    pub images: usize,
    pub skipped: usize,
}
fn raw_image(stream: &Stream) -> Result<image::RgbImage> {
    let d = &stream.dict;
    if d.get(b"ColorSpace")?.as_name()? != b"DeviceRGB"
        || d.get(b"BitsPerComponent")?.as_i64()? != 8
        || d.has(b"Decode")
        || d.has(b"DecodeParms")
        || d.has(b"Mask")
        || d.has(b"ImageMask")
    {
        return Err(err("Unsupported image encoding"));
    }
    let width =
        u32::try_from(d.get(b"Width")?.as_i64()?).map_err(|_| err("Invalid image width"))?;
    let height =
        u32::try_from(d.get(b"Height")?.as_i64()?).map_err(|_| err("Invalid image height"))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(err("Image dimensions exceed compression limits"));
    }
    match d.get(b"Filter").ok().and_then(|o| o.as_name().ok()) {
        Some(b"DCTDecode") => {
            let mut reader = image::ImageReader::with_format(
                std::io::Cursor::new(&stream.content),
                image::ImageFormat::Jpeg,
            );
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(width);
            limits.max_image_height = Some(height);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            let image = reader
                .decode()
                .map_err(|_| err("Invalid JPEG image"))?
                .to_rgb8();
            if image.dimensions() != (width, height) {
                return Err(err("JPEG dimensions do not match the PDF"));
            }
            Ok(image)
        }
        Some(b"FlateDecode") | None => {
            let bytes = if d.has(b"Filter") {
                stream.decompressed_content_with_limit(MAX_FILE)?
            } else {
                stream.content.clone()
            };
            image::RgbImage::from_raw(width, height, bytes)
                .ok_or_else(|| err("Invalid RGB image data"))
        }
        _ => Err(err("Unsupported image encoding")),
    }
}
impl Engine {
    pub fn compress_images(
        &self,
        doc: &mut Document,
        max_edge: u32,
        quality: u8,
    ) -> Result<CompressionReport> {
        guarded(|| {
            if !(128..=8192).contains(&max_edge) || !(10..=95).contains(&quality) {
                return Err(err(
                    "Choose an image edge from 128 to 8192 and quality from 10 to 95",
                ));
            }
            let before = doc.bytes.len();
            let mut pdf = parse(&doc.bytes)?;
            let mut images = 0;
            let mut skipped = 0;
            let ids: Vec<_> = pdf
                .objects
                .iter()
                .filter_map(|(id, o)| {
                    o.as_stream()
                        .ok()
                        .filter(|s| {
                            s.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok())
                                == Some(b"Image")
                        })
                        .map(|_| *id)
                })
                .collect();
            let count = ids.len();
            for (index, id) in ids.into_iter().enumerate() {
                operation::checkpoint("Compressing images", index, count)?;
                let original = pdf.get_object(id)?.as_stream()?.clone();
                let Ok(image) = raw_image(&original) else {
                    skipped += 1;
                    continue;
                };
                let ratio =
                    (max_edge as f64 / f64::from(image.width().max(image.height()))).min(1.0);
                let width = (f64::from(image.width()) * ratio).round().max(1.0) as u32;
                let height = (f64::from(image.height()) * ratio).round().max(1.0) as u32;
                let mask = if let Ok(mask) = original.dict.get(b"SMask") {
                    let source = pdf.dereference(mask)?.1.as_stream()?;
                    let data = source.decompressed_content_with_limit(MAX_FILE)?;
                    let Some(gray) =
                        image::GrayImage::from_raw(image.width(), image.height(), data)
                    else {
                        skipped += 1;
                        continue;
                    };
                    let gray = image::imageops::resize(
                        &gray,
                        width,
                        height,
                        image::imageops::FilterType::Triangle,
                    );
                    let mut mask = source.clone();
                    mask.dict.remove(b"Filter");
                    mask.dict.remove(b"DecodeParms");
                    mask.dict.set("Width", width);
                    mask.dict.set("Height", height);
                    mask.set_content(gray.into_raw());
                    mask.compress()?;
                    Some(mask)
                } else {
                    None
                };
                let image = image::imageops::resize(
                    &image,
                    width,
                    height,
                    image::imageops::FilterType::Triangle,
                );
                let mut jpeg = vec![];
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
                    .encode(
                        image.as_raw(),
                        width,
                        height,
                        image::ExtendedColorType::Rgb8,
                    )
                    .map_err(|_| err("JPEG compression failed"))?;
                let mut stream = original.clone();
                stream.dict.set("Filter", "DCTDecode");
                stream.dict.remove(b"DecodeParms");
                stream.dict.set("Width", width);
                stream.dict.set("Height", height);
                stream.set_content(jpeg);
                if let Some(mask) = mask {
                    let id = pdf.add_object(mask);
                    stream.dict.set("SMask", id);
                }
                pdf.objects.insert(id, Object::Stream(stream));
                images += 1;
            }
            if images == 0 {
                return Ok(CompressionReport {
                    before,
                    after: before,
                    images: 0,
                    skipped,
                });
            }
            let candidate = self.candidate(doc, pdf)?;
            let after = candidate.bytes.len();
            if after >= before {
                return Ok(CompressionReport {
                    before,
                    after: before,
                    images: 0,
                    skipped,
                });
            }
            operation::checkpoint("Compressing images", count, count)?;
            doc.accept(candidate.bytes);
            Ok(CompressionReport {
                before,
                after,
                images,
                skipped,
            })
        })
    }
}
