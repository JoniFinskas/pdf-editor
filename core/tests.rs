use super::*;
use lopdf::{
    Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use std::sync::Mutex;
static PDFIUM: Mutex<()> = Mutex::new(());
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
            .contains("Encrypted")
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
    assert!(e.open(&path).err().unwrap().0.contains("Encrypted"));
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
