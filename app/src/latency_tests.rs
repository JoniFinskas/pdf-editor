use super::*;

#[test]
fn generated_preview_batch_rechecks_late_scales_without_crossing_mutations() {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = root.join("tests/fixtures/generated/multi-page.pdf");
    let original = std::fs::read(&source).unwrap();
    for mutation in [false, true] {
        let (requests, inbox) = channel();
        // Rendezvous replies keep the worker in its already-drained batch.
        let (outbox, replies) = sync_channel(0);
        requests.send(Request::Open(source.clone())).unwrap();
        requests.send(Request::Render { page: 0, scale: 1.0 }).unwrap();
        requests.send(Request::Render { page: 1, scale: 1.0 }).unwrap();
        if mutation {
            requests.send(Request::PageBatch(pdf_editor_core::PageBatch::Rotate(vec![1]))).unwrap();
        }
        let library = root.join("target/debug/pdfium.dll");
        let worker = thread::spawn(move || run(
            Default::default(), library, Substitution::Line, Default::default(),
            inbox, outbox, Default::default(),
        ));
        let Reply::Opened { generation, pages, .. } = replies.recv_timeout(Duration::from_secs(10)).unwrap() else {
            panic!("Generated document did not open");
        };
        assert!(pages.len() >= 2);
        // This arrives before the first render reply is accepted, but after
        // the older page-one request was removed from the input channel.
        requests.send(Request::Render { page: 1, scale: 2.0 }).unwrap();
        drop(requests);
        let mut rendered = Vec::new();
        let mut events = Vec::new();
        loop {
            match replies.recv_timeout(Duration::from_secs(10)) {
                Ok(Reply::Rendered { page, scale, generation, .. }) => {
                    rendered.push((page, scale, generation));
                    events.push("render");
                }
                Ok(Reply::Changed { .. }) => events.push("change"),
                Ok(Reply::Failed { message, .. }) => panic!("Generated preview failed: {message}"),
                Ok(_) => panic!("Unexpected generated preview reply"),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("Generated preview worker did not finish"),
            }
        }
        worker.join().unwrap();
        if mutation {
            assert_eq!(rendered, [(0, 1.0, generation), (1, 1.0, generation), (1, 2.0, generation + 1)]);
            assert_eq!(events, ["render", "render", "change", "render"]);
        } else {
            assert_eq!(rendered, [(0, 1.0, generation), (1, 2.0, generation)], "Superseded scale must not reach the native renderer");
            assert_eq!(events, ["render", "render"]);
        }
        assert_eq!(std::fs::read(&source).unwrap(), original);
    }
}

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
        comparison_original: None,
        clipboard: SharedClipboard::default(),
        engine: Some(engine),
        engine_error: String::new(),
        doc: None,
        working_copy: None,
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
