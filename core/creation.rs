use crate::*;
use lopdf::{
    Dictionary, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlankPdf {
    pub width: f32,
    pub height: f32,
    pub pages: u16,
}
impl Default for BlankPdf {
    fn default() -> Self {
        Self {
            width: 595.276,
            height: 841.89,
            pages: 1,
        }
    }
}
impl BlankPdf {
    /// Keep explicit dimensions unless landscape is requested; never rotate a wide page back to portrait.
    pub fn with_landscape(mut self, landscape: bool) -> Self {
        if landscape && self.width < self.height {
            std::mem::swap(&mut self.width, &mut self.height);
        }
        self
    }
    pub fn validate(self) -> Result<()> {
        if !(1..=1000).contains(&self.pages)
            || [self.width, self.height]
                .iter()
                .any(|n| !n.is_finite() || !(1.0..=14400.).contains(n))
        {
            return Err(err(
                "Choose 1–1000 pages, with dimensions from 1 to 14400 points",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default)]
pub struct CvSection {
    pub heading: String,
    pub body: String,
}
#[derive(Clone, Debug)]
pub struct CvSpec {
    pub name: String,
    pub headline: String,
    pub contact: Vec<String>,
    pub sections: Vec<CvSection>,
    pub letter: bool,
}
impl Default for CvSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            headline: String::new(),
            contact: vec![],
            letter: false,
            sections: ["Summary", "Experience", "Education", "Skills"]
                .into_iter()
                .map(|heading| CvSection {
                    heading: heading.into(),
                    body: String::new(),
                })
                .collect(),
        }
    }
}
impl CvSpec {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(err("Enter a name."));
        }
        if self.name.len() > 500 {
            return Err(err("Name exceeds 500 bytes."));
        }
        if self.contact.len() > 10 {
            return Err(err("Use at most 10 contact lines."));
        }
        if self.sections.len() > 20 {
            return Err(err("Use at most 20 CV sections."));
        }
        for section in &self.sections {
            if !section.body.trim().is_empty() && section.heading.trim().is_empty() {
                return Err(err("Give each filled section a heading."));
            }
            if section.heading.len() > 120 || section.heading.contains(['\r', '\n']) {
                return Err(err("Keep section headings on one line, up to 120 bytes."));
            }
        }
        let strings = std::iter::once(self.name.as_str())
            .chain(std::iter::once(self.headline.as_str()))
            .chain(self.contact.iter().map(String::as_str))
            .chain(
                self.sections
                    .iter()
                    .flat_map(|s| [s.heading.as_str(), s.body.as_str()]),
            );
        let mut bytes = 0;
        for text in strings {
            bytes += text.len();
            if bytes > 64000 {
                return Err(err("CV text exceeds 64 KB. Shorten the content."));
            }
            if text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
            {
                return Err(err("Remove unsupported control characters from the CV."));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct CvCheck {
    pub pages: usize,
    pub bytes: usize,
    pub text: String,
    pub warnings: Vec<String>,
}

fn seed() -> Document {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    Document {
        bytes: vec![],
        original: PathBuf::from(format!(
            "Untitled-{}-{}.pdf",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )),
        undo: vec![],
        redo: vec![],
        revision: 0,
        protected: false,
        substitution: Substitution::default(),
    }
}
fn finish_pages(pdf: &mut lopdf::Document, root: lopdf::ObjectId, kids: Vec<Object>) {
    pdf.objects.insert(
        root,
        dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids}.into(),
    );
    let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    pdf.trailer.set("Root", catalog);
}
impl Engine {
    pub fn blank_document(&self, spec: BlankPdf) -> Result<Document> {
        guarded(|| {
            spec.validate()?;
            let mut pdf = lopdf::Document::with_version("1.7");
            let root = pdf.new_object_id();
            let mut kids = vec![];
            for index in 0..spec.pages {
                operation::checkpoint("Creating blank pages", index as usize, spec.pages as usize)?;
                let id=pdf.add_object(dictionary! {"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),spec.width.into(),spec.height.into()],"Resources"=>Dictionary::new()});
                kids.push(id.into());
            }
            finish_pages(&mut pdf, root, kids);
            self.candidate(&seed(), pdf)
        })
    }
    /// Single-column selectable text, with explicit font sizes and no text in headers/footers.
    pub fn create_cv(&self, spec: &CvSpec) -> Result<Document> {
        guarded(|| {
            spec.validate()?;
            let pieces = std::iter::once(spec.name.as_str())
                .chain(std::iter::once(spec.headline.as_str()))
                .chain(spec.contact.iter().map(String::as_str))
                .chain(
                    spec.sections
                        .iter()
                        .flat_map(|s| [s.heading.as_str(), s.body.as_str()]),
                );
            let strings: Vec<_> = pieces.collect();
            let alphabet = strings.join(" ").replace(['\r', '\n', '\t'], " ");
            let regular = fonts::find("Arial", &alphabet)?;
            let heading_alphabet = std::iter::once(spec.name.as_str())
                .chain(spec.sections.iter().map(|s| s.heading.as_str()))
                .collect::<Vec<_>>()
                .join(" ")
                .replace(['\r', '\n', '\t'], " ");
            let bold = fonts::find("Arial Bold", &heading_alphabet)?;
            let regular_face = FontRef::new(&regular.bytes).map_err(|_| err("Invalid CV font"))?;
            let bold_face = FontRef::new(&bold.bytes).map_err(|_| err("Invalid CV font"))?;
            let (width, height) = if spec.letter {
                (612., 792.)
            } else {
                (595.276, 841.89)
            };
            let max_width = width - 84.;
            let mut layout = Layout {
                pages: vec![vec![]],
                y: height - 42.,
                height,
            };
            layout.block(
                &wrap(&spec.name, 20., max_width, &bold_face)?,
                20.,
                true,
                0.,
            )?;
            if !spec.headline.trim().is_empty() {
                layout.block(
                    &wrap(&spec.headline, 11.5, max_width, &regular_face)?,
                    11.5,
                    false,
                    3.,
                )?;
            }
            for contact in spec.contact.iter().filter(|s| !s.trim().is_empty()) {
                layout.block(
                    &wrap(contact, 10.5, max_width, &regular_face)?,
                    10.5,
                    false,
                    2.,
                )?;
            }
            for section in &spec.sections {
                if section.body.trim().is_empty() {
                    continue;
                }
                let body = wrap(section.body.trim(), 11., max_width, &regular_face)?;
                let heading = wrap(&section.heading, 12., max_width, &bold_face)?;
                layout.space(
                    13. + heading.len() as f32 * 12. * 1.35
                        + 4.
                        + body.len().min(2) as f32 * 11. * 1.35,
                )?;
                layout.block(&heading, 12., true, 12.)?;
                layout.block(&body, 11., false, 4.)?;
            }
            let mut pdf = lopdf::Document::with_version("1.7");
            let root = pdf.new_object_id();
            let normal = fonts::embed(&mut pdf, &regular, &alphabet)?;
            let bold = fonts::embed(&mut pdf, &bold, &heading_alphabet)?;
            let mut kids = vec![];
            let mut expected = vec![];
            for (index, lines) in layout.pages.iter().enumerate() {
                operation::checkpoint("Building CV pages", index, layout.pages.len())?;
                let mut ops = vec![];
                for line in lines {
                    expected.push((index as u16, line.text.clone()));
                    ops.extend([
                        Operation::new("BT", vec![]),
                        Operation::new(
                            "Tf",
                            vec![
                                Object::Name(if line.bold {
                                    b"B".to_vec()
                                } else {
                                    b"R".to_vec()
                                }),
                                line.size.into(),
                            ],
                        ),
                        Operation::new(
                            "Tm",
                            vec![
                                1.into(),
                                0.into(),
                                0.into(),
                                1.into(),
                                42.into(),
                                line.y.into(),
                            ],
                        ),
                        Operation::new(
                            "Tj",
                            vec![Object::String(
                                if line.bold {
                                    bold.encode(&line.text)?
                                } else {
                                    normal.encode(&line.text)?
                                },
                                lopdf::StringFormat::Hexadecimal,
                            )],
                        ),
                        Operation::new("ET", vec![]),
                    ]);
                }
                let contents = pdf.add_object(Stream::new(
                    Dictionary::new(),
                    Content { operations: ops }.encode()?,
                ));
                let page=pdf.add_object(dictionary! {"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),width.into(),height.into()],"Resources"=>dictionary! {"Font"=>dictionary! {"R"=>normal.id,"B"=>bold.id}},"Contents"=>contents});
                kids.push(page.into());
            }
            finish_pages(&mut pdf, root, kids);
            pdf.compress();
            let candidate = self.candidate(&seed(), pdf)?;
            let mut actual = vec![];
            for page in 0..layout.pages.len() {
                for object in self.text_objects(&candidate, page as u16)? {
                    actual.push((page as u16, object.text));
                }
            }
            if actual != expected {
                return Err(err("CV text did not preserve its reading order"));
            }
            operation::checkpoint("Finishing CV", layout.pages.len(), layout.pages.len())?;
            Ok(candidate)
        })
    }
    pub fn check_cv(&self, doc: &Document) -> Result<CvCheck> {
        guarded(|| {
            let pages = self.pages(doc)?;
            if pages.len() > 100 {
                return Err(err("CV text checks support up to 100 pages"));
            }
            let native = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
            let mut text = String::new();
            let mut images = false;
            for (index, page) in native.pages().iter().enumerate() {
                operation::checkpoint("Reading CV text", index, pages.len())?;
                if index > 0 {
                    text.push_str("\n\n");
                }
                text.push_str(&page.text()?.all());
                if text.len() > 4_000_000 {
                    return Err(err("CV text exceeds preview limits"));
                }
                images |= self
                    .page_items(doc, index as u16)?
                    .iter()
                    .any(|i| i.kind.eq_ignore_ascii_case("Image"));
            }
            let mut warnings = vec![];
            if text.trim().is_empty() {
                warnings.push("No selectable text was found.".into());
            }
            if images {
                warnings
                    .push("Images are present. Keep essential details in selectable text.".into());
            }
            if doc.bytes.len() > 2_500_000 {
                warnings.push(
                    "File exceeds 2.5 MB. Check the application portal's upload limit.".into(),
                );
            }
            Ok(CvCheck {
                pages: pages.len(),
                bytes: doc.bytes.len(),
                text,
                warnings,
            })
        })
    }
}
struct Line {
    text: String,
    size: f32,
    bold: bool,
    y: f32,
}
struct Layout {
    pages: Vec<Vec<Line>>,
    y: f32,
    height: f32,
}
impl Layout {
    fn space(&mut self, height: f32) -> Result<()> {
        if self.y - height < 42. {
            if self.pages.len() >= 20 {
                return Err(err("CV exceeds 20 pages; shorten the content"));
            }
            self.pages.push(vec![]);
            self.y = self.height - 42.;
        }
        Ok(())
    }
    fn block(&mut self, lines: &[String], size: f32, bold: bool, gap: f32) -> Result<()> {
        self.y -= gap;
        for (index, text) in lines.iter().enumerate() {
            let line_height = size * 1.35;
            if lines.len() - index == 2 {
                self.space(line_height * 2.)?;
            } else {
                self.space(line_height)?;
            }
            self.y -= size;
            if !text.is_empty() {
                self.pages
                    .last_mut()
                    .ok_or_else(|| err("Missing CV page"))?
                    .push(Line {
                        text: text.clone(),
                        size,
                        bold,
                        y: self.y,
                    });
            }
            self.y -= line_height - size;
        }
        Ok(())
    }
}
fn wrap(text: &str, size: f32, width: f32, font: &FontRef<'_>) -> Result<Vec<String>> {
    let metrics = font.glyph_metrics(Size::new(size), LocationRef::default());
    let charmap = font.charmap();
    let measure = |text: &str| {
        text.chars().try_fold(0., |sum, c| {
            Ok::<_, Error>(
                sum + charmap
                    .map(c)
                    .and_then(|g| metrics.advance_width(g))
                    .ok_or_else(|| err("CV font cannot represent a character"))?,
            )
        })
    };
    let mut output = vec![];
    for source in text.lines() {
        let mut line = String::new();
        for word in source.split_whitespace() {
            if measure(word)? > width {
                return Err(err(
                    "A CV word or link is too wide. Shorten it or add a space.",
                ));
            }
            let proposed = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && measure(&proposed)? > width {
                output.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        output.push(line);
    }
    Ok(output)
}
