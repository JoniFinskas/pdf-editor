use super::*;
use lopdf::{
    Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use std::sync::Mutex;
static PDFIUM: Mutex<()> = Mutex::new(());

#[test]
fn grouped_text_preparation_keeps_page_transparency_and_allows_editing() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("grouped-text.pdf");
    fs::write(&path, generated("Grouped", false, 0, 0., false, false)).unwrap();
    let original = e.open(&path).unwrap();
    let mut pdf = parse(&original.bytes).unwrap();
    let page = pdf.get_pages()[&1];
    let form = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(),0.into(),612.into(),792.into()],
            "Group" => dictionary! { "S" => "Transparency", "I" => true, "CS" => "DeviceRGB" },
            "Resources" => content::resources(&pdf, page).unwrap(),
        },
        pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap(),
    ));
    let stream = pdf.add_object(Stream::new(
        lopdf::Dictionary::new(),
        b"q /Design Do Q".to_vec(),
    ));
    let dict = pdf.get_dictionary_mut(page).unwrap();
    dict.set("Contents", stream);
    dict.set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Design" => form } },
    );
    let mut doc = e.candidate(&original, pdf).unwrap();
    let original_bytes = doc.bytes.clone();
    let before = e.render(&doc, 0, 2.).unwrap();
    assert!(e.text_objects(&doc, 0).unwrap().is_empty());
    let group = e.page_items(&doc, 0).unwrap().remove(0);
    e.prepare_grouped_text(&mut doc, &group).unwrap();
    let after = e.render(&doc, 0, 2.).unwrap();
    assert_eq!(before.rgba.len(), after.rgba.len());
    assert!(
        before
            .rgba
            .iter()
            .zip(&after.rgba)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
    let text = e.text_objects(&doc, 0).unwrap().remove(0);
    e.edit(&mut doc, &text, "Group").unwrap();
    assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, "Group");
    let saved = temp("grouped-text-saved.pdf");
    let _ = fs::remove_file(&saved);
    doc.save_as(&saved).unwrap();
    assert_eq!(
        e.text_objects(&e.open(&saved).unwrap(), 0).unwrap()[0].text,
        "Group"
    );
    doc.undo();
    doc.undo();
    assert_eq!(doc.bytes, original_bytes);
    fs::remove_file(path).unwrap();
    fs::remove_file(saved).unwrap();
}

#[test]
fn ungroup_nested_shared_forms_preserves_render_and_allows_edits() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        let path = temp(&format!("ungroup-{rotation}.pdf"));
        fs::write(
            &path,
            generated("Grouped", false, rotation, 0., false, false),
        )
        .unwrap();
        let original = e.open(&path).unwrap();
        let mut pdf = parse(&original.bytes).unwrap();
        let page = pdf.get_pages()[&1];
        let mut resources = content::resources(&pdf, page).unwrap();
        let picture = pdf.add_object(Stream::new(dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1,
            "Height" => 1, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        }, vec![255, 0, 0]));
        resources.set("XObject", dictionary! { "Picture" => picture });
        let mut data = pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap();
        data.extend_from_slice(b" q 0 0 1 rg 20 20 70 20 re f Q");
        data.extend_from_slice(b" q 25 0 0 25 60 60 cm /Picture Do Q");
        let child = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![30.into(),30.into(),550.into(),780.into()],
                "Resources" => resources,
            },
            data,
        ));
        let parent = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![30.into(),30.into(),550.into(),780.into()],
                "Matrix" => vec![0.8.into(),0.into(),0.into(),0.8.into(),10.into(),15.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Inner" => child } },
            },
            b"/Inner Do".to_vec(),
        ));
        let stream = pdf.add_object(Stream::new(
            lopdf::Dictionary::new(),
            b"q /Group Do Q q 1 0 0 1 100 0 cm /Group Do Q".to_vec(),
        ));
        let page_dict = pdf.get_dictionary_mut(page).unwrap();
        page_dict.set("Contents", stream);
        page_dict.set(
            "Resources",
            dictionary! { "XObject" => dictionary! { "Group" => parent } },
        );
        let mut doc = e.candidate(&original, pdf.clone()).unwrap();
        let before = doc.bytes.clone();
        let image = e.render(&doc, 0, 1.).unwrap();
        let item = e.page_items(&doc, 0).unwrap().remove(0);
        assert_eq!(item.kind, "XObjectForm");
        e.ungroup_page_item(&mut doc, &item).unwrap();
        assert_eq!(image.rgba, e.render(&doc, 0, 1.).unwrap().rgba);
        let items = e.page_items(&doc, 0).unwrap();
        assert_eq!(items.iter().filter(|i| i.kind == "XObjectForm").count(), 1);
        assert!(items.iter().any(|i| i.kind == "Text"));
        assert!(items.iter().any(|i| i.kind == "Image"));
        let path_item = items.iter().find(|i| i.kind == "Path").unwrap();
        e.edit_page_items(
            &mut doc,
            std::slice::from_ref(path_item),
            ObjectAction::Move { dx: 10., dy: 8. },
        )
        .unwrap();
        assert_ne!(image.rgba, e.render(&doc, 0, 1.).unwrap().rgba);
        doc.undo();
        let editable = e.text_objects(&doc, 0).unwrap().remove(0);
        e.edit(&mut doc, &editable, "Group").unwrap();
        assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, "Group");
        doc.undo();
        let text = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.kind == "Text")
            .unwrap();
        e.edit_page_items(&mut doc, &[text], ObjectAction::Delete)
            .unwrap();
        doc.undo();
        assert_eq!(image.rgba, e.render(&doc, 0, 1.).unwrap().rgba);
        let saved = temp(&format!("ungroup-saved-{rotation}.pdf"));
        let _ = fs::remove_file(&saved);
        doc.save_as(&saved).unwrap();
        assert_eq!(
            image.rgba,
            e.render(&e.open(&saved).unwrap(), 0, 1.).unwrap().rgba
        );
        doc.undo();
        assert_eq!(doc.bytes, before);
        assert!(e.ungroup_page_item(&mut doc, &item).is_err());
        pdf.get_object_mut(parent)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("Group", dictionary! { "S" => "Transparency" });
        let mut unsupported = e.candidate(&original, pdf).unwrap();
        let bytes = unsupported.bytes.clone();
        let item = e.page_items(&unsupported, 0).unwrap().remove(0);
        assert!(e.ungroup_page_item(&mut unsupported, &item).is_err());
        assert_eq!(unsupported.bytes, bytes);
        assert!(!unsupported.can_undo());
        fs::remove_file(path).unwrap();
        fs::remove_file(saved).unwrap();
    }
}
/// Repository root; the core manifest lives one level below it.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core crate is inside the repository")
        .to_path_buf()
}
fn engine() -> Engine {
    Engine::new(&root().join("target/debug/pdfium.dll"))
        .expect("Run scripts/setup-pdfium.ps1 first")
}
fn temp(name: &str) -> PathBuf {
    let dir = root().join("target/test-output");
    fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{}-{}", std::process::id(), name))
}
fn fixture(name: &str) -> PathBuf {
    root().join("tests/fixtures/generated").join(name)
}
fn generated(
    text: &str,
    subset: bool,
    rotation: i64,
    spacing: f32,
    kern: bool,
    missing: bool,
) -> Vec<u8> {
    let mut pdf = lopdf::Document::with_version("1.7");
    let font = fonts::Font {
        bytes: fs::read(root().join(if subset {
            "tests/fonts/PdfEditorFixture.ttf"
        } else {
            "tests/fonts/Inter-Regular.ttf"
        }))
        .unwrap(),
        name: if subset {
            "ABCDEF+PdfEditorFixture"
        } else if missing {
            "MissingFixtureSans"
        } else {
            "Inter-Regular"
        }
        .into(),
        exact: true,
    };
    let alphabet = format!("{text}unchanged AB äöå€");
    let f = fonts::embed(&mut pdf, &font, &alphabet).unwrap();
    let pages = pdf.new_object_id();
    let show = if kern {
        Operation::new(
            "TJ",
            vec![Object::Array(vec![
                Object::String(f.encode("A").unwrap(), lopdf::StringFormat::Hexadecimal),
                (-140).into(),
                Object::String(f.encode("B").unwrap(), lopdf::StringFormat::Hexadecimal),
            ])],
        )
    } else {
        Operation::new(
            "Tj",
            vec![Object::String(
                f.encode(text).unwrap(),
                lopdf::StringFormat::Hexadecimal,
            )],
        )
    };
    let ops = Content {
        operations: vec![
            Operation::new("q", vec![]),
            Operation::new("rg", vec![0.1.into(), 0.25.into(), 0.5.into()]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
            Operation::new("Tc", vec![spacing.into()]),
            Operation::new(
                "Tm",
                vec![
                    1.into(),
                    0.into(),
                    0.into(),
                    1.into(),
                    50.into(),
                    220.into(),
                ],
            ),
            show,
            Operation::new(
                "Tj",
                vec![Object::String(
                    f.encode("unchanged").unwrap(),
                    lopdf::StringFormat::Hexadecimal,
                )],
            ),
            Operation::new("ET", vec![]),
            Operation::new("Q", vec![]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
            Operation::new(
                "Tm",
                vec![1.into(), 0.into(), 0.into(), 1.into(), 50.into(), 80.into()],
            ),
            Operation::new(
                "Tj",
                vec![Object::String(
                    f.encode("unchanged").unwrap(),
                    lopdf::StringFormat::Hexadecimal,
                )],
            ),
            Operation::new("ET", vec![]),
        ],
    };
    let content = pdf.add_object(Stream::new(lopdf::Dictionary::new(), ops.encode().unwrap()));
    let page=pdf.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),500.into(),300.into()],
        "Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>f.id}},"Contents"=>content,"Rotate"=>rotation});
    pdf.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}.into(),
    );
    let catalog = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    pdf.trailer.set("Root", catalog);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    bytes
}
/// Browser-print style page: a Type3 font without BaseFont, glyph names in
/// /Differences plus a ToUnicode CMap, and one show operator per glyph placed
/// with Td, followed by an ordinary run on a second line.
fn type3_glyph_runs() -> Vec<u8> {
    let mut pdf = lopdf::Document::with_version("1.7");
    let alphabet: Vec<char> = "Helo wrdunchag".chars().collect();
    let code = |c: char| alphabet.iter().position(|a| *a == c).unwrap() as u8 + 1;
    let mut procs = lopdf::Dictionary::new();
    let mut differences = vec![Object::Integer(1)];
    let mut widths = vec![];
    let mut cmap = String::new();
    for (i, c) in alphabet.iter().enumerate() {
        let name = format!("g{}", i + 1);
        let width = if *c == ' ' { 250 } else { 500 };
        // Distinct shapes per glyph keep render diffs meaningful.
        let body = if *c == ' ' {
            format!("{width} 0 d0")
        } else {
            format!(
                "{width} 0 0 0 450 700 d1 40 0 {} {} re f",
                360 - i * 10,
                300 + i * 25
            )
        };
        let proc_id = pdf.add_object(Stream::new(lopdf::Dictionary::new(), body.into_bytes()));
        procs.set(name.as_bytes(), proc_id);
        differences.push(Object::Name(name.into_bytes()));
        widths.push(Object::Integer(width));
        cmap.push_str(&format!("<{:02X}> <{:04X}>\n", i + 1, *c as u32));
    }
    let to_unicode = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapName /PdfEditorGlyphs def /CMapType 2 def\n1 begincodespacerange <00> <FF> endcodespacerange\n{} beginbfchar\n{cmap}endbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end",
        alphabet.len()
    );
    let to_unicode = pdf.add_object(Stream::new(
        lopdf::Dictionary::new(),
        to_unicode.into_bytes(),
    ));
    let font = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type3",
        "FontBBox" => vec![0.into(), 0.into(), 500.into(), 700.into()],
        "FontMatrix" => vec![0.001.into(), 0.into(), 0.into(), 0.001.into(), 0.into(), 0.into()],
        "CharProcs" => procs,
        "Encoding" => dictionary! {"Type" => "Encoding", "Differences" => differences},
        "FirstChar" => 1,
        "LastChar" => alphabet.len() as i64,
        "Widths" => widths,
        "Resources" => lopdf::Dictionary::new(),
        "ToUnicode" => to_unicode,
    });
    let show = |text: &str| {
        Operation::new(
            "Tj",
            vec![Object::String(
                text.chars().map(code).collect(),
                lopdf::StringFormat::Hexadecimal,
            )],
        )
    };
    let mut operations = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"T3".to_vec()), 24.into()]),
        Operation::new(
            "Tm",
            vec![
                0.75.into(),
                0.into(),
                0.into(),
                0.75.into(),
                50.into(),
                200.into(),
            ],
        ),
    ];
    let line: Vec<char> = "Hello world".chars().collect();
    for (i, c) in line.iter().enumerate() {
        if i > 0 {
            let advance = if line[i - 1] == ' ' { 6.0 } else { 12.5 };
            operations.push(Operation::new("Td", vec![advance.into(), 0.into()]));
        }
        operations.push(show(&c.to_string()));
    }
    operations.extend([
        Operation::new("ET", vec![]),
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"T3".to_vec()), 18.into()]),
        Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                50.into(),
                100.into(),
            ],
        ),
        show("unchanged"),
        Operation::new("ET", vec![]),
    ]);
    let content = pdf.add_object(Stream::new(
        lopdf::Dictionary::new(),
        Content { operations }.encode().unwrap(),
    ));
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 500.into(), 300.into()],
        "Resources" => dictionary! {"Font" => dictionary! {"T3" => font}},
        "Contents" => content,
    });
    pdf.objects.insert(
        pages,
        dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}.into(),
    );
    let catalog = pdf.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    pdf.trailer.set("Root", catalog);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    bytes
}
#[test]
#[ignore = "Explicit fixture regeneration"]
fn generate_fixtures() {
    fs::create_dir_all(root().join("tests/fixtures/generated")).unwrap();
    for (name, text, subset, rotate, spacing, kern, missing) in [
        ("unicode-cid.pdf", "äöå€ AB", false, 0, 0.0, false, false),
        ("subset.pdf", "Subset test", true, 0, 0.0, false, false),
        ("rotated.pdf", "äöå€ AB", false, 90, 0.0, false, false),
        ("spacing.pdf", "äöå€ AB", false, 0, 2.5, false, false),
        ("kerning.pdf", "AB", false, 0, 0.0, true, false),
        (
            "missing-glyph.pdf",
            "Subset test",
            true,
            0,
            1.0,
            false,
            true,
        ),
        ("missing-font.pdf", "AB", false, 0, 0.0, false, true),
    ] {
        fs::write(
            fixture(name),
            generated(text, subset, rotate, spacing, kern, missing),
        )
        .unwrap();
    }
    fs::write(fixture("glyph-runs.pdf"), type3_glyph_runs()).unwrap();
    fs::write(
        fixture("malformed.pdf"),
        b"%PDF-1.7\nthis is not a PDF\n%%EOF",
    )
    .unwrap();
    let mut pdf = lopdf::Document::load_mem(&generated("AB", false, 0, 0.0, false, false)).unwrap();
    pdf.trailer.set(
        "ID",
        vec![
            Object::string_literal("fixture-id-00001"),
            Object::string_literal("fixture-id-00001"),
        ],
    );
    let encryption = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V1 {
        document: &pdf,
        owner_password: "fixture-owner",
        user_password: "fixture-reader",
        permissions: lopdf::Permissions::all(),
    })
    .unwrap();
    pdf.encrypt(&encryption).unwrap();
    pdf.save(fixture("encrypted.pdf")).unwrap();
    let mut transformed =
        lopdf::Document::load_mem(&generated("AB", false, 0, 1.5, false, false)).unwrap();
    let page = transformed.get_pages()[&1];
    let mut ops = Content::decode(&transformed.get_page_content(page)).unwrap();
    let matrix = ops
        .operations
        .iter_mut()
        .find(|op| op.operator == "Tm")
        .unwrap();
    matrix.operands = vec![
        0.8660254.into(),
        0.5.into(),
        (-0.5).into(),
        0.8660254.into(),
        50.into(),
        180.into(),
    ];
    let pos = ops
        .operations
        .iter()
        .position(|op| op.operator == "Tm")
        .unwrap();
    ops.operations.splice(
        pos..pos,
        vec![
            Operation::new("Tz", vec![85.into()]),
            Operation::new("Ts", vec![4.into()]),
            Operation::new("Tw", vec![3.into()]),
        ],
    );
    let stream =
        transformed.add_object(Stream::new(lopdf::Dictionary::new(), ops.encode().unwrap()));
    transformed
        .get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    transformed.save(fixture("transformed.pdf")).unwrap();
    let mut multi =
        lopdf::Document::load_mem(&generated("Subset test", true, 0, 1.0, false, true)).unwrap();
    let first = multi.get_pages()[&1];
    let page = multi.get_object(first).unwrap().clone();
    let parent = page
        .as_dict()
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    let second = multi.add_object(page);
    let first_font = multi.get_page_fonts(first).unwrap()[b"F1".as_slice()].clone();
    let mut second_font = first_font;
    second_font.set(
        "BaseFont",
        Object::Name(b"GHIJKL+PdfEditorFixture".to_vec()),
    );
    let font_id = multi.add_object(second_font);
    multi
        .get_object_mut(second)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Resources",
            dictionary! {"Font"=>dictionary!{"F1"=>font_id}},
        );
    let pages = multi.get_object_mut(parent).unwrap().as_dict_mut().unwrap();
    pages.set(
        "Kids",
        vec![Object::Reference(first), Object::Reference(second)],
    );
    pages.set("Count", 2);
    multi.save(fixture("multi-page.pdf")).unwrap();
}
fn assert_outside_equal(
    engine: &Engine,
    before: &Document,
    after: &Document,
    old: &TextObject,
    new: &TextObject,
) {
    let a = engine.render(before, old.page, 1.5).unwrap();
    let b = engine.render(after, old.page, 1.5).unwrap();
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut xs = vec![];
    let mut ys = vec![];
    for bounds in [old.bounds, new.bounds] {
        for x in [bounds.left, bounds.right] {
            for y in [bounds.bottom, bounds.top] {
                let (x, y) = engine.page_to_pixel(before, old.page, 1.5, x, y).unwrap();
                xs.push(x);
                ys.push(y);
            }
        }
    }
    let left = xs.iter().min().unwrap() - 4;
    let right = xs.iter().max().unwrap() + 4;
    let top = ys.iter().min().unwrap() - 4;
    let bottom = ys.iter().max().unwrap() + 4;
    let mut changed = 0;
    for y in 0..a.height as i32 {
        for x in 0..a.width as i32 {
            let i = (y as usize * a.width as usize + x as usize) * 4;
            if a.rgba[i..i + 4] != b.rgba[i..i + 4] {
                changed += 1;
                assert!(
                    x >= left && x <= right && y >= top && y <= bottom,
                    "pixel changed outside edit at {x},{y}"
                );
            }
        }
    }
    assert!(changed > 0, "edit should change rendered pixels");
}
fn all_pdfs(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            all_pdfs(&path, files)
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            files.push(path)
        }
    }
}
#[test]
fn fixture_roundtrip_and_render_diff() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let engine = engine();
    let mut files = vec![];
    all_pdfs(&root().join("tests/fixtures"), &mut files);
    assert!(files.len() >= 9);
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy();
        if name == "malformed.pdf" || name == "encrypted.pdf" {
            assert!(engine.open(&path).is_err(), "{name}");
            continue;
        }
        let original = engine.open(&path).unwrap();
        let source_bytes = fs::read(&path).unwrap();
        for page in 0..engine.pages(&original).unwrap().len() as u16 {
            let mut doc = engine.open(&path).unwrap();
            let objects = engine.text_objects(&doc, page).unwrap();
            assert!(!objects.is_empty(), "{name}");
            let selected = &objects[0];
            let replacement = if name == "subset.pdf" {
                "test Subset"
            } else if name == "missing-glyph.pdf" {
                "XYZ"
            } else if name == "glyph-runs.pdf" {
                // Glyphs present in the Type3 font are reused.
                "eH"
            } else {
                "BA"
            };
            let result = engine
                .edit(&mut doc, selected, replacement)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let after = engine.text_objects(&doc, page).unwrap();
            assert_eq!(after[0].text, replacement, "{name}");
            assert!(
                after[1]
                    .matrix
                    .iter()
                    .zip(objects[1].matrix)
                    .all(|(a, b)| (a - b).abs() < 0.0001),
                "neighbor moved: {name}"
            );
            assert_eq!(after[0].matrix, selected.matrix, "matrix changed: {name}");
            assert_eq!(after[0].size, selected.size, "font size changed: {name}");
            assert_eq!(after[0].color, selected.color, "color changed: {name}");
            if name != "missing-glyph.pdf" {
                assert_eq!(after[0].font, selected.font, "{name}");
                assert!(result.warning.is_none());
            }
            assert_outside_equal(&engine, &original, &doc, selected, &after[0]);
            let output = temp(&format!("{}-{page}-edited.pdf", name));
            if output.exists() {
                fs::remove_file(&output).unwrap();
            }
            doc.save_as(&output).unwrap();
            let reopened = engine.open(&output).unwrap();
            assert_eq!(
                engine.text_objects(&reopened, page).unwrap()[0].text,
                replacement
            );
            assert!(doc.undo());
            assert_eq!(
                engine.text_objects(&doc, page).unwrap()[0].text,
                selected.text
            );
            assert!(doc.redo());
            assert_eq!(
                engine.text_objects(&doc, page).unwrap()[0].text,
                replacement
            );
            assert!(engine.edit(&mut doc, selected, "AB").is_err());
            assert!(doc.save_as(&path).is_err());
            assert!(doc.save_as(&output).is_err());
            assert_eq!(fs::read(&path).unwrap(), source_bytes);
            eprintln!(
                "PASS {name} page {}: edit, render diff, save/reopen, history, original protection",
                page + 1
            );
        }
    }
}
#[test]
fn limits_settings_and_hit_testing() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    let mut d = e.open(&fixture("unicode-cid.pdf")).unwrap();
    let o = e.text_objects(&d, 0).unwrap().remove(0);
    assert!(
        e.hit_test(
            &d,
            0,
            (o.bounds.left + o.bounds.right) / 2.0,
            (o.bounds.top + o.bounds.bottom) / 2.0
        )
        .unwrap()
        .is_some()
    );
    assert!(e.render(&d, 0, f32::NAN).is_err());
    assert!(e.render(&d, 0, 10000.0).is_err());
    assert!(e.edit(&mut d, &o, "bad\ntext").is_err());
    assert!(!d.can_undo());
    let p = temp("settings.txt");
    if p.exists() {
        fs::remove_file(&p).unwrap();
    }
    assert_eq!(Substitution::load(&p).unwrap(), Substitution::Line);
    Substitution::Document.save(&p).unwrap();
    assert_eq!(Substitution::load(&p).unwrap(), Substitution::Document);
    Substitution::Line.save(&p).unwrap();
    assert_eq!(Substitution::load(&p).unwrap(), Substitution::Line);
    fs::write(&p, b"invalid").unwrap();
    assert!(Substitution::load(&p).is_err());
}

