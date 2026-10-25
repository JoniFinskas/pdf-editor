use super::*;

#[test]
fn text_characters_copy_unicode_and_generated_line_breaks() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("qol-copy.pdf");
    fs::write(&path, generated("Ää öå €", false, 0, 0., false, false)).unwrap();
    let doc = e.open(&path).unwrap();
    let chars = e.page_text_characters(&doc, 0).unwrap();
    assert!(!chars.is_empty());
    assert!(chars.iter().enumerate().all(|(i, c)| {
        c.page == 0
            && c.index == i
            && [c.bounds.left, c.bounds.right, c.bounds.bottom, c.bounds.top]
                .iter()
                .all(|v| v.is_finite())
    }));
    let copied = selected_text(&chars, 0, chars.len() - 1);
    assert!(copied.text.starts_with("Ää öå €"));
    assert!(copied.text.contains("\nunchanged"));
    assert!(!copied.text.contains('\r'));
    assert_eq!(selected_text(&chars, chars.len() - 1, 0).text, copied.text);
    assert_eq!(selected_text(&chars, 0, 1).text, "Ää");
    assert!(!copied.bounds.is_empty());
    assert!(e.page_text_characters(&doc, 9).is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn search_options_and_replacement_share_unicode_pages_and_character_regions() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("qol-search.pdf");
    fs::write(
        &path,
        generated("Ää ää ääni ää_ ää end", false, 0, 0., false, false),
    )
    .unwrap();
    let mut doc = e.open(&path).unwrap();
    e.edit_pages(
        &mut doc,
        &PageEdit::Insert {
            path: path.clone(),
            before: 1,
        },
    )
    .unwrap();
    let options = SearchOptions {
        whole_word: true,
        pages: Some(vec![1]),
        ..Default::default()
    };
    let hits = e.find_text_with_options(&doc, "ää", &options).unwrap();
    assert_eq!(hits.len(), 3);
    assert!(
        hits.iter()
            .all(|h| h.page == 1 && h.excerpt.chars().count() <= 162)
    );
    assert_eq!(
        e.replacement_count_with_options(&doc, "ää", &options)
            .unwrap(),
        hits.len()
    );
    let exact = SearchOptions {
        case_sensitive: true,
        ..options.clone()
    };
    assert_eq!(
        e.find_text_with_options(&doc, "Ää", &exact).unwrap().len(),
        1
    );
    let mut region = hits[0].bounds[0];
    for b in &hits[0].bounds {
        region.left = region.left.min(b.left);
        region.right = region.right.max(b.right);
        region.bottom = region.bottom.min(b.bottom);
        region.top = region.top.max(b.top);
    }
    region.left -= 0.01;
    region.right += 0.01;
    region.bottom -= 0.01;
    region.top += 0.01;
    let scoped = SearchOptions {
        regions: vec![(1, region)],
        ..options
    };
    assert_eq!(
        e.find_text_with_options(&doc, "ää", &scoped).unwrap().len(),
        1
    );
    let original = doc.bytes.clone();
    assert_eq!(
        e.replace_text_with_options(&mut doc, "ää", "ö", None, &scoped)
            .unwrap(),
        1
    );
    assert!(e.text_objects(&doc, 1).unwrap()[0].text.starts_with("ö ää"));
    assert!(
        e.text_objects(&doc, 0).unwrap()[0]
            .text
            .starts_with("Ää ää")
    );
    assert!(doc.undo());
    assert_eq!(doc.bytes, original);
    let invalid = SearchOptions {
        pages: Some(vec![9]),
        ..Default::default()
    };
    assert!(
        e.replace_text_with_options(&mut doc, "ää", "ö", None, &invalid)
            .is_err()
    );
    assert_eq!(doc.bytes, original);
    fs::remove_file(path).unwrap();
}

