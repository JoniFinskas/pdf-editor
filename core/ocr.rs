use crate::*;
use lopdf::{
    Dictionary, Object, Stream,
    content::{Content, Operation},
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};

pub(crate) struct RecognizedWord {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Engine {
    pub fn ocr_page(&self, doc: &mut Document, page: u16) -> Result<usize> {
        guarded(|| {
            if !self.text_objects(doc, page)?.is_empty() {
                return Err(err("This page already has a text layer"));
            }
            let info = self
                .pages(doc)?
                .get(page as usize)
                .ok_or_else(|| err("Invalid page"))?
                .clone();
            let scale = (200.0f32 / 72.0)
                .min(2500.0 / info.width.max(info.height))
                .min((12_000_000.0 / (info.width * info.height)).sqrt());
            let image = self.render(doc, page, scale)?;
            let words = recognize(&image)?;
            if words.is_empty() {
                return Err(err("Windows OCR did not recognize any text on this page"));
            }
            self.add_ocr_words(doc, page, scale, &words)?;
            Ok(words.len())
        })
    }
    pub(crate) fn add_ocr_words(
        &self,
        doc: &mut Document,
        page: u16,
        scale: f32,
        words: &[RecognizedWord],
    ) -> Result<()> {
        if words.len() > 10000
            || words.iter().any(|w| {
                w.text.is_empty()
                    || w.text.len() > 4096
                    || w.text.chars().any(char::is_control)
                    || ![w.x, w.y, w.width, w.height]
                        .iter()
                        .all(|v| v.is_finite() && v.abs() < 100000.0)
                    || w.width <= 0.0
                    || w.height <= 0.0
            })
        {
            return Err(err("Invalid OCR output"));
        }
        let alphabet = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if alphabet.len() > 1_000_000 {
            return Err(err("OCR text exceeds page limits"));
        }
        let font = fonts::find("Arial", &alphabet)?;
        let face = FontRef::new(&font.bytes).map_err(|_| err("Invalid OCR font"))?;
        let metrics = face.glyph_metrics(Size::new(1.0), LocationRef::default());
        let mut pdf = parse(&doc.bytes)?;
        let id = *pdf
            .get_pages()
            .get(&(u32::from(page) + 1))
            .ok_or_else(|| err("Invalid page"))?;
        let embedded = fonts::embed(&mut pdf, &font, &alphabet)?;
        let mut resources = content::resources(&pdf, id)?;
        let mut fonts = resources
            .get(b"Font")
            .ok()
            .and_then(|o| pdf.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
            .cloned()
            .unwrap_or_default();
        let mut name = format!("OCRFont{}", embedded.id.0);
        while fonts.has(name.as_bytes()) {
            name.push('X');
        }
        fonts.set(name.as_bytes(), embedded.id);
        resources.set("Font", fonts);
        pdf.get_dictionary_mut(id)?.set("Resources", resources);
        let mut ops = vec![
            Operation::new("q", vec![]),
            Operation::new("BT", vec![]),
            Operation::new("Tr", vec![3.into()]),
            Operation::new("Tf", vec![Object::Name(name.into_bytes()), 1.into()]),
        ];
        for word in words {
            let width = word
                .text
                .chars()
                .map(|c| {
                    face.charmap()
                        .map(c)
                        .and_then(|g| metrics.advance_width(g))
                        .unwrap_or(0.0)
                })
                .sum::<f32>();
            if width <= 0.0 {
                return Err(err("OCR font cannot represent a recognized word"));
            }
            let point = |x: f32, y: f32| {
                self.pixel_to_page(doc, page, scale, x.round() as i32, y.round() as i32)
            };
            let (x, y) = point(word.x, word.y + word.height * 0.85)?;
            let (right_x, right_y) = point(word.x + word.width, word.y + word.height * 0.85)?;
            let (top_x, top_y) = point(word.x, word.y - word.height * 0.15)?;
            ops.push(Operation::new(
                "Tm",
                vec![
                    ((right_x - x) / width).into(),
                    ((right_y - y) / width).into(),
                    (top_x - x).into(),
                    (top_y - y).into(),
                    x.into(),
                    y.into(),
                ],
            ));
            ops.push(Operation::new(
                "Tj",
                vec![Object::String(
                    embedded.encode(&word.text)?,
                    lopdf::StringFormat::Hexadecimal,
                )],
            ));
        }
        ops.extend([Operation::new("ET", vec![]), Operation::new("Q", vec![])]);
        let mut data = b"q\n".to_vec();
        data.extend(pdf.get_page_content_with_limit(id, MAX_STREAM)?);
        data.extend(b"\nQ\n");
        data.extend(Content { operations: ops }.encode()?);
        let stream = pdf.add_object(Stream::new(Dictionary::new(), data));
        pdf.get_dictionary_mut(id)?.set("Contents", stream);
        let candidate = self.candidate(doc, pdf)?;
        let actual = self.text_objects(&candidate, page)?;
        for word in words {
            if !actual.iter().any(|o| o.text == word.text) {
                return Err(err("OCR text layer failed verification"));
            }
        }
        doc.accept(candidate.bytes);
        Ok(())
    }
}
#[cfg(windows)]
fn recognize(image: &Image) -> Result<Vec<RecognizedWord>> {
    use windows::{
        Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    };
    let failure = |_: windows::core::Error| {
        err("Windows OCR is unavailable; install an OCR language in Windows language settings")
    };
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(failure)?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(failure)?;
    let maximum = OcrEngine::MaxImageDimension().map_err(failure)?;
    if image.width > maximum || image.height > maximum {
        return Err(err("Page exceeds the Windows OCR image limit"));
    }
    let mut bgra = image.rgba.clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let writer = DataWriter::new().map_err(failure)?;
    writer.WriteBytes(&bgra).map_err(failure)?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &writer.DetachBuffer().map_err(failure)?,
        BitmapPixelFormat::Bgra8,
        image.width as i32,
        image.height as i32,
    )
    .map_err(failure)?;
    let recognized = engine
        .RecognizeAsync(&bitmap)
        .map_err(failure)?
        .join()
        .map_err(failure)?;
    let mut words = vec![];
    for line in recognized.Lines().map_err(failure)? {
        for word in line.Words().map_err(failure)? {
            let rect = word.BoundingRect().map_err(failure)?;
            words.push(RecognizedWord {
                text: word.Text().map_err(failure)?.to_string(),
                x: rect.X,
                y: rect.Y,
                width: rect.Width,
                height: rect.Height,
            });
            if words.len() > 10000 {
                return Err(err("Windows OCR returned too many words"));
            }
        }
    }
    Ok(words)
}
#[cfg(not(windows))]
fn recognize(_: &Image) -> Result<Vec<RecognizedWord>> {
    Err(err("OCR requires Windows"))
}
