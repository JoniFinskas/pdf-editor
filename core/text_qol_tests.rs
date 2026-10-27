use super::*;

#[test]
fn bulk_replacement_reuses_font_decoding_without_mixing_character_maps() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let seed = generated("AB", false, 0, 0., false, false);
    let mut excessive = vec![];
    for (total, mixed) in [(1000, false), (1000, true), (4000, false)] {
        let mut bytes = dense_fixture::dense_page(&seed, total);
        if mixed {
            let mut pdf = parse(&bytes).unwrap();
            let page = pdf.get_pages()[&1];
            let font = fonts::Font {
                bytes: fs::read(root().join("tests/fonts/Inter-Regular.ttf")).unwrap(),
                name: "Inter-Regular".into(), exact: true,
            };
            // Same face, different CID ordering: a name-keyed cache would corrupt text.
            let second = fonts::embed(&mut pdf, &font, "BA").unwrap();
            let mut resources = content::resources(&pdf, page).unwrap();
            let mut font_map = resources.get_deref(b"Font", &pdf).unwrap().as_dict().unwrap().clone();
            font_map.set("F2", second.id);
            resources.set("Font", font_map);
            pdf.get_dictionary_mut(page).unwrap().set("Resources", resources);
            let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
            let mut index = 0;
            let mut alternate = false;
            for op in &mut ops.operations {
                if op.operator == "Tf" {
                    alternate = index % 2 == 1;
                    index += 1;
                    if alternate { op.operands[0] = Object::Name(b"F2".to_vec()); }
                } else if op.operator == "Tj" && alternate {
                    op.operands[0] = Object::String(second.encode("AB").unwrap(), lopdf::StringFormat::Hexadecimal);
                }
            }
            pdf.change_page_content(page, ops.encode().unwrap()).unwrap();
            bytes.clear();
            pdf.save_to(&mut bytes).unwrap();
        }
        let source = temp(&format!("bulk-encoding-{mixed}.pdf"));
        fs::write(&source, &bytes).unwrap();
        let mut doc = e.open(&source).unwrap();
        let before = e.text_objects(&doc, 0).unwrap();
        assert_eq!(before.len(), total);
        content::ENCODING_BUILDS.with(|count| count.set(0));
        content::ORDINAL_PROBES.with(|count| count.set(0));
        let started = std::time::Instant::now();
        assert_eq!(e.replace_text(&mut doc, "AB", "BA", None).unwrap(), total);
        let builds = content::ENCODING_BUILDS.with(|count| count.get());
        let probes = content::ORDINAL_PROBES.with(|count| count.get());
        eprintln!("objects={total} mixed={mixed} encoder_builds={builds} ordinal_probes={probes} elapsed_us={}", started.elapsed().as_micros());
        if probes > total * 8 { excessive.push((total, probes)); }
        assert!(builds <= 16, "Repeated font map construction: {builds}");
        let after = e.text_objects(&doc, 0).unwrap();
        assert_eq!(after.len(), before.len());
        for (old, new) in before.iter().zip(&after) {
            assert_eq!(new.text, "BA");
            assert_eq!(new.font, old.font);
            assert!(old.matrix.iter().zip(new.matrix).all(|(a,b)| (a-b).abs() < 0.001));
        }
        let changed = doc.bytes.clone();
        let saved = temp(&format!("bulk-encoding-saved-{mixed}.pdf"));
        if saved.exists() { fs::remove_file(&saved).unwrap(); }
        doc.save_as(&saved).unwrap();
        assert_eq!(e.render(&e.open(&saved).unwrap(), 0, 0.5).unwrap().rgba, e.render(&doc, 0, 0.5).unwrap().rgba);
        assert!(doc.undo());
        assert!(doc.bytes == bytes);
        assert!(doc.redo());
        assert!(doc.bytes == changed);
        assert_eq!(fs::read(&source).unwrap(), bytes);
        fs::remove_file(source).unwrap();
        fs::remove_file(saved).unwrap();
    }
    assert!(excessive.is_empty(), "Repeated ordinal scans: {excessive:?}");
}

