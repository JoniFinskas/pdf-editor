//! Container-only harness. Never emit document strings or parser diagnostics.
use pdf_editor_core::Engine;
use std::path::Path;

fn check(input: &Path, output: &Path) -> Result<(), &'static str> {
    let engine = Engine::new(Path::new("/opt/pdfium/lib/libpdfium.so")).map_err(|_| "runtime")?;
    let mut doc = engine.open(input).map_err(|_| "open")?;
    let pages = engine.pages(&doc).map_err(|_| "pages")?;
    for page in 0..pages.len() {
        let page = u16::try_from(page).map_err(|_| "page_limit")?;
        let before = engine.render(&doc, page, 1.0).map_err(|_| "render")?;
        let objects = engine.text_objects(&doc, page).map_err(|_| "objects")?;
        if let Some(object) = objects.iter().find(|o| !o.text.is_empty()) {
            // Remove one character: an actual edit using only existing glyphs.
            let replacement: String = object.text.chars().skip(1).collect();
            engine.find_text(&doc, &object.text).map_err(|_| "find")?;
            engine
                .edit(&mut doc, object, &replacement)
                .map_err(|_| "edit")?;
            let after = engine.render(&doc, page, 1.0).map_err(|_| "render_after")?;
            if before.width != after.width || before.height != after.height {
                return Err("dimensions");
            }
            for (i, (a, b)) in before
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .zip(after.rgba.as_chunks::<4>().0.iter())
                .enumerate()
            {
                if a != b {
                    let (x, y) = engine
                        .pixel_to_page(
                            &doc,
                            page,
                            1.0,
                            (i as u32 % before.width) as i32,
                            (i as u32 / before.width) as i32,
                        )
                        .map_err(|_| "coordinates")?;
                    let b = object.bounds;
                    if x < b.left - 4.0
                        || x > b.right + 4.0
                        || y < b.bottom - 4.0
                        || y > b.top + 4.0
                    {
                        return Err("render_diff");
                    }
                }
            }
        }
    }
    doc.save_as(output).map_err(|_| "save")?;
    let reopened = engine.open(output).map_err(|_| "reopen")?;
    if engine.pages(&reopened).map_err(|_| "reopen_pages")?.len() != pages.len() {
        return Err("page_count");
    }
    for page in 0..pages.len() {
        let page = u16::try_from(page).map_err(|_| "page_limit")?;
        let expected = engine.render(&doc, page, 1.0).map_err(|_| "render_after")?;
        let actual = engine
            .render(&reopened, page, 1.0)
            .map_err(|_| "render_after")?;
        if expected.width != actual.width
            || expected.height != actual.height
            || expected.rgba != actual.rgba
        {
            return Err("render_diff");
        }
    }
    Ok(())
}
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
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
    println!("{status}");
}
