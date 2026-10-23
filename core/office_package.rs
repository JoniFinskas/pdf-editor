//! Minimal stored ZIP packages for Office Open XML, with bounded output.
use crate::*;
pub(crate) fn xml(s: &str) -> String {
    s.chars()
        .filter(|c| {
            *c == '\n'
                || *c == '\r'
                || *c == '\t'
                || matches!(*c as u32, 0x20..=0xd7ff | 0xe000..=0xfffd | 0x10000..=0x10ffff)
        })
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '\"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            _ => c.to_string(),
        })
        .collect()
}
fn crc(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in bytes {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(c & 1)));
        }
    }
    !c
}
fn u16le(out: &mut Vec<u8>, n: u16) {
    out.extend(n.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
pub(crate) fn package(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    if entries.len() > 60000
        || entries
            .iter()
            .any(|(n, b)| n.len() > 65535 || b.len() > MAX_FILE)
    {
        return Err(err("Office export exceeds package limits"));
    }
    let mut out = vec![];
    let mut central = vec![];
    for (name, bytes) in entries {
        if out.len() + bytes.len() + central.len() > MAX_FILE {
            return Err(err("Office export exceeds 64 MiB"));
        }
        let offset = out.len() as u32;
        let checksum = crc(bytes);
        u32le(&mut out, 0x04034b50);
        for n in [20, 0x800, 0, 0, 0x21] {
            u16le(&mut out, n);
        }
        for n in [checksum, bytes.len() as u32, bytes.len() as u32] {
            u32le(&mut out, n);
        }
        u16le(&mut out, name.len() as u16);
        u16le(&mut out, 0);
        out.extend(name.as_bytes());
        out.extend(bytes);
        u32le(&mut central, 0x02014b50);
        for n in [20, 20, 0x800, 0, 0, 0x21] {
            u16le(&mut central, n);
        }
        for n in [checksum, bytes.len() as u32, bytes.len() as u32] {
            u32le(&mut central, n);
        }
        for n in [name.len() as u16, 0, 0, 0, 0] {
            u16le(&mut central, n);
        }
        u32le(&mut central, 0);
        u32le(&mut central, offset);
        central.extend(name.as_bytes());
    }
    let offset = out.len() as u32;
    let size = central.len() as u32;
    out.extend(central);
    u32le(&mut out, 0x06054b50);
    for n in [0, 0, entries.len() as u16, entries.len() as u16] {
        u16le(&mut out, n);
    }
    u32le(&mut out, size);
    u32le(&mut out, offset);
    u16le(&mut out, 0);
    if out.len() > MAX_FILE {
        return Err(err("Office export exceeds 64 MiB"));
    }
    Ok(out)
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| err("Choose a new export filename; existing files are never overwritten"))?;
    if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
        drop(f);
        let _ = fs::remove_file(path);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xml_filters_invalid_characters_and_crc_matches_standard() {
        assert_eq!(xml("a\u{0}b\u{ffff}c\n<&"), "abc\n&lt;&amp;");
        assert_eq!(crc(b"123456789"), 0xcbf43926);
    }
    #[test]
    fn package_contains_local_directory_and_end_records() {
        let p = package(&[("a.xml".into(), b"<a/>".to_vec())]).unwrap();
        assert_eq!(&p[..4], b"PK\x03\x04");
        assert_eq!(&p[p.len() - 22..p.len() - 18], b"PK\x05\x06");
        let offset = u32::from_le_bytes(p[p.len() - 6..p.len() - 2].try_into().unwrap()) as usize;
        assert_eq!(&p[offset..offset + 4], b"PK\x01\x02");
    }
}
