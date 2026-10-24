//! Generated-only dense-page editing benchmark. Does not launch a GUI.
#[path = "support/dense_fixture.rs"]
mod dense_fixture;
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let folder = root.join("target/performance");
    std::fs::create_dir_all(&folder)?;
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll"))?;
    let seed = std::fs::read(root.join("tests/fixtures/generated/unicode-cid.pdf"))?;
    for count in [100, 500, 1000] {
        let path = folder.join(format!("generated-dense-{count}.pdf"));
        std::fs::write(&path, dense_fixture::dense_page(&seed, count))?;
        for iteration in 0..3 {
            let mut doc = engine.open(&path)?;
            let start = Instant::now();
            let objects = engine.text_objects(&doc, 0)?;
            let listed = start.elapsed();
            assert_eq!(objects.len(), count);
            let start = Instant::now();
            engine.edit(&mut doc, &objects[count / 2], "AB")?;
            let edited = start.elapsed();
            let after = engine.text_objects(&doc, 0)?;
            assert_eq!(after.len(), count);
            assert_eq!(after[count / 2].text, "AB");
            for (i, (old, new)) in objects.iter().zip(&after).enumerate() {
                if i != count / 2 {
                    assert_eq!(old.text, new.text);
                }
                assert!(
                    old.matrix
                        .iter()
                        .zip(new.matrix)
                        .all(|(a, b)| (a - b).abs() < 0.001)
                );
            }
            println!(
                "runs={count} iteration={iteration} list_us={} edit_us={}",
                listed.as_micros(),
                edited.as_micros()
            );
        }
    }
    Ok(())
}
