use crate::*;
use lopdf::{Object, ObjectId, dictionary};

#[derive(Clone, Debug)]
pub enum PageEdit {
    Rotate {
        page: u16,
    },
    Delete {
        page: u16,
    },
    /// Move a page to its final zero-based index.
    Move {
        from: u16,
        to: u16,
    },
    /// Insert all source pages before an index (page count means append).
    Insert {
        path: PathBuf,
        before: u16,
    },
}

pub(crate) fn inherited(
    pdf: &lopdf::Document,
    mut id: ObjectId,
    key: &[u8],
) -> Result<Option<Object>> {
    for _ in 0..128 {
        let dict = pdf.get_dictionary(id)?;
        if let Ok(value) = dict.get(key) {
            return Ok(Some(value.clone()));
        }
        match dict.get(b"Parent") {
            Ok(parent) => id = parent.as_reference()?,
            Err(_) => return Ok(None),
        }
    }
    Err(err("Excessive page tree depth"))
}

impl Engine {
    pub fn edit_pages(&self, doc: &mut Document, edit: &PageEdit) -> Result<()> {
        guarded(|| {
            let mut pdf = parse(&doc.bytes)?;
            let mut pages: Vec<_> = pdf.get_pages().into_values().collect();
            let count = pages.len();
            match edit {
                PageEdit::Rotate { page } => {
                    let id = *pages
                        .get(*page as usize)
                        .ok_or_else(|| err("Invalid page"))?;
                    let rotation = inherited(&pdf, id, b"Rotate")?
                        .map(|v| v.as_i64())
                        .transpose()?
                        .unwrap_or(0);
                    pdf.get_dictionary_mut(id)?
                        .set("Rotate", (rotation.rem_euclid(360) + 90) % 360);
                }
                PageEdit::Delete { page } => {
                    if count <= 1 {
                        return Err(err("A PDF must retain at least one page"));
                    }
                    if *page as usize >= count {
                        return Err(err("Invalid page"));
                    }
                    pages.remove(*page as usize);
                    flatten(&mut pdf, &pages)?;
                }
                PageEdit::Move { from, to } => {
                    if *from as usize >= count || *to as usize >= count {
                        return Err(err("Invalid page"));
                    }
                    if from == to {
                        return Ok(());
                    }
                    let id = pages.remove(*from as usize);
                    pages.insert(*to as usize, id);
                    flatten(&mut pdf, &pages)?;
                }
                PageEdit::Insert { path, before } => {
                    if *before as usize > count {
                        return Err(err("Invalid insertion position"));
                    }
                    let source = self.open(path)?;
                    let source_pdf = self.pdfium.load_pdf_from_byte_slice(&source.bytes, None)?;
                    let added = source_pdf.pages().len();
                    if count + added as usize > 10_000 {
                        return Err(err("Too many pages"));
                    }
                    let mut destination = self.pdfium.load_pdf_from_byte_slice(&doc.bytes, None)?;
                    destination.pages_mut().copy_page_range_from_document(
                        &source_pdf,
                        0..=added - 1,
                        *before as i32,
                    )?;
                    pdf = parse(&destination.save_to_bytes()?)?;
                }
            }
            let candidate = self.candidate(doc, pdf)?;
            doc.accept(candidate.bytes);
            Ok(())
        })
    }
}

fn flatten(pdf: &mut lopdf::Document, pages: &[ObjectId]) -> Result<()> {
    // Materialize inheritable attributes before replacing the page tree.
    let parent = pdf.new_object_id();
    for &id in pages {
        for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
            if let Some(value) = inherited(pdf, id, key)? {
                pdf.get_dictionary_mut(id)?.set(key, value);
            }
        }
    }
    for &id in pages {
        pdf.get_dictionary_mut(id)?.set("Parent", parent);
    }
    pdf.objects.insert(
        parent,
        Object::Dictionary(dictionary! {
            "Type"=>"Pages", "Count"=>pages.len() as i64,
            "Kids"=>pages.iter().map(|id|Object::Reference(*id)).collect::<Vec<_>>()
        }),
    );
    let root = pdf.trailer.get(b"Root")?.as_reference()?;
    pdf.get_dictionary_mut(root)?.set("Pages", parent);
    Ok(())
}