#[test]
fn substitution_scope_empty_edit_and_branching_history() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    for policy in [Substitution::Line, Substitution::Document] {
        let mut d = e.open(&fixture("missing-glyph.pdf")).unwrap();
        d.substitution = policy;
        let before = e.text_objects(&d, 0).unwrap();
        let result = e.edit(&mut d, &before[0], "XYZ").unwrap();
        assert!(
            result
                .warning
                .as_ref()
                .is_some_and(|w| w.starts_with("Font ") && w.contains(" not found, used "))
        );
        let after = e.text_objects(&d, 0).unwrap();
        if policy == Substitution::Document {
            assert_eq!(result.objects_changed, 3);
            assert!(after.iter().all(|o| o.font == after[0].font));
        } else {
            assert_eq!(result.objects_changed, 1);
            assert_eq!(after[1].font, before[1].font);
        }
        assert!(d.undo());
        assert_eq!(e.text_objects(&d, 0).unwrap()[0].font, before[0].font);
        let current = e.text_objects(&d, 0).unwrap().remove(0);
        e.edit(&mut d, &current, "test").unwrap();
        assert!(!d.can_redo());
    }
    let mut d = e.open(&fixture("unicode-cid.pdf")).unwrap();
    let before = e.text_objects(&d, 0).unwrap();
    e.edit(&mut d, &before[0], "").unwrap();
    let after = e.text_objects(&d, 0).unwrap();
    assert_eq!(after.len(), before.len() - 1);
    assert_eq!(after[0].text, before[1].text);
    assert_eq!(after[0].matrix, before[1].matrix);
}

#[test]
fn malformed_inputs_and_original_path_are_safe() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    let original = fs::read(fixture("unicode-cid.pdf")).unwrap();
    for (i, bytes) in [
        vec![],
        b"not a pdf".to_vec(),
        original[..100].to_vec(),
        vec![0; 4096],
    ]
    .into_iter()
    .enumerate()
    {
        let path = temp(&format!("malformed-{i}.pdf"));
        fs::write(&path, bytes).unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.open(&path)))
                .unwrap()
                .is_err()
        );
    }
    let path = temp("original.pdf");
    fs::write(&path, &original).unwrap();
    let d = e.open(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(d.save_as(&path).is_err());
    assert!(!path.exists());
    assert!(Engine::new(Path::new("pdfium.dll")).is_err());
}

#[test]
fn same_system_font_replacement_has_no_warning() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    let system = fonts::find("Arial", "ABXYZunchanged äöå€").unwrap();
    assert!(system.exact);
    let mut pdf = lopdf::Document::load_mem(&generated("AB", false, 0, 0.0, false, false)).unwrap();
    let f = fonts::embed(&mut pdf, &system, "ABunchanged").unwrap();
    let cid = pdf
        .get_dictionary(f.id)
        .unwrap()
        .get(b"DescendantFonts")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .as_reference()
        .unwrap();
    let descriptor = pdf
        .get_dictionary(cid)
        .unwrap()
        .get(b"FontDescriptor")
        .unwrap()
        .as_reference()
        .unwrap();
    pdf.get_object_mut(descriptor)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .remove(b"FontFile2");
    let page = pdf.get_pages()[&1];
    // A nonembedded Arial resource must resolve to the installed matching family.
    let mut ops = Content::decode(&pdf.get_page_content(page)).unwrap();
    let mut seen = 0;
    for op in &mut ops.operations {
        if op.operator == "Tj" {
            op.operands = vec![Object::String(
                f.encode(if seen == 0 { "AB" } else { "unchanged" })
                    .unwrap(),
                lopdf::StringFormat::Hexadecimal,
            )];
            seen += 1;
        }
    }
    let stream = pdf.add_object(Stream::new(lopdf::Dictionary::new(), ops.encode().unwrap()));
    let p = pdf.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    p.set("Contents", stream);
    p.set("Resources", dictionary! {"Font"=>dictionary!{"F1"=>f.id}});
    let path = temp("system-font.pdf");
    pdf.save(&path).unwrap();
    let mut d = e.open(&path).unwrap();
    let selected = e.text_objects(&d, 0).unwrap().remove(0);
    let result = e.edit(&mut d, &selected, "XYZ").unwrap();
    assert!(result.warning.is_none());
    assert_eq!(e.text_objects(&d, 0).unwrap()[0].text, "XYZ");
}

#[test]
fn embedded_new_mapping_and_document_scope_across_pages() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    let mut d = e.open(&fixture("unicode-cid.pdf")).unwrap();
    let old = e.text_objects(&d, 0).unwrap().remove(0);
    let result = e.edit(&mut d, &old, "XYZ").unwrap();
    let new = e.text_objects(&d, 0).unwrap().remove(0);
    assert_eq!(old.font, new.font);
    assert!(result.warning.is_none());
    assert_eq!(new.text, "XYZ");
    let mut d = e.open(&fixture("multi-page.pdf")).unwrap();
    d.substitution = Substitution::Document;
    let second_font = e.text_objects(&d, 1).unwrap()[0].font.clone();
    let old = e.text_objects(&d, 0).unwrap().remove(0);
    let result = e.edit(&mut d, &old, "XYZ").unwrap();
    assert_eq!(result.objects_changed, 6);
    assert!(result.warning.is_some());
    let font = e.text_objects(&d, 0).unwrap()[0].font.clone();
    assert!(
        e.text_objects(&d, 1)
            .unwrap()
            .iter()
            .all(|o| o.font == font)
    );
    assert!(d.undo());
    assert_eq!(e.text_objects(&d, 1).unwrap()[0].font, second_font);
}

#[test]
fn encryption_and_compression_limits_are_explicit() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    assert!(
        e.open(&fixture("encrypted.pdf"))
            .err()
            .unwrap()
            .0
            .contains("password")
    );
    let mut pdf = lopdf::Document::load_mem(&generated("AB", false, 0, 0.0, false, false)).unwrap();
    pdf.trailer.set(
        "ID",
        vec![
            Object::string_literal("fixture-id-00002"),
            Object::string_literal("fixture-id-00002"),
        ],
    );
    let encryption = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V1 {
        document: &pdf,
        owner_password: "fixture-owner",
        user_password: "",
        permissions: lopdf::Permissions::all(),
    })
    .unwrap();
    pdf.encrypt(&encryption).unwrap();
    let path = temp("empty-password.pdf");
    pdf.save(&path).unwrap();
    assert!(e.open(&path).unwrap().is_password_protected());
    let mut pdf = lopdf::Document::load_mem(&generated("AB", false, 0, 0.0, false, false)).unwrap();
    let page = pdf.get_pages()[&1];
    let mut stream = Stream::new(lopdf::Dictionary::new(), vec![b' '; MAX_STREAM + 1]);
    stream.compress().unwrap();
    let id = pdf.add_object(stream);
    pdf.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", id);
    assert!(pdf.get_page_content_with_limit(page, MAX_STREAM).is_err());
}

#[test]
fn browser_glyph_runs_edit_as_one_line() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let e = engine();
    let mut d = e.open(&fixture("glyph-runs.pdf")).unwrap();
    let before = e.text_objects(&d, 0).unwrap();
    // "Hello world" is eleven one-glyph objects; the space reports no text.
    assert_eq!(before.len(), 12);
    assert_eq!(before[5].text.trim(), "");
    let original = e.open(&fixture("glyph-runs.pdf")).unwrap();
    let hello = &before[0..5];
    let result = e.edit_group(&mut d, hello, "Hole").unwrap();
    assert!(result.warning.is_none(), "Type3 glyphs are reused");
    let after = e.text_objects(&d, 0).unwrap();
    assert_eq!(after.len(), before.len() - 4);
    assert_eq!(after[0].text, "Hole");
    assert_eq!(after[0].font, before[0].font);
    assert_eq!(after[0].matrix, before[0].matrix);
    for (new, old) in after[1..].iter().zip(&before[5..]) {
        assert_eq!(new.matrix, old.matrix, "neighbor moved");
        assert_eq!(new.text, old.text);
    }
    let mut union = before[0].clone();
    for o in hello {
        union.bounds.left = union.bounds.left.min(o.bounds.left);
        union.bounds.right = union.bounds.right.max(o.bounds.right);
        union.bounds.bottom = union.bounds.bottom.min(o.bounds.bottom);
        union.bounds.top = union.bounds.top.max(o.bounds.top);
    }
    assert_outside_equal(&e, &original, &d, &union, &after[0]);
    assert!(d.undo());
    assert_eq!(e.text_objects(&d, 0).unwrap().len(), before.len());
    // Letters missing from the Type3 font fall back to a system font.
    let before = e.text_objects(&d, 0).unwrap();
    let result = e.edit_group(&mut d, &before[6..11], "Zebra").unwrap();
    assert!(
        result
            .warning
            .is_some_and(|w| w.contains(" not found, used "))
    );
    let after = e.text_objects(&d, 0).unwrap();
    assert_eq!(after[6].text, "Zebra");
    assert_eq!(after.last().unwrap().text, "unchanged");
    assert!(e.edit_group(&mut d, &[], "x").is_err());
    let stale = before[0].clone();
    assert!(e.edit_group(&mut d, &[stale], "x").is_err());
}

