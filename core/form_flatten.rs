use crate::*;
use lopdf::{Dictionary, Object, Stream, dictionary};

fn array(pdf: &lopdf::Document, d: &Dictionary, key: &[u8]) -> Result<Vec<Object>> {
    match d.get(key) {
        Ok(o) => Ok(pdf.dereference(o)?.1.as_array()?.clone()),
        Err(_) => Ok(vec![]),
    }
}
fn four(o: &Object) -> Result<[f32; 4]> {
    let a = o.as_array()?;
    if a.len() != 4 {
        return Err(err("Invalid appearance rectangle"));
    }
    let b = [
        a[0].as_float()?,
        a[1].as_float()?,
        a[2].as_float()?,
        a[3].as_float()?,
    ];
    if b.iter().any(|v| !v.is_finite() || v.abs() > 100000.) || b[2] <= b[0] || b[3] <= b[1] {
        return Err(err("Invalid appearance rectangle"));
    }
    Ok(b)
}
impl Engine {
    /// Bake each widget's selected normal appearance into its page, preserving other annotations.
    pub fn flatten_forms(&self, doc: &mut Document) -> Result<usize> {
        guarded(|| {
            let mut pdf = parse(&doc.bytes)?;
            let mut count = 0;
            for (_, page) in pdf.get_pages() {
                let annotations = array(&pdf, pdf.get_dictionary(page)?, b"Annots")?;
                let mut keep = vec![];
                let mut paint = String::new();
                let mut resources = content::resources(&pdf, page)?;
                let mut xobjects = resources
                    .get(b"XObject")
                    .ok()
                    .map(|o| pdf.dereference(o).and_then(|(_, o)| o.as_dict()))
                    .transpose()?
                    .cloned()
                    .unwrap_or_default();
                for object in annotations {
                    let d = pdf.dereference(&object)?.1.as_dict()?.clone();
                    if d.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Widget") {
                        keep.push(object);
                        continue;
                    }
                    count += 1;
                    if count > 10000 {
                        return Err(err("Too many form fields"));
                    }
                    let flags = d.get(b"F").and_then(Object::as_i64).unwrap_or(0);
                    if flags & (1 | 2 | 32) != 0 {
                        continue;
                    }
                    let ap = d
                        .get_deref(b"AP", &pdf)
                        .and_then(Object::as_dict)
                        .map_err(|_| err("A field has no appearance; fill it before flattening"))?;
                    let mut normal = ap.get(b"N")?.clone();
                    if let Ok(states) = pdf.dereference(&normal)?.1.as_dict() {
                        let state = d
                            .get(b"AS")
                            .and_then(Object::as_name)
                            .map_err(|_| err("A field has no selected appearance"))?;
                        normal = states.get(state)?.clone();
                    }
                    let stream = pdf.dereference(&normal)?.1.as_stream()?.clone();
                    let bbox = four(stream.dict.get(b"BBox")?)?;
                    let rect = four(d.get(b"Rect")?)?;
                    let mut matrix = [1., 0., 0., 1., 0., 0.];
                    if let Ok(m) = stream.dict.get(b"Matrix") {
                        let m = m.as_array()?;
                        if m.len() != 6 {
                            return Err(err("Invalid appearance matrix"));
                        }
                        for (i, v) in m.iter().enumerate() {
                            matrix[i] = v.as_float()?;
                        }
                    }
                    if matrix.iter().any(|v| !v.is_finite() || v.abs() > 100000.) {
                        return Err(err("Invalid appearance matrix"));
                    }
                    let [a, b, c, d, e, f] = matrix;
                    let points = [
                        (bbox[0], bbox[1]),
                        (bbox[0], bbox[3]),
                        (bbox[2], bbox[1]),
                        (bbox[2], bbox[3]),
                    ]
                    .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
                    let minx = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
                    let maxx = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
                    let miny = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
                    let maxy = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
                    if maxx - minx < 0.0001 || maxy - miny < 0.0001 {
                        return Err(err("Degenerate field appearance"));
                    }
                    let sx = (rect[2] - rect[0]) / (maxx - minx);
                    let sy = (rect[3] - rect[1]) / (maxy - miny);
                    let tx = rect[0] - minx * sx;
                    let ty = rect[1] - miny * sy;
                    let id = match normal.as_reference() {
                        Ok(id) => id,
                        Err(_) => pdf.add_object(stream),
                    };
                    let mut name = format!("FlattenedField{count}");
                    while xobjects.has(name.as_bytes()) {
                        name.push('_');
                    }
                    xobjects.set(name.as_bytes(), id);
                    paint.push_str(&format!(
                        "q {} {} {} {} re W n {sx} 0 0 {sy} {tx} {ty} cm /{name} Do Q\n",
                        rect[0],
                        rect[1],
                        rect[2] - rect[0],
                        rect[3] - rect[1]
                    ));
                }
                if !paint.is_empty() {
                    resources.set("XObject", xobjects);
                    pdf.get_dictionary_mut(page)?.set("Resources", resources);
                    // Isolate old content's graphics state from the appended appearances.
                    let before = pdf.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
                    let after = pdf.add_object(Stream::new(
                        dictionary! {},
                        format!("\nQ\n{paint}").into_bytes(),
                    ));
                    let mut streams = vec![Object::Reference(before)];
                    if let Ok(contents) = pdf.get_dictionary(page)?.get(b"Contents") {
                        let resolved = pdf.dereference(contents)?.1;
                        if let Ok(items) = resolved.as_array() {
                            streams.extend(items.clone());
                        } else {
                            streams.push(contents.clone());
                        }
                    }
                    streams.push(after.into());
                    pdf.get_dictionary_mut(page)?.set("Contents", streams);
                }
                pdf.get_dictionary_mut(page)?.set("Annots", keep);
            }
            if count == 0 {
                return Ok(0);
            }
            // No widget or editable field roots remain; pruning removes detached values and actions.
            pdf.catalog_mut()?.remove(b"AcroForm");
            let candidate = self.candidate(doc, pdf)?;
            if !self.form_fields(&candidate)?.is_empty() {
                return Err(err("Some fields could not be flattened"));
            }
            doc.accept(candidate.bytes);
            Ok(count)
        })
    }
}
