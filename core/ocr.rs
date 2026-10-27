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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OcrReport {
    pub pages_recognized: usize,
    pub words_added: usize,
    pub skipped_text: usize,
    pub skipped_empty: usize,
}

impl Engine {
    pub fn ocr_page(&self, doc: &mut Document, page: u16) -> Result<usize> {
        if !self.text_objects(doc, page)?.is_empty() {
            return Err(err("This page already has a text layer"));
        }
        let mut recognizer = Recognizer::default();
        let report = self.ocr_pages_with(doc, &[page], |_, image| recognizer.recognize(image))?;
        if report.pages_recognized == 0 {
            return Err(err("Windows OCR did not recognize any text on this page"));
        }
        Ok(report.words_added)
    }

    /// Recognizes selected pages sequentially and publishes one atomic undo step.
    pub fn ocr_pages(&self, doc: &mut Document, pages: &[u16]) -> Result<OcrReport> {
        let mut recognizer = Recognizer::default();
        self.ocr_pages_with(doc, pages, |_, image| recognizer.recognize(image))
    }

    pub(crate) fn ocr_pages_with(
        &self,
        doc: &mut Document,
        selected: &[u16],
        mut recognize: impl FnMut(u16, &Image) -> Result<Vec<RecognizedWord>>,
    ) -> Result<OcrReport> {
        let pages: std::collections::BTreeSet<_> = selected.iter().copied().collect();
        if selected.is_empty() || selected.len() > 100 || pages.len() != selected.len() {
            return Err(err("Select 1–100 valid pages once each for OCR"));
        }
        let total = pages.len() * 3;
        operation::checkpoint("Checking OCR pages", 0, total)?;
        let info = self.pages(doc)?;
        operation::check_cancelled()?;
        if pages.iter().any(|p| usize::from(*p) >= info.len()) {
            return Err(err("Select 1–100 valid pages once each for OCR"));
        }
        let mut report = OcrReport::default();
        let mut staged = doc.comparison_snapshot();
        for (index, page) in pages.into_iter().enumerate() {
            let base = index * 3;
            operation::checkpoint("Rendering OCR page", base, total)?;
            if !self.text_objects(&staged, page)?.is_empty() {
                report.skipped_text += 1;
                operation::checkpoint("Skipped page with text", base + 3, total)?;
                continue;
            }
            let size = &info[usize::from(page)];
            let scale = (200.0f32 / 72.0)
                .min(2500.0 / size.width.max(size.height))
                .min((12_000_000.0 / (size.width * size.height)).sqrt());
            let image = self.render(&staged, page, scale)?;
            operation::checkpoint("Recognizing text", base + 1, total)?;
            // No PDFium handles or operation mutex survive into native OCR.
            let words = recognize(page, &image)?;
            drop(image);
            operation::checkpoint("Adding OCR text", base + 2, total)?;
            if words.is_empty() {
                report.skipped_empty += 1;
            } else {
                if report.words_added.saturating_add(words.len()) > 100_000 {
                    return Err(err("OCR exceeds the 100,000-word batch limit"));
                }
                staged.bytes = guarded(|| self.ocr_layer_bytes(&staged, page, scale, &words))?;
                report.pages_recognized += 1;
                report.words_added += words.len();
            }
            operation::checkpoint("Recognizing selected pages", base + 3, total)?;
        }
        operation::checkpoint("Finishing OCR", total, total)?;
        if report.pages_recognized > 0 {
            doc.accept(staged.bytes);
        }
        Ok(report)
    }
    #[cfg(test)]
    pub(crate) fn add_ocr_words(
        &self,
        doc: &mut Document,
        page: u16,
        scale: f32,
        words: &[RecognizedWord],
    ) -> Result<()> {
        guarded(|| {
            let bytes = self.ocr_layer_bytes(doc, page, scale, words)?;
            operation::checkpoint("Adding OCR text", 3, 3)?;
            doc.accept(bytes);
            Ok(())
        })
    }
    fn ocr_layer_bytes(
        &self,
        doc: &Document,
        page: u16,
        scale: f32,
        words: &[RecognizedWord],
    ) -> Result<Vec<u8>> {
        let before = self.text_objects(doc, page)?;
        if words.is_empty()
            || words.len() > 10000
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
        let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
        let native_page = native.pages().get(i32::from(page))?;
        let config = render_config(&native_page, scale)?;
        #[cfg(test)]
        MAPPING_SETUPS.with(|count| count.set(count.get() + 1));
        let mut expected = Vec::with_capacity(words.len());
        for word in words {
            operation::check_cancelled()?;
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
                let (x, y) =
                    native_page.pixels_to_points(x.round() as i32, y.round() as i32, &config)?;
                Ok::<_, Error>((x.value, y.value))
            };
            let (x, y) = point(word.x, word.y + word.height * 0.85)?;
            let (right_x, right_y) = point(word.x + word.width, word.y + word.height * 0.85)?;
            let (top_x, top_y) = point(word.x, word.y - word.height * 0.15)?;
            let matrix = [
                (right_x - x) / width,
                (right_y - y) / width,
                top_x - x,
                top_y - y,
                x,
                y,
            ];
            expected.push(matrix);
            ops.push(Operation::new(
                "Tm",
                matrix.into_iter().map(Object::Real).collect(),
            ));
            ops.push(Operation::new(
                "Tj",
                vec![Object::String(
                    embedded.encode(&word.text)?,
                    lopdf::StringFormat::Hexadecimal,
                )],
            ));
        }
        drop(native_page);
        drop(native);
        ops.extend([Operation::new("ET", vec![]), Operation::new("Q", vec![])]);
        let mut data = b"q\n".to_vec();
        data.extend(pdf.get_page_content_with_limit(id, MAX_STREAM)?);
        data.extend(b"\nQ\n");
        data.extend(Content { operations: ops }.encode()?);
        let stream = pdf.add_object(Stream::new(Dictionary::new(), data));
        pdf.get_dictionary_mut(id)?.set("Contents", stream);
        let candidate = self.candidate(doc, pdf)?;
        let actual = self.text_objects(&candidate, page)?;
        if actual.len() != before.len() + words.len() {
            return Err(err("OCR text layer failed verification"));
        }
        for (selected, actual) in before.iter().zip(&actual) {
            validate_text_selection(selected, actual)?;
        }
        for ((word, matrix), actual) in words.iter().zip(expected).zip(&actual[before.len()..]) {
            if actual.text != word.text
                || actual
                    .matrix
                    .iter()
                    .zip(matrix)
                    .any(|(a, b)| (a - b).abs() > 0.001)
            {
                return Err(err("OCR text layer failed verification"));
            }
        }
        Ok(candidate.bytes)
    }
}
#[cfg(test)]
thread_local! { pub(crate) static MAPPING_SETUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(Default)]
struct Recognizer {
    #[cfg(windows)]
    native: Option<NativeRecognizer>,
}
#[cfg(windows)]
struct NativeRecognizer {
    engine: windows::Media::Ocr::OcrEngine,
    // Fields drop in declaration order: engine must release before apartment.
    _apartment: Apartment,
}
#[cfg(windows)]
struct Apartment;
#[cfg(windows)]
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::WinRT::RoUninitialize() };
    }
}
impl Recognizer {
    #[cfg(windows)]
    fn recognize(&mut self, image: &Image) -> Result<Vec<RecognizedWord>> {
        use windows::{
            Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
            Media::Ocr::OcrEngine,
            Storage::Streams::DataWriter,
            Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
        };
        let failure = |_: windows::core::Error| {
            err("Windows OCR is unavailable; install an OCR language in Windows language settings")
        };
        if self.native.is_none() {
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(failure)?;
            let apartment = Apartment;
            let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(failure)?;
            self.native = Some(NativeRecognizer {
                engine,
                _apartment: apartment,
            });
        }
        let engine = &self
            .native
            .as_ref()
            .ok_or_else(|| err("Windows OCR is unavailable"))?
            .engine;
        let maximum = OcrEngine::MaxImageDimension().map_err(failure)?;
        if image.width > maximum || image.height > maximum {
            return Err(err("Page exceeds the Windows OCR image limit"));
        }
        operation::check_cancelled()?;
        let mut bgra = image.rgba.clone();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let writer = DataWriter::new().map_err(failure)?;
        writer.WriteBytes(&bgra).map_err(failure)?;
        drop(bgra);
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &writer.DetachBuffer().map_err(failure)?,
            BitmapPixelFormat::Bgra8,
            image.width as i32,
            image.height as i32,
        )
        .map_err(failure)?;
        let job = engine.RecognizeAsync(&bitmap).map_err(failure)?;
        let mut cancelled = false;
        while job.Status().map_err(failure)?.0 == 0 {
            if !cancelled && operation::check_cancelled().is_err() {
                cancelled = true;
                let _ = job.Cancel();
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        let result = job.join();
        if cancelled {
            return Err(err("Operation cancelled"));
        }
        operation::check_cancelled()?;
        let recognized = result.map_err(failure)?;
        let mut words = vec![];
        for line in recognized.Lines().map_err(failure)? {
            for word in line.Words().map_err(failure)? {
                operation::check_cancelled()?;
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
    fn recognize(&mut self, _: &Image) -> Result<Vec<RecognizedWord>> {
        Err(err("OCR requires Windows"))
    }
}
