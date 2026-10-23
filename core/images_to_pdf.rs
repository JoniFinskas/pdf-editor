use crate::*;
use lopdf::{Object, Stream, dictionary};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImagePageSize {
    #[default]
    A4,
    Letter,
    Image,
}

/// Decode PNG/JPEG, or HEIC using the installed Windows image codec.
pub fn import_picture(path: &Path) -> Result<Picture> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("heic") || e.eq_ignore_ascii_case("heif"))
    {
        decode_heic(&read_limited(path, MAX_FILE)?)
    } else {
        Picture::open(path)
    }
}

#[cfg(windows)]
fn decode_heic(bytes: &[u8]) -> Result<Picture> {
    use windows::{
        Graphics::Imaging::{
            BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform,
            ColorManagementMode, ExifOrientationMode,
        },
        Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    };
    let failure = |_: windows::core::Error| {
        err("HEIC decoding failed. Install the Windows HEIF/HEVC image extensions for this file.")
    };
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(failure)?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let stream = InMemoryRandomAccessStream::new().map_err(failure)?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(failure)?;
    writer.WriteBytes(bytes).map_err(failure)?;
    writer
        .StoreAsync()
        .map_err(failure)?
        .join()
        .map_err(failure)?;
    writer.DetachStream().map_err(failure)?;
    stream.Seek(0).map_err(failure)?;
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(failure)?
        .join()
        .map_err(failure)?;
    let width = decoder.OrientedPixelWidth().map_err(failure)?;
    let height = decoder.OrientedPixelHeight().map_err(failure)?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(err("Image must contain at most 16 million pixels"));
    }
    let provider = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &BitmapTransform::new().map_err(failure)?,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::ColorManageToSRgb,
        )
        .map_err(failure)?
        .join()
        .map_err(failure)?;
    let pixels = provider.DetachPixelData().map_err(failure)?;
    let picture = Picture {
        width,
        height,
        rgba: pixels.to_vec(),
    };
    validate_picture(&picture)?;
    Ok(picture)
}
#[cfg(not(windows))]
fn decode_heic(_: &[u8]) -> Result<Picture> {
    Err(err("HEIC import requires the Windows HEIF image codec"))
}

pub(crate) fn validate_picture(picture: &Picture) -> Result<()> {
    let pixels = u64::from(picture.width) * u64::from(picture.height);
    if pixels == 0 || pixels > 16_000_000 || pixels * 4 != picture.rgba.len() as u64 {
        return Err(err("Invalid image dimensions or pixel buffer"));
    }
    Ok(())
}

impl Engine {
    /// Create a new document in the supplied order, with one centered image per page.
    pub fn images_to_pdf(&self, paths: &[PathBuf], size: ImagePageSize) -> Result<Document> {
        if paths.is_empty() || paths.len() > 100 {
            return Err(err("Choose 1 to 100 images"));
        }
        self.pictures_to_pdf(paths.iter().map(|p| import_picture(p)), size)
    }
    pub(crate) fn pictures_to_pdf(
        &self,
        pictures: impl IntoIterator<Item = Result<Picture>>,
        size: ImagePageSize,
    ) -> Result<Document> {
        guarded(|| {
            let mut pdf = lopdf::Document::with_version("1.7");
            let pages = pdf.new_object_id();
            let mut kids = Vec::new();
            let mut total = 0;
            for picture in pictures {
                let picture = picture?;
                validate_picture(&picture)?;
                if kids.len() >= 100 {
                    return Err(err("Choose at most 100 images"));
                }
                let mut rgb = Vec::with_capacity(picture.rgba.len() / 4 * 3);
                for p in picture.rgba.chunks_exact(4) {
                    for c in &p[..3] {
                        rgb.push(
                            ((u32::from(*c) * u32::from(p[3]) + 255 * (255 - u32::from(p[3])))
                                / 255) as u8,
                        );
                    }
                }
                let mut jpeg = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
                    .encode(
                        &rgb,
                        picture.width,
                        picture.height,
                        image::ExtendedColorType::Rgb8,
                    )
                    .map_err(|_| err("Image encoding failed"))?;
                total += jpeg.len();
                if total > MAX_FILE - 1024 * 1024 {
                    return Err(err("Combined images exceed the 64 MiB document limit"));
                }
                let image = pdf.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>picture.width, "Height"=>picture.height, "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8, "Filter"=>"DCTDecode"}, jpeg));
                let (width, height) = match size {
                    ImagePageSize::A4 => (595.276, 841.89),
                    ImagePageSize::Letter => (612.0, 792.0),
                    ImagePageSize::Image => {
                        (picture.width as f32 * 0.75, picture.height as f32 * 0.75)
                    }
                };
                let scale = (width / picture.width as f32).min(height / picture.height as f32);
                let (w, h) = (picture.width as f32 * scale, picture.height as f32 * scale);
                let content = pdf.add_object(Stream::new(
                    dictionary! {},
                    format!(
                        "q {w} 0 0 {h} {} {} cm /Photo Do Q",
                        (width - w) / 2.0,
                        (height - h) / 2.0
                    )
                    .into_bytes(),
                ));
                let page = pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "MediaBox"=>vec![0.into(),0.into(),width.into(),height.into()], "Resources"=>dictionary!{"XObject"=>dictionary!{"Photo"=>image}}, "Contents"=>content});
                kids.push(Object::Reference(page));
            }
            if kids.is_empty() {
                return Err(err("Choose at least one image"));
            }
            pdf.objects.insert(
                pages,
                dictionary! {"Type"=>"Pages", "Count"=>kids.len() as i64,"Kids"=>kids}.into(),
            );
            let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
            pdf.trailer.set("Root", catalog);
            let seed = Document {
                bytes: vec![],
                original: PathBuf::from("images.pdf"),
                undo: vec![],
                redo: vec![],
                revision: 0,
                protected: false,
                substitution: Substitution::default(),
            };
            self.candidate(&seed, pdf)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_picture_is_rejected() {
        assert!(
            validate_picture(&Picture {
                width: u32::MAX,
                height: u32::MAX,
                rgba: vec![]
            })
            .is_err()
        );
        assert!(
            validate_picture(&Picture {
                width: 1,
                height: 1,
                rgba: vec![0; 4]
            })
            .is_ok()
        );
    }
}
