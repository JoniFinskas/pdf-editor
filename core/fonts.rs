use crate::{MAX_STREAM, Result, err, read_limited};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    string::StringId,
};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone)]
pub(crate) struct Font {
    pub bytes: Vec<u8>,
    pub name: String,
    pub exact: bool,
}
pub(crate) fn base_name(name: &str) -> &str {
    if name.as_bytes().get(6) == Some(&b'+')
        && name.as_bytes()[..6].iter().all(u8::is_ascii_uppercase)
    {
        &name[7..]
    } else {
        name
    }
}
pub(crate) fn family(name: &str) -> String {
    base_name(name)
        .to_lowercase()
        .replace([' ', '-', ','], "")
        .replace("bold", "")
        .replace("italic", "")
        .replace("oblique", "")
        .replace("regular", "")
}
fn covers(bytes: &[u8], text: &str) -> bool {
    FontRef::new(bytes).is_ok_and(|f| {
        text.chars()
            .all(|c| f.charmap().map(c).is_some_and(|g| g.to_u32() != 0))
    })
}
pub(crate) fn embedded_font(doc: &Document, font: &Dictionary, text: &str) -> Option<Font> {
    let check = || -> Result<Font> {
        let base = if font.get(b"Subtype")?.as_name()? == b"Type0" {
            let child = font
                .get(b"DescendantFonts")?
                .as_array()?
                .first()
                .ok_or_else(|| err("Missing CID font"))?;
            doc.dereference(child)?.1.as_dict()?
        } else {
            font
        };
        let descriptor = doc.dereference(base.get(b"FontDescriptor")?)?.1.as_dict()?;
        let stream = doc
            .dereference(descriptor.get(b"FontFile2")?)?
            .1
            .as_stream()?;
        let data = stream.get_plain_content_with_limit(MAX_STREAM)?;
        if !covers(&data, text) {
            return Err(err("Embedded font lacks a glyph"));
        }
        let name = String::from_utf8_lossy(font.get(b"BaseFont")?.as_name()?).into_owned();
        Ok(Font {
            bytes: data,
            name,
            exact: true,
        })
    };
    check().ok()
}
pub(crate) fn find(name: &str, text: &str) -> Result<Font> {
    let mut roots = vec![PathBuf::from(r"C:\Windows\Fonts")];
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
    }
    let mut paths = vec![];
    for root in roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            paths.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("ttc"))
            }));
        }
    }
    paths.sort();
    let wanted = family(name);
    let lower = name.to_lowercase();
    let bold = lower.contains("bold");
    let italic = lower.contains("italic") || lower.contains("oblique");
    let mut best: Option<(u32, Font)> = None;
    for path in paths {
        let Ok(collection) = read_limited(&path, MAX_STREAM) else {
            continue;
        };
        let faces = if collection.starts_with(b"ttcf") {
            read_u32(&collection, 8).unwrap_or(0).min(128)
        } else {
            1
        };
        for index in 0..faces {
            let Ok(face) = FontRef::from_index(&collection, index) else {
                continue;
            };
            let full = face
                .localized_strings(StringId::POSTSCRIPT_NAME)
                .english_or_first()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let family_name = face
                .localized_strings(StringId::FAMILY_NAME)
                .english_or_first()
                .map(|s| s.to_string())
                .unwrap_or_default();
            if full.is_empty() {
                continue;
            }
            let attr = face.attributes();
            let style_match = (attr.weight.value() >= 600.0) == bold
                && (attr.style != skrifa::attribute::Style::Normal) == italic;
            let exact = (family(&family_name) == wanted || family(&full) == wanted) && style_match;
            let score = if exact {
                0
            } else {
                let preferred = if wanted.contains("courier") || wanted.contains("mono") {
                    "consolas"
                } else if wanted.contains("times") || wanted.contains("serif") {
                    "timesnewroman"
                } else {
                    "arial"
                };
                10 + if family(&family_name) == preferred {
                    0
                } else {
                    10
                } + if style_match { 0 } else { 5 }
            };
            if best.as_ref().is_some_and(|(s, _)| *s <= score)
                || !text
                    .chars()
                    .all(|c| face.charmap().map(c).is_some_and(|g| g.to_u32() != 0))
            {
                continue;
            }
            // Restricted embedding is rejected; no license override is inferred from local installation.
            use skrifa::raw::TableProvider;
            if let Ok(os2) = face.os2() {
                let flags = os2.fs_type();
                if flags & 0x0202 != 0 || (flags & 0x0004 != 0 && flags & 0x0008 == 0) {
                    continue;
                }
            }
            let Ok(bytes) = collection_face(&collection, index) else {
                continue;
            };
            best = Some((
                score,
                Font {
                    bytes,
                    name: full,
                    exact,
                },
            ));
            if exact {
                break;
            }
        }
        if best.as_ref().is_some_and(|(score, _)| *score == 0) {
            break;
        }
    }
    best.map(|(_, f)| f)
        .ok_or_else(|| err("No embeddable system font contains every required glyph"))
}
/// Explicit formatting requires the requested family and style, without fallback.
pub(crate) fn find_style(name: &str, bold: bool, italic: bool, text: &str) -> Result<Font> {
    if name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(err("Choose an installed font family"));
    }
    let requested = format!("{}{}{}", family(name), if bold { " Bold" } else { "" },
        if italic { " Italic" } else { "" });
    let font = find(&requested, text)?;
    if !font.exact {
        return Err(err("Requested font family and style cannot embed every selected character; choose another font"));
    }
    Ok(font)
}
fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let raw = data
        .get(offset..offset + 4)
        .ok_or_else(|| err("Invalid font collection"))?;
    Ok(u32::from_be_bytes(
        raw.try_into().map_err(|_| err("Invalid font table"))?,
    ))
}
fn collection_face(data: &[u8], index: u32) -> Result<Vec<u8>> {
    if !data.starts_with(b"ttcf") {
        return Ok(data.to_vec());
    }
    let start = read_u32(data, 12 + index as usize * 4)? as usize;
    let header = data
        .get(start..start + 12)
        .ok_or_else(|| err("Invalid font collection"))?;
    let count = u16::from_be_bytes([header[4], header[5]]) as usize;
    if count == 0 || count > 256 {
        return Err(err("Excessive font tables"));
    }
    let directory = data
        .get(start..start + 12 + 16 * count)
        .ok_or_else(|| err("Invalid font directory"))?;
    let mut out = directory.to_vec();
    let mut head = None;
    for i in 0..count {
        let entry = 12 + i * 16;
        let offset = read_u32(directory, entry + 8)? as usize;
        let length = read_u32(directory, entry + 12)? as usize;
        let table = data
            .get(
                offset
                    ..offset
                        .checked_add(length)
                        .ok_or_else(|| err("Invalid font table"))?,
            )
            .ok_or_else(|| err("Invalid font table"))?;
        if out.len() + length > MAX_STREAM {
            return Err(err("Font exceeds size limit"));
        }
        let position = out.len();
        out.extend_from_slice(table);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out[entry + 8..entry + 12].copy_from_slice(&(position as u32).to_be_bytes());
        if &directory[entry..entry + 4] == b"head" {
            if length < 12 {
                return Err(err("Invalid font header"));
            }
            out[position + 8..position + 12].fill(0);
            head = Some(position);
        }
        let sum = checksum(&out[position..]);
        out[entry + 4..entry + 8].copy_from_slice(&sum.to_be_bytes());
    }
    let head = head.ok_or_else(|| err("Missing font header"))?;
    let adjustment = 0xB1B0AFBAu32.wrapping_sub(checksum(&out));
    out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    Ok(out)
}
fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}
pub(crate) struct Embedded {
    pub id: ObjectId,
    pub codes: BTreeMap<char, u16>,
}
pub(crate) fn embed(doc: &mut Document, font: &Font, text: &str) -> Result<Embedded> {
    let face = FontRef::new(&font.bytes).map_err(|_| err("Invalid system font"))?;
    let mut codes = BTreeMap::new();
    for c in text.chars() {
        if !codes.contains_key(&c) {
            let n = u16::try_from(codes.len() + 1).map_err(|_| err("Too many distinct glyphs"))?;
            codes.insert(c, n);
        }
    }
    let metrics = face.metrics(Size::new(1000.0), LocationRef::default());
    let glyphs = face.glyph_metrics(Size::new(1000.0), LocationRef::default());
    let mut gid_map = vec![0u8; (codes.len() + 1) * 2];
    let mut widths = vec![];
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /EditorUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<_> = codes.iter().collect();
    for chunk in entries.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (c, cid) in chunk {
            let gid = face
                .charmap()
                .map(**c)
                .ok_or_else(|| err("Missing glyph"))?;
            let g = u16::try_from(gid.to_u32()).map_err(|_| err("Invalid glyph ID"))?;
            gid_map[**cid as usize * 2..**cid as usize * 2 + 2].copy_from_slice(&g.to_be_bytes());
            let width = glyphs
                .advance_width(gid)
                .ok_or_else(|| err("Missing glyph width"))?;
            widths.push(Object::Integer(**cid as i64));
            widths.push(Object::Array(vec![Object::Real(width)]));
            let mut utf16 = [0u16; 2];
            let unicode = c
                .encode_utf16(&mut utf16)
                .iter()
                .map(|v| format!("{v:04X}"))
                .collect::<String>();
            cmap.push_str(&format!("<{:04X}> <{}>\n", cid, unicode));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let program = doc.add_object(Stream::new(
        dictionary! {"Length1"=>font.bytes.len() as i64},
        font.bytes.clone(),
    ));
    let descriptor = doc.add_object(dictionary! {
        "Type"=>"FontDescriptor", "FontName"=>Object::Name(font.name.as_bytes().to_vec()),
        "Flags"=>32, "FontBBox"=>vec![(-200).into(),(-400).into(),2000.into(),2000.into()],
        "ItalicAngle"=>metrics.italic_angle, "Ascent"=>metrics.ascent,"Descent"=>metrics.descent,
        "CapHeight"=>metrics.cap_height.unwrap_or(metrics.ascent), "StemV"=>80, "FontFile2"=>program
    });
    let map = doc.add_object(Stream::new(Dictionary::new(), gid_map));
    let cid=doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"CIDFontType2", "BaseFont"=>Object::Name(font.name.as_bytes().to_vec()),
        "CIDSystemInfo"=>dictionary!{"Registry"=>Object::string_literal("Adobe"),"Ordering"=>Object::string_literal("Identity"),"Supplement"=>0},
        "FontDescriptor"=>descriptor,"CIDToGIDMap"=>map,"DW"=>1000,"W"=>widths
    });
    let unicode = doc.add_object(Stream::new(Dictionary::new(), cmap.into_bytes()));
    let id = doc.add_object(dictionary! {
        "Type"=>"Font","Subtype"=>"Type0","BaseFont"=>Object::Name(font.name.as_bytes().to_vec()),
        "Encoding"=>"Identity-H","DescendantFonts"=>vec![cid.into()],"ToUnicode"=>unicode
    });
    Ok(Embedded { id, codes })
}
impl Embedded {
    pub(crate) fn encode(&self, text: &str) -> Result<Vec<u8>> {
        text.chars()
            .map(|c| {
                self.codes
                    .get(&c)
                    .copied()
                    .ok_or_else(|| err("Missing font mapping"))
            })
            .collect::<Result<Vec<_>>>()
            .map(|v| v.into_iter().flat_map(u16::to_be_bytes).collect())
    }
}

pub(crate) fn system_names() -> Result<Vec<String>> {
    let mut roots = vec![PathBuf::from(r"C:\Windows\Fonts")];
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
    }
    let mut names = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for path in entries.flatten().map(|e| e.path()).filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("ttc"))
        }) {
            let Ok(bytes) = read_limited(&path, MAX_STREAM) else {
                continue;
            };
            let count = if bytes.starts_with(b"ttcf") {
                read_u32(&bytes, 8).unwrap_or(0).min(128)
            } else {
                1
            };
            for index in 0..count {
                if let Ok(face) = FontRef::from_index(&bytes, index)
                    && let Some(name) = face
                        .localized_strings(StringId::POSTSCRIPT_NAME)
                        .english_or_first()
                {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}
