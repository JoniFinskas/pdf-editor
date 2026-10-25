use crate::*;
use lopdf::{Object, Stream, dictionary};

fn engine() -> Engine {
    Engine::new(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/debug/pdfium.dll"),
    )
    .unwrap()
}

fn document(engine: &Engine, rotation: i64) -> Document {
    let mut pdf = lopdf::Document::with_version("1.7");
    let parent = pdf.new_object_id();
    let font =
        pdf.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let mut pages = vec![];
    for i in 0..4 {
        let data = format!(
            "BT /F1 12 Tf 1 0 0 1 20 50 Tm (Page {i}) Tj ET\nBT /F1 12 Tf 1 0 0 1 100 120 Tm (Second) Tj ET\nBT /F1 12 Tf 1 0 0 1 250 200 Tm (Third) Tj ET\nBT /F1 12 Tf 1 0 0 1 20 280 Tm (Neighbor) Tj ET\n0 0 1 rg 30 320 20 30 re f\n"
        );
        let content = pdf.add_object(Stream::new(dictionary! {}, data.into_bytes()));
        let page=pdf.add_object(dictionary!{"Type"=>"Page","Parent"=>parent,"MediaBox"=>vec![0.into(),0.into(),400.into(),500.into()],"Rotate"=>rotation,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},"Contents"=>content});
        pages.push(Object::Reference(page));
    }
    pdf.objects.insert(
        parent,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Count"=>4,"Kids"=>pages}),
    );
    let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>parent});
    pdf.trailer.set("Root", catalog);
    let state = Document {
        bytes: vec![],
        original: PathBuf::from("generated.pdf"),
        undo: vec![],
        redo: vec![],
        revision: 0,
        protected: false,
        substitution: Substitution::default(),
    };
    engine.candidate(&state, pdf).unwrap()
}

