use super::*;

fn bytes_contain(pdf: &lopdf::Document, needle: &[u8]) -> bool {
    pdf.objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .any(|s| {
            s.get_plain_content()
                .unwrap()
                .windows(needle.len())
                .any(|w| w == needle)
        })
}

#[test]
fn text_vector_redaction_preserves_other_runs_and_reopens() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        let mut pdf = parse(&generated("Secret", false, rotation, 0.0, false, false)).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let mut data = pdf.get_page_content(page);
        data.extend_from_slice(
            b"\nq 0 1 0 rg 40 210 20 20 re f Q\nq 0 0 1 rg 240 30 20 20 re f Q\n",
        );
        let id = pdf.add_object(Stream::new(lopdf::Dictionary::new(), data));
        pdf.get_dictionary_mut(page).unwrap().set("Contents", id);
        let seed = Document {
            bytes: generated("seed", false, 0, 0.0, false, false),
            original: PathBuf::new(),
            undo: vec![],
            redo: vec![],
            revision: 0,
            protected: false,
            substitution: Substitution::default(),
        };
        let mut doc = e.candidate(&seed, pdf).unwrap();
        let before = doc.bytes.clone();
        let items = e.text_objects(&doc, 0).unwrap();
        let selected = items.iter().find(|o| o.text.contains("Secret")).unwrap();
        let encoded = selected.text.clone();
        let region = Bounds {
            left: 39.0,
            bottom: 209.0,
            right: selected.bounds.right + 1.0,
            top: 240.0,
        };
        e.redact(&mut doc, 0, region).unwrap();
        let after = e.text_objects(&doc, 0).unwrap();
        assert!(after.iter().all(|o| !o.text.contains(&encoded)));
        assert!(after.iter().any(|o| o.text.contains("unchanged")));
        assert!(
            e.page_items(&doc, 0)
                .unwrap()
                .iter()
                .any(|o| o.kind == "Path" && o.bounds.left >= 239.0)
        );
        let path = temp(&format!("redact-rotation-{rotation}.pdf"));
        let _ = fs::remove_file(&path);
        doc.save_as(&path).unwrap();
        let mut reopened = e.open(&path).unwrap();
        assert!(
            e.text_objects(&reopened, 0)
                .unwrap()
                .iter()
                .all(|o| !o.text.contains("Secret"))
        );
        assert_eq!(
            e.render(&doc, 0, 1.0).unwrap().rgba,
            e.render(&reopened, 0, 1.0).unwrap().rgba
        );
        assert!(!bytes_contain(&parse(&doc.bytes).unwrap(), b"40 210 20 20"));
        let remaining = e
            .text_objects(&reopened, 0)
            .unwrap()
            .into_iter()
            .find(|o| o.text.contains("unchanged"))
            .unwrap();
        e.edit(&mut reopened, &remaining, "AB").unwrap();
        assert!(
            e.text_objects(&reopened, 0)
                .unwrap()
                .iter()
                .any(|o| o.text == "AB")
        );
        doc.undo();
        assert_eq!(doc.bytes, before);
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn redaction_batch_invalid_region_is_atomic() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = doc.bytes.clone();
    assert!(
        e.redact_regions(
            &mut doc,
            &[
                (
                    0,
                    Bounds {
                        left: 1.0,
                        bottom: 1.0,
                        right: 20.0,
                        top: 20.0
                    }
                ),
                (
                    999,
                    Bounds {
                        left: 1.0,
                        bottom: 1.0,
                        right: 20.0,
                        top: 20.0
                    }
                )
            ]
        )
        .is_err()
    );
    assert_eq!(doc.bytes, before);
    assert!(!doc.can_undo());
}

