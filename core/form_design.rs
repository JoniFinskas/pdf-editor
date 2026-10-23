use crate::*;
use lopdf::{Dictionary, Object, ObjectId, Stream, dictionary, text_string as text};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesignFieldKind {
    Text,
    Date,
    Checkbox,
    Radio { options: Vec<String> },
    Dropdown { options: Vec<String> },
}
#[derive(Clone, Debug)]
pub struct NewFormField {
    pub page: u16,
    pub name: String,
    pub required: bool,
    pub bounds: Bounds,
    pub kind: DesignFieldKind,
}
#[derive(Clone, Debug)]
pub struct DesignedField {
    pub id: ObjectId,
    pub revision: u64,
    pub page: u16,
    pub name: String,
    pub required: bool,
    pub bounds: Bounds,
    pub kind: String,
}
#[derive(Clone, Debug)]
pub enum FormDesignEdit {
    Add(NewFormField),
    Update {
        field: DesignedField,
        name: String,
        required: bool,
        bounds: Bounds,
    },
    Delete(DesignedField),
}

fn array(pdf: &lopdf::Document, dict: &Dictionary, key: &[u8]) -> Result<Vec<Object>> {
    match dict.get(key) {
        Ok(o) => Ok(pdf.dereference(o)?.1.as_array()?.clone()),
        Err(_) => Ok(vec![]),
    }
}
fn inherited(pdf: &lopdf::Document, id: ObjectId, key: &[u8]) -> Result<Object> {
    let mut id = id;
    for _ in 0..64 {
        let d = pdf.get_dictionary(id)?;
        if let Ok(v) = d.get(key) {
            return Ok(v.clone());
        }
        id = d.get(b"Parent")?.as_reference()?;
    }
    Err(err("Excessive form field nesting"))
}
fn rect_object(b: Bounds) -> Object {
    Object::Array(vec![
        b.left.into(),
        b.bottom.into(),
        b.right.into(),
        b.top.into(),
    ])
}
fn read_rect(o: &Object) -> Result<Bounds> {
    let a = o.as_array()?;
    if a.len() != 4 {
        return Err(err("Invalid field rectangle"));
    }
    let values = [
        a[0].as_float()?,
        a[1].as_float()?,
        a[2].as_float()?,
        a[3].as_float()?,
    ];
    if values.iter().any(|v| !v.is_finite() || v.abs() > 100000.0) {
        return Err(err("Invalid field rectangle"));
    }
    Ok(Bounds {
        left: values[0].min(values[2]),
        bottom: values[1].min(values[3]),
        right: values[0].max(values[2]),
        top: values[1].max(values[3]),
    })
}
fn validate(name: &str, b: Bounds) -> Result<()> {
    if name.trim().is_empty() || name.len() > 256 || name.contains(['\0', '.']) {
        return Err(err("Use a field name of 1–256 bytes without dots or NUL"));
    }
    if [b.left, b.bottom, b.right, b.top]
        .iter()
        .any(|n| !n.is_finite() || n.abs() > 100000.)
        || b.right - b.left < 4.
        || b.top - b.bottom < 4.
    {
        return Err(err("Field must be at least 4 points wide and high"));
    }
    Ok(())
}
fn appearance(pdf: &mut lopdf::Document, w: f32, h: f32, on: bool) -> ObjectId {
    let mut content = format!(
        "q 1 g 0 0 {w} {h} re f 0.35 G 1 w 0.5 0.5 {} {} re S",
        w - 1.,
        h - 1.
    );
    if on {
        content.push_str(&format!(
            " 0 G 2 w 3 3 m {} {} l 3 {} m {} 3 l S",
            w - 3.,
            h - 3.,
            h - 3.,
            w - 3.
        ));
    }
    content.push_str(" Q");
    pdf.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),w.into(),h.into()],"Resources"=>Dictionary::new()},content.into_bytes()))
}
impl Engine {
    pub fn designed_fields(&self, doc: &Document) -> Result<Vec<DesignedField>> {
        guarded(|| {
            let pdf = parse(&doc.bytes)?;
            let mut fields = vec![];
            for (page_no, page) in pdf.get_pages() {
                for obj in array(&pdf, pdf.get_dictionary(page)?, b"Annots")? {
                    let id = obj.as_reference()?;
                    let d = pdf.get_dictionary(id)?;
                    if d.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Widget") {
                        continue;
                    }
                    let ft = inherited(&pdf, id, b"FT")?;
                    let flags = inherited(&pdf, id, b"Ff")
                        .ok()
                        .and_then(|o| o.as_i64().ok())
                        .unwrap_or(0);
                    let name = inherited(&pdf, id, b"T")
                        .ok()
                        .and_then(|o| lopdf::decode_text_string(&o).ok())
                        .unwrap_or_default();
                    let kind = match ft.as_name()? {
                        b"Tx" => {
                            if d.has(b"PdfEditorDate") {
                                "Date"
                            } else {
                                "Text"
                            }
                        }
                        b"Btn" => {
                            if flags & 32768 != 0 {
                                "Radio"
                            } else {
                                "Checkbox"
                            }
                        }
                        b"Ch" => "Dropdown",
                        _ => continue,
                    };
                    fields.push(DesignedField {
                        id,
                        revision: doc.revision,
                        page: (page_no - 1) as u16,
                        name,
                        required: flags & 2 != 0,
                        bounds: read_rect(d.get(b"Rect")?)?,
                        kind: kind.into(),
                    });
                    if fields.len() > 10000 {
                        return Err(err("Too many form fields"));
                    }
                }
            }
            Ok(fields)
        })
    }
    pub fn design_form(&self, doc: &mut Document, edit: FormDesignEdit) -> Result<()> {
        guarded(|| {
            let mut pdf = parse(&doc.bytes)?;
            match edit {
                FormDesignEdit::Add(f) => {
                    validate(&f.name, f.bounds)?;
                    if self.designed_fields(doc)?.iter().any(|o| o.name == f.name) {
                        return Err(err("A field with this name already exists"));
                    }
                    let page = *pdf
                        .get_pages()
                        .get(&(u32::from(f.page) + 1))
                        .ok_or_else(|| err("Invalid field page"))?;
                    let mut annots = array(&pdf, pdf.get_dictionary(page)?, b"Annots")?;
                    let mut acro = match pdf.catalog()?.get(b"AcroForm") {
                        Ok(o) => pdf.dereference(o)?.1.as_dict()?.clone(),
                        Err(_) => Dictionary::new(),
                    };
                    let mut roots = array(&pdf, &acro, b"Fields")?;
                    let font = pdf.add_object(
                        dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"},
                    );
                    let mut dr = acro
                        .get(b"DR")
                        .ok()
                        .and_then(|o| pdf.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .cloned()
                        .unwrap_or_default();
                    let mut fonts = dr
                        .get(b"Font")
                        .ok()
                        .and_then(|o| pdf.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .cloned()
                        .unwrap_or_default();
                    let mut font_name = format!("PdfEditorFormFont{}", font.0);
                    while fonts.has(font_name.as_bytes()) {
                        font_name.push('_');
                    }
                    fonts.set(font_name.as_bytes(), font);
                    dr.set("Font", fonts);
                    acro.set("DR", dr);
                    let (ft, flags) = match &f.kind {
                        DesignFieldKind::Text | DesignFieldKind::Date => ("Tx", 0),
                        DesignFieldKind::Checkbox => ("Btn", 0),
                        DesignFieldKind::Radio { .. } => ("Btn", 32768),
                        DesignFieldKind::Dropdown { .. } => ("Ch", 131072),
                    };
                    let mut base = dictionary! {"FT"=>ft,"T"=>text(&f.name),"Ff"=>flags+if f.required {2}else{0},"DA"=>Object::string_literal(format!("/{font_name} 12 Tf 0 g"))};
                    let radio = matches!(f.kind, DesignFieldKind::Radio { .. });
                    let options = match &f.kind {
                        DesignFieldKind::Radio { options }
                        | DesignFieldKind::Dropdown { options } => options.clone(),
                        _ => vec![String::new()],
                    };
                    if options.is_empty()
                        || options.len() > 100
                        || options.iter().any(|o| o.len() > 256 || o.contains('\0'))
                    {
                        return Err(err("Provide 1–100 options, each at most 256 bytes"));
                    }
                    if radio && options.len() < 2 {
                        return Err(err("A radio group needs at least two options"));
                    }
                    if matches!(f.kind, DesignFieldKind::Dropdown { .. }) {
                        base.set("Opt", options.iter().map(|s| text(s)).collect::<Vec<_>>());
                    }
                    if matches!(f.kind, DesignFieldKind::Date) {
                        base.set("PdfEditorDate", true);
                        base.set("TU", text("Date (YYYY-MM-DD)"));
                    }
                    let parent = if radio {
                        Some(pdf.add_object(base.clone()))
                    } else {
                        None
                    };
                    let mut kids = vec![];
                    let count = if radio { options.len() } else { 1 };
                    for (n, option) in options.iter().take(count).enumerate() {
                        let mut b = f.bounds;
                        if radio {
                            let height = (b.top - b.bottom) / count as f32;
                            if height < 4. {
                                return Err(err("Radio group is too short"));
                            }
                            b.top -= height * n as f32;
                            b.bottom = b.top - height;
                        }
                        let mut d = if radio {
                            Dictionary::new()
                        } else {
                            base.clone()
                        };
                        d.set("Type", "Annot");
                        d.set("Subtype", "Widget");
                        d.set("Rect", rect_object(b));
                        d.set("F", 4);
                        d.set("P", page);
                        let off = appearance(&mut pdf, b.right - b.left, b.top - b.bottom, false);
                        if ft == "Btn" {
                            let on = appearance(&mut pdf, b.right - b.left, b.top - b.bottom, true);
                            let state = if radio {
                                format!("Option{}", n + 1)
                            } else {
                                "Yes".into()
                            };
                            let mut normal = dictionary! {"Off"=>off};
                            normal.set(state, on);
                            d.set("AP", dictionary! {"N"=>normal});
                            d.set("AS", "Off");
                            if !radio {
                                d.set("V", "Off");
                            }
                        } else {
                            d.set("AP", dictionary! {"N"=>off});
                            d.set("MK",dictionary!{"BC"=>vec![0.35.into(),0.35.into(),0.35.into()],"BG"=>vec![1.into(),1.into(),1.into()]});
                        }
                        if let Some(parent) = parent {
                            d.set("Parent", parent);
                            d.set("TU", text(option));
                        }
                        let id = pdf.add_object(d);
                        annots.push(id.into());
                        kids.push(Object::Reference(id));
                    }
                    if let Some(parent) = parent {
                        pdf.get_dictionary_mut(parent)?.set("Kids", kids);
                        roots.push(parent.into());
                    } else {
                        roots.extend(kids);
                    }
                    pdf.get_dictionary_mut(page)?.set("Annots", annots);
                    acro.set("Fields", roots);
                    pdf.catalog_mut()?.set("AcroForm", acro);
                }
                FormDesignEdit::Update {
                    field,
                    name,
                    required,
                    bounds,
                } => {
                    self.check_designed(doc, &field)?;
                    validate(&name, bounds)?;
                    if self
                        .designed_fields(doc)?
                        .iter()
                        .any(|o| o.id != field.id && o.name == name && o.name != field.name)
                    {
                        return Err(err("A field with this name already exists"));
                    }
                    let parent = pdf
                        .get_dictionary(field.id)?
                        .get(b"Parent")
                        .ok()
                        .and_then(|o| o.as_reference().ok())
                        .unwrap_or(field.id);
                    let flags = inherited(&pdf, field.id, b"Ff")
                        .ok()
                        .and_then(|o| o.as_i64().ok())
                        .unwrap_or(0);
                    pdf.get_dictionary_mut(parent)?.set("T", text(&name));
                    pdf.get_dictionary_mut(parent)?
                        .set("Ff", if required { flags | 2 } else { flags & !2 });
                    pdf.get_dictionary_mut(field.id)?
                        .set("Rect", rect_object(bounds));
                }
                FormDesignEdit::Delete(field) => {
                    self.check_designed(doc, &field)?;
                    let page = *pdf
                        .get_pages()
                        .get(&(u32::from(field.page) + 1))
                        .ok_or_else(|| err("Invalid field page"))?;
                    let mut annots = array(&pdf, pdf.get_dictionary(page)?, b"Annots")?;
                    annots.retain(|o| o.as_reference().ok() != Some(field.id));
                    pdf.get_dictionary_mut(page)?.set("Annots", annots);
                    // Remove the widget and recursively prune now-empty field ancestors.
                    let mut id = field.id;
                    for depth in 0..64 {
                        if depth == 63 {
                            return Err(err("Excessive form field nesting"));
                        }
                        let parent = pdf
                            .get_dictionary(id)?
                            .get(b"Parent")
                            .ok()
                            .and_then(|o| o.as_reference().ok());
                        if let Some(parent) = parent {
                            let mut kids = array(&pdf, pdf.get_dictionary(parent)?, b"Kids")?;
                            kids.retain(|o| o.as_reference().ok() != Some(id));
                            pdf.get_dictionary_mut(parent)?.set("Kids", kids.clone());
                            if kids.is_empty() {
                                id = parent;
                                continue;
                            }
                            break;
                        }
                        let mut acro = pdf
                            .catalog()?
                            .get_deref(b"AcroForm", &pdf)?
                            .as_dict()?
                            .clone();
                        let mut roots = array(&pdf, &acro, b"Fields")?;
                        roots.retain(|o| o.as_reference().ok() != Some(id));
                        acro.set("Fields", roots);
                        pdf.catalog_mut()?.set("AcroForm", acro);
                        break;
                    }
                }
            }
            let candidate = self.candidate(doc, pdf)?;
            self.designed_fields(&candidate)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
    fn check_designed(&self, doc: &Document, field: &DesignedField) -> Result<()> {
        if field.revision != doc.revision
            || !self
                .designed_fields(doc)?
                .iter()
                .any(|f| f.id == field.id && f.page == field.page)
        {
            return Err(err("Form changed; refresh the designer"));
        }
        Ok(())
    }
}
