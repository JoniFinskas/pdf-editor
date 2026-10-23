//! Generated-only startup benchmark; no external PDFs are accepted.
use lopdf::{Document, Object, Stream, dictionary};
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let folder = root.join("target/performance");
    std::fs::create_dir_all(&folder)?;
    let path = folder.join("generated-1000-pages.pdf");
    let mut pdf = Document::with_version("1.7");
    let pages = pdf.new_object_id();
    let font =
        pdf.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let resources = pdf.add_object(dictionary! {"Font"=> dictionary! {"F1"=>font}});
    let mut kids = Vec::<Object>::new();
    for index in 0..1000 {
        let content = pdf.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 30 750 Td (Generated page {index}) Tj ET").into_bytes(),
        ));
        let page = pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "Resources"=>resources, "MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()], "Contents"=>content});
        kids.push(page.into());
    }
    pdf.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages", "Count"=>1000, "Kids"=>kids}.into(),
    );
    let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
    pdf.trailer.set("Root", catalog);
    pdf.save(&path)?;
    for iteration in 0..4 {
        let start = Instant::now();
        let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll"))?;
        let doc = engine.open(&path)?;
        let metadata = engine.pages(&doc)?;
        let opened = start.elapsed();
        assert_eq!(metadata.len(), 1000);
        engine.render(&doc, 0, 1.0)?;
        let first_frame = start.elapsed();
        println!(
            "iteration={iteration} pages=1000 open_and_metadata_ms={} first_render_ms={}",
            opened.as_millis(),
            first_frame.as_millis()
        );
        if opened.as_secs_f64() >= 2.0 {
            return Err("1000-page startup exceeded 2 seconds".into());
        }
    }
    Ok(())
}