#[test]
fn nested_form_redaction_drops_unused_streams_and_preserves_shared_page() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    for shared in [false, true] {
        let seed = Document {
            bytes: generated("NestedSecret", false, 0, 0.0, false, false),
            original: PathBuf::new(),
            undo: vec![],
            redo: vec![],
            revision: 0,
            protected: false,
            substitution: Substitution::default(),
        };
        let mut pdf = parse(&seed.bytes).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let resources = content::resources(&pdf, page).unwrap();
        let contents = pdf.get_page_content(page);
        let inner=pdf.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),300.into(),300.into()],"Resources"=>resources},contents));
        let outer=pdf.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),300.into(),300.into()],"Resources"=>dictionary!{"XObject"=>dictionary!{"Inner"=>inner}}},b"q 0 0 300 300 re W n /Inner Do Q".to_vec()));
        let stream = pdf.add_object(Stream::new(
            lopdf::Dictionary::new(),
            b"q /Outer Do Q".to_vec(),
        ));
        let p = pdf.get_dictionary_mut(page).unwrap();
        p.set(
            "Resources",
            dictionary! {"XObject"=>dictionary!{"Outer"=>outer}},
        );
        p.set("Contents", stream);
        if shared {
            let copy = pdf.get_dictionary(page).unwrap().clone();
            let parent = copy.get(b"Parent").unwrap().as_reference().unwrap();
            let second = pdf.add_object(copy);
            let tree = pdf.get_dictionary_mut(parent).unwrap();
            tree.set(
                "Kids",
                vec![Object::Reference(page), Object::Reference(second)],
            );
            tree.set("Count", 2);
        }
        let mut doc = e.candidate(&seed, pdf).unwrap();
        let unaffected = if shared {
            Some(e.render(&doc, 1, 1.0).unwrap().rgba)
        } else {
            None
        };
        e.redact(
            &mut doc,
            0,
            Bounds {
                left: 45.0,
                bottom: 210.0,
                right: 100.0,
                top: 240.0,
            },
        )
        .unwrap();
        assert!(e.text_objects(&doc, 0).unwrap().is_empty());
        if shared {
            assert_eq!(unaffected.unwrap(), e.render(&doc, 1, 1.0).unwrap().rgba);
            assert!(
                e.pdfium
                    .load_pdf_from_byte_slice(&doc.bytes, None)
                    .unwrap()
                    .pages()
                    .get(1)
                    .unwrap()
                    .text()
                    .unwrap()
                    .all()
                    .contains("NestedSecret")
            );
        } else {
            assert!(
                parse(&doc.bytes)
                    .unwrap()
                    .objects
                    .values()
                    .filter_map(|o| o.as_stream().ok())
                    .all(
                        |s| s.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok())
                            != Some(b"Form")
                    )
            );
        }
    }
}

#[test]
fn redaction_removes_unused_resource_graphs_and_metadata() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    let seed = Document {
        bytes: generated("Secret", false, 0, 0.0, false, false),
        original: PathBuf::new(),
        undo: vec![],
        redo: vec![],
        revision: 0,
        protected: false,
        substitution: Substitution::default(),
    };
    let mut pdf = parse(&seed.bytes).unwrap();
    let page = *pdf.get_pages().values().next().unwrap();
    let leak = pdf.add_object(Stream::new(
        dictionary! {"Type"=>"Metadata"},
        b"REDACTION_PRIVATE_MARKER_7391".to_vec(),
    ));
    let mut resources = content::resources(&pdf, page).unwrap();
    for category in ["XObject", "Pattern", "Shading", "ExtGState"] {
        resources.set(category, dictionary! {"Unused"=>leak});
    }
    let fonts = resources.get(b"Font").unwrap().as_dict().unwrap().clone();
    for (_, v) in fonts.iter() {
        pdf.get_dictionary_mut(v.as_reference().unwrap())
            .unwrap()
            .set("PrivateSource", leak);
    }
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Resources", resources);
    let mut doc = e.candidate(&seed, pdf).unwrap();
    e.redact(
        &mut doc,
        0,
        Bounds {
            left: 45.0,
            bottom: 210.0,
            right: 220.0,
            top: 240.0,
        },
    )
    .unwrap();
    let result = parse(&doc.bytes).unwrap();
    assert!(!bytes_contain(&result, b"REDACTION_PRIVATE_MARKER_7391"));
    assert!(!result.extract_text(&[1]).unwrap().contains("Secret"));
    assert!(result.extract_text(&[1]).unwrap().contains("unchanged"));
}
