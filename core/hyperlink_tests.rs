use super::*;

fn link(page: u16, target: LinkTarget) -> LinkSpec {
    LinkSpec {
        page,
        bounds: Bounds {
            left: 20.,
            bottom: 30.,
            right: 160.,
            top: 60.,
        },
        target,
    }
}

#[test]
fn links_validate_destinations_without_executing_actions() {
    for bad in [
        "javascript:alert(1)",
        "file:///private",
        "data:text/plain,hello",
        "ftp://example.org",
        "https://user:password@example.org",
        "https://example.org/%0a",
        "https://example.org/%",
        "https://:443",
        "http://example.org:65536",
        "http://example.org:0",
        "mailto:missing",
        "mailto:a@example.org%0d",
        "https://example.org\\path",
    ] {
        assert!(LinkTarget::uri(bad).is_err(), "Accepted {bad}");
    }
    for valid in [
        "https://example.org",
        "http://localhost:8080/path?q=1#top",
        "https://[::1]:443/a",
        "mailto:reader@example.org?subject=Hello%20there",
    ] {
        assert_eq!(
            LinkTarget::uri(valid).unwrap(),
            LinkTarget::Uri(valid.into())
        );
    }
    assert_eq!(
        LinkTarget::uri(" HTTPS://example.org/ä b ").unwrap(),
        LinkTarget::Uri("https://example.org/%C3%A4%20b".into())
    );
}

#[test]
fn links_crud_roundtrip_preserves_pixels_history_and_source() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = fixture("multi-page.pdf");
    let source = fs::read(&path).unwrap();
    let mut doc = e.open(&path).unwrap();
    let original = doc.bytes.clone();
    let before = e.render(&doc, 0, 1.).unwrap().rgba;
    for target in [
        LinkTarget::uri("https://example.org/a").unwrap(),
        LinkTarget::uri("mailto:reader@example.org").unwrap(),
        LinkTarget::Page(1),
    ] {
        let spec = link(0, target);
        e.add_link(&mut doc, &spec).unwrap();
        let selected = e.links(&doc).unwrap().pop().unwrap();
        assert_eq!(selected.link, spec);
        let state = doc.recovery_bytes().unwrap();
        let revision = doc.revision;
        e.update_link(&mut doc, &selected, &spec).unwrap();
        assert_eq!(doc.recovery_bytes().unwrap(), state);
        assert_eq!(doc.revision, revision);
        let mut changed = spec.clone();
        changed.bounds.left += 5.;
        changed.target = LinkTarget::uri("https://example.org/changed").unwrap();
        e.update_link(&mut doc, &selected, &changed).unwrap();
        assert!(e.delete_link(&mut doc, &selected).is_err());
        assert_eq!(before, e.render(&doc, 0, 1.).unwrap().rgba);
        let output = temp("link-roundtrip.pdf");
        let _ = fs::remove_file(&output);
        doc.save_as(&output).unwrap();
        let opened = e.open(&output).unwrap();
        assert!(
            e.links(&opened)
                .unwrap()
                .iter()
                .any(|item| item.link == changed)
        );
        fs::remove_file(output).unwrap();
        assert!(doc.undo());
        assert_eq!(e.links(&doc).unwrap().last().unwrap().link, spec);
        assert!(doc.redo());
        let current = e.links(&doc).unwrap().pop().unwrap();
        e.delete_link(&mut doc, &current).unwrap();
        assert!(e.links(&doc).unwrap().is_empty());
    }
    assert_eq!(fs::read(path).unwrap(), source);
    while doc.undo() {}
    assert_eq!(doc.bytes, original);
}

