use super::*;
#[test]
fn tiles_match_full_renders_and_bound_large_zoom_allocations() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let engine = engine();
    for name in [
        "unicode-cid.pdf",
        "forms.pdf",
        "transformed.pdf",
        "rotated.pdf",
    ] {
        let path = fixture(name);
        let doc = engine.open(&path).unwrap();
        let whole = engine.render(&doc, 0, 2.).unwrap();
        let x = whole.width / 4;
        let y = whole.height / 4;
        let width = (whole.width / 2).min(600);
        let height = (whole.height / 2).min(600);
        let tile = engine
            .render_tile(&doc, 0, 2., [x, y, width, height])
            .unwrap();
        // PDFium can round a few antialiased edge channels differently
        // after an integer origin translation; geometry and density must agree.
        let mut max_diff = 0u8;
        let mut total_diff = 0u64;
        let mut mismatched = 0;
        for row in 0..height as usize {
            let from = ((y as usize + row) * whole.width as usize + x as usize) * 4;
            let to = row * width as usize * 4;
            for (a, b) in tile.rgba[to..to + width as usize * 4]
                .iter()
                .zip(&whole.rgba[from..from + width as usize * 4])
            {
                let diff = a.abs_diff(*b);
                max_diff = max_diff.max(diff);
                total_diff += u64::from(diff);
                mismatched += usize::from(diff > 0);
            }
        }
        eprintln!(
            "{name}: max={max_diff}, total={total_diff}, channels={mismatched}, count={}",
            tile.rgba.len()
        );
        assert!(
            max_diff <= 100
                && total_diff as f64 / (tile.rgba.len() as f64) < 0.03
                && mismatched as f64 / (tile.rgba.len() as f64) < 0.001,
            "tile render differs: max={max_diff} total={total_diff}"
        );
        assert!(engine.render_tiles(&doc, 0, 2., &[]).is_err());
        assert!(
            engine
                .render_tiles(&doc, 0, 2., &[[0, 0, 2, 2]; 17])
                .is_err()
        );
        assert!(
            engine
                .render_tiles(&doc, 0, 2., &[[0, 0, 2048, 2048]; 2])
                .is_err()
        );
        let tile = engine
            .render_tile(&doc, 0, 32., [1200, 1400, 512, 512])
            .unwrap();
        assert_eq!(tile.rgba.len(), 512 * 512 * 4);
        for invalid in [[0, 0, 0, 2], [0, 0, 2049, 2], [u32::MAX, 0, 10, 10]] {
            assert!(engine.render_tile(&doc, 0, 2., invalid).is_err());
        }
    }
}
