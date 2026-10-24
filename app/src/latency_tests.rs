use super::*;

#[test]
fn preview_queue_prioritizes_text_and_keeps_mutation_boundaries() {
    let requests = coalesce(vec![
        Request::Thumbnail(0),
        Request::Render {
            page: 0,
            scale: 1.0,
        },
        Request::Thumbnail(0),
        Request::Objects { page: 0 },
        Request::Render {
            page: 0,
            scale: 2.0,
        },
        Request::Undo,
        Request::Objects { page: 0 },
        Request::Thumbnail(0),
        Request::Thumbnail(0),
    ]);
    assert_eq!(
        requests.len(),
        6,
        "Duplicate previews must coalesce within each document revision"
    );
    assert!(matches!(requests[0], Request::Objects { page: 0 }));
    assert!(matches!(requests[1],Request::Render{page:0,scale} if scale==2.0));
    assert!(matches!(requests[2], Request::Thumbnail(0)));
    assert!(matches!(requests[3], Request::Undo));
    assert!(matches!(requests[4], Request::Objects { page: 0 }));
    assert!(matches!(requests[5], Request::Thumbnail(0)));
}

#[test]
fn generated_worker_stage_timings() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let mut state = State {
        clipboard: SharedClipboard::default(),
        engine: Some(engine),
        engine_error: String::new(),
        doc: None,
        generation: 0,
        policy: Substitution::Line,
    };
    for fixture in [
        "multi-page.pdf",
        "unicode-cid.pdf",
        "transformed.pdf",
        "spacing.pdf",
        "glyph-runs.pdf",
    ] {
        for iteration in 0..3 {
            let mut measure = |stage, request| {
                let start = std::time::Instant::now();
                let reply = handle(&mut state, request).unwrap();
                if let Reply::Failed { message, .. } = &reply {
                    panic!("{fixture} {stage}: {message}");
                }
                println!(
                    "fixture={fixture} iteration={iteration} stage={stage} us={}",
                    start.elapsed().as_micros()
                );
                reply
            };
            measure(
                "open",
                Request::Open(root.join("tests/fixtures/generated").join(fixture)),
            );
            let Reply::Objects { objects, .. } = measure("objects", Request::Objects { page: 0 })
            else {
                panic!()
            };
            measure(
                "render",
                Request::Render {
                    page: 0,
                    scale: 1.0,
                },
            );
            measure("thumbnail", Request::Thumbnail(0));
            if let Some(object) = objects.first() {
                let text = object.text.chars().take(1).collect();
                measure(
                    "edit",
                    Request::Edit {
                        objects: vec![object.clone()],
                        text,
                    },
                );
            }
        }
    }
}
