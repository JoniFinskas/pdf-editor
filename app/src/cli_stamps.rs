use super::{Command, pdf_files};
use pdf_editor_core::{
    BatesNumber, Engine, RangeStamp, StampPlacement, TextStamp, page_ranges, stamp_date_today,
};

pub(super) fn options(c: &Command) -> Result<(RangeStamp, Option<BatesNumber>), String> {
    let placement = match c.text("--position", "footer")?.as_str() {
        "header" => StampPlacement::Header,
        "footer" => StampPlacement::Footer,
        "absolute" => StampPlacement::Absolute,
        _ => return Err("Position must be header, footer or absolute".into()),
    };
    let color = c.text("--color", "000000")?;
    if color.len() != 6 || !color.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Color must be six hexadecimal digits".into());
    }
    let color = [
        u8::from_str_radix(&color[0..2], 16).unwrap(),
        u8::from_str_radix(&color[2..4], 16).unwrap(),
        u8::from_str_radix(&color[4..6], 16).unwrap(),
    ];
    let stamp = RangeStamp {
        stamp: TextStamp {
            template: c.text("--text", "{page} / {pages}")?,
            font: c.text("--font", "Arial")?,
            size: c.number("--size", "12")?,
            color,
            opacity: c.number("--opacity", "1")?,
            x: c.number("--left", "30")?,
            y: c.number("--margin", "24")?,
        },
        pages: c.text("--pages", "")?,
        date: c.text("--date", &stamp_date_today())?,
        placement,
    };
    if ![
        stamp.stamp.size,
        stamp.stamp.opacity,
        stamp.stamp.x,
        stamp.stamp.y,
    ]
    .iter()
    .all(|v| v.is_finite())
        || !(1.0..=1000.0).contains(&stamp.stamp.size)
        || !(0.0..=1.0).contains(&stamp.stamp.opacity)
    {
        return Err("Use finite coordinates, size 1–1000 and opacity 0–1".into());
    }
    let bates = if c.name == "bates" {
        let value = BatesNumber {
            prefix: c.text("--prefix", "")?,
            start: c.number("--start", "1")?,
            digits: c.number("--digits", "6")?,
        };
        value.label(0).map_err(|e| e.to_string())?;
        Some(value)
    } else {
        None
    };
    Ok((stamp, bates))
}

pub(super) fn execute(engine: &Engine, c: &Command) -> Result<(), String> {
    let (stamp, bates) = options(c)?;
    let folder = c.inputs[0].is_dir();
    let inputs = if folder {
        pdf_files(&c.inputs[0])?
    } else {
        c.inputs.clone()
    };
    let output = if c.dry { None } else { Some(c.output()?) };
    if folder && output.as_ref().is_some_and(|p| !p.is_dir()) {
        return Err("Output must be an existing directory".into());
    }
    let mut failures = 0;
    let mut stamped = 0;
    for (index, input) in inputs.iter().enumerate() {
        let result = (|| {
            let destination = output.as_ref().map(|o| {
                if folder {
                    o.join(input.file_name().unwrap_or_default())
                } else {
                    o.clone()
                }
            });
            if destination.as_ref().is_some_and(|p| p.exists()) {
                return Err("Output already exists; nothing overwritten".to_owned());
            }
            let mut doc = engine.open(input).map_err(|e| e.to_string())?;
            let count = engine.pages(&doc).map_err(|e| e.to_string())?.len();
            let selected = if stamp.pages.trim().is_empty() {
                count
            } else {
                page_ranges(&stamp.pages, count)
                    .map_err(|e| e.to_string())?
                    .len()
            };
            // Dry-run performs the same validation and mutation on an in-memory document.
            if let Some(bates) = &bates {
                let mut next = bates.clone();
                next.start = next
                    .start
                    .checked_add(stamped as u64)
                    .ok_or_else(|| "Bates batch sequence overflow".to_owned())?;
                engine.stamp_bates(&mut doc, &stamp, &next)
            } else {
                engine.stamp_page_range(&mut doc, &stamp)
            }
            .map_err(|e| e.to_string())?;
            if let Some(destination) = destination {
                doc.save_as(&destination).map_err(|e| e.to_string())?;
            }
            Ok::<usize, String>(selected)
        })();
        match result {
            Ok(count) => {
                stamped += count;
                println!("file={} pages={count} dry_run={}", index + 1, c.dry);
            }
            Err(error) => {
                failures += 1;
                eprintln!("file={} error={error}", index + 1);
            }
        }
    }
    println!("files={} pages={stamped} failed={failures}", inputs.len());
    if failures > 0 {
        Err("One or more files failed; successful outputs are retained".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, path::Path};
    #[test]
    fn batch_stamps_generated_files_and_keeps_sources_and_collisions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let dir = root.join(format!(
            "target/stamp-batch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let input = dir.join("input");
        let output = dir.join("output");
        std::fs::create_dir_all(input.join("nested")).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let original = std::fs::read(root.join("tests/fixtures/generated/multi-page.pdf")).unwrap();
        for name in ["a.pdf", "b.PDF", "nested/skip.pdf"] {
            std::fs::write(input.join(name), &original).unwrap();
        }
        std::fs::write(input.join("ignore.txt"), b"ignore").unwrap();
        let invoke = |name: &str, dry: bool| {
            let mut a: Vec<OsString> = vec![
                name.into(),
                input.as_os_str().into(),
                "--pages".into(),
                "2".into(),
            ];
            if dry {
                a.push("--dry-run".into());
            } else {
                a.extend(["--output".into(), output.as_os_str().into()]);
            }
            if name == "stamp" {
                a.extend([
                    "--text".into(),
                    "Batch {date} {page}".into(),
                    "--date".into(),
                    "2026-10-23".into(),
                ]);
            } else {
                a.extend([
                    "--prefix".into(),
                    "CASE-".into(),
                    "--start".into(),
                    "9".into(),
                    "--digits".into(),
                    "4".into(),
                ]);
            }
            execute(&engine, &super::super::parse(a).unwrap())
        };
        invoke("stamp", true).unwrap();
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
        invoke("bates", false).unwrap();
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 2);
        for (name, label) in [("a.pdf", "CASE-0009"), ("b.PDF", "CASE-0010")] {
            let doc = engine.open(&output.join(name)).unwrap();
            assert_eq!(engine.find_text(&doc, label).unwrap().len(), 1);
            assert!(
                !engine
                    .text_objects(&doc, 0)
                    .unwrap()
                    .iter()
                    .any(|t| t.text.contains("CASE-"))
            );
            assert_eq!(std::fs::read(input.join(name)).unwrap(), original);
        }
        let saved = std::fs::read(output.join("a.pdf")).unwrap();
        assert!(invoke("stamp", false).is_err());
        assert_eq!(std::fs::read(output.join("a.pdf")).unwrap(), saved);
        std::fs::write(input.join("broken.pdf"), b"not a PDF").unwrap();
        assert!(invoke("stamp", true).is_err());
    }
}