#[test]
fn text_handles_from_other_content_or_tampered_groups_are_rejected() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source_a = temp("selection-source-a.pdf");
    let source_b = temp("selection-source-b.pdf");
    fs::write(&source_a, generated("AB", false, 0, 0., false, false)).unwrap();
    fs::write(&source_b, generated("BA", false, 0, 0., false, false)).unwrap();
    let a = e.open(&source_a).unwrap();
    let foreign = e.text_objects(&a, 0).unwrap().remove(0);
    for action in 0..4 {
        let mut doc = e.open(&source_b).unwrap();
        let bytes = doc.bytes.clone();
        let history = doc.recovery_bytes().unwrap();
        let result = match action {
            0 => e.edit(&mut doc, &foreign, "BA").map(|_| ()),
            1 => e.delete_text(&mut doc, &foreign).map(|_| ()),
            2 => e.style_text(&mut doc, &foreign, foreign.size + 1., [1,2,3]),
            _ => e.edit_group(&mut doc, std::slice::from_ref(&foreign), "Changed").map(|_| ()),
        };
        assert!(result.is_err(), "Foreign selection accepted for action {action}");
        assert!(doc.bytes == bytes);
        assert!(doc.recovery_bytes().unwrap() == history);
    }
    let mut doc = e.open(&fixture("paragraph-Left.pdf")).unwrap();
    let bytes = doc.bytes.clone();
    let mut selected = paragraphs(&e.text_objects(&doc, 0).unwrap()).remove(0);
    selected.objects[1].text = "Tampered".into();
    assert!(e.edit_paragraph(&mut doc, &selected, "Muutos").is_err());
    assert!(doc.bytes == bytes);
    fs::remove_file(source_a).unwrap();
    fs::remove_file(source_b).unwrap();
}

#[test]
fn ordinal_indexes_preserve_first_targets_and_sparse_deletion_order() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("ordinal-targets.pdf");
    let source = generated("AB", false, 0, 0., false, false);
    fs::write(&path, &source).unwrap();
    for sparse in [false, true] {
        let mut doc = e.open(&path).unwrap();
        let before = e.text_objects(&doc, 0).unwrap();
        assert_eq!(before.len(), 3);
        let targets = if sparse { vec![(2, String::new()), (0, "BA".into())] }
            else { vec![(0, "BA".into()), (0, "AB".into())] };
        let (bytes, result) = content::edit(&e, &doc, &before[0], &targets).unwrap();
        assert_eq!(result.objects_changed, if sparse { 2 } else { 1 });
        doc.accept(bytes);
        let after = e.text_objects(&doc, 0).unwrap();
        assert_eq!(after[0].text, "BA");
        assert_eq!(after.len(), if sparse { 2 } else { 3 });
        assert_eq!(after[1].text, before[1].text);
        assert_eq!(after[1].matrix, before[1].matrix);
        assert_eq!(after[1].font, before[1].font);
        let changed = doc.bytes.clone();
        assert!(doc.undo());
        assert!(doc.bytes == source);
        assert!(doc.redo());
        assert!(doc.bytes == changed);
    }
    assert_eq!(fs::read(&path).unwrap(), source);
    fs::remove_file(path).unwrap();
}

#[test]
fn unchanged_find_replace_preserves_structure_history_and_redo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for name in ["unicode-cid.pdf", "glyph-runs.pdf"] {
        let source = fixture(name);
        let original = fs::read(&source).unwrap();
        let mut doc = e.open(&source).unwrap();
        let query = if name == "glyph-runs.pdf" { "Hello world".to_owned() }
            else { e.text_objects(&doc, 0).unwrap()[0].text.clone() };
        let count = e.replacement_count(&doc, &query).unwrap();
        assert!(count > 0);
        for with_redo in [false, true] {
            if with_redo {
                let object = e.text_objects(&doc, 0).unwrap().remove(0);
                e.delete_text(&mut doc, &object).unwrap();
                assert!(doc.undo());
            }
            let history = doc.recovery_bytes().unwrap();
            for occurrence in [None, Some(0)] {
                let replaced = e.replace_text(&mut doc, &query, &query, occurrence).unwrap();
                assert!(doc.recovery_bytes().unwrap() == history, "Identical replacement altered PDF history: {name}");
                assert_eq!(replaced, 0);
                assert!(doc.bytes == original);
                assert_eq!(doc.can_redo(), with_redo);
            }
            assert!(e.replace_text(&mut doc, &query, &query, Some(count)).is_err());
            assert!(doc.recovery_bytes().unwrap() == history);
        }
        assert_eq!(fs::read(source).unwrap(), original);
    }
}

#[test]
fn find_replace_skips_identical_matches_without_renumbering_occurrences() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source = temp("replace-case-identity.pdf");
    let bytes = generated("Ää ää ÄÄ", false, 0, 0., false, false);
    fs::write(&source, &bytes).unwrap();
    for (occurrence, count, expected) in [
        (Some(1), 0, "Ää ää ÄÄ"), (Some(2), 1, "Ää ää ää"), (None, 2, "ää ää ää"),
    ] {
        let mut doc = e.open(&source).unwrap();
        assert_eq!(e.replacement_count(&doc, "ää").unwrap(), 3);
        assert_eq!(e.replace_text(&mut doc, "ää", "ää", occurrence).unwrap(), count);
        assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, expected);
        assert_eq!(doc.can_undo(), count > 0);
        if count > 0 { assert!(doc.undo()); }
        assert!(doc.bytes == bytes);
    }
    assert_eq!(fs::read(&source).unwrap(), bytes);
    fs::remove_file(source).unwrap();
}