#[test]
fn search_matches_and_empty_query() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let doc = e.open(&fixture("unicode-cid.pdf")).unwrap();
    assert!(e.find_text(&doc, "").unwrap().is_empty());
    assert!(e.find_text(&doc, "not-present-xyz").unwrap().is_empty());
    let matches = e.find_text(&doc, "UNCHANGED").unwrap();
    assert!(!matches.is_empty());
    assert!(matches.iter().all(|m| !m.bounds.is_empty()));
    assert!(e.find_text(&doc, "\0").is_err());
}

#[test]
fn new_text_roundtrip_and_history() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("rotated.pdf")).unwrap();
    let before = e.text_objects(&doc, 0).unwrap();
    let text = NewText {
        page: 0,
        x: 80.0,
        y: 150.0,
        text: "Added äöå".into(),
        font: "Arial".into(),
        size: 22.0,
        color: [30, 80, 160],
    };
    assert!(!e.system_fonts().unwrap().is_empty());
    e.add_text(&mut doc, &text).unwrap();
    let after = e.text_objects(&doc, 0).unwrap();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after.last().unwrap().text, text.text);
    assert_eq!(after.last().unwrap().color[..3], text.color);
    assert!((after.last().unwrap().size - text.size).abs() < 0.01);
    for (old, new) in before.iter().zip(&after) {
        assert_eq!(old.text, new.text);
        assert_eq!(old.matrix, new.matrix);
    }
    assert!(doc.undo());
    assert_eq!(e.text_objects(&doc, 0).unwrap().len(), before.len());
    assert!(doc.redo());
    assert_eq!(e.text_objects(&doc, 0).unwrap().len(), before.len() + 1);
    let mut invalid = text.clone();
    invalid.size = f32::NAN;
    assert!(e.add_text(&mut doc, &invalid).is_err());
    invalid = text;
    invalid.font = "NonexistentSelectedFontXYZ".into();
    assert!(e.add_text(&mut doc, &invalid).is_err());
}

#[test]
fn delete_object_preserves_neighbors_and_rejects_stale_selection() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("kerning.pdf")).unwrap();
    let before = e.text_objects(&doc, 0).unwrap();
    let old = doc.bytes.clone();
    e.delete_text(&mut doc, &before[0]).unwrap();
    let after = e.text_objects(&doc, 0).unwrap();
    assert_eq!(after.len() + 1, before.len());
    for (a, b) in before[1..].iter().zip(&after) {
        assert_eq!(a.text, b.text);
        assert_eq!(a.matrix, b.matrix);
    }
    assert!(e.delete_text(&mut doc, &before[0]).is_err());
    assert!(doc.undo());
    assert_eq!(doc.bytes, old);
    assert!(doc.redo());
}

#[test]
fn style_preserves_text_font_and_neighbors() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for name in [
        "kerning.pdf",
        "spacing.pdf",
        "rotated.pdf",
        "unicode-cid.pdf",
    ] {
        let mut doc = e.open(&fixture(name)).unwrap();
        let before = e.text_objects(&doc, 0).unwrap();
        let old = doc.bytes.clone();
        e.style_text(&mut doc, &before[0], 28.0, [50, 90, 140])
            .unwrap();
        let after = e.text_objects(&doc, 0).unwrap();
        assert_eq!(after.len(), before.len());
        for (i, (a, b)) in before.iter().zip(&after).enumerate() {
            assert_eq!(a.text, b.text);
            assert_eq!(a.font, b.font);
            assert_eq!(a.matrix, b.matrix);
            if i > 0 {
                assert_eq!(a.color, b.color);
                assert_eq!(a.size, b.size);
            }
        }
        assert_eq!(after[0].color[..3], [50, 90, 140]);
        assert!((after[0].size - 28.0).abs() < 0.01);
        assert!(e.style_text(&mut doc, &before[0], 10.0, [0, 0, 0]).is_err());
        assert!(doc.undo());
        assert_eq!(doc.bytes, old);
        assert!(doc.redo());
    }
}

#[test]
fn thumbnail_dimensions_and_limits() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for file in ["rotated.pdf", "unicode-cid.pdf"] {
        let doc = e.open(&fixture(file)).unwrap();
        let image = e.thumbnail(&doc, 0, 150).unwrap();
        assert!(image.width <= 151 && image.height <= 151);
        assert!(image.width.max(image.height) >= 149);
        assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
        assert!(e.thumbnail(&doc, 0, 0).is_err());
        assert!(e.thumbnail(&doc, u16::MAX, 150).is_err());
    }
}

#[test]
fn page_operations_roundtrip_and_undo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("unicode-cid.pdf")).unwrap();
    let original = doc.bytes.clone();
    let image = e.render(&doc, 0, 1.0).unwrap();
    assert!(
        e.edit_pages(&mut doc, &PageEdit::Delete { page: 0 })
            .is_err()
    );
    e.edit_pages(&mut doc, &PageEdit::Rotate { page: 0 })
        .unwrap();
    let rotated = e.render(&doc, 0, 1.0).unwrap();
    assert_eq!(rotated.width, image.height);
    assert_eq!(rotated.height, image.width);
    assert!(doc.undo());
    assert_eq!(doc.bytes, original);
    e.edit_pages(
        &mut doc,
        &PageEdit::Insert {
            path: fixture("kerning.pdf"),
            before: 1,
        },
    )
    .unwrap();
    assert_eq!(e.pages(&doc).unwrap().len(), 2);
    assert_eq!(e.render(&doc, 0, 1.0).unwrap().rgba, image.rgba);
    let inserted = e.render(&doc, 1, 1.0).unwrap();
    e.edit_pages(&mut doc, &PageEdit::Move { from: 1, to: 0 })
        .unwrap();
    assert_eq!(e.render(&doc, 0, 1.0).unwrap().rgba, inserted.rgba);
    e.edit_pages(&mut doc, &PageEdit::Delete { page: 0 })
        .unwrap();
    assert_eq!(e.pages(&doc).unwrap().len(), 1);
    assert_eq!(e.render(&doc, 0, 1.0).unwrap().rgba, image.rgba);
    assert!(doc.undo());
    assert_eq!(e.pages(&doc).unwrap().len(), 2);
    assert!(doc.redo());
    assert!(
        e.edit_pages(
            &mut doc,
            &PageEdit::Insert {
                path: fixture("malformed.pdf"),
                before: 0
            }
        )
        .is_err()
    );
    assert!(
        e.edit_pages(&mut doc, &PageEdit::Move { from: 0, to: 10 })
            .is_err()
    );
}

#[test]
fn recent_files_migrate_deduplicate_and_preserve_policy() {
    let path = temp("recent-settings.txt");
    fs::write(&path, b"document\n").unwrap();
    let mut settings = Settings::load(&path).unwrap();
    assert_eq!(settings.substitution, Substitution::Document);
    for i in 0..12 {
        settings.remember(&PathBuf::from(format!("C:/Docs/{i} ä.pdf")));
    }
    assert_eq!(settings.recent_files.len(), 10);
    let last = settings.recent_files[9].clone();
    settings.remember(&last);
    assert_eq!(settings.recent_files[0], last);
    settings.save(&path).unwrap();
    assert_eq!(
        Settings::load(&path).unwrap().recent_files,
        settings.recent_files
    );
    Substitution::Line.save(&path).unwrap();
    let loaded = Settings::load(&path).unwrap();
    assert_eq!(loaded.recent_files, settings.recent_files);
    assert_eq!(loaded.substitution, Substitution::Line);
    fs::write(&path, b"line\nrecent=xyz\n").unwrap();
    assert!(Settings::load(&path).is_err());
    let _ = fs::remove_file(path);
}

#[test]
fn basic_feature_workflow_saves_and_reopens() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = fixture("unicode-cid.pdf");
    let original = fs::read(&path).unwrap();
    let mut doc = e.open(&path).unwrap();
    e.add_text(
        &mut doc,
        &NewText {
            page: 0,
            x: 80.0,
            y: 150.0,
            text: "Inserted".into(),
            font: "Arial".into(),
            size: 18.0,
            color: [10, 20, 30],
        },
    )
    .unwrap();
    let selected = e.text_objects(&doc, 0).unwrap().pop().unwrap();
    e.style_text(&mut doc, &selected, 26.0, [30, 60, 90])
        .unwrap();
    let selected = e.text_objects(&doc, 0).unwrap().pop().unwrap();
    e.edit(&mut doc, &selected, "DistinctivePhrase").unwrap();
    assert_eq!(e.find_text(&doc, "DistinctivePhrase").unwrap().len(), 1);
    e.edit_pages(
        &mut doc,
        &PageEdit::Insert {
            path: fixture("kerning.pdf"),
            before: 0,
        },
    )
    .unwrap();
    e.edit_pages(&mut doc, &PageEdit::Move { from: 0, to: 1 })
        .unwrap();
    e.edit_pages(&mut doc, &PageEdit::Rotate { page: 1 })
        .unwrap();
    let output = temp("basic-workflow.pdf");
    let _ = fs::remove_file(&output);
    doc.save_as(&output).unwrap();
    let reopened = e.open(&output).unwrap();
    assert_eq!(e.pages(&reopened).unwrap().len(), 2);
    assert_eq!(
        e.find_text(&reopened, "DistinctivePhrase").unwrap().len(),
        1
    );
    assert_eq!(
        e.render(&doc, 0, 1.0).unwrap().rgba,
        e.render(&reopened, 0, 1.0).unwrap().rgba
    );
    assert_eq!(fs::read(path).unwrap(), original);
    let _ = fs::remove_file(output);
}

#[test]
fn print_spool_matches_page_count_and_cleans_up() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let engine = engine();
    let doc = engine.open(&fixture("multi-page.pdf")).unwrap();
    let job = engine.prepare_print(&doc).unwrap();
    let path = job.directory().to_path_buf();
    assert_eq!(job.page_count(), engine.pages(&doc).unwrap().len());
    assert_eq!(fs::read_dir(&path).unwrap().count(), job.page_count());
    assert_eq!(&fs::read(path.join("00000.bmp")).unwrap()[..2], b"BM");
    drop(job);
    assert!(!path.exists());
}

#[test]
fn signature_round_trip_preserves_text_and_undo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = e.render(&doc, 0, 1.0).unwrap().rgba;
    let text: Vec<_> = e
        .text_objects(&doc, 0)
        .unwrap()
        .into_iter()
        .map(|o| o.text)
        .collect();
    let signature = signature_image(&[vec![[0.1, 0.2], [0.5, 0.8], [0.9, 0.3]]]).unwrap();
    let folder = temp("signature-library");
    fs::create_dir_all(&folder).unwrap();
    let stored = store_signature(&folder, &signature).unwrap();
    let loaded = Picture::open(&stored).unwrap();
    assert_eq!(loaded.rgba, signature.rgba);
    assert!(signature_files(&folder).unwrap().contains(&stored));
    e.place_picture(
        &mut doc,
        &loaded,
        Placement {
            page: 0,
            left: 30.0,
            bottom: 40.0,
            width: 180.0,
            height: 60.0,
        },
    )
    .unwrap();
    let after = e.render(&doc, 0, 1.0).unwrap().rgba;
    assert_ne!(before, after);
    assert_eq!(
        text,
        e.text_objects(&doc, 0)
            .unwrap()
            .into_iter()
            .map(|o| o.text)
            .collect::<Vec<_>>()
    );
    let output = temp("signature-roundtrip.pdf");
    let _ = fs::remove_file(&output);
    doc.save_as(&output).unwrap();
    assert_eq!(
        after,
        e.render(&e.open(&output).unwrap(), 0, 1.0).unwrap().rgba
    );
    doc.undo();
    assert_eq!(before, e.render(&doc, 0, 1.0).unwrap().rgba);
    fs::remove_file(output).unwrap();
    fs::remove_file(stored).unwrap();
}

fn form_fixture() -> Vec<u8> {
    let mut pdf = parse(&generated("Form", false, 0, 0.0, false, false)).unwrap();
    let page = *pdf.get_pages().values().next().unwrap();
    let font =
        pdf.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let off = pdf.add_object(Stream::new(dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),20.into(),20.into()]}, b"0 0 20 20 re S".to_vec()));
    let on = pdf.add_object(Stream::new(dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),20.into(),20.into()]}, b"0 0 20 20 re S 2 2 m 18 18 l 2 18 m 18 2 l S".to_vec()));
    let mut fields = vec![];
    for (i, (name, kind, flags)) in [
        ("Name", "Tx", 0),
        ("Agree", "Btn", 0),
        ("Option", "Btn", 32768),
        ("Country", "Ch", 131072),
    ]
    .iter()
    .enumerate()
    {
        let y = 230 - i as i32 * 50;
        let mut field = dictionary! {"Type"=>"Annot","Subtype"=>"Widget","FT"=>*kind,"T"=>Object::string_literal(*name),"Ff"=>*flags,"F"=>4,"P"=>page,
        "Rect"=>vec![50.into(),y.into(),200.into(),(y+25).into()],"DA"=>Object::string_literal("/Helv 12 Tf 0 g")};
        if *kind == "Btn" {
            field.set("AP", dictionary! {"N"=>dictionary!{"Off"=>off,"Yes"=>on}});
            field.set("AS", "Off");
            field.set("V", "Off");
        }
        if *kind == "Ch" {
            field.set(
                "Opt",
                vec![
                    Object::string_literal("Finland"),
                    Object::string_literal("UK"),
                ],
            );
        }
        fields.push(Object::Reference(pdf.add_object(field)));
    }
    pdf.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", fields.clone());
    let acro=pdf.add_object(dictionary! {"Fields"=>fields,"DA"=>Object::string_literal("/Helv 12 Tf 0 g"),"DR"=>dictionary!{"Font"=>dictionary!{"Helv"=>font}}});
    pdf.catalog_mut().unwrap().set("AcroForm", acro);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    bytes
}
#[test]
fn forms_edit_all_supported_types_and_undo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("form.pdf");
    fs::write(&path, form_fixture()).unwrap();
    let mut doc = e.open(&path).unwrap();
    for (index, value) in [
        (0, FieldValue::Text("Example".into())),
        (1, FieldValue::Check(true)),
        (2, FieldValue::Radio(true)),
        (
            3,
            FieldValue::Choice {
                options: vec![],
                selected: Some(1),
            },
        ),
    ] {
        let fields = e.form_fields(&doc).unwrap();
        assert_eq!(fields.len(), 4);
        let before = e.render(&doc, 0, 1.0).unwrap().rgba;
        e.fill_field(&mut doc, &fields[index], &value).unwrap();
        assert!(
            before != e.render(&doc, 0, 1.0).unwrap().rgba,
            "field {index} appearance"
        );
        assert!(
            e.fill_field(&mut doc, &fields[index], &value).is_err(),
            "stale selection"
        );
        doc.undo();
        assert_eq!(before, e.render(&doc, 0, 1.0).unwrap().rgba);
        let fields = e.form_fields(&doc).unwrap();
        e.fill_field(&mut doc, &fields[index], &value).unwrap();
    }
    let output = temp("form-filled.pdf");
    let _ = fs::remove_file(&output);
    doc.save_as(&output).unwrap();
    assert_eq!(
        e.render(&doc, 0, 1.0).unwrap().rgba,
        e.render(&e.open(&output).unwrap(), 0, 1.0).unwrap().rgba
    );
    fs::remove_file(path).unwrap();
    fs::remove_file(output).unwrap();
}

