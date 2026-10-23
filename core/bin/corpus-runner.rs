//! Container-only harness. Never emit document strings or parser diagnostics.
use pdf_editor_core::Engine;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
static FAILURES: std::sync::Mutex<std::collections::BTreeSet<(&'static str, u64)>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());
fn classify(error: impl std::fmt::Display, stage: &'static str) -> &'static str {
    let mut hash = 14695981039346656037u64;
    for byte in error.to_string().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
    FAILURES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert((stage, hash));
    stage
}
static PANICKED: AtomicBool = AtomicBool::new(false);

fn check(input: &Path, output: &Path) -> Result<(), &'static str> {
    let engine = Engine::new(Path::new("/opt/pdfium/lib/libpdfium.so"))
        .map_err(|error| classify(error, "runtime"))?;
    let mut doc = engine
        .open(input)
        .map_err(|error| classify(error, "open"))?;
    let pages = engine
        .pages(&doc)
        .map_err(|error| classify(error, "pages"))?;
    for page in 0..pages.len() {
        let _result = (|| -> Result<(), &'static str> {
            let page = u16::try_from(page).map_err(|error| classify(error, "page_limit"))?;
            let before = engine
                .render(&doc, page, 1.0)
                .map_err(|error| classify(error, "render"))?;
            let objects = engine
                .text_objects(&doc, page)
                .map_err(|error| classify(error, "objects"))?;
            if let Some(object) = objects.iter().find(|o| !o.text.is_empty()) {
                // Remove one character: an actual edit using only existing glyphs.
                let replacement: String = object.text.chars().skip(1).collect();
                engine
                    .find_text(
                        &doc,
                        &object
                            .text
                            .chars()
                            .filter(|c| *c != '\0')
                            .take(256)
                            .collect::<String>(),
                    )
                    .map_err(|error| classify(error, "find"))?;
                engine
                    .edit(&mut doc, object, &replacement)
                    .map_err(|error| classify(error, "edit"))?;
                let after = engine
                    .render(&doc, page, 1.0)
                    .map_err(|error| classify(error, "render_after"))?;
                if before.width != after.width || before.height != after.height {
                    return Err(classify("dimensions", "dimensions"));
                }
                let origin = engine
                    .pixel_to_page(&doc, page, 1.0, 0, 0)
                    .map_err(|error| classify(error, "coordinates"))?;
                let horizontal = engine
                    .pixel_to_page(&doc, page, 1.0, before.width as i32, 0)
                    .map_err(|error| classify(error, "coordinates"))?;
                let vertical = engine
                    .pixel_to_page(&doc, page, 1.0, 0, before.height as i32)
                    .map_err(|error| classify(error, "coordinates"))?;
                for (i, (a, b)) in before
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(after.rgba.as_chunks::<4>().0.iter())
                    .enumerate()
                {
                    if a != b {
                        let u = (i as u32 % before.width) as f32 / before.width as f32;
                        let v = (i as u32 / before.width) as f32 / before.height as f32;
                        let x =
                            origin.0 + u * (horizontal.0 - origin.0) + v * (vertical.0 - origin.0);
                        let y =
                            origin.1 + u * (horizontal.1 - origin.1) + v * (vertical.1 - origin.1);
                        let b = object.bounds;
                        if x < b.left - 4.0
                            || x > b.right + 4.0
                            || y < b.bottom - 4.0
                            || y > b.top + 4.0
                        {
                            return Err(classify("render_diff", "render_diff"));
                        }
                    }
                }
            }
            Ok(())
        })();
    }
    doc.save_as(output)
        .map_err(|error| classify(error, "save"))?;
    let reopened = engine
        .open(output)
        .map_err(|error| classify(error, "reopen"))?;
    if engine
        .pages(&reopened)
        .map_err(|error| classify(error, "reopen_pages"))?
        .len()
        != pages.len()
    {
        return Err(classify("page_count", "page_count"));
    }
    for page in 0..pages.len() {
        let page = u16::try_from(page).map_err(|error| classify(error, "page_limit"))?;
        let expected = engine
            .render(&doc, page, 1.0)
            .map_err(|error| classify(error, "render_after"))?;
        let actual = engine
            .render(&reopened, page, 1.0)
            .map_err(|error| classify(error, "render_after"))?;
        if expected.width != actual.width
            || expected.height != actual.height
            || expected.rgba != actual.rgba
        {
            return Err(classify("render_diff", "render_diff"));
        }
    }
    Ok(())
}
fn main() {
    std::panic::set_hook(Box::new(|_| {
        PANICKED.store(true, Ordering::Relaxed);
    }));
    if std::env::var("PDF_EDITOR_CONTAINER").as_deref() != Ok("1") {
        std::process::exit(2);
    }
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        std::process::exit(2);
    }
    let result = std::panic::catch_unwind(|| check(Path::new(&args[1]), Path::new(&args[2])));
    let status = match result {
        Ok(Ok(())) => "ok",
        Ok(Err(kind)) => kind,
        Err(_) => "panic",
    };
    let status = if PANICKED.load(Ordering::Relaxed) {
        "panic"
    } else {
        status
    };
    let mut failures = FAILURES.lock().unwrap_or_else(|p| p.into_inner());
    if status == "panic" {
        failures.insert(("panic", 0));
    }
    if failures.is_empty() {
        println!("{status}:0");
    } else {
        for (stage, hash) in failures.iter() {
            println!("{stage}:{hash}");
        }
    }
}