fn order(engine: &Engine, doc: &Document) -> Vec<String> {
    (0..engine.pages(doc).unwrap().len())
        .map(|page| {
            engine
                .text_objects(doc, page as u16)
                .unwrap()
                .first()
                .map(|o| o.text.clone())
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn page_batch_move_slots_preserve_order_bookmarks_and_one_step_history() {
    let e = engine();
    for before in 0..=4 {
        let mut doc = document(&e, 0);
        e.edit_bookmarks(
            &mut doc,
            0,
            BookmarkEdit::Add {
                title: "Destination".into(),
                page: 2,
                parent: None,
            },
        )
        .unwrap();
        doc.undo.clear();
        let original = doc.bytes.clone();
        e.edit_page_batch(
            &mut doc,
            &PageBatch::Move {
                pages: vec![3, 1],
                before,
            },
        )
        .unwrap();
        let mut expected = vec![0, 2];
        let slot =
            usize::from(before) - [1, 3].iter().filter(|p| **p < usize::from(before)).count();
        expected.splice(slot..slot, [1, 3]);
        assert_eq!(
            order(&e, &doc),
            expected
                .iter()
                .map(|p| format!("Page {p}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            e.bookmarks(&doc).unwrap().items[0].page,
            Some(expected.iter().position(|p| *p == 2).unwrap() as u16)
        );
        if doc.bytes != original {
            assert_eq!(doc.undo.len(), 1);
            assert!(doc.undo());
            assert_eq!(doc.bytes, original);
            assert!(doc.redo());
        }
    }
}

#[test]
fn page_batch_duplicate_and_blank_save_reopen_and_content_are_independent() {
    let e = engine();
    let mut doc = document(&e, 90);
    let original = doc.bytes.clone();
    e.edit_page_batch(
        &mut doc,
        &PageBatch::Duplicate {
            pages: vec![3, 1],
            before: 2,
        },
    )
    .unwrap();
    assert_eq!(
        order(&e, &doc),
        ["Page 0", "Page 1", "Page 1", "Page 3", "Page 2", "Page 3"]
    );
    assert_eq!(doc.undo.len(), 1);
    let item = e.page_items(&doc, 2).unwrap()[0].clone();
    let before = e.text_objects(&doc, 1).unwrap()[0].matrix;
    e.edit_page_items(&mut doc, &[item], ObjectAction::Move { dx: 17.0, dy: 9.0 })
        .unwrap();
    assert_eq!(e.text_objects(&doc, 1).unwrap()[0].matrix, before);
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.bytes, original);
    assert!(doc.redo());
    e.edit_page_batch(
        &mut doc,
        &PageBatch::Blank {
            before: 1,
            width: 300.0,
            height: 600.0,
        },
    )
    .unwrap();
    assert_eq!(order(&e, &doc)[1], "");
    let sizes = e.pages(&doc).unwrap();
    assert_eq!((sizes[1].width, sizes[1].height), (300.0, 600.0));
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/test-output");
    fs::create_dir_all(&folder).unwrap();
    let saved = folder.join(format!("{}-batch-roundtrip.pdf", std::process::id()));
    let _ = fs::remove_file(&saved);
    doc.save_as(&saved).unwrap();
    assert_eq!(order(&e, &doc), order(&e, &e.open(&saved).unwrap()));
    fs::remove_file(saved).unwrap();
}

#[test]
fn page_batch_validation_rotation_delete_and_bookmark_failure_are_atomic() {
    let e = engine();
    let mut doc = document(&e, 0);
    let bytes = doc.bytes.clone();
    for edit in [
        PageBatch::Delete(vec![0, 1, 2, 3]),
        PageBatch::Rotate(vec![0, 0]),
        PageBatch::Move {
            pages: vec![4],
            before: 0,
        },
        PageBatch::Duplicate {
            pages: vec![0],
            before: 5,
        },
        PageBatch::Blank {
            before: 0,
            width: f32::NAN,
            height: 20.0,
        },
    ] {
        assert!(e.edit_page_batch(&mut doc, &edit).is_err());
        assert_eq!(doc.bytes, bytes);
        assert!(!doc.can_undo());
    }
    e.edit_page_batch(&mut doc, &PageBatch::Rotate(vec![3, 1]))
        .unwrap();
    let sizes = e.pages(&doc).unwrap();
    assert_eq!((sizes[0].width, sizes[1].width), (400.0, 500.0));
    assert!(doc.undo());
    assert_eq!(doc.bytes, bytes);
    e.edit_page_batch(&mut doc, &PageBatch::Delete(vec![3, 1]))
        .unwrap();
    assert_eq!(order(&e, &doc), ["Page 0", "Page 2"]);
    assert!(doc.undo());
    let revision = doc.revision;
    e.edit_bookmarks(
        &mut doc,
        revision,
        BookmarkEdit::Add {
            title: "Page".into(),
            page: 1,
            parent: None,
        },
    )
    .unwrap();
    let bytes = doc.bytes.clone();
    assert!(
        e.edit_page_batch(&mut doc, &PageBatch::Delete(vec![1]))
            .unwrap_err()
            .0
            .contains("bookmark")
    );
    assert_eq!(doc.bytes, bytes);
}

#[test]
fn page_batch_forms_keep_values_and_duplicate_independent_names() {
    let e = engine();
    let mut doc = document(&e, 0);
    for page in [0, 1] {
        e.design_form(
            &mut doc,
            FormDesignEdit::Add(NewFormField {
                page,
                name: format!("Field{page}"),
                required: false,
                bounds: Bounds {
                    left: 20.0,
                    bottom: 350.0,
                    right: 150.0,
                    top: 380.0,
                },
                kind: DesignFieldKind::Text,
            }),
        )
        .unwrap();
    }
    let field = e.form_fields(&doc).unwrap()[1].clone();
    e.fill_field(&mut doc, &field, &FieldValue::Text("Saved value".into()))
        .unwrap();
    doc.undo.clear();
    let original = doc.bytes.clone();
    e.edit_page_batch(
        &mut doc,
        &PageBatch::Duplicate {
            pages: vec![1],
            before: 4,
        },
    )
    .unwrap();
    let fields = e.form_fields(&doc).unwrap();
    assert_eq!(fields.len(), 3);
    assert_ne!(fields[1].name, fields[2].name);
    assert_eq!(fields[1].value, fields[2].value);
    let copied = fields[2].clone();
    e.fill_field(&mut doc, &copied, &FieldValue::Text("Independent".into()))
        .unwrap();
    assert_eq!(
        e.form_fields(&doc).unwrap()[1].value,
        FieldValue::Text("Saved value".into())
    );
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.bytes, original);
    e.edit_page_batch(&mut doc, &PageBatch::Delete(vec![0]))
        .unwrap();
    let fields = e.form_fields(&doc).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "Field1");
    assert_eq!(fields[0].page, 0);
}

#[test]
fn layout_all_modes_preserve_rotated_text_fonts_neighbors_and_undo() {
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        for layout in [
            ObjectLayout::AlignLeft,
            ObjectLayout::AlignRight,
            ObjectLayout::AlignTop,
            ObjectLayout::AlignBottom,
            ObjectLayout::AlignHorizontalCenter,
            ObjectLayout::AlignVerticalCenter,
            ObjectLayout::DistributeHorizontal,
            ObjectLayout::DistributeVertical,
        ] {
            let mut doc = document(&e, rotation);
            let original = doc.bytes.clone();
            let before = e.text_objects(&doc, 0).unwrap();
            let items = e.page_items(&doc, 0).unwrap();
            e.layout_page_items(&mut doc, &items[..3], layout).unwrap();
            let after = e.text_objects(&doc, 0).unwrap();
            assert_eq!(
                before
                    .iter()
                    .map(|o| (&o.text, &o.font, o.size))
                    .collect::<Vec<_>>(),
                after
                    .iter()
                    .map(|o| (&o.text, &o.font, o.size))
                    .collect::<Vec<_>>()
            );
            assert_eq!(before[3].matrix, after[3].matrix);
            let bounds: Vec<_> = e
                .page_items(&doc, 0)
                .unwrap()
                .into_iter()
                .take(3)
                .map(|i| i.bounds)
                .collect();
            let positions: Vec<_> = bounds
                .iter()
                .map(|b| match layout {
                    ObjectLayout::AlignLeft => b.left,
                    ObjectLayout::AlignRight => b.right,
                    ObjectLayout::AlignTop => b.top,
                    ObjectLayout::AlignBottom => b.bottom,
                    ObjectLayout::AlignHorizontalCenter => (b.left + b.right) * 0.5,
                    ObjectLayout::AlignVerticalCenter => (b.bottom + b.top) * 0.5,
                    _ => 0.0,
                })
                .collect();
            if !matches!(
                layout,
                ObjectLayout::DistributeHorizontal | ObjectLayout::DistributeVertical
            ) {
                assert!(
                    positions.windows(2).all(|p| (p[0] - p[1]).abs() < 0.1),
                    "{layout:?} {rotation}"
                );
            } else {
                let mut bounds = bounds;
                let horizontal = layout == ObjectLayout::DistributeHorizontal;
                bounds.sort_by(|a, b| {
                    if horizontal {
                        a.left.total_cmp(&b.left)
                    } else {
                        a.bottom.total_cmp(&b.bottom)
                    }
                });
                let gaps: Vec<_> = bounds
                    .windows(2)
                    .map(|p| {
                        if horizontal {
                            p[1].left - p[0].right
                        } else {
                            p[1].bottom - p[0].top
                        }
                    })
                    .collect();
                assert!((gaps[0] - gaps[1]).abs() < 0.1);
            }
            assert_eq!(doc.undo.len(), 1);
            assert!(doc.undo());
            assert_eq!(doc.bytes, original);
            assert!(doc.redo());
        }
    }
}

#[test]
fn layout_selection_and_object_no_op_validation_are_atomic() {
    let e = engine();
    let mut doc = document(&e, 0);
    let items = e.page_items(&doc, 0).unwrap();
    let bytes = doc.bytes.clone();
    let mut missing = items[0].clone();
    missing.index = 99;
    assert!(
        e.edit_page_items(
            &mut doc,
            &[missing],
            ObjectAction::Move { dx: 0.0, dy: 0.0 }
        )
        .is_err()
    );
    assert!(
        e.edit_page_items(
            &mut doc,
            &[items[0].clone(), items[0].clone()],
            ObjectAction::Move { dx: 0.0, dy: 0.0 }
        )
        .is_err()
    );
    assert!(
        e.layout_page_items(&mut doc, &items[..2], ObjectLayout::DistributeHorizontal)
            .is_err()
    );
    assert!(
        e.layout_page_items(
            &mut doc,
            &[items[0].clone(), items[0].clone()],
            ObjectLayout::AlignLeft
        )
        .is_err()
    );
    let other = e.page_items(&doc, 1).unwrap()[0].clone();
    assert!(
        e.layout_page_items(
            &mut doc,
            &[items[0].clone(), other],
            ObjectLayout::AlignLeft
        )
        .is_err()
    );
    assert_eq!(doc.bytes, bytes);
    assert!(!doc.can_undo());
}

#[test]
fn mixed_layout_preserves_text_and_unselected_artwork() {
    let e = engine();
    let mut doc = document(&e, 0);
    let original = doc.bytes.clone();
    let items = e.page_items(&doc, 0).unwrap();
    let selected = vec![items[0].clone(), items[2].clone(), items[4].clone()];
    let before = e.text_objects(&doc, 0).unwrap();
    e.layout_page_items(&mut doc, &selected, ObjectLayout::AlignLeft)
        .unwrap();
    let after = e.text_objects(&doc, 0).unwrap();
    assert_eq!(
        before
            .iter()
            .map(|o| (&o.text, &o.font, o.size))
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(|o| (&o.text, &o.font, o.size))
            .collect::<Vec<_>>()
    );
    assert_eq!(before[1].matrix, after[1].matrix);
    assert_eq!(before[3].matrix, after[3].matrix);
    let items = e.page_items(&doc, 0).unwrap();
    assert!(
        selected
            .iter()
            .all(|old| (items[old.index].bounds.left - items[0].bounds.left).abs() < 0.1)
    );
    assert!(doc.undo());
    assert_eq!(doc.bytes, original);
}

#[test]
fn batch_form_pages_extract_reordered_and_reopen_with_values() {
    let e = engine();
    let mut doc = document(&e, 0);
    for page in [0, 2] {
        e.design_form(
            &mut doc,
            FormDesignEdit::Add(NewFormField {
                page,
                name: format!("Name{page}"),
                required: false,
                bounds: Bounds {
                    left: 20.0,
                    bottom: 350.0,
                    right: 150.0,
                    top: 380.0,
                },
                kind: DesignFieldKind::Text,
            }),
        )
        .unwrap();
    }
    let field = e.form_fields(&doc).unwrap()[1].clone();
    e.fill_field(&mut doc, &field, &FieldValue::Text("Kept".into()))
        .unwrap();
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/test-output");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{}-batch-extract.pdf", std::process::id()));
    let _ = fs::remove_file(&path);
    e.extract_pages(&doc, &[2, 0], &path).unwrap();
    let extracted = e.open(&path).unwrap();
    assert_eq!(order(&e, &extracted), ["Page 2", "Page 0"]);
    let fields = e.form_fields(&extracted).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].value, FieldValue::Text("Kept".into()));
    assert_eq!(fields[0].name, "Name2");
    assert_eq!(fields[1].name, "Name0");
    fs::remove_file(path).unwrap();
}