#[test]
#[ignore = "Explicit form fixture regeneration"]
fn generate_form_fixture() {
    fs::write(fixture("forms.pdf"), form_fixture()).unwrap();
}

#[test]
fn replace_one_all_preserves_fonts_and_is_atomic() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("replace.pdf");
    fs::write(&path, generated("AB AB", false, 0, 0.0, false, false)).unwrap();
    let mut doc = e.open(&path).unwrap();
    let before = doc.bytes.clone();
    let font = e.text_objects(&doc, 0).unwrap()[0].font.clone();
    assert_eq!(e.replacement_count(&doc, "ab").unwrap(), 2);
    assert_eq!(e.replace_text(&mut doc, "ab", "ä", Some(1)).unwrap(), 1);
    assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, "AB ä");
    assert_eq!(e.text_objects(&doc, 0).unwrap()[0].font, font);
    doc.undo();
    assert_eq!(doc.bytes, before);
    assert_eq!(e.replace_text(&mut doc, "AB", "ö", None).unwrap(), 2);
    assert_eq!(e.text_objects(&doc, 0).unwrap()[0].text, "ö ö");
    let stable = doc.bytes.clone();
    assert!(e.replace_text(&mut doc, "ö", "bad\ntext", None).is_err());
    assert_eq!(doc.bytes, stable);
    doc.undo();
    assert_eq!(doc.bytes, before);
    e.edit_pages(
        &mut doc,
        &PageEdit::Insert {
            path: path.clone(),
            before: 1,
        },
    )
    .unwrap();
    assert_eq!(e.replacement_count(&doc, "AB").unwrap(), 4);
    assert_eq!(e.replace_text(&mut doc, "AB", "å", None).unwrap(), 4);
    assert_eq!(e.replacement_count(&doc, "å").unwrap(), 4);
    doc.undo();
    assert_eq!(e.replacement_count(&doc, "AB").unwrap(), 4);
    fs::remove_file(path).unwrap();
}

#[test]
fn password_open_and_aes256_save_roundtrip() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    assert!(
        e.open_with_password(&fixture("encrypted.pdf"), "wrong")
            .is_err()
    );
    let mut doc = e
        .open_with_password(&fixture("encrypted.pdf"), "fixture-reader")
        .unwrap();
    assert!(doc.is_password_protected());
    let object = e.text_objects(&doc, 0).unwrap().remove(0);
    e.edit(&mut doc, &object, "BA").unwrap();
    assert!(doc.is_password_protected());
    let path = temp("aes256.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as_with_password(&path, "test-password-ä").unwrap();
    assert!(e.open(&path).is_err());
    assert!(e.open_with_password(&path, "wrong").is_err());
    let reopened = e.open_with_password(&path, "test-password-ä").unwrap();
    assert_eq!(e.text_objects(&reopened, 0).unwrap()[0].text, "BA");
    assert!(e.render(&doc, 0, 1.0).unwrap().rgba == e.render(&reopened, 0, 1.0).unwrap().rgba);
    assert!(doc.save_as_with_password(&path, "other").is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn merge_extract_split_keep_order_and_source() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = doc.bytes.clone();
    let count = e.pages(&doc).unwrap().len();
    assert!(
        e.merge_pdfs(
            &mut doc,
            &[fixture("multi-page.pdf"), fixture("does-not-exist.pdf")]
        )
        .is_err()
    );
    assert_eq!(doc.bytes, before);
    e.merge_pdfs(
        &mut doc,
        &[fixture("multi-page.pdf"), fixture("multi-page.pdf")],
    )
    .unwrap();
    assert_eq!(e.pages(&doc).unwrap().len(), count * 3);
    doc.undo();
    assert_eq!(doc.bytes, before);
    let path = temp("extracted.pdf");
    let _ = fs::remove_file(&path);
    e.extract_pages(&doc, &[1, 0], &path).unwrap();
    let extracted = e.open(&path).unwrap();
    assert!(e.render(&extracted, 0, 1.0).unwrap().rgba == e.render(&doc, 1, 1.0).unwrap().rgba);
    assert!(e.render(&extracted, 1, 1.0).unwrap().rgba == e.render(&doc, 0, 1.0).unwrap().rgba);
    assert!(e.extract_pages(&doc, &[0], &path).is_err());
    let folder = temp("split-outputs");
    fs::create_dir_all(&folder).unwrap();
    for name in ["split-1.pdf", "split-2.pdf"] {
        let _ = fs::remove_file(folder.join(name));
    }
    let outputs = e.split_pdf(&doc, "1;2", &folder).unwrap();
    assert_eq!(outputs.len(), 2);
    for (i, path) in outputs.iter().enumerate() {
        assert_eq!(e.pages(&e.open(path).unwrap()).unwrap().len(), 1);
        assert!(
            e.render(&e.open(path).unwrap(), 0, 1.0).unwrap().rgba
                == e.render(&doc, i as u16, 1.0).unwrap().rgba
        );
    }
    assert!(e.split_pdf(&doc, "1;2", &folder).is_err());
    assert_eq!(doc.bytes, before);
    for path in outputs {
        fs::remove_file(path).unwrap();
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn real_annotations_edit_delete_and_roundtrip() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = e.render(&doc, 0, 1.0).unwrap().rgba;
    for (i, kind) in [
        MarkupKind::Highlight,
        MarkupKind::Underline,
        MarkupKind::Strikethrough,
        MarkupKind::Note,
    ]
    .into_iter()
    .enumerate()
    {
        e.add_annotation(
            &mut doc,
            &Markup {
                page: 0,
                kind,
                bounds: Bounds {
                    left: 20.0,
                    bottom: 20.0 + i as f32 * 30.0,
                    right: 160.0,
                    top: 40.0 + i as f32 * 30.0,
                },
                color: [230, 100, 20],
                opacity: 0.6,
                text: "Comment äöå".into(),
            },
        )
        .unwrap();
    }
    let annotations = e.annotations(&doc).unwrap();
    assert_eq!(annotations.len(), 4);
    assert!(before != e.render(&doc, 0, 1.0).unwrap().rgba);
    assert_eq!(annotations[3].markup.text, "Comment äöå");
    let mut change = annotations[3].markup.clone();
    change.text = "Changed note".into();
    change.color = [40, 80, 220];
    e.update_annotation(&mut doc, &annotations[3], &change)
        .unwrap();
    assert!(e.delete_annotation(&mut doc, &annotations[0]).is_err());
    let current = e.annotations(&doc).unwrap();
    assert_eq!(current[3].markup.text, "Changed note");
    e.delete_annotation(&mut doc, &current[0]).unwrap();
    assert_eq!(e.annotations(&doc).unwrap().len(), 3);
    doc.undo();
    assert_eq!(e.annotations(&doc).unwrap().len(), 4);
    let path = temp("annotations.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as(&path).unwrap();
    let reopened = e.open(&path).unwrap();
    assert_eq!(e.annotations(&reopened).unwrap().len(), 4);
    assert!(e.render(&doc, 0, 1.0).unwrap().rgba == e.render(&reopened, 0, 1.0).unwrap().rgba);
    let pdf = parse(&fs::read(&path).unwrap()).unwrap();
    assert!(
        pdf.objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(|d| d.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Highlight"))
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn inserted_images_move_resize_and_survive_text_edits() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let image = Picture {
        width: 2,
        height: 2,
        rgba: vec![
            200, 30, 80, 255, 20, 180, 40, 255, 20, 50, 200, 255, 255, 255, 255, 128,
        ],
    };
    e.place_picture(
        &mut doc,
        &image,
        Placement {
            page: 0,
            left: 20.0,
            bottom: 20.0,
            width: 50.0,
            height: 50.0,
        },
    )
    .unwrap();
    let items = e.placed_pictures(&doc).unwrap();
    assert_eq!(items.len(), 1);
    let before = e.render(&doc, 0, 1.0).unwrap().rgba;
    let at = Placement {
        page: 0,
        left: 100.0,
        bottom: 80.0,
        width: 90.0,
        height: 30.0,
    };
    e.move_picture(&mut doc, &items[0], at).unwrap();
    assert!(before != e.render(&doc, 0, 1.0).unwrap().rgba);
    assert!(e.move_picture(&mut doc, &items[0], at).is_err());
    let object = e.text_objects(&doc, 0).unwrap().remove(0);
    e.edit(&mut doc, &object, "AB").unwrap();
    let items = e.placed_pictures(&doc).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].placement.width, 90.0);
    let path = temp("images.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as(&path).unwrap();
    let reopened = e.open(&path).unwrap();
    assert_eq!(e.placed_pictures(&reopened).unwrap().len(), 1);
    assert!(e.render(&doc, 0, 1.0).unwrap().rgba == e.render(&reopened, 0, 1.0).unwrap().rgba);
    fs::remove_file(path).unwrap();
    for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg] {
        let path = temp(if format == image::ImageFormat::Png {
            "image.png"
        } else {
            "image.jpg"
        });
        image::save_buffer_with_format(
            &path,
            &[200, 30, 80, 20, 180, 40, 20, 50, 200, 255, 255, 255],
            2,
            2,
            image::ExtendedColorType::Rgb8,
            format,
        )
        .unwrap();
        let loaded = Picture::open(&path).unwrap();
        assert_eq!((loaded.width, loaded.height), (2, 2));
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn independent_documents_keep_separate_undo_histories() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut first = e.open(&fixture("multi-page.pdf")).unwrap();
    let mut second = e.open(&fixture("multi-page.pdf")).unwrap();
    let original = first.bytes.clone();
    let object = e.text_objects(&first, 0).unwrap().remove(0);
    e.edit(&mut first, &object, "AB").unwrap();
    assert_eq!(second.bytes, original);
    assert!(!second.can_undo());
    e.edit_pages(&mut second, &PageEdit::Rotate { page: 0 })
        .unwrap();
    let rotated = second.bytes.clone();
    first.undo();
    assert_eq!(first.bytes, original);
    assert_eq!(second.bytes, rotated);
    assert!(second.can_undo());
    second.undo();
    assert_eq!(second.bytes, original);
    first.redo();
    assert_ne!(first.bytes, original);
}

#[test]
fn ocr_layer_is_invisible_searchable_and_undoable() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = e.render(&doc, 0, 1.0).unwrap().rgba;
    e.add_ocr_words(
        &mut doc,
        0,
        1.0,
        &[crate::ocr::RecognizedWord {
            text: "ScannedWord".into(),
            x: 20.0,
            y: 30.0,
            width: 150.0,
            height: 20.0,
        }],
    )
    .unwrap();
    assert!(before == e.render(&doc, 0, 1.0).unwrap().rgba);
    assert_eq!(e.find_text(&doc, "ScannedWord").unwrap().len(), 1);
    let path = temp("ocr-layer.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as(&path).unwrap();
    assert_eq!(
        e.find_text(&e.open(&path).unwrap(), "ScannedWord")
            .unwrap()
            .len(),
        1
    );
    doc.undo();
    assert!(e.find_text(&doc, "ScannedWord").unwrap().is_empty());
    assert!(e.ocr_page(&mut doc, 0).is_err());
    fs::remove_file(path).unwrap();
}
#[test]
#[ignore = "Requires an installed Windows OCR recognition language"]
fn windows_ocr_recognizes_scanned_page() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("ocr-source.pdf");
    fs::write(&path, generated("HELLO WORLD", false, 0, 0.0, false, false)).unwrap();
    let mut doc = e.open(&path).unwrap();
    let image = e.render(&doc, 0, 2.0).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    let id = pdf.get_pages()[&1];
    pdf.get_dictionary_mut(id).unwrap().remove(b"Contents");
    pdf.get_dictionary_mut(id).unwrap().remove(b"Resources");
    doc = e.candidate(&doc, pdf).unwrap();
    e.place_picture(
        &mut doc,
        &Picture {
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        },
        Placement {
            page: 0,
            left: 0.0,
            bottom: 0.0,
            width: 500.0,
            height: 300.0,
        },
    )
    .unwrap();
    assert!(e.text_objects(&doc, 0).unwrap().is_empty());
    let before = e.render(&doc, 0, 1.0).unwrap().rgba;
    assert!(e.ocr_page(&mut doc, 0).unwrap() > 0);
    assert!(!e.find_text(&doc, "HELLO").unwrap().is_empty());
    assert!(before == e.render(&doc, 0, 1.0).unwrap().rgba);
    fs::remove_file(path).unwrap();
}

#[test]
fn redaction_removes_source_objects_and_pixels() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    e.place_picture(
        &mut doc,
        &Picture {
            width: 1,
            height: 1,
            rgba: vec![250, 30, 90, 255],
        },
        Placement {
            page: 0,
            left: 20.0,
            bottom: 20.0,
            width: 80.0,
            height: 80.0,
        },
    )
    .unwrap();
    let original = doc.bytes.clone();
    let pages = e.pages(&doc).unwrap().len();
    e.redact(
        &mut doc,
        0,
        Bounds {
            left: 20.0,
            bottom: 20.0,
            right: 100.0,
            top: 100.0,
        },
    )
    .unwrap();
    assert_eq!(e.pages(&doc).unwrap().len(), pages);
    assert!(!e.text_objects(&doc, 0).unwrap().is_empty());
    assert!(e.placed_pictures(&doc).unwrap().is_empty());
    let pdf = parse(&doc.bytes).unwrap();
    assert!(!pdf.trailer.has(b"Info"));
    let image = e.render(&doc, 0, 1.0).unwrap();
    let (x, y) = e.page_to_pixel(&doc, 0, 1.0, 60.0, 60.0).unwrap();
    let offset = (y as usize * image.width as usize + x as usize) * 4;
    assert_eq!(&image.rgba[offset..offset + 3], &[0, 0, 0]);
    let path = temp("redacted.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as(&path).unwrap();
    let reopened = e.open(&path).unwrap();
    assert!(!e.text_objects(&reopened, 0).unwrap().is_empty());
    assert!(image.rgba == e.render(&reopened, 0, 1.0).unwrap().rgba);
    doc.undo();
    assert_eq!(doc.bytes, original);
    fs::remove_file(path).unwrap();
}

#[test]
fn compression_reduces_images_and_preserves_text() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let mut seed = 12345u32;
    let mut rgba = vec![];
    for _ in 0..512 * 512 {
        for _ in 0..3 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            rgba.push((seed >> 24) as u8);
        }
        rgba.push(255);
    }
    e.place_picture(
        &mut doc,
        &Picture {
            width: 512,
            height: 512,
            rgba,
        },
        Placement {
            page: 0,
            left: 20.0,
            bottom: 20.0,
            width: 150.0,
            height: 150.0,
        },
    )
    .unwrap();
    let original = doc.bytes.clone();
    let text = e
        .text_objects(&doc, 0)
        .unwrap()
        .into_iter()
        .map(|o| o.text)
        .collect::<Vec<_>>();
    let result = e.compress_images(&mut doc, 128, 55).unwrap();
    assert!(result.after < result.before);
    assert_eq!(result.images, 1);
    assert_eq!(
        text,
        e.text_objects(&doc, 0)
            .unwrap()
            .into_iter()
            .map(|o| o.text)
            .collect::<Vec<_>>()
    );
    let pdf = parse(&doc.bytes).unwrap();
    assert!(
        pdf.objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .any(
                |s| s.dict.get(b"Filter").ok().and_then(|o| o.as_name().ok()) == Some(b"DCTDecode")
                    && s.dict.get(b"Width").unwrap().as_i64().unwrap() == 128
            )
    );
    e.render(&doc, 0, 1.0).unwrap();
    doc.undo();
    assert_eq!(doc.bytes, original);
    assert!(e.compress_images(&mut doc, 0, 90).is_err());
}

#[test]
fn page_numbers_watermarks_have_opacity_and_single_undo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let original = doc.bytes.clone();
    let count = e.pages(&doc).unwrap().len();
    e.stamp_pages(
        &mut doc,
        &TextStamp {
            template: "Page {page} of {pages}".into(),
            font: "Arial".into(),
            size: 12.0,
            color: [20, 40, 60],
            opacity: 0.4,
            x: 30.0,
            y: 20.0,
        },
    )
    .unwrap();
    for page in 0..count {
        assert!(
            e.text_objects(&doc, page as u16)
                .unwrap()
                .iter()
                .any(|o| o.text == format!("Page {} of {count}", page + 1))
        );
    }
    let pdf = parse(&doc.bytes).unwrap();
    assert!(
        pdf.objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(|d| d.get(b"ca").ok().and_then(|o| o.as_float().ok()) == Some(0.4))
    );
    doc.undo();
    assert_eq!(doc.bytes, original);
    e.stamp_pages(
        &mut doc,
        &TextStamp {
            template: "DRAFT".into(),
            font: "Arial".into(),
            size: 40.0,
            color: [80, 80, 80],
            opacity: 0.2,
            x: 70.0,
            y: 120.0,
        },
    )
    .unwrap();
    assert_eq!(e.find_text(&doc, "DRAFT").unwrap().len(), count);
    e.render(&doc, 0, 1.0).unwrap();
}