#[test]
fn formatting_changes_family_style_as_one_verified_undo_step() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let path = temp("qol-format.pdf");
        fs::write(&path, generated("Ää €", false, 0, 1., false, false)).unwrap();
        let mut doc = e.open(&path).unwrap();
        let before = e.text_objects(&doc, 0).unwrap();
        let original = doc.bytes.clone();
        let selected = vec![before[0].clone(), before[2].clone()];
        let format = TextFormat {
            font: "Arial".into(),
            bold,
            italic,
            size: 21.,
            color: [10, 80, 160],
        };
        let result = e.format_text(&mut doc, &selected, &format).unwrap();
        assert_eq!(result.objects_changed, 2);
        let after = e.text_objects(&doc, 0).unwrap();
        for i in [0, 2] {
            assert_eq!(after[i].text, before[i].text);
            assert_eq!(after[i].color[..3], format.color);
            assert!((after[i].size - format.size).abs() < 0.01);
            let name = after[i].font.to_lowercase();
            assert!(name.contains("arial"));
            assert_eq!(name.contains("bold"), bold);
            assert_eq!(name.contains("italic") || name.contains("oblique"), italic);
            assert!(
                after[i]
                    .matrix
                    .iter()
                    .zip(before[i].matrix)
                    .all(|(a, b)| (a - b).abs() < 0.001)
            );
        }
        assert_eq!(before[1].bounds, after[1].bounds);
        let stable = doc.bytes.clone();
        assert!(e.format_text(&mut doc, &selected, &format).is_err());
        assert_eq!(doc.bytes, stable);
        assert!(doc.undo());
        assert_eq!(doc.bytes, original);
        let invalid = TextFormat {
            font: "UnavailableFixtureFamily".into(),
            ..format
        };
        let fresh = e.text_objects(&doc, 0).unwrap();
        assert!(e.format_text(&mut doc, &fresh[..1], &invalid).is_err());
        assert_eq!(doc.bytes, original);
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn advanced_find_includes_nested_form_text_with_plain_search_options() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("qol-nested-search.pdf");
    fs::write(
        &path,
        generated("Ää ää ääni ää_ ää end", false, 0, 0., false, false),
    )
    .unwrap();
    let plain = e.open(&path).unwrap();
    let mut pdf = parse(&plain.bytes).unwrap();
    let page = pdf.get_pages()[&1];
    let inner = pdf.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Form",
            "BBox"=>vec![0.into(),0.into(),500.into(),300.into()],
            "Resources"=>content::resources(&pdf,page).unwrap(),
        },
        pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap(),
    ));
    let outer = pdf.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Form",
            "BBox"=>vec![0.into(),0.into(),500.into(),300.into()],
            "Resources"=>dictionary! {"XObject"=>dictionary! {"Inner"=>inner}},
        },
        b"/Inner Do".to_vec(),
    ));
    let contents = pdf.add_object(Stream::new(lopdf::Dictionary::new(), b"/Outer Do".to_vec()));
    let page_dict = pdf.get_dictionary_mut(page).unwrap();
    page_dict.set("Contents", contents);
    page_dict.set(
        "Resources",
        dictionary! {"XObject"=>dictionary! {"Outer"=>outer}},
    );
    let mut nested = e.candidate(&plain, pdf).unwrap();
    assert!(e.text_objects(&nested, 0).unwrap().is_empty());
    for (query, options, expected) in [
        ("ää", SearchOptions::default(), 5),
        (
            "Ää",
            SearchOptions {
                case_sensitive: true,
                ..Default::default()
            },
            1,
        ),
        (
            "ää",
            SearchOptions {
                whole_word: true,
                ..Default::default()
            },
            3,
        ),
        (
            "Ää",
            SearchOptions {
                whole_word: true,
                case_sensitive: true,
                ..Default::default()
            },
            1,
        ),
    ] {
        let plain_hits = e.find_text_with_options(&plain, query, &options).unwrap();
        let nested_hits = e.find_text_with_options(&nested, query, &options).unwrap();
        assert_eq!(plain_hits.len(), expected);
        assert_eq!(nested_hits.len(), expected);
        assert_eq!(
            e.replacement_count_with_options(&plain, query, &options)
                .unwrap(),
            expected
        );
        for (a, b) in plain_hits.iter().zip(&nested_hits) {
            assert_eq!(a.page, b.page);
            assert_eq!(a.bounds, b.bounds);
            assert_eq!(a.excerpt, b.excerpt);
        }
    }
    assert_eq!(e.find_text(&nested, "ää").unwrap().len(),e.find_text(&plain,"ää").unwrap().len());
    let mut region = e
        .find_text_with_options(
            &nested,
            "Ää",
            &SearchOptions {
                case_sensitive: true,
                ..Default::default()
            },
        )
        .unwrap()[0]
        .bounds[0];
    let matched = e
        .find_text_with_options(
            &nested,
            "Ää",
            &SearchOptions {
                case_sensitive: true,
                ..Default::default()
            },
        )
        .unwrap()
        .remove(0);
    for b in matched.bounds {
        region.left = region.left.min(b.left);
        region.right = region.right.max(b.right);
        region.bottom = region.bottom.min(b.bottom);
        region.top = region.top.max(b.top);
    }
    let scoped = SearchOptions {
        whole_word: true,
        regions: vec![(0, region)],
        ..Default::default()
    };
    assert_eq!(
        e.find_text_with_options(&nested, "ää", &scoped)
            .unwrap()
            .len(),
        1
    );
    assert!(
        e.find_text_with_options(
            &nested,
            "ää",
            &SearchOptions {
                pages: Some(vec![]),
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty()
    );
    let original = nested.bytes.clone();
    assert!(
        e.replace_text_with_options(&mut nested, "ää", "ö", Some(0), &scoped)
            .is_err()
    );
    assert_eq!(nested.bytes, original);
    fs::remove_file(path).unwrap();
}
