use crate::*;
use lopdf::{
    Object, Stream,
    content::{Content, Operation},
    dictionary,
};
#[derive(Clone, Debug)]
pub struct TextStamp {
    pub template: String,
    pub font: String,
    pub size: f32,
    pub color: [u8; 3],
    pub opacity: f32,
    pub x: f32,
    pub y: f32,
}
/// Position relative to the visible page, including crop and rotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampPlacement {
    Absolute,
    Header,
    Footer,
}
#[derive(Clone, Debug)]
pub struct RangeStamp {
    pub stamp: TextStamp,
    /// One-based ranges; empty means all pages.
    pub pages: String,
    /// Literal date value used for every selected page.
    pub date: String,
    pub placement: StampPlacement,
}
/// UTC date captured once when opening the stamping tool or starting a batch.
pub fn stamp_date_today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs() / 86400)
        .unwrap_or(0) as i64;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    format!("{:04}-{m:02}-{d:02}", y + i64::from(m <= 2))
}
fn stamp_matrix(
    pdf: &lopdf::Document,
    page: lopdf::ObjectId,
    stamp: &TextStamp,
    placement: StampPlacement,
) -> Result<[f32; 6]> {
    if placement == StampPlacement::Absolute {
        return Ok([1., 0., 0., 1., stamp.x, stamp.y]);
    }
    let bbox = pages::inherited(pdf, page, b"CropBox")?
        .or(pages::inherited(pdf, page, b"MediaBox")?)
        .ok_or_else(|| err("Missing page bounds"))?;
    let a = bbox.as_array()?;
    if a.len() != 4 {
        return Err(err("Invalid page bounds"));
    }
    let b: Vec<f32> = a
        .iter()
        .map(|v| v.as_float())
        .collect::<std::result::Result<_, _>>()?;
    if b.iter().any(|v| !v.is_finite()) || b[2] <= b[0] || b[3] <= b[1] {
        return Err(err("Invalid page bounds"));
    }
    let rotation = pages::inherited(pdf, page, b"Rotate")?
        .map(|v| v.as_i64())
        .transpose()?
        .unwrap_or(0)
        .rem_euclid(360);
    let height = if rotation == 90 || rotation == 270 {
        b[2] - b[0]
    } else {
        b[3] - b[1]
    };
    let width = if rotation == 90 || rotation == 270 {
        b[3] - b[1]
    } else {
        b[2] - b[0]
    };
    if stamp.x < 0. || stamp.x >= width || stamp.y < 0. || stamp.y + stamp.size >= height {
        return Err(err("Stamp margins lie outside the page"));
    }
    let x = stamp.x;
    let y = if placement == StampPlacement::Header {
        height - stamp.y - stamp.size
    } else {
        stamp.y
    };
    Ok(match rotation {
        0 => [1., 0., 0., 1., b[0] + x, b[1] + y],
        90 => [0., 1., -1., 0., b[2] - y, b[1] + x],
        180 => [-1., 0., 0., -1., b[2] - x, b[3] - y],
        270 => [0., -1., 1., 0., b[0] + y, b[3] - x],
        _ => return Err(err("Unsupported page rotation")),
    })
}
impl Engine {
    pub fn stamp_pages(&self, doc: &mut Document, stamp: &TextStamp) -> Result<()> {
        self.stamp_page_range(
            doc,
            &RangeStamp {
                stamp: stamp.clone(),
                pages: String::new(),
                date: stamp_date_today(),
                placement: StampPlacement::Absolute,
            },
        )
    }
    pub fn stamp_page_range(&self, doc: &mut Document, options: &RangeStamp) -> Result<()> {
        let stamp = &options.stamp;
        guarded(|| {
            if stamp.template.is_empty()
                || stamp.template.len() > 4096
                || stamp.template.chars().any(char::is_control)
                || ![stamp.size, stamp.opacity, stamp.x, stamp.y]
                    .iter()
                    .all(|v| v.is_finite())
                || !(1.0..=1000.0).contains(&stamp.size)
                || !(0.0..=1.0).contains(&stamp.opacity)
            {
                return Err(err("Invalid stamp text, size, position or opacity"));
            }
            let mut pdf = parse(&doc.bytes)?;
            let pages = pdf.get_pages();
            if options.date.len() > 128 || options.date.chars().any(char::is_control) {
                return Err(err("Invalid stamp date"));
            }
            let selected = if options.pages.trim().is_empty() {
                (0..pages.len() as u16).collect()
            } else {
                page_ranges(&options.pages, pages.len())?
            };
            let pages: Vec<_> = pages
                .into_iter()
                .filter(|(n, _)| selected.contains(&((*n - 1) as u16)))
                .collect();
            let total = self.pages(doc)?.len();
            let texts: Vec<_> = pages
                .iter()
                .map(|(n, _)| n)
                .map(|n| {
                    stamp
                        .template
                        .replace("{page}", &n.to_string())
                        .replace("{pages}", &total.to_string())
                        .replace("{date}", &options.date)
                })
                .collect();
            let alphabet = texts.join(" ");
            if alphabet.len() > 1_000_000 {
                return Err(err("Stamp text exceeds document limits"));
            }
            let font = fonts::find(&stamp.font, &alphabet)?;
            let embedded = fonts::embed(&mut pdf, &font, &alphabet)?;
            let gs = pdf.add_object(
                dictionary! {"Type"=>"ExtGState","ca"=>stamp.opacity,"CA"=>stamp.opacity},
            );
            for ((_, page), text) in pages.into_iter().zip(texts) {
                let matrix = stamp_matrix(&pdf, page, stamp, options.placement)?;
                let mut resources = content::resources(&pdf, page)?;
                let mut names = vec![];
                for (category, id, prefix) in [
                    (b"Font".as_slice(), embedded.id, "StampFont"),
                    (b"ExtGState".as_slice(), gs, "StampOpacity"),
                ] {
                    let mut entries = resources
                        .get(category)
                        .ok()
                        .and_then(|o| pdf.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .cloned()
                        .unwrap_or_default();
                    let mut name = format!("{prefix}{}", id.0);
                    while entries.has(name.as_bytes()) {
                        name.push('X');
                    }
                    entries.set(name.as_bytes(), id);
                    resources.set(category, entries);
                    names.push(name);
                }
                pdf.get_dictionary_mut(page)?.set("Resources", resources);
                let ops = Content {
                    operations: vec![
                        Operation::new("q", vec![]),
                        Operation::new("gs", vec![Object::Name(names[1].as_bytes().to_vec())]),
                        Operation::new("BT", vec![]),
                        Operation::new("Tr", vec![0.into()]),
                        Operation::new(
                            "Tf",
                            vec![
                                Object::Name(names[0].as_bytes().to_vec()),
                                stamp.size.into(),
                            ],
                        ),
                        Operation::new(
                            "rg",
                            stamp
                                .color
                                .iter()
                                .map(|c| Object::Real(f32::from(*c) / 255.0))
                                .collect(),
                        ),
                        Operation::new("Tm", matrix.into_iter().map(Object::Real).collect()),
                        Operation::new(
                            "Tj",
                            vec![Object::String(
                                embedded.encode(&text)?,
                                lopdf::StringFormat::Hexadecimal,
                            )],
                        ),
                        Operation::new("ET", vec![]),
                        Operation::new("Q", vec![]),
                    ],
                };
                let mut data = b"q\n".to_vec();
                data.extend(pdf.get_page_content_with_limit(page, MAX_STREAM)?);
                data.extend(b"\nQ\n");
                data.extend(ops.encode()?);
                let stream = pdf.add_object(Stream::new(lopdf::Dictionary::new(), data));
                pdf.get_dictionary_mut(page)?.set("Contents", stream);
            }
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