#[test]
fn page_image_exports_respect_dpi_format_and_existing_files() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let original = doc.bytes.clone();
    for (format, name, dpi) in [
        (PageImageFormat::Png, "export.png", 72),
        (PageImageFormat::Jpeg, "export.jpg", 144),
    ] {
        let path = temp(name);
        let _ = fs::remove_file(&path);
        e.export_page_image(&doc, 0, dpi, format, &path).unwrap();
        let decoded = Picture::open(&path).unwrap();
        let expected = e.render(&doc, 0, f32::from(dpi) / 72.0).unwrap();
        assert_eq!(
            (decoded.width, decoded.height),
            (expected.width, expected.height)
        );
        if format == PageImageFormat::Png {
            assert_eq!(decoded.rgba, expected.rgba);
        }
        assert!(e.export_page_image(&doc, 0, dpi, format, &path).is_err());
        fs::remove_file(path).unwrap();
    }
    assert_eq!(doc.bytes, original);
    assert!(!doc.can_undo());
    assert!(
        e.export_page_image(&doc, 0, 0, PageImageFormat::Png, &temp("invalid-dpi.png"))
            .is_err()
    );
}

#[test]
fn cropping_changes_view_and_preserves_content_for_undo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let original = doc.bytes.clone();
    let crop = Bounds {
        left: 20.0,
        bottom: 20.0,
        right: 300.0,
        top: 200.0,
    };
    e.crop_page(&mut doc, 0, crop).unwrap();
    let image = e.render(&doc, 0, 1.0).unwrap();
    assert_eq!((image.width, image.height), (280, 180));
    let path = temp("cropped.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as(&path).unwrap();
    assert!(image.rgba == e.render(&e.open(&path).unwrap(), 0, 1.0).unwrap().rgba);
    assert!(
        e.crop_page(&mut doc, 0, Bounds { left: -1.0, ..crop })
            .is_err()
    );
    doc.undo();
    assert_eq!(doc.bytes, original);
    fs::remove_file(path).unwrap();
}

#[test]
fn drawings_are_real_annotations_and_render_after_reopen() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let before = doc.bytes.clone();
    let raster = e.render(&doc, 0, 1.0).unwrap().rgba;
    for (i, kind) in [
        DrawingKind::Freehand,
        DrawingKind::Rectangle,
        DrawingKind::Ellipse,
        DrawingKind::Line,
        DrawingKind::Arrow,
    ]
    .into_iter()
    .enumerate()
    {
        e.add_drawing(
            &mut doc,
            &Drawing {
                page: 0,
                kind,
                points: vec![
                    [20.0 + i as f32 * 50.0, 20.0],
                    [60.0 + i as f32 * 50.0, 80.0],
                ],
                color: [40, 100, 220],
                width: 2.0,
                opacity: 0.8,
            },
        )
        .unwrap();
    }
    let pdf = parse(&doc.bytes).unwrap();
    let page = pdf.get_pages()[&1];
    assert_eq!(
        pdf.get_dictionary(page)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(raster != e.render(&doc, 0, 1.0).unwrap().rgba);
    let path = temp("drawings.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as(&path).unwrap();
    assert!(
        e.render(&doc, 0, 1.0).unwrap().rgba
            == e.render(&e.open(&path).unwrap(), 0, 1.0).unwrap().rgba
    );
    for _ in 0..5 {
        doc.undo();
    }
    assert_eq!(doc.bytes, before);
    fs::remove_file(path).unwrap();
}

#[test]
fn bookmarks_edit_nested_tree_and_round_trip() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let original = doc.bytes.clone();
    let edit = |doc: &mut Document, action| {
        let revision = doc.revision;
        e.edit_bookmarks(doc, revision, action).unwrap();
    };
    edit(
        &mut doc,
        BookmarkEdit::Add {
            title: "First".into(),
            page: 0,
            parent: None,
        },
    );
    let root = e.bookmarks(&doc).unwrap().items[0].id;
    edit(
        &mut doc,
        BookmarkEdit::Add {
            title: "Child".into(),
            page: 1,
            parent: Some(root),
        },
    );
    edit(
        &mut doc,
        BookmarkEdit::Add {
            title: "Last".into(),
            page: 1,
            parent: None,
        },
    );
    let list = e.bookmarks(&doc).unwrap();
    let last = list.items[2].id;
    assert_eq!(
        list.items.iter().map(|b| b.depth).collect::<Vec<_>>(),
        vec![0, 1, 0]
    );
    assert_eq!(list.items[1].page, Some(1));
    edit(
        &mut doc,
        BookmarkEdit::Rename {
            id: root,
            title: "Ää ö €".into(),
        },
    );
    edit(
        &mut doc,
        BookmarkEdit::Move {
            id: last,
            position: 0,
        },
    );
    let list = e.bookmarks(&doc).unwrap();
    assert_eq!(
        list.items
            .iter()
            .map(|b| b.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Last", "Ää ö €", "Child"]
    );
    assert!(
        e.edit_bookmarks(
            &mut doc,
            list.revision - 1,
            BookmarkEdit::Delete { id: root }
        )
        .is_err()
    );
    let path = temp("bookmarks.pdf");
    let _ = fs::remove_file(&path);
    doc.save_as(&path).unwrap();
    let reopened = e.open(&path).unwrap();
    assert_eq!(e.bookmarks(&reopened).unwrap().items.len(), 3);
    assert_eq!(e.bookmarks(&reopened).unwrap().items[2].page, Some(1));
    edit(&mut doc, BookmarkEdit::Delete { id: root });
    assert_eq!(e.bookmarks(&doc).unwrap().items.len(), 1);
    doc.undo();
    assert_eq!(e.bookmarks(&doc).unwrap().items.len(), 3);
    for _ in 0..5 {
        doc.undo();
    }
    assert_eq!(doc.bytes, original);
    fs::remove_file(path).unwrap();
}

#[test]
fn bookmarks_resolve_named_destinations_and_reject_cycles() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let mut pdf = parse(&doc.bytes).unwrap();
    let page = pdf.get_pages()[&2];
    let catalog = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let root = pdf.new_object_id();
    let item = pdf.new_object_id();
    let mut dict = lopdf::Dictionary::new();
    dict.set("Title", lopdf::text_string("Named destination"));
    dict.set("Parent", root);
    dict.set("Dest", Object::Name(b"chapter".to_vec()));
    pdf.objects.insert(item, Object::Dictionary(dict));
    let mut outlines = lopdf::Dictionary::new();
    outlines.set("First", item);
    outlines.set("Last", item);
    outlines.set("Count", 1);
    pdf.objects.insert(root, Object::Dictionary(outlines));
    let mut names = lopdf::Dictionary::new();
    names.set(
        "chapter",
        vec![Object::Reference(page), Object::Name(b"Fit".to_vec())],
    );
    pdf.get_dictionary_mut(catalog).unwrap().set("Dests", names);
    pdf.get_dictionary_mut(catalog)
        .unwrap()
        .set("Outlines", root);
    let valid = e.candidate(&doc, pdf.clone()).unwrap();
    assert_eq!(e.bookmarks(&valid).unwrap().items[0].page, Some(1));
    pdf.get_dictionary_mut(item).unwrap().set("Next", item);
    let invalid = e.candidate(&doc, pdf).unwrap();
    assert!(e.bookmarks(&invalid).is_err());
}

#[test]
fn compare_generated_pages_is_read_only_and_detects_edits() {
    let _lock = PDFIUM.lock().unwrap();
    let e = engine();
    let left_path = temp("compare-left.pdf");
    let right_path = temp("compare-right.pdf");
    fs::write(&left_path, generated("First", false, 0, 0., false, false)).unwrap();
    fs::write(&right_path, generated("Second", false, 0, 0., false, false)).unwrap();
    let left = e.open(&left_path).unwrap();
    let right = e.open(&right_path).unwrap();
    let bytes = left.bytes.clone();
    let same = e.compare_page(&left, &left, 0, false).unwrap();
    assert!(same.changes.is_empty());
    assert_eq!(same.changed_pixels, 0);
    let changed = e.compare_page(&left, &right, 0, true).unwrap();
    assert!(!changed.changes.is_empty());
    assert!(changed.changed_pixels > 0);
    assert!(!changed.left_marks.is_empty());
    assert!(!changed.right_marks.is_empty());
    assert_eq!(left.bytes, bytes);
    assert!(!left.can_undo());
    assert!(e.compare_page(&left, &right, 1, false).is_err());
    let mut extra = e.open(&right_path).unwrap();
    e.edit_pages(
        &mut extra,
        &PageEdit::Insert {
            path: right_path.clone(),
            before: 1,
        },
    )
    .unwrap();
    let added = e.compare_page(&left, &extra, 1, false).unwrap();
    assert!(added.left.is_none());
    assert!(added.right.is_some());
    assert!(added.changed_pixels > 0);
    assert!(
        added
            .changes
            .iter()
            .all(|change| change.kind == ChangeKind::Added)
    );
}

#[test]
fn loaded_page_geometry_matches_fast_metadata_for_generated_rotations() {
    let _lock = PDFIUM.lock().unwrap_or_else(|p| p.into_inner());
    let engine = engine();
    for rotation in [0, 90, 180, 270] {
        let bytes = generated("Generated geometry", false, rotation, 0.0, false, false);
        let native = engine
            .pdfium
            .load_pdf_from_byte_slice(&bytes, None)
            .unwrap();
        let metadata = native.pages().page_sizes().unwrap();
        let loaded = loaded_page_sizes(native.pages()).unwrap();
        assert_eq!(metadata.len(), loaded.len());
        assert!(!loaded.is_empty());
        for (expected, actual) in metadata.iter().zip(loaded) {
            assert_eq!(expected.width().value, actual.width);
            assert_eq!(expected.height().value, actual.height);
        }
    }
    assert!(checked_page_size(f32::NAN, 100.0).is_err());
    assert!(checked_page_size(100.0, f32::INFINITY).is_err());
    assert!(checked_page_size(0.0, 100.0).is_err());
}

#[test]
fn page_item_moves_preserve_text_and_neighbors_and_undo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        let path = temp(&format!("object-move-{rotation}.pdf"));
        fs::write(
            &path,
            generated("Move me", false, rotation, 0.0, false, false),
        )
        .unwrap();
        let original = e.open(&path).unwrap();
        let mut doc = e.open(&path).unwrap();
        let before = e.text_objects(&doc, 0).unwrap();
        let item = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.index == before[0].index)
            .unwrap();
        e.edit_page_items(
            &mut doc,
            std::slice::from_ref(&item),
            ObjectAction::Move {
                dx: 24.0,
                dy: -16.0,
            },
        )
        .unwrap();
        let after = e.text_objects(&doc, 0).unwrap();
        assert_eq!(before.len(), after.len());
        for (i, (a, b)) in before.iter().zip(&after).enumerate() {
            assert_eq!(a.text, b.text);
            assert_eq!(a.font, b.font);
            assert_eq!(a.color, b.color);
            if i > 0 {
                assert_eq!(a.matrix, b.matrix);
            }
        }
        assert!((after[0].bounds.left - before[0].bounds.left - 24.0).abs() < 0.1);
        assert!((after[0].bounds.bottom - before[0].bounds.bottom + 16.0).abs() < 0.1);
        assert_outside_equal(&e, &original, &doc, &before[0], &after[0]);
        assert!(
            e.edit_page_items(&mut doc, &[item], ObjectAction::Delete)
                .is_err()
        );
        let saved = temp(&format!("object-move-saved-{rotation}.pdf"));
        let _ = fs::remove_file(&saved);
        doc.save_as(&saved).unwrap();
        assert_eq!(
            e.text_objects(&e.open(&saved).unwrap(), 0).unwrap()[0].matrix,
            after[0].matrix
        );
        assert!(doc.undo());
        assert_eq!(doc.bytes, original.bytes);
        assert!(doc.redo());
    }
}

#[test]
fn page_item_duplicate_delete_and_invalid_move_are_atomic() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("unicode-cid.pdf")).unwrap();
    let before = doc.bytes.clone();
    let items = e.page_items(&doc, 0).unwrap();
    assert!(
        e.edit_page_items(
            &mut doc,
            &items[..1],
            ObjectAction::Move {
                dx: f32::NAN,
                dy: 0.0
            }
        )
        .is_err()
    );
    assert_eq!(doc.bytes, before);
    e.edit_page_items(
        &mut doc,
        &items[..1],
        ObjectAction::Duplicate {
            dx: 20.0,
            dy: -30.0,
        },
    )
    .unwrap();
    let after = e.page_items(&doc, 0).unwrap();
    assert_eq!(after.len(), items.len() + 1);
    e.edit_page_items(&mut doc, &after[after.len() - 1..], ObjectAction::Delete)
        .unwrap();
    assert_eq!(e.page_items(&doc, 0).unwrap().len(), items.len());
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.bytes, before);
}

