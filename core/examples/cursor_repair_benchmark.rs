//! Generated-only repeated cursor repair benchmark.
#[path = "support/cursor_fixture.rs"]
mod cursor_fixture;
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll"))?;
    let seed = std::fs::read(root.join("tests/fixtures/generated/unicode-cid.pdf"))?;
    let folder = root.join("target/performance");
    std::fs::create_dir_all(&folder)?;
    for chain in [false, true] {
        for pairs in [10, 50, 150] {
            let path = folder.join(format!("generated-cursor-{chain}-{pairs}.pdf"));
            std::fs::write(&path, cursor_fixture::cursor_page(&seed, pairs, chain))?;
            for iteration in 0..3 {
                let mut doc = engine.open(&path)?;
                let before = engine.text_objects(&doc, 0)?;
                assert_eq!(before.len(), pairs * 2);
                let query = before[0].text.clone();
                assert_ne!(query, before[1].text);
                let start = Instant::now();
                assert_eq!(engine.replace_text(&mut doc, &query, "AB", None)?, pairs);
                let elapsed = start.elapsed();
                let after = engine.text_objects(&doc, 0)?;
                assert_eq!(before.len(), after.len());
                for (i, (a, b)) in before.iter().zip(&after).enumerate() {
                    assert_eq!(b.text, if i % 2 == 0 { "AB" } else { &a.text });
                    assert!(
                        a.matrix
                            .iter()
                            .zip(b.matrix)
                            .all(|(a, b)| (a - b).abs() < 0.001)
                    );
                }
                println!(
                    "chain={chain} pairs={pairs} iteration={iteration} edit_us={}",
                    elapsed.as_micros()
                );
            }
        }
    }
    Ok(())
}
