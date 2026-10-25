//! Independent structural checks for the small graph produced by page rebuilding.
use super::*;
fn fields(dict: &Dictionary, allowed: &[&[u8]]) -> Result<()> {
    if dict
        .iter()
        .any(|(key, _)| !allowed.contains(&key.as_slice()))
    {
        return Err(err("Rebuilt copy contains document extras"));
    }
    Ok(())
}
fn jpeg(bytes: &[u8]) -> Result<()> {
    let invalid = || err("Rebuilt copy contains image metadata or invalid image data");
    if !bytes.starts_with(&[255, 216]) {
        return Err(invalid());
    }
    let mut at = 2;
    let mut scan = false;
    while at < bytes.len() {
        if bytes[at] != 255 {
            if !scan {
                return Err(invalid());
            }
            at += 1;
            continue;
        }
        while bytes.get(at) == Some(&255) {
            at += 1;
        }
        let marker = *bytes.get(at).ok_or_else(invalid)?;
        at += 1;
        if scan && (marker == 0 || (0xd0..=0xd7).contains(&marker)) {
            continue;
        }
        if marker == 0xd9 {
            return if at == bytes.len() {
                Ok(())
            } else {
                Err(invalid())
            };
        }
        let size = bytes.get(at..at + 2).ok_or_else(invalid)?;
        let length = u16::from_be_bytes([size[0], size[1]]) as usize;
        if length < 2 {
            return Err(invalid());
        }
        let segment = bytes.get(at + 2..at + length).ok_or_else(invalid)?;
        if (0xe0..=0xef).contains(&marker) || marker == 0xfe {
            // Only the encoder's fixed JFIF density header is expected. EXIF,
            // XMP, comments, embedded profiles and trailing payloads are absent.
            if marker != 0xe0 || length != 16 || !segment.starts_with(b"JFIF\0") {
                return Err(invalid());
            }
        }
        if marker == 0xda {
            scan = true;
        }
        at += length;
    }
    Err(invalid())
}
pub(super) fn canonicalize(pdf: &mut lopdf::Document) {
    pdf.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    pdf.version = "1.7".into();
    pdf.binary_mark = vec![0xe2, 0xe3, 0xcf, 0xd3];
    let extras = pdf
        .trailer
        .iter()
        .filter(|(key, _)| !matches!(key.as_slice(), b"Root" | b"Size"))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in extras {
        pdf.trailer.remove(&key);
    }
    pdf.prune_objects();
    pdf.renumber_objects();
}
pub(super) fn base(pdf: &lopdf::Document, bytes: &[u8]) -> Result<()> {
    fields(&pdf.trailer, &[b"Root", b"Size"])?;
    if !matches!(
        pdf.reference_table.cross_reference_type,
        lopdf::xref::XrefType::CrossReferenceTable
    ) || pdf.objects.values().any(|object| {
        object.as_stream().is_ok_and(|stream| {
            stream
                .dict
                .get(b"Type")
                .and_then(Object::as_name)
                .is_ok_and(|kind| kind == b"XRef")
        })
    }) {
        return Err(err("Unsupported sanitized copy structure"));
    }
    let mut normalized = pdf.clone();
    canonicalize(&mut normalized);
    let mut canonical = Vec::new();
    normalized.save_to(&mut canonical)?;
    if canonical != bytes {
        return Err(err("Sanitized copy contains unused or non-canonical data"));
    }
    Ok(())
}
pub(super) fn raster(pdf: &lopdf::Document) -> Result<()> {
    fields(pdf.catalog()?, &[b"Type", b"Pages"])?;
    for object in pdf.objects.values() {
        if let Ok(dict) = object.as_dict()
            && dict
                .get(b"Type")
                .and_then(Object::as_name)
                .is_ok_and(|name| name == b"Pages")
        {
            fields(dict, &[b"Type", b"Kids", b"Count", b"Parent"])?;
        }
        if let Ok(stream) = object.as_stream() {
            if stream
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|kind| kind == b"Image")
            {
                fields(
                    &stream.dict,
                    &[
                        b"Type",
                        b"Subtype",
                        b"Width",
                        b"Height",
                        b"ColorSpace",
                        b"BitsPerComponent",
                        b"Filter",
                        b"Length",
                    ],
                )?;
                if !stream
                    .dict
                    .get(b"ColorSpace")
                    .and_then(Object::as_name)
                    .is_ok_and(|value| value == b"DeviceRGB")
                    || stream
                        .dict
                        .get(b"BitsPerComponent")
                        .and_then(Object::as_i64)?
                        != 8
                    || !stream
                        .dict
                        .get(b"Filter")
                        .and_then(Object::as_name)
                        .is_ok_and(|value| value == b"DCTDecode")
                {
                    return Err(err("Rebuilt copy contains an unexpected image format"));
                }
                jpeg(&stream.content)?;
            } else {
                fields(&stream.dict, &[b"Length", b"Filter"])?;
            }
        }
    }
    for id in pdf.get_pages().values() {
        fields(
            pdf.get_dictionary(*id)?,
            &[b"Type", b"Parent", b"MediaBox", b"Resources", b"Contents"],
        )?;
        let resources = page_resources(pdf, *id)?;
        fields(&resources, &[b"XObject"])?;
        let images = resolve(pdf, resources.get(b"XObject")?)?.as_dict()?;
        for (_, value) in images.iter() {
            if !resolve(pdf, value)?
                .as_stream()?
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|kind| kind == b"Image")
            {
                return Err(err("Rebuilt copy contains a non-image resource"));
            }
        }
    }
    Ok(())
}