#[test]
fn artwork_objects_copy_move_delete_and_paste_snapshot() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut pdf = parse(&generated("Artwork", false, 0, 0.0, false, false)).unwrap();
    let page = *pdf.get_pages().get(&1).unwrap();
    let mut bytes = pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap();
    bytes.extend(b"\nq 0 0 1 rg 60 60 30 30 re f Q\n");
    let form = pdf.add_object(Stream::new(dictionary! {
        "Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),20.into(),20.into()],
        "Resources"=>dictionary! {}
    }, b"0 1 0 rg 0 0 20 20 re f".to_vec()));
    let resources = pdf
        .get_dictionary(page)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .clone();
    let mut resources = match resources {
        Object::Reference(id) => pdf.get_dictionary(id).unwrap().clone(),
        Object::Dictionary(d) => d,
        _ => panic!("fixture resources"),
    };
    resources.set("XObject", dictionary! { "Group"=>form });
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Resources", resources);
    bytes.extend(b"\nq 1 0 0 1 170 60 cm /Group Do Q\n");
    let stream = pdf.add_object(Stream::new(dictionary! {}, bytes));
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let path = temp("object-artwork.pdf");
    pdf.save(&path).unwrap();
    let mut doc = e.open(&path).unwrap();
    e.place_picture(
        &mut doc,
        &Picture {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        },
        Placement {
            page: 0,
            left: 120.0,
            bottom: 60.0,
            width: 20.0,
            height: 20.0,
        },
    )
    .unwrap();
    for kind in ["Path", "Image", "XObjectForm"] {
        let item = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.kind == kind)
            .unwrap();
        let original = doc.bytes.clone();
        let b = item.bounds;
        let target = Bounds {
            right: b.left + (b.right - b.left) * 2.0,
            top: b.bottom + (b.top - b.bottom) * 1.5,
            ..b
        };
        e.edit_page_items(
            &mut doc,
            std::slice::from_ref(&item),
            ObjectAction::Resize { bounds: target },
        )
        .unwrap();
        let resized = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.index == item.index)
            .unwrap();
        assert!((resized.bounds.right - target.right).abs() < 0.2);
        assert!((resized.bounds.top - target.top).abs() < 0.2);
        assert!(doc.undo());
        assert_eq!(doc.bytes, original);
        let item = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.kind == kind)
            .unwrap();
        if kind == "XObjectForm" {
            let count = e.page_items(&doc, 0).unwrap().len();
            e.edit_page_items(
                &mut doc,
                std::slice::from_ref(&item),
                ObjectAction::Duplicate { dx: 15.0, dy: 5.0 },
            )
            .unwrap();
            assert_eq!(e.page_items(&doc, 0).unwrap().len(), count + 1);
            assert!(doc.undo());
            assert_eq!(doc.bytes, original);
        }
        let item = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.kind == kind)
            .unwrap();
        let clipboard = e
            .copy_page_items(&doc, std::slice::from_ref(&item))
            .unwrap();
        let original = doc.bytes.clone();
        e.edit_page_items(&mut doc, &[item], ObjectAction::Move { dx: 15.0, dy: 10.0 })
            .unwrap();
        assert!(doc.undo());
        assert_eq!(doc.bytes, original);
        let item = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.kind == kind)
            .unwrap();
        let count = e.page_items(&doc, 0).unwrap().len();
        e.edit_page_items(&mut doc, &[item], ObjectAction::Delete)
            .unwrap();
        assert_eq!(e.page_items(&doc, 0).unwrap().len(), count - 1);
        e.paste_page_items(&mut doc, &clipboard, 0).unwrap();
        assert_eq!(e.page_items(&doc, 0).unwrap().len(), count);
        e.render(&doc, 0, 1.0).unwrap();
        assert!(doc.undo());
        assert!(doc.undo());
        assert_eq!(doc.bytes, original);
    }
}

#[test]
fn ranged_headers_footers_expand_tokens_and_undo() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let original = doc.bytes.clone();
    let count = e.pages(&doc).unwrap().len();
    let mut stamp = RangeStamp {
        stamp: TextStamp {
            template: "Report {date} {page}/{pages}".into(),
            font: "Arial".into(),
            size: 12.,
            color: [0, 0, 0],
            opacity: 1.,
            x: 30.,
            y: 20.,
        },
        pages: "2".into(),
        date: "2026-10-23".into(),
        placement: StampPlacement::Header,
    };
    e.stamp_page_range(&mut doc, &stamp).unwrap();
    assert!(
        e.text_objects(&doc, 0)
            .unwrap()
            .iter()
            .all(|t| !t.text.contains("Report"))
    );
    assert!(
        e.text_objects(&doc, 1)
            .unwrap()
            .iter()
            .any(|t| t.text == format!("Report 2026-10-23 2/{count}"))
    );
    doc.undo();
    assert_eq!(doc.bytes, original);
    stamp.pages = "0".into();
    assert!(e.stamp_page_range(&mut doc, &stamp).is_err());
    assert_eq!(doc.bytes, original);
    for rotation in [0, 90, 180, 270] {
        let path = temp("stamp-rotation.pdf");
        fs::write(
            &path,
            generated("Original", false, rotation, 0., false, false),
        )
        .unwrap();
        let mut doc = e.open(&path).unwrap();
        stamp.pages = "1".into();
        stamp.placement = StampPlacement::Footer;
        e.stamp_page_range(&mut doc, &stamp).unwrap();
        assert!(
            e.text_objects(&doc, 0)
                .unwrap()
                .iter()
                .any(|t| t.text.contains("Report 2026-10-23"))
        );
        e.render(&doc, 0, 1.).unwrap();
    }
}

#[test]
fn images_to_pdf_preserves_order_and_page_size() {
    let _lock = PDFIUM.lock().unwrap();
    let engine = engine();
    let red = Picture { width: 32, height: 16, rgba: [255,0,0,255].repeat(32*16) };
    let blue = Picture { width: 16, height: 32, rgba: [0,0,255,255].repeat(32*16) };
    let doc = engine.pictures_to_pdf([Ok(red.clone()), Ok(blue.clone())], ImagePageSize::Image).unwrap();
    let pages = engine.pages(&doc).unwrap();
    assert_eq!(pages.len(),2);
    let output = temp("images-created.pdf");
    if output.exists() { fs::remove_file(&output).unwrap(); }
    doc.save_as(&output).unwrap();
    assert!(doc.save_as(&output).is_err());
    assert_eq!(engine.pages(&engine.open(&output).unwrap()).unwrap().len(), 2);
    assert_eq!((pages[0].width,pages[0].height),(24.0,12.0));
    assert_eq!((pages[1].width,pages[1].height),(12.0,24.0));
    for (page, channel) in [(0,0),(1,2)] {
        let image = engine.render(&doc,page,2.0).unwrap();
        let center = (image.height as usize/2*image.width as usize+image.width as usize/2)*4;
        assert!(image.rgba[center+channel]>240);
        assert!(image.rgba[center+(channel+1)%3]<10);
    }
    let letter = engine.pictures_to_pdf([Ok(red)],ImagePageSize::Letter).unwrap();
    assert_eq!(engine.pages(&letter).unwrap()[0].width,612.0);
    let a4 = engine.pictures_to_pdf([Ok(blue)],ImagePageSize::A4).unwrap();
    assert!((engine.pages(&a4).unwrap()[0].height-841.89).abs()<0.01);
    assert!(engine.images_to_pdf(&[],ImagePageSize::A4).is_err());
    assert!(engine.pictures_to_pdf([Ok(Picture{width:0,height:0,rgba:vec![]})],ImagePageSize::Image).is_err());
}

#[test]
fn image_import_decodes_png_jpeg_and_rejects_bad_heic() {
    let dir = temp("image-import");
    fs::create_dir_all(&dir).unwrap();
    for format in [image::ImageFormat::Png,image::ImageFormat::Jpeg] {
        let path = dir.join(format!("fixture.{}",format.extensions_str()[0]));
        image::save_buffer_with_format(&path,&[80,120,200].repeat(16),4,4,image::ColorType::Rgb8,format).unwrap();
        let picture = import_picture(&path).unwrap();
        assert_eq!((picture.width,picture.height),(4,4));
    }
    let bad = dir.join("invalid.heic");
    fs::write(&bad,b"not a HEIC image").unwrap();
    assert!(import_picture(&bad).is_err());
}

#[path = "redaction_tests.rs"]
mod redaction_tests;

#[test]
fn resize_text_preserves_content_and_undo_on_rotated_pages() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for rotation in [0, 90, 180, 270] {
        let path = temp(&format!("object-resize-{rotation}.pdf"));
        fs::write(
            &path,
            generated("Resize me", false, rotation, 0.0, false, false),
        )
        .unwrap();
        let mut doc = e.open(&path).unwrap();
        let original = doc.bytes.clone();
        let before = e.text_objects(&doc, 0).unwrap();
        let item = e
            .page_items(&doc, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.index == before[0].index)
            .unwrap();
        let b = item.bounds;
        let target = Bounds {
            right: b.left + (b.right - b.left) * 1.8,
            top: b.bottom + (b.top - b.bottom) * 1.4,
            ..b
        };
        for invalid in [Bounds { right: b.left, ..b }, Bounds { top: f32::NAN, ..b }] {
            assert!(
                e.edit_page_items(
                    &mut doc,
                    std::slice::from_ref(&item),
                    ObjectAction::Resize { bounds: invalid }
                )
                .is_err()
            );
            assert_eq!(doc.bytes, original);
        }
        e.edit_page_items(&mut doc, &[item], ObjectAction::Resize { bounds: target })
            .unwrap();
        let after = e.text_objects(&doc, 0).unwrap();
        assert_eq!(after[0].text, before[0].text);
        assert_eq!(after[0].font, before[0].font);
        for (a, b) in before.iter().zip(&after).skip(1) {
            assert_eq!(a.matrix, b.matrix);
        }
        assert!((after[0].bounds.left - target.left).abs() < 0.2);
        assert!((after[0].bounds.right - target.right).abs() < 0.2);
        assert!((after[0].bounds.top - target.top).abs() < 0.2);
        e.render(&doc, 0, 1.0).unwrap();
        assert!(doc.undo());
        assert_eq!(doc.bytes, original);
        assert!(doc.redo());
    }
}

#[test]
fn object_clipboard_copies_text_and_images_between_independent_documents() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut source = e.open(&fixture("unicode-cid.pdf")).unwrap();
    e.place_picture(
        &mut source,
        &Picture {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        },
        Placement {
            page: 0,
            left: 100.0,
            bottom: 100.0,
            width: 25.0,
            height: 25.0,
        },
    )
    .unwrap();
    let text = e.text_objects(&source, 0).unwrap()[0].clone();
    let items: Vec<_> = e
        .page_items(&source, 0)
        .unwrap()
        .into_iter()
        .filter(|i| i.index == text.index || i.kind == "Image")
        .collect();
    assert_eq!(items.len(), 2);
    let clipboard = e.copy_page_items(&source, &items).unwrap();
    drop(source);
    let mut destination = e.open(&fixture("rotated.pdf")).unwrap();
    let original = destination.bytes.clone();
    let count = e.page_items(&destination, 0).unwrap().len();
    assert!(
        e.paste_page_items(&mut destination, &clipboard, u16::MAX)
            .is_err()
    );
    assert_eq!(destination.bytes, original);
    e.paste_page_items(&mut destination, &clipboard, 0).unwrap();
    assert_eq!(e.page_items(&destination, 0).unwrap().len(), count + 2);
    let pasted_text = e
        .text_objects(&destination, 0)
        .unwrap()
        .into_iter()
        .find(|item| item.index == count)
        .unwrap();
    assert_eq!(pasted_text.text, text.text);
    assert_eq!(pasted_text.font, text.font);
    assert_eq!(
        e.page_items(&destination, 0).unwrap()[count + 1].kind,
        "Image"
    );
    let pasted = destination.bytes.clone();
    e.edit(&mut destination, &pasted_text, "AB").unwrap();
    assert_eq!(
        e.text_objects(&destination, 0)
            .unwrap()
            .into_iter()
            .find(|item| item.index == count)
            .unwrap()
            .text,
        "AB"
    );
    assert!(destination.undo());
    assert_eq!(destination.bytes, pasted);
    e.render(&destination, 0, 1.0).unwrap();
    let path = temp("cross-document-paste.pdf");
    let _ = fs::remove_file(&path);
    destination.save_as(&path).unwrap();
    assert_eq!(
        e.page_items(&e.open(&path).unwrap(), 0).unwrap().len(),
        count + 2
    );
    assert!(destination.undo());
    assert_eq!(destination.bytes, original);
    assert!(destination.redo());
}