#[test]
fn deleting_empty_extracted_glyph_runs_removes_the_object_and_preserves_pixels() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source = fixture("glyph-runs.pdf");
    let original = fs::read(&source).unwrap();
    let mut doc = e.open(&source).unwrap();
    let before = e.text_objects(&doc, 0).unwrap();
    let selected = before.iter().find(|o| o.text.is_empty()).expect("Generated browser PDF must contain a blank glyph run");
    let image = e.render(&doc, 0, 2.).unwrap();
    let result = e.delete_text(&mut doc, selected).unwrap();
    assert_eq!(result.objects_changed, 1, "The blank PDF object was not deleted");
    let after = e.text_objects(&doc, 0).unwrap();
    assert_eq!(after.len() + 1, before.len());
    for (old, new) in before.iter().filter(|o| o.index != selected.index).zip(&after) {
        assert_eq!(old.text, new.text);
        assert_eq!(old.font, new.font);
        assert!(old.matrix.iter().zip(new.matrix).all(|(a,b)| (a-b).abs() < 0.001));
    }
    assert_eq!(e.render(&doc, 0, 2.).unwrap().rgba, image.rgba);
    let saved = temp("blank-glyph-deleted.pdf");
    if saved.exists() { fs::remove_file(&saved).unwrap(); }
    doc.save_as(&saved).unwrap();
    assert_eq!(e.text_objects(&e.open(&saved).unwrap(), 0).unwrap().len(), after.len());
    let changed = doc.bytes.clone();
    assert!(doc.undo());
    assert!(doc.bytes == original);
    assert!(doc.redo());
    assert!(doc.bytes == changed);
    assert_eq!(fs::read(source).unwrap(), original);
    fs::remove_file(saved).unwrap();
}

#[test]
fn unchanged_style_checks_visible_stroke_color_before_skipping() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let original = e.open(&fixture("unicode-cid.pdf")).unwrap();
    for mode in [1, 2] {
        let mut pdf = parse(&generated("AB", false, 0, 0., false, false)).unwrap();
        let page = pdf.get_pages()[&1];
        let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
        let show = ops.operations.iter().position(|op| op.operator == "Tj").unwrap();
        ops.operations.splice(show..show, [
            Operation::new("RG", vec![0.8.into(), 0.1.into(), 0.1.into()]),
            Operation::new("Tr", vec![mode.into()]),
        ]);
        pdf.change_page_content(page, ops.encode().unwrap()).unwrap();
        let mut doc = e.candidate(&original, pdf).unwrap();
        let selected = e.text_objects(&doc, 0).unwrap().remove(0);
        let image = e.render(&doc, 0, 2.).unwrap();
        let before = doc.bytes.clone();
        e.style_text(&mut doc, &selected, selected.size, selected.color[..3].try_into().unwrap()).unwrap();
        assert!(e.render(&doc, 0, 2.).unwrap().rgba != image.rgba, "Visible stroke color was mistaken for an unchanged style");
        let selected = e.text_objects(&doc, 0).unwrap().remove(0);
        let after = doc.bytes.clone();
        let revision = doc.revision();
        e.style_text(&mut doc, &selected, selected.size, selected.color[..3].try_into().unwrap()).unwrap();
        assert!(doc.bytes == after);
        assert_eq!(doc.revision(), revision);
        assert!(doc.undo());
        assert!(doc.bytes == before);
    }
}

