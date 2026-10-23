use crate::*;
use lopdf::{Object, Stream, dictionary};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingKind {
    Freehand,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
}
#[derive(Clone, Debug)]
pub struct Drawing {
    pub page: u16,
    pub kind: DrawingKind,
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 3],
    pub width: f32,
    pub opacity: f32,
}
impl Engine {
    pub fn add_drawing(&self, doc: &mut Document, drawing: &Drawing) -> Result<()> {
        guarded(|| {
            let d = drawing;
            if d.points.len() < 2
                || d.points.len() > 10000
                || !d
                    .points
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && v.abs() < 100000.0)
                || !d.width.is_finite()
                || !(0.1..=100.0).contains(&d.width)
                || !d.opacity.is_finite()
                || !(0.0..=1.0).contains(&d.opacity)
            {
                return Err(err("Invalid drawing geometry or style"));
            }
            let first = d.points[0];
            let last = *d.points.last().ok_or_else(|| err("Empty drawing"))?;
            let pad = d.width + 10.0;
            let left = d.points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) - pad;
            let bottom = d.points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) - pad;
            let right = d
                .points
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max)
                + pad;
            let top = d
                .points
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max)
                + pad;
            let (x, y) = (first[0] - left, first[1] - bottom);
            let (ex, ey) = (last[0] - left, last[1] - bottom);
            let mut path = String::new();
            let subtype = match d.kind {
                DrawingKind::Freehand => {
                    path.push_str(&format!("{x} {y} m "));
                    for p in d.points.iter().skip(1) {
                        path.push_str(&format!("{} {} l ", p[0] - left, p[1] - bottom));
                    }
                    path.push('S');
                    "Ink"
                }
                DrawingKind::Rectangle => {
                    path = format!(
                        "{} {} {} {} re S",
                        x.min(ex),
                        y.min(ey),
                        (ex - x).abs(),
                        (ey - y).abs()
                    );
                    "Square"
                }
                DrawingKind::Ellipse => {
                    let cx = (x + ex) / 2.0;
                    let cy = (y + ey) / 2.0;
                    let rx = (ex - x).abs() / 2.0;
                    let ry = (ey - y).abs() / 2.0;
                    let k = 0.5522848;
                    path = format!(
                        "{} {cy} m {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c S",
                        cx + rx,
                        cx + rx,
                        cy + ry * k,
                        cx + rx * k,
                        cy + ry,
                        cy + ry,
                        cx - rx * k,
                        cy + ry,
                        cx - rx,
                        cy + ry * k,
                        cx - rx,
                        cx - rx,
                        cy - ry * k,
                        cx - rx * k,
                        cy - ry,
                        cy - ry,
                        cx + rx * k,
                        cy - ry,
                        cx + rx,
                        cy - ry * k,
                        cx + rx
                    );
                    "Circle"
                }
                DrawingKind::Line | DrawingKind::Arrow => {
                    path = format!("{x} {y} m {ex} {ey} l S");
                    if d.kind == DrawingKind::Arrow {
                        let length = ((ex - x).powi(2) + (ey - y).powi(2)).sqrt();
                        if length < 0.1 {
                            return Err(err("Arrow must have a nonzero length"));
                        }
                        let ux = (ex - x) / length;
                        let uy = (ey - y) / length;
                        let head = (d.width * 3.0 + 6.0).min(length * 0.4);
                        path.push_str(&format!(
                            " {} {} m {ex} {ey} l {} {} l S",
                            ex - ux * head - uy * head * 0.5,
                            ey - uy * head + ux * head * 0.5,
                            ex - ux * head + uy * head * 0.5,
                            ey - uy * head - ux * head * 0.5
                        ));
                    }
                    "Line"
                }
            };
            let mut pdf = parse(&doc.bytes)?;
            let page = *pdf
                .get_pages()
                .get(&(u32::from(d.page) + 1))
                .ok_or_else(|| err("Invalid page"))?;
            let [r, g, b] = d.color.map(|c| f32::from(c) / 255.0);
            let ap=pdf.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),(right-left).into(),(top-bottom).into()],"Resources"=>dictionary!{"ExtGState"=>dictionary!{"GS"=>dictionary!{"CA"=>d.opacity,"ca"=>d.opacity}}}},format!("q /GS gs {r} {g} {b} RG {} w 1 J 1 j {path} Q",d.width).into_bytes()));
            let mut annotation = dictionary! {"Type"=>"Annot","Subtype"=>subtype,"P"=>page,"F"=>4,"Rect"=>vec![left.into(),bottom.into(),right.into(),top.into()],"C"=>vec![r.into(),g.into(),b.into()],"CA"=>d.opacity,"BS"=>dictionary!{"W"=>d.width,"S"=>"S"},"AP"=>dictionary!{"N"=>ap}};
            match d.kind {
                DrawingKind::Freehand => annotation.set(
                    "InkList",
                    vec![Object::Array(
                        d.points
                            .iter()
                            .flatten()
                            .map(|v| Object::Real(*v))
                            .collect(),
                    )],
                ),
                DrawingKind::Line | DrawingKind::Arrow => {
                    annotation.set(
                        "L",
                        vec![
                            first[0].into(),
                            first[1].into(),
                            last[0].into(),
                            last[1].into(),
                        ],
                    );
                    annotation.set(
                        "LE",
                        vec![
                            Object::Name(b"None".to_vec()),
                            Object::Name(if d.kind == DrawingKind::Arrow {
                                b"OpenArrow".to_vec()
                            } else {
                                b"None".to_vec()
                            }),
                        ],
                    );
                }
                _ => annotation.set("RD", vec![Object::Real(pad); 4]),
            }
            let id = pdf.add_object(annotation);
            let mut annots = pdf
                .get_dictionary(page)?
                .get(b"Annots")
                .ok()
                .and_then(|o| pdf.dereference(o).ok())
                .and_then(|(_, o)| o.as_array().ok())
                .cloned()
                .unwrap_or_default();
            annots.push(Object::Reference(id));
            pdf.get_dictionary_mut(page)?.set("Annots", annots);
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}