#[test]
fn grouped_clipboard_preserves_shared_forms_clips_and_paint_order() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut pdf = parse(&generated("Unselected", false, 0, 0.0, false, false)).unwrap();
    let page = *pdf.get_pages().get(&1).unwrap();
    let form = pdf.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Form",
            "BBox"=>vec![0.into(),0.into(),20.into(),20.into()],
            "Matrix"=>vec![0.into(),1.into(),(-1).into(),0.into(),20.into(),0.into()],
            "Resources"=>dictionary! {}
        },
        b"0 1 0 rg 0 0 30 20 re f 1 0 0 rg 3 3 8 8 re f".to_vec(),
    ));
    let resource = pdf
        .get_dictionary(page)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .clone();
    let mut resources = match resource {
        Object::Reference(id) => pdf.get_dictionary(id).unwrap().clone(),
        Object::Dictionary(d) => d,
        _ => panic!("fixture resources"),
    };
    resources.set("XObject", dictionary! { "Shared"=>form });
    resources.set(
        "ExtGState",
        dictionary! { "Half"=>dictionary! { "Type"=>"ExtGState","ca"=>0.5,"CA"=>0.5 } },
    );
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Resources", resources);
    let selected = b"q 65 65 18 18 re W n /Half gs 1.5 0 0 1.5 60 60 cm /Shared Do Q\nq 0 0 1 rg 68 68 7 7 re f Q\nq 1 0 0 1 110 70 cm /Shared Do Q\n";
    let mut content = pdf.get_page_content_with_limit(page, MAX_STREAM).unwrap();
    content.extend_from_slice(selected);
    content.extend(b"q 1 0 0 1 210 140 cm /Shared Do Q");
    let stream = pdf.add_object(Stream::new(dictionary! {}, content));
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let path = temp("grouped-copy-source.pdf");
    pdf.save(&path).unwrap();
    let source = e.open(&path).unwrap();
    let original = source.bytes.clone();
    let items: Vec<_> = e
        .page_items(&source, 0)
        .unwrap()
        .into_iter()
        .filter(|item| item.kind != "Text" && item.bounds.left < 180.0)
        .collect();
    assert_eq!(items.len(), 3);
    let clipboard = e.copy_page_items(&source, &items).unwrap();
    assert_eq!(source.bytes, original);
    drop(source);
    let empty = pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
    pdf.get_dictionary_mut(page).unwrap().set("Contents", empty);
    pdf.get_dictionary_mut(page).unwrap().set("Rotate", 90);
    let destination_path = temp("grouped-copy-destination.pdf");
    pdf.save(&destination_path).unwrap();
    let mut destination = e.open(&destination_path).unwrap();
    let before = destination.bytes.clone();
    e.paste_page_items(&mut destination, &clipboard, 0).unwrap();
    assert_eq!(e.page_items(&destination, 0).unwrap().len(), 3);
    let mut expected = b"q 1 0 0 1 12 -12 cm\n".to_vec();
    expected.extend_from_slice(selected);
    expected.extend(b"Q");
    let stream = pdf.add_object(Stream::new(dictionary! {}, expected));
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let expected_path = temp("grouped-copy-expected.pdf");
    pdf.save(&expected_path).unwrap();
    let expected = e.render(&e.open(&expected_path).unwrap(), 0, 2.0).unwrap();
    let actual = e.render(&destination, 0, 2.0).unwrap();
    assert_eq!(
        actual.rgba, expected.rgba,
        "Form clips, transforms, opacity and ordering must survive copy"
    );
    let saved = temp("grouped-copy-saved.pdf");
    let _ = fs::remove_file(&saved);
    destination.save_as(&saved).unwrap();
    assert_eq!(
        e.render(&e.open(&saved).unwrap(), 0, 2.0).unwrap().rgba,
        expected.rgba
    );
    assert!(destination.undo());
    assert_eq!(destination.bytes, before);
    assert!(destination.redo());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn table_export_generated_ruled_grid_and_office_files() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let engine = engine();
    let mut pdf = parse(&generated("Soluä", false, 0, 0., false, false)).unwrap();
    let id = *pdf.get_pages().values().next().unwrap();
    let base = Content::decode(&pdf.get_page_content(id)).unwrap();
    let mut ops = vec![];
    for y in [220, 150] {
        for x in [50, 250] {
            let mut block = base.operations[2..7].to_vec();
            block[3].operands[4] = x.into();
            block[3].operands[5] = y.into();
            ops.extend(block);
            ops.push(Operation::new("ET", vec![]));
        }
    }
    for x in [20, 220, 440] {
        ops.push(Operation::new("m", vec![x.into(), 100.into()]));
        ops.push(Operation::new("l", vec![x.into(), 260.into()]));
        ops.push(Operation::new("S", vec![]));
    }
    for y in [100, 180, 260] {
        ops.push(Operation::new("m", vec![20.into(), y.into()]));
        ops.push(Operation::new("l", vec![440.into(), y.into()]));
        ops.push(Operation::new("S", vec![]));
    }
    pdf.change_page_content(
        id,
        Content {
            operations: ops.clone(),
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    let source = temp("table.pdf");
    pdf.save(&source).unwrap();
    let doc = engine.open(&source).unwrap();
    let grid = engine
        .detect_table(
            &doc,
            0,
            Bounds {
                left: 0.,
                bottom: 0.,
                right: 500.,
                top: 300.,
            },
        )
        .unwrap();
    assert_eq!(
        grid.cells,
        vec![vec!["Soluä", "Soluä"], vec!["Soluä", "Soluä"]]
    );
    ops.retain(|o| !matches!(o.operator.as_str(), "m" | "l" | "S"));
    pdf.change_page_content(id, Content { operations: ops }.encode().unwrap())
        .unwrap();
    let plain = temp("table-unruled.pdf");
    pdf.save(&plain).unwrap();
    let plain = engine.open(&plain).unwrap();
    assert_eq!(
        engine
            .detect_table(
                &plain,
                0,
                Bounds {
                    left: 0.,
                    bottom: 0.,
                    right: 500.,
                    top: 300.
                }
            )
            .unwrap(),
        grid
    );

    for (format, name) in [
        (TableFormat::Xlsx, "table.xlsx"),
        (TableFormat::Csv, "table.csv"),
    ] {
        let path = temp(name);
        let _ = fs::remove_file(&path);
        grid.export(&path, format).unwrap();
        assert!(grid.export(&path, format).is_err());
    }
    let csv = temp("formula.csv");
    let _ = fs::remove_file(&csv);
    TableGrid {
        cells: vec![vec!["=1+1".into(), "@SUM(A1)".into(), "ä,\"ö".into()]],
    }
    .export(&csv, TableFormat::Csv)
    .unwrap();
    let saved = fs::read_to_string(csv).unwrap();
    assert!(saved.contains("'=1+1"));
    assert!(saved.contains("'@SUM(A1)"));
    assert!(saved.contains("ä,\"\"ö"));
    let too_long = temp("table-too-long.xlsx");
    let _ = fs::remove_file(&too_long);
    assert!(
        TableGrid {
            cells: vec![vec!["😀".repeat(16384)]]
        }
        .export(&too_long, TableFormat::Xlsx)
        .is_err()
    );
    assert!(!too_long.exists());
}

#[test]
fn designed_fields_fill_move_resize_delete_and_reopen() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let path = temp("design-fields.pdf");
    fs::write(&path, generated("Form", false, 0, 0., false, false)).unwrap();
    let mut doc = e.open(&path).unwrap();
    for (i, kind) in [
        DesignFieldKind::Text,
        DesignFieldKind::Checkbox,
        DesignFieldKind::Radio {
            options: vec!["One".into(), "Two".into()],
        },
        DesignFieldKind::Dropdown {
            options: vec!["Finland".into(), "UK".into()],
        },
        DesignFieldKind::Date,
    ]
    .into_iter()
    .enumerate()
    {
        e.design_form(
            &mut doc,
            FormDesignEdit::Add(NewFormField {
                page: 0,
                name: format!("Field{i}"),
                required: true,
                bounds: Bounds {
                    left: 30.,
                    bottom: 30. + i as f32 * 45.,
                    right: 200.,
                    top: 65. + i as f32 * 45.,
                },
                kind,
            }),
        )
        .unwrap();
    }
    assert_eq!(e.designed_fields(&doc).unwrap().len(), 6);
    let fields = e.designed_fields(&doc).unwrap();
    assert!(fields.iter().all(|f| f.required));
    let mut moved = fields[0].bounds;
    moved.left += 12.;
    moved.right += 35.;
    moved.top += 8.;
    e.design_form(
        &mut doc,
        FormDesignEdit::Update {
            field: fields[0].clone(),
            name: "Renamed".into(),
            required: false,
            bounds: moved,
        },
    )
    .unwrap();
    assert!(
        e.design_form(&mut doc, FormDesignEdit::Delete(fields[0].clone()))
            .is_err()
    );
    let current = e.designed_fields(&doc).unwrap();
    assert_eq!(current[0].bounds, moved);
    assert!(!current[0].required);
    for (index, value) in [
        (0, FieldValue::Text("Example".into())),
        (1, FieldValue::Check(true)),
        (2, FieldValue::Radio(true)),
        (3, FieldValue::Radio(true)),
        (
            4,
            FieldValue::Choice {
                options: vec![],
                selected: Some(1),
            },
        ),
        (5, FieldValue::Text("2026-10-23".into())),
    ] {
        let fields = e.form_fields(&doc).unwrap();
        let before = e.render(&doc, 0, 1.).unwrap().rgba;
        e.fill_field(&mut doc, &fields[index], &value).unwrap();
        assert_ne!(before, e.render(&doc, 0, 1.).unwrap().rgba, "field {index}");
    }
    let fields = e.form_fields(&doc).unwrap();
    assert_eq!(fields[2].value, FieldValue::Radio(false));
    assert_eq!(fields[3].value, FieldValue::Radio(true));
    let output = temp("designed-filled.pdf");
    let _ = fs::remove_file(&output);
    doc.save_as(&output).unwrap();
    let reopened = e.open(&output).unwrap();
    assert_eq!(
        e.render(&doc, 0, 1.).unwrap().rgba,
        e.render(&reopened, 0, 1.).unwrap().rgba
    );
    let field = e.designed_fields(&doc).unwrap()[0].clone();
    let before = doc.bytes.clone();
    e.design_form(&mut doc, FormDesignEdit::Delete(field))
        .unwrap();
    assert_eq!(e.designed_fields(&doc).unwrap().len(), 5);
    assert!(doc.undo());
    assert_eq!(doc.bytes, before);
    let before = doc.bytes.clone();
    assert!(
        e.design_form(
            &mut doc,
            FormDesignEdit::Add(NewFormField {
                page: 0,
                name: "Invalid".into(),
                required: false,
                bounds: Bounds {
                    left: f32::NAN,
                    bottom: 0.,
                    right: 100.,
                    top: 40.
                },
                kind: DesignFieldKind::Text
            })
        )
        .is_err()
    );
    assert_eq!(doc.bytes, before);
}

#[path = "form_flatten_tests.rs"]
mod form_flatten_tests;

#[test]
fn scan_cleanup_is_atomic_undoable_and_keeps_other_pages() {
    let _lock = PDFIUM.lock().unwrap();
    let engine = engine();
    let mut photo = Picture {
        width: 100,
        height: 80,
        rgba: [30, 30, 30, 255].repeat(100 * 80),
    };
    for y in 8..72 {
        for x in 10..90 {
            let i = (y * 100 + x) * 4;
            photo.rgba[i..i + 4].copy_from_slice(&[240, 230, 200, 255]);
        }
    }
    let other = Picture {
        width: 40,
        height: 40,
        rgba: [20, 40, 220, 255].repeat(40 * 40),
    };
    let mut doc = engine
        .pictures_to_pdf([Ok(photo), Ok(other)], ImagePageSize::Image)
        .unwrap();
    let original = doc.bytes.clone();
    let neighbor = engine.render(&doc, 1, 2.0).unwrap().rgba;
    let options = ScanOptions {
        corners: [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
        color: ScanColor::Grayscale,
        ..Default::default()
    };
    engine.cleanup_scan(&mut doc, 0, &options).unwrap();
    assert_eq!(engine.render(&doc, 1, 2.0).unwrap().rgba, neighbor);
    let info = engine.pages(&doc).unwrap();
    assert!((info[0].width - 60.0).abs() < 1.0);
    assert!((info[0].height - 48.0).abs() < 1.0);
    assert_eq!(engine.page_items(&doc, 0).unwrap().len(), 1);
    let cleaned = doc.bytes.clone();
    let invalid = ScanOptions {
        corners: [[0.0, 0.0]; 4],
        ..Default::default()
    };
    assert!(engine.cleanup_scan(&mut doc, 0, &invalid).is_err());
    assert_eq!(doc.bytes, cleaned);
    assert!(doc.undo());
    assert_eq!(doc.bytes, original);
    assert!(doc.redo());
    assert_eq!(doc.bytes, cleaned);
    let output = temp("cleaned-photo.pdf");
    if output.exists() {
        fs::remove_file(&output).unwrap();
    }
    doc.save_as(&output).unwrap();
    assert_eq!(
        engine.pages(&engine.open(&output).unwrap()).unwrap().len(),
        2
    );
    engine
        .add_annotation(
            &mut doc,
            &Markup {
                page: 0,
                kind: MarkupKind::Note,
                bounds: Bounds {
                    left: 5.0,
                    bottom: 5.0,
                    right: 10.0,
                    top: 10.0,
                },
                color: [255, 200, 0],
                opacity: 1.0,
                text: "Keep this note".into(),
            },
        )
        .unwrap();
    assert!(engine.scan_preview(&doc, 0).is_err());
    let text_path = temp("scan-text-guard.pdf");
    fs::write(
        &text_path,
        generated("Keep text", false, 0, 0.0, false, false),
    )
    .unwrap();
    assert!(
        engine
            .scan_preview(&engine.open(&text_path).unwrap(), 0)
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn generated_heic_round_trip_when_windows_encoder_available() {
    use windows::{
        Graphics::Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat},
        Storage::Streams::{DataReader, InMemoryRandomAccessStream},
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    };
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.unwrap();
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let encode = || -> windows::core::Result<Vec<u8>> {
        let stream = InMemoryRandomAccessStream::new()?;
        let encoder =
            BitmapEncoder::CreateAsync(BitmapEncoder::HeifEncoderId()?, &stream)?.join()?;
        encoder.SetPixelData(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Ignore,
            64,
            64,
            96.0,
            96.0,
            &[200, 40, 20, 255].repeat(64 * 64),
        )?;
        encoder.FlushAsync()?.join()?;
        let size = stream.Size()?;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size as u32)?.join()?;
        let mut bytes = vec![0; size as usize];
        reader.ReadBytes(&mut bytes)?;
        Ok(bytes)
    };
    let bytes = match encode() {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "HEIC positive test unavailable: Windows encoder HRESULT {:?}",
                error.code()
            );
            return;
        }
    };
    let path = temp("generated-native.heic");
    fs::write(&path, bytes).unwrap();
    let picture = import_picture(&path).unwrap();
    assert_eq!((picture.width, picture.height), (64, 64));
    assert!(picture.rgba[0] > 180 && picture.rgba[1] < 60 && picture.rgba[2] < 40);
}

#[cfg(windows)]
#[test]
fn scan_cleanup_ocr_on_generated_photo_when_language_available() {
    let _lock = PDFIUM.lock().unwrap();
    let engine = engine();
    let source = temp("scan-ocr-source.pdf");
    fs::write(
        &source,
        generated("Scan cleanup", false, 0, 0.0, false, false),
    )
    .unwrap();
    let text = engine.open(&source).unwrap();
    let image = engine.render(&text, 0, 2.5).unwrap();
    let mut doc = engine
        .pictures_to_pdf(
            [Ok(Picture {
                width: image.width,
                height: image.height,
                rgba: image.rgba,
            })],
            ImagePageSize::Image,
        )
        .unwrap();
    let original = doc.bytes.clone();
    match engine.cleanup_scan(
        &mut doc,
        0,
        &ScanOptions {
            ocr: true,
            ..Default::default()
        },
    ) {
        Ok(()) => {
            let recognized = engine
                .text_objects(&doc, 0)
                .unwrap()
                .into_iter()
                .map(|t| t.text)
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                recognized.to_lowercase().contains("scan"),
                "Generated scan text was not recognized"
            );
            assert!(doc.undo());
            assert_eq!(doc.bytes, original);
        }
        Err(error) if error.0.contains("Windows OCR is unavailable") => {
            eprintln!("Optional OCR test unavailable: {error}");
            assert_eq!(doc.bytes, original);
        }
        Err(error) => panic!("Generated photo OCR failed: {error}"),
    }
}

#[test]
fn scan_cleanup_resets_inherited_rotation_and_crop() {
    let _lock = PDFIUM.lock().unwrap();
    let engine = engine();
    for rotation in [90, 180, 270] {
        let mut doc = engine.pictures_to_pdf([Ok(Picture {
            width: 80, height: 60, rgba: [200, 100, 30, 255].repeat(80 * 60),
        })], ImagePageSize::Image).unwrap();
        let mut pdf = parse(&doc.bytes).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
        let parent = pdf.get_dictionary_mut(parent).unwrap();
        parent.set("Rotate", rotation);
        parent.set("CropBox", vec![0.into(), 0.into(), 60.into(), 45.into()]);
        doc = engine.candidate(&doc, pdf).unwrap();
        let before = engine.pages(&doc).unwrap()[0].clone();
        engine.cleanup_scan(&mut doc, 0, &ScanOptions::default()).unwrap();
        let after = engine.pages(&doc).unwrap()[0].clone();
        assert!((before.width - after.width).abs() < 1.0);
        assert!((before.height - after.height).abs() < 1.0);
        let pdf = parse(&doc.bytes).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        assert_eq!(pdf.get_dictionary(page).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 0);
    }
}