#[test]
fn unchanged_style_preserves_bytes_revision_and_redo_branch() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for name in ["spacing.pdf", "kerning.pdf", "rotated.pdf"] {
        let source = fixture(name);
        let original = fs::read(&source).unwrap();
        let mut doc = e.open(&source).unwrap();
        for with_redo in [false, true] {
            let selected = e.text_objects(&doc, 0).unwrap().remove(0);
            if with_redo {
                e.style_text(&mut doc, &selected, selected.size + 2., [30, 60, 90]).unwrap();
                assert!(doc.undo());
                assert!(e.style_text(&mut doc, &selected, selected.size, selected.color[..3].try_into().unwrap()).is_err());
            }
            let selected = e.text_objects(&doc, 0).unwrap().remove(0);
            let history = doc.recovery_bytes().unwrap();
            let revision = doc.revision();
            for _ in 0..3 {
                e.style_text(&mut doc, &selected, selected.size, selected.color[..3].try_into().unwrap()).unwrap();
                assert!(doc.bytes == original, "Unchanged style rewrote {name}");
                assert_eq!(doc.revision(), revision);
                assert!(doc.recovery_bytes().unwrap() == history, "Unchanged style altered history");
                assert_eq!(doc.can_redo(), with_redo);
                assert!(!doc.can_undo());
            }
        }
        assert_eq!(fs::read(source).unwrap(), original);
    }
}

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
        e.style_text(&mut doc, &after[1], after[1].size + 1., [80, 40, 20]).unwrap();
        let later = doc.bytes.clone();
        assert!(doc.undo());
        let fresh = e.text_objects(&doc, 0).unwrap();
        let same = vec![fresh[0].clone(), fresh[2].clone()];
        let history = doc.recovery_bytes().unwrap();
        let revision = doc.revision();
        for _ in 0..3 {
            assert_eq!(e.format_text(&mut doc, &same, &format).unwrap().objects_changed, 0);
            assert!(doc.bytes == stable);
            assert_eq!(doc.revision(), revision);
            assert!(doc.recovery_bytes().unwrap() == history);
            assert!(doc.can_redo());
        }
        assert!(doc.redo());
        assert!(doc.bytes == later);
        assert!(doc.undo());
        assert!(doc.bytes == stable);
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
fn identical_font_names_do_not_hide_program_width_or_stroke_changes() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let source = temp("format-identity.pdf");
    let original = generated("AB", false, 0, 0., false, false);
    fs::write(&source, &original).unwrap();
    let mut baseline = e.open(&source).unwrap();
    let format = TextFormat { font: "Arial".into(), bold: false, italic: false, size: 21., color: [10, 80, 160] };
    let selected = e.text_objects(&baseline, 0).unwrap().remove(0);
    e.format_text(&mut baseline, &[selected], &format).unwrap();
    for case in ["width", "program", "stroke"] {
        let mut pdf = parse(&baseline.bytes).unwrap();
        let page = pdf.get_pages()[&1];
        let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
        let runs = content::runs(&pdf, page, &ops).unwrap();
        let cid = runs[0].font.get(b"DescendantFonts").unwrap().as_array().unwrap()[0].as_reference().unwrap();
        match case {
            "width" => {
                pdf.get_dictionary_mut(cid).unwrap().get_mut(b"W").unwrap().as_array_mut().unwrap()[1]
                    .as_array_mut().unwrap()[0] = Object::Real(2000.);
            }
            "program" => {
                let descriptor = pdf.get_dictionary(cid).unwrap().get(b"FontDescriptor").unwrap().as_reference().unwrap();
                let program = pdf.get_dictionary(descriptor).unwrap().get(b"FontFile2").unwrap().as_reference().unwrap();
                let bytes = fs::read(root().join("tests/fonts/Inter-Regular.ttf")).unwrap();
                *pdf.get_object_mut(program).unwrap() = Object::Stream(Stream::new(dictionary! {"Length1"=>bytes.len() as i64}, bytes));
            }
            _ => {
                let show = ops.operations.iter().position(|op| op.operator == "Tj").unwrap();
                ops.operations.splice(show..show, [
                    Operation::new("RG", vec![0.8.into(), 0.1.into(), 0.1.into()]),
                    Operation::new("Tr", vec![2.into()]),
                ]);
                pdf.change_page_content(page, ops.encode().unwrap()).unwrap();
            }
        }
        let mut doc = e.candidate(&baseline, pdf).unwrap();
        let before = doc.bytes.clone();
        let image = e.render(&doc, 0, 2.).unwrap();
        let selected = e.text_objects(&doc, 0).unwrap().remove(0);
        assert_eq!(e.format_text(&mut doc, &[selected], &format).unwrap().objects_changed, 1, "{case}");
        let after = e.render(&doc, 0, 2.).unwrap();
        assert!(image.rgba != after.rgba, "Unchanged identity was assumed for {case}");
        let selected = e.text_objects(&doc, 0).unwrap().remove(0);
        let bytes = doc.bytes.clone();
        assert_eq!(e.format_text(&mut doc, &[selected], &format).unwrap().objects_changed, 0, "{case}");
        assert!(doc.bytes == bytes);
        let saved = temp(&format!("format-identity-{case}.pdf"));
        if saved.exists() { fs::remove_file(&saved).unwrap(); }
        doc.save_as(&saved).unwrap();
        assert_eq!(e.render(&e.open(&saved).unwrap(), 0, 2.).unwrap().rgba, after.rgba);
        assert!(doc.undo());
        assert!(doc.bytes == before);
        assert!(doc.redo());
        assert!(doc.bytes == bytes);
        fs::remove_file(saved).unwrap();
    }
    assert_eq!(fs::read(&source).unwrap(), original);
    fs::remove_file(source).unwrap();
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