#[test]
fn moving_links_preserves_named_destination_view_and_shared_annotations() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    let pages = pdf.get_pages();
    let destination = Object::Array(vec![
        pages[&2].into(),
        Object::Name(b"XYZ".to_vec()),
        40.into(),
        200.into(),
        Object::Real(1.5),
    ]);
    pdf.catalog_mut()
        .unwrap()
        .set("Dests", dictionary! {"chapter"=>destination.clone()});
    let annotation=pdf.add_object(dictionary! {"Type"=>"Annot","Subtype"=>"Link","Rect"=>vec![20.into(),30.into(),160.into(),60.into()],"Dest"=>Object::Name(b"chapter".to_vec()),"Border"=>vec![0.into(),0.into(),2.into()],"C"=>vec![1.into(),0.into(),0.into()],"AA"=>dictionary! {"E"=>dictionary! {"S"=>"JavaScript","JS"=>Object::string_literal("unused()")}}});
    pdf.get_dictionary_mut(pages[&1]).unwrap().set(
        "Annots",
        vec![Object::Reference(annotation), Object::Reference(annotation)],
    );
    doc = e.candidate(&doc, pdf).unwrap();
    let initial = doc.bytes.clone();
    let selected = e.links(&doc).unwrap().remove(0);
    assert_eq!(selected.link.target, LinkTarget::Page(1));
    let mut spec = selected.link.clone();
    spec.bounds.left += 10.;
    e.update_link(&mut doc, &selected, &spec).unwrap();
    let links = e.links(&doc).unwrap();
    assert_eq!(links[1].link, selected.link);
    assert_ne!(links[0].object_id, links[1].object_id);
    let pdf = parse(&doc.bytes).unwrap();
    let current = pdf.get_dictionary(links[0].object_id.unwrap()).unwrap();
    assert_eq!(
        current.get(b"Dest").unwrap(),
        &Object::Name(b"chapter".to_vec())
    );
    assert!(!current.has(b"AA"));
    assert!(!current.has(b"A"));
    assert_eq!(
        current.get(b"Border").unwrap().as_array().unwrap()[2],
        Object::Integer(2)
    );
    assert_eq!(
        pdf.catalog()
            .unwrap()
            .get(b"Dests")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"chapter")
            .unwrap(),
        &destination
    );
    assert!(pdf.get_dictionary(annotation).unwrap().has(b"AA"));
    assert!(doc.undo());
    assert_eq!(doc.bytes, initial);
}

#[test]
fn links_reject_foreign_tampered_and_invalid_mutations_atomically() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    e.add_link(&mut doc, &link(0, LinkTarget::Page(1))).unwrap();
    let selected = e.links(&doc).unwrap().remove(0);
    let state = doc.recovery_bytes().unwrap();
    let mut foreign = selected.clone();
    foreign.link.bounds.right += 1.;
    assert!(e.delete_link(&mut doc, &foreign).is_err());
    let mut foreign = selected.clone();
    foreign.object_id = Some((99999, 0));
    assert!(e.update_link(&mut doc, &foreign, &selected.link).is_err());
    for spec in [
        link(900, LinkTarget::Page(0)),
        link(0, LinkTarget::Page(900)),
        link(0, LinkTarget::Uri("javascript:unused()".into())),
        link(0, LinkTarget::Unsupported),
        LinkSpec {
            bounds: Bounds {
                left: f32::NAN,
                ..selected.link.bounds
            },
            ..selected.link.clone()
        },
    ] {
        assert!(e.add_link(&mut doc, &spec).is_err());
    }
    assert_eq!(doc.recovery_bytes().unwrap(), state);
    e.delete_link(&mut doc, &selected).unwrap();
    assert!(doc.undo());
    let selected = e.links(&doc).unwrap().remove(0);
    let state = doc.recovery_bytes().unwrap();
    e.update_link(&mut doc, &selected, &selected.link).unwrap();
    assert_eq!(doc.recovery_bytes().unwrap(), state);
    assert!(doc.can_redo());
}

#[test]
fn link_replacement_removes_secondary_actions_and_retains_goto_view() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    let pages = pdf.get_pages();
    let view = Object::Array(vec![
        pages[&2].into(),
        Object::Name(b"FitH".to_vec()),
        123.into(),
    ]);
    let item=pdf.add_object(dictionary! {"Subtype"=>"Link","Rect"=>vec![10.into(),20.into(),60.into(),70.into()],"A"=>dictionary! {"S"=>"GoTo","D"=>view.clone(),"Next"=>dictionary! {"S"=>"Launch","F"=>Object::string_literal("unused")}}});
    pdf.get_dictionary_mut(pages[&1])
        .unwrap()
        .set("Annots", vec![Object::Reference(item)]);
    doc = e.candidate(&doc, pdf).unwrap();
    let selected = e.links(&doc).unwrap().remove(0);
    let mut spec = selected.link.clone();
    spec.bounds.right += 20.;
    e.update_link(&mut doc, &selected, &spec).unwrap();
    let selected = e.links(&doc).unwrap().remove(0);
    let pdf = parse(&doc.bytes).unwrap();
    let dict = pdf.get_dictionary(selected.object_id.unwrap()).unwrap();
    assert_eq!(dict.get(b"Dest").unwrap(), &view);
    assert!(!dict.has(b"A"));
    let mut spec = selected.link.clone();
    spec.target = LinkTarget::uri("HTTPS://example.org/new").unwrap();
    e.update_link(&mut doc, &selected, &spec).unwrap();
    let selected = e.links(&doc).unwrap().remove(0);
    let pdf = parse(&doc.bytes).unwrap();
    let dict = pdf.get_dictionary(selected.object_id.unwrap()).unwrap();
    assert!(!dict.has(b"Dest"));
    let action = dict.get(b"A").unwrap().as_dict().unwrap();
    assert_eq!(action.len(), 2);
    assert_eq!(
        action.get(b"URI").unwrap().as_str().unwrap(),
        b"https://example.org/new"
    );
}