#[test]
fn sanitize_removes_active_payloads_hidden_text_and_restores_undo() {
    let _lock = PDFIUM.lock().unwrap();
    let engine = engine();
    let mut pdf = parse(&generated("secret", false, 0, 0.0, false, false)).unwrap();
    let page = pdf.get_pages()[&1];
    let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let mut content = Content::decode(&pdf.get_page_content(page)).unwrap();
    content
        .operations
        .insert(3, Operation::new("Tr", vec![3.into()]));
    let stream = pdf.add_object(Stream::new(
        lopdf::Dictionary::new(),
        content.encode().unwrap(),
    ));
    pdf.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    let metadata = pdf.add_object(Stream::new(
        dictionary! {"Type"=>"Metadata", "Subtype"=>"XML"},
        b"PRIVATE_METADATA".to_vec(),
    ));
    let info = pdf.add_object(dictionary! {"Author"=>Object::string_literal("PRIVATE_AUTHOR")});
    pdf.trailer.set("Info", info);
    let payload = pdf.add_object(Stream::new(dictionary! {}, b"PRIVATE_ATTACHMENT".to_vec()));
    let attachment = pdf.add_object(dictionary!{"Type"=>"Filespec", "F"=>Object::string_literal("private.txt"), "EF"=>dictionary!{"F"=>payload}});
    let action = pdf.add_object(dictionary!{"S"=>"JavaScript", "JS"=>Object::string_literal("PRIVATE_SCRIPT"), "Next"=>dictionary!{"S"=>"Launch", "F"=>Object::string_literal("never.exe")}});
    let catalog = pdf.get_object_mut(root).unwrap().as_dict_mut().unwrap();
    catalog.set("Metadata", metadata);
    catalog.set("OpenAction", action);
    catalog.set("Names", dictionary!{"EmbeddedFiles"=>dictionary!{"Names"=>vec![Object::string_literal("private"),attachment.into()]}, "JavaScript"=>dictionary!{"Names"=>vec![Object::string_literal("script"),action.into()]}});
    catalog.set("AF", vec![Object::Reference(attachment)]);
    catalog.set("CustomAction", action);
    let link = pdf.add_object(dictionary!{"Type"=>"Annot", "Subtype"=>"Link", "Rect"=>vec![0.into(),0.into(),10.into(),10.into()], "A"=>dictionary!{"S"=>"URI","URI"=>Object::string_literal("https://invalid.example")}});
    let comment = pdf.add_object(dictionary!{"Type"=>"Annot", "Subtype"=>"Text", "Rect"=>vec![0.into(),0.into(),10.into(),10.into()], "Contents"=>Object::string_literal("PRIVATE_COMMENT")});
    let file = pdf.add_object(dictionary!{"Type"=>"Annot", "Subtype"=>"FileAttachment", "Rect"=>vec![0.into(),0.into(),10.into(),10.into()], "FS"=>attachment});
    pdf.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", vec![link.into(), comment.into(), file.into()]);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    let path = temp("sanitize-input.pdf");
    fs::write(&path, &bytes).unwrap();
    let mut doc = engine.open(&path).unwrap();
    let before = engine.render(&doc, 0, 1.0).unwrap();
    let report = engine
        .sanitize(
            &mut doc,
            SanitizeOptions {
                remove_comments: true,
                remove_links: true,
            },
        )
        .unwrap();
    assert!(report.metadata >= 2 && report.attachments >= 2 && report.actions >= 2);
    assert_eq!(report.hidden_text_blocks, 1);
    assert_eq!((report.comments, report.links), (1, 1));
    assert!(
        !engine
            .text_objects(&doc, 0)
            .unwrap()
            .iter()
            .any(|o| o.text.contains("secret"))
    );
    let after = engine.render(&doc, 0, 1.0).unwrap();
    // Annotation appearances may change, so compare the text region only.
    for y in 50..250 {
        for x in 30..450 {
            let at = (y * before.width as usize + x) * 4;
            assert_eq!(&before.rgba[at..at + 4], &after.rgba[at..at + 4]);
        }
    }
    for marker in [
        b"PRIVATE_METADATA".as_slice(),
        b"PRIVATE_AUTHOR",
        b"PRIVATE_ATTACHMENT",
        b"PRIVATE_SCRIPT",
        b"PRIVATE_COMMENT",
        b"never.exe",
    ] {
        assert!(!doc.bytes.windows(marker.len()).any(|s| s == marker));
    }
    let output = temp("sanitize-output.pdf");
    fs::write(&output, &doc.bytes).unwrap();
    let reopened = engine.open(&output).unwrap();
    assert!(
        !engine
            .text_objects(&reopened, 0)
            .unwrap()
            .iter()
            .any(|o| o.text.contains("secret"))
    );
    let cleaned = doc.bytes.clone();
    assert!(doc.undo());
    assert_eq!(doc.bytes, bytes);
    assert!(doc.redo());
    assert_eq!(doc.bytes, cleaned);
    assert!(doc.undo());
    let report = engine
        .sanitize(&mut doc, SanitizeOptions::default())
        .unwrap();
    assert_eq!((report.comments, report.links), (0, 0));
    let saved = parse(&doc.bytes).unwrap();
    let page = saved.get_pages()[&1];
    let annotations = saved
        .get_object(page)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(annotations.len(), 2);
    let link = saved
        .dereference(&annotations[0])
        .unwrap()
        .1
        .as_dict()
        .unwrap();
    let action = link.get(b"A").unwrap().as_dict().unwrap();
    assert_eq!(action.get(b"S").unwrap().as_name().unwrap(), b"URI");
    assert_eq!(
        action.get(b"URI").unwrap().as_str().unwrap(),
        b"https://invalid.example"
    );
}

#[test]
fn sanitize_rejects_ambiguous_visibility_without_changes() {
    let _lock = PDFIUM.lock().unwrap();
    let engine = engine();
    let mut pdf = parse(&generated("secret", false, 0, 0.0, false, false)).unwrap();
    let page = pdf.get_pages()[&1];
    let mut content = Content::decode(&pdf.get_page_content(page)).unwrap();
    let first = content
        .operations
        .iter()
        .position(|op| op.operator == "Tj")
        .unwrap();
    content
        .operations
        .insert(first + 1, Operation::new("Tr", vec![3.into()]));
    let stream = pdf.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    pdf.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    let mut bytes = vec![];
    pdf.save_to(&mut bytes).unwrap();
    let path = temp("sanitize-rejected.pdf");
    fs::write(&path, &bytes).unwrap();
    let mut doc = engine.open(&path).unwrap();
    assert!(
        engine
            .sanitize(&mut doc, SanitizeOptions::default())
            .is_err()
    );
    assert_eq!(doc.bytes, bytes);
    assert!(!doc.undo());
}

#[test]
fn sanitize_rejects_shared_resources_appearances_unused_forms_and_payload_aliases() {
    let _lock = PDFIUM.lock().unwrap();
    let engine = engine();
    for case in [
        "shared",
        "appearance",
        "unused",
        "attachment",
        "script",
        "chain",
        "ef-alias",
        "xfa",
        "info-alias",
        "metadata-alias",
    ] {
        let mut pdf = parse(&generated("secret", false, 0, 0.0, false, false)).unwrap();
        let page = pdf.get_pages()[&1];
        let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let resources = pdf
            .get_object(page)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .clone();
        if matches!(case, "info-alias" | "metadata-alias") {
            let payload = if case == "info-alias" {
                pdf.add_object(dictionary! {"Author"=>Object::string_literal("PRIVATE_AUTHOR")})
            } else {
                pdf.add_object(Stream::new(dictionary! {}, b"PRIVATE_METADATA".to_vec()))
            };
            if case == "info-alias" {
                pdf.trailer.set("Info", payload);
            } else {
                pdf.get_object_mut(root)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap()
                    .set("Metadata", payload);
            }
            pdf.get_object_mut(root)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("CustomPayload", payload);
        } else if case == "xfa" {
            let packet = pdf.add_object(Stream::new(
                dictionary! {},
                b"<script contentType='application/x-javascript'>PRIVATE_SCRIPT</script>".to_vec(),
            ));
            pdf.get_object_mut(root)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("AcroForm", dictionary! {"XFA"=>packet});
        } else if matches!(case, "shared" | "appearance" | "unused") {
            let mut dict = dictionary! {"Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),500.into(),300.into()]};
            let mut content = Content::decode(&pdf.get_page_content(page)).unwrap();
            if case == "shared" {
                content.operations.insert(
                    0,
                    Operation::new("gs", vec![Object::Name(b"Alpha".to_vec())]),
                );
            } else {
                dict.set("Resources", resources.clone());
                content
                    .operations
                    .insert(0, Operation::new("Tr", vec![3.into()]));
            }
            let form = pdf.add_object(Stream::new(dict, content.encode().unwrap()));
            if case == "appearance" {
                let annotation = pdf.add_object(dictionary! {"Type"=>"Annot", "Subtype"=>"Text", "Rect"=>vec![0.into(),0.into(),500.into(),300.into()], "AP"=>dictionary!{"N"=>form}});
                pdf.get_object_mut(page)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap()
                    .set("Annots", vec![Object::Reference(annotation)]);
            } else {
                let mut resources = resources.as_dict().unwrap().clone();
                resources.set("XObject", dictionary! {"Shared"=>form});
                resources.set("ExtGState", dictionary! {"Alpha"=>dictionary!{"ca"=>0}});
                pdf.get_object_mut(page)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap()
                    .set("Resources", resources.clone());
                if case == "shared" {
                    let invocation =
                        pdf.add_object(Stream::new(dictionary! {}, b"/Shared Do".to_vec()));
                    pdf.get_object_mut(page)
                        .unwrap()
                        .as_dict_mut()
                        .unwrap()
                        .set("Contents", invocation);
                    let mut second = pdf.get_object(page).unwrap().as_dict().unwrap().clone();
                    resources.set("ExtGState", dictionary! {"Alpha"=>dictionary!{"ca"=>1}});
                    second.set("Resources", resources);
                    let parent = second.get(b"Parent").unwrap().as_reference().unwrap();
                    let second = pdf.add_object(second);
                    let pages = pdf.get_object_mut(parent).unwrap().as_dict_mut().unwrap();
                    pages.set(
                        "Kids",
                        vec![Object::Reference(page), Object::Reference(second)],
                    );
                    pages.set("Count", 2);
                }
            }
        } else {
            let payload = pdf.add_object(Stream::new(dictionary! {}, b"PRIVATE_PAYLOAD".to_vec()));
            let chain = if case == "chain" {
                pdf.add_object(Object::Reference(payload))
            } else {
                payload
            };
            let ef = pdf.add_object(dictionary! {"F"=>chain});
            let owner = if matches!(case, "attachment" | "chain" | "ef-alias") {
                pdf.add_object(dictionary! {"Type"=>"Filespec", "EF"=>ef})
            } else {
                pdf.add_object(dictionary! {"S"=>"JavaScript", "JS"=>payload})
            };
            let catalog = pdf.get_object_mut(root).unwrap().as_dict_mut().unwrap();
            catalog.set(
                "CustomPayload",
                if case == "ef-alias" { ef } else { payload },
            );
            if matches!(case, "attachment" | "chain" | "ef-alias") {
                catalog.set("AF", vec![Object::Reference(owner)]);
            } else {
                catalog.set("OpenAction", owner);
            }
        }
        let mut bytes = vec![];
        pdf.save_to(&mut bytes).unwrap();
        let path = temp(&format!("sanitize-{case}.pdf"));
        fs::write(&path, &bytes).unwrap();
        let mut doc = engine.open(&path).unwrap();
        if case == "shared" {
            // A resource-dependent shared Form must be rejected even when an extractor
            // cannot enumerate its nested text as top-level editable objects.
            assert_eq!(engine.pages(&doc).unwrap().len(), 2);
        }
        let error = engine
            .sanitize(&mut doc, SanitizeOptions::default())
            .unwrap_err();
        let expected = match case {
            "shared" => "inherited resources",
            "appearance" => "annotation appearances",
            "unused" => "unused retained",
            "xfa" => "XFA forms",
            _ => "aliased",
        };
        assert!(error.to_string().contains(expected), "{case}: {error}");
        assert_eq!(doc.bytes, bytes, "{case}");
        assert!(!doc.undo(), "{case}");
    }
}

#[test]
fn privacy_preview_modes_and_stale_apply() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    for (n, query, text) in [
        (0, PrivacyQuery::Text("secret".into()), "secret "),
        (1, PrivacyQuery::Email, "a@b.fi "),
        (2, PrivacyQuery::Phone, "+358401234567 "),
        (3, PrivacyQuery::Iban, "FI2112345600000785 "),
        (4, PrivacyQuery::FinnishId, "131052-308T "),
        (5, PrivacyQuery::CreditCard, "4111111111111111 "),
    ] {
        let path = temp(&format!("privacy-{n}.pdf"));
        let mut pdf = parse(&generated(text, false, 0, 0.0, false, false)).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let mut content = Content::decode(&pdf.get_page_content(page)).unwrap();
        let index = content
            .operations
            .iter()
            .position(|o| o.operator == "Tj")
            .unwrap();
        let bytes = content.operations[index].operands[0]
            .as_str()
            .unwrap()
            .to_vec();
        let middle = (bytes.len() / 4) * 2;
        content.operations[index].operands[0] =
            Object::String(bytes[..middle].to_vec(), lopdf::StringFormat::Hexadecimal);
        content.operations.insert(
            index + 1,
            Operation::new(
                "Tj",
                vec![Object::String(
                    bytes[middle..].to_vec(),
                    lopdf::StringFormat::Hexadecimal,
                )],
            ),
        );
        let stream = pdf.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        pdf.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
        pdf.save(&path).unwrap();
        let mut doc = e.open(&path).unwrap();
        let original = doc.bytes.clone();
        let preview = e.preview_redactions(&doc, &query).unwrap();
        assert!(!preview.matches.is_empty(), "{query:?}");
        assert!(
            preview
                .matches
                .iter()
                .all(|m| m.page == 0 && !m.bounds.is_empty())
        );
        let other_path = temp(&format!("privacy-other-{n}.pdf"));
        fs::write(
            &other_path,
            generated("other document", false, 0, 0.0, false, false),
        )
        .unwrap();
        let mut other = e.open(&other_path).unwrap();
        let other_bytes = other.bytes.clone();
        assert!(e.apply_redaction_preview(&mut other, &preview).is_err());
        assert_eq!(other.bytes, other_bytes);
        e.apply_redaction_preview(&mut doc, &preview).unwrap();
        assert!(
            e.find_text(&doc, text.trim()).unwrap().is_empty(),
            "{query:?}"
        );
        let changed = doc.bytes.clone();
        assert!(e.apply_redaction_preview(&mut doc, &preview).is_err());
        assert_eq!(changed, doc.bytes);
        doc.undo();
        assert_eq!(original, doc.bytes);
    }
}

#[test]
fn bates_numbers_keep_padding_ranges_and_atomic_overflow() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let mut doc = e.open(&fixture("multi-page.pdf")).unwrap();
    let original = doc.bytes.clone();
    let options = RangeStamp {
        stamp: TextStamp {
            template: "unused\n".into(),
            font: "Arial".into(),
            size: 12.,
            color: [0, 0, 0],
            opacity: 1.,
            x: 30.,
            y: 20.,
        },
        pages: "2,1".into(),
        date: String::new(),
        placement: StampPlacement::Footer,
    };
    let mut bates = BatesNumber {
        prefix: "CASE-".into(),
        start: 9,
        digits: 4,
    };
    e.stamp_bates(&mut doc, &options, &bates).unwrap();
    for (page, label) in [(0, "CASE-0009"), (1, "CASE-0010")] {
        assert!(
            e.text_objects(&doc, page)
                .unwrap()
                .iter()
                .any(|t| t.text == label)
        );
    }
    doc.undo();
    assert_eq!(doc.bytes, original);
    doc.redo();
    let saved = temp(&format!(
        "bates-{}.pdf",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    doc.save_as(&saved).unwrap();
    let reopened = e.open(&saved).unwrap();
    assert_eq!(e.find_text(&reopened, "CASE-0009").unwrap().len(), 1);
    assert_eq!(e.find_text(&reopened, "CASE-0010").unwrap().len(), 1);
    doc.undo();
    bates.start = u64::MAX;
    bates.digits = 20;
    assert!(e.stamp_bates(&mut doc, &options, &bates).is_err());
    assert_eq!(doc.bytes, original);
    bates.start = 9999;
    bates.digits = 4;
    assert!(e.stamp_bates(&mut doc, &options, &bates).is_err());
    assert_eq!(doc.bytes, original);
    bates.prefix = "bad\nprefix".into();
    bates.start = 1;
    assert!(e.stamp_bates(&mut doc, &options, &bates).is_err());
    assert_eq!(doc.bytes, original);
    bates.digits = 0;
    assert!(bates.label(0).is_err());
}
