use pdf_editor_core::{Engine, ImagePageSize};
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn invoke(args: Vec<OsString>) -> Output {
    let executable = std::env::var_os("PDF_EDITOR_TEST_CLI")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_pdf-editor-cli").into());
    invoke_at(Path::new(&executable), args)
}

fn invoke_at(executable: &Path, args: Vec<OsString>) -> Output {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("CLI exceeded its processing deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn generated_cli_explains_missing_runtime_without_exposing_paths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let folder = root.join(format!(
        "target/missing-runtime-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ));
    std::fs::create_dir(&folder).unwrap();
    let executable = folder.join(if cfg!(windows) { "pdf-editor-cli.exe" } else { "pdf-editor-cli" });
    std::fs::copy(env!("CARGO_BIN_EXE_pdf-editor-cli"), &executable).unwrap();
    for args in [vec!["--help".into()],vec!["--version".into()],vec!["commands".into(),"--schema".into()]] {
        assert_eq!(invoke_at(&executable, args).status.code(), Some(0));
    }
    let source = root.join("tests/fixtures/generated/multi-page.pdf");
    let original = std::fs::read(&source).unwrap();
    let output = folder.join("new.pdf");
    let result = invoke_at(&executable, vec![
        "rotate".into(), source.clone().into(), "--output".into(), output.clone().into(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    let error = String::from_utf8(result.stderr).unwrap();
    let library = if cfg!(windows) { "pdfium.dll" } else { "libpdfium.so" };
    assert!(error.contains(library), "Missing runtime should identify the required library: {error}");
    assert!(!error.contains(root.to_str().unwrap()));
    assert!(!output.exists());
    assert_eq!(std::fs::read(&source).unwrap(), original);
    std::fs::remove_file(executable).unwrap();
    std::fs::remove_dir(folder).unwrap();
}

fn success(args: Vec<OsString>) -> Output {
    let output = invoke(args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn generated_creation_cli_builds_real_blank_and_cv_files_and_stops_bad_scripts() {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let folder=root.join(format!("target/creation-cli-{}",std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let blank=folder.join("blank.pdf");
    success(vec!["new".into(),"--output".into(),blank.clone().into(),"--size".into(),"letter".into(),"--landscape".into(),"--pages".into(),"2".into()]);
    let engine=Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let doc=engine.open(&blank).unwrap();let pages=engine.pages(&doc).unwrap();
    assert_eq!(pages.len(),2);assert_eq!((pages[0].width,pages[0].height),(792.,612.));
    assert!(engine.text_objects(&doc,0).unwrap().is_empty());
    let wide=folder.join("wide.pdf");
    success(vec!["new".into(),"--output".into(),wide.clone().into(),"--size".into(),"custom".into(),"--width".into(),"792".into(),"--height".into(),"612".into(),"--landscape".into()]);
    let wide_pages=engine.pages(&engine.open(&wide).unwrap()).unwrap();
    assert_eq!((wide_pages[0].width,wide_pages[0].height),(792.,612.));
    let original=std::fs::read(&blank).unwrap();
    assert_eq!(invoke(vec!["new".into(),"--output".into(),blank.clone().into()]).status.code(),Some(1));
    assert_eq!(std::fs::read(&blank).unwrap(),original);
    let script=folder.join("build.jsonl");
    std::fs::write(&script,concat!(
        "{\"op\":\"cv.create\",\"name\":\"Avery Example\",\"contact\":[\"reader@example.org\"],\"sections\":[{\"heading\":\"Experience\",\"body\":\"Built a fictional Rust project.\"}]}\n",
        "{\"op\":\"inspect\",\"view\":\"text\"}\n",
        "{\"op\":\"cv.check\"}\n",
        "{\"op\":\"render\",\"path\":\"CV.png\",\"dpi\":36}\n",
        "{\"op\":\"save\",\"document\":1,\"revision\":0,\"path\":\"CV.pdf\"}\n",
        "{\"op\":\"status\"}\n",
    )).unwrap();
    let result=success(vec!["commands".into(),"--root".into(),folder.clone().into(),"--script".into(),"build.jsonl".into()]);
    let output=String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(),6);assert_eq!(output.matches("\"ok\":true").count(),6);
    assert!(output.lines().last().unwrap().contains("working_copy"));
    let cv=engine.open(&folder.join("CV.pdf")).unwrap();let review=engine.check_cv(&cv).unwrap();
    assert!(review.text.find("Avery Example").unwrap()<review.text.find("reader@example.org").unwrap());
    assert!(review.text.contains("Built a fictional Rust project."));
    assert!(review.warnings.is_empty());assert!(folder.join("CV.png").metadata().unwrap().len()>100);
    std::fs::write(&script,"{\"op\":\"new\"}\n{\"op\":\"text.add\",\"document\":1,\"revision\":99,\"text\":\"Stale\"}\n{\"op\":\"save\",\"path\":\"must-not-exist.pdf\"}\n").unwrap();
    let result=invoke(vec!["commands".into(),"--root".into(),folder.clone().into(),"--script".into(),"build.jsonl".into()]);
    assert_eq!(result.status.code(),Some(1));assert_eq!(String::from_utf8(result.stdout).unwrap().lines().count(),2);
    assert!(!folder.join("must-not-exist.pdf").exists());
    assert_eq!(std::fs::read(blank).unwrap(),original);
    let schema=success(vec!["commands".into(),"--schema".into()]);
    assert!(String::from_utf8(schema.stdout).unwrap().contains("privacy.apply"));
    for name in ["blank.pdf","wide.pdf","CV.pdf","CV.png","build.jsonl"] {std::fs::remove_file(folder.join(name)).unwrap();}
    std::fs::remove_dir(folder).unwrap();
}

#[test]
fn generated_cli_matrix_roundtrips_outputs_and_preserves_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let folder = root.join(format!("target/feature-cli-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let source = folder.join("generated ä & spaces.pdf");
    let original = std::fs::read(root.join("tests/fixtures/generated/multi-page.pdf")).unwrap();
    std::fs::write(&source, &original).unwrap();
    let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    let document = engine.open(&source).unwrap();
    let count = engine.pages(&document).unwrap().len();
    for iteration in 0..3 {
        let out = folder.join(format!("run-{iteration}"));
        std::fs::create_dir(&out).unwrap();
        for (name, options, pages) in [
            ("rotate", vec!["--degrees", "270", "--pages", "2,1"], count),
            ("extract", vec!["--pages", "2,1"], 2),
            ("compress", vec!["--edge", "128", "--quality", "10"], count),
            (
                "find-replace",
                vec!["--find", "Subset", "--replace", "AB"],
                count,
            ),
            (
                "stamp",
                vec!["--text", "Generated {page}/{pages}", "--date", "2026-10-25"],
                count,
            ),
            (
                "bates",
                vec!["--prefix", "CASE-", "--start", "99", "--digits", "4"],
                count,
            ),
        ] {
            let output = out.join(format!("{name}.pdf"));
            let mut args = vec![
                name.into(),
                source.as_os_str().to_owned(),
                "--output".into(),
                output.clone().into(),
            ];
            args.extend(options.into_iter().map(OsString::from));
            success(args.clone());
            let reopened = engine.open(&output).unwrap();
            assert_eq!(engine.pages(&reopened).unwrap().len(), pages);
            for page in 0..pages as u16 {
                engine.render(&reopened, page, 0.5).unwrap();
            }
            if name == "find-replace" {
                assert_eq!(engine.replacement_count(&reopened, "Subset").unwrap(), 0);
                assert_eq!(engine.replacement_count(&reopened, "AB").unwrap(), count);
            }
            let bytes = std::fs::read(&output).unwrap();
            assert_eq!(invoke(args).status.code(), Some(1));
            assert_eq!(std::fs::read(&output).unwrap(), bytes);
        }
        let merged = out.join("merged.pdf");
        success(vec![
            "merge".into(),
            source.clone().into(),
            source.clone().into(),
            "--output".into(),
            merged.clone().into(),
        ]);
        assert_eq!(
            engine.pages(&engine.open(&merged).unwrap()).unwrap().len(),
            count * 2
        );
        success(vec![
            "split".into(),
            source.clone().into(),
            "--pages".into(),
            "2;1".into(),
            "--output".into(),
            out.clone().into(),
        ]);
        for (i, page) in [1, 0].into_iter().enumerate() {
            let split = engine
                .open(&out.join(format!("split-{}.pdf", i + 1)))
                .unwrap();
            assert_eq!(
                engine.render(&split, 0, 0.5).unwrap().rgba,
                engine.render(&document, page, 0.5).unwrap().rgba
            );
        }
        for format in ["png", "jpg"] {
            success(vec![
                "to-image".into(),
                source.clone().into(),
                "--pages".into(),
                "1".into(),
                "--format".into(),
                format.into(),
                "--dpi".into(),
                "72".into(),
                "--output".into(),
                out.clone().into(),
            ]);
            let picture =
                pdf_editor_core::import_picture(&out.join(format!("export-1-page-1.{format}")))
                    .unwrap();
            assert!(picture.width > 0 && picture.height > 0);
        }
        let scanned = engine
            .images_to_pdf(&[out.join("export-1-page-1.png")], ImagePageSize::Image)
            .unwrap();
        let scan_path = out.join("scan.pdf");
        scanned.save_as(&scan_path).unwrap();
        let ocr_path = out.join("ocr.pdf");
        let result = invoke(vec![
            "ocr".into(),
            scan_path.into(),
            "--output".into(),
            ocr_path.clone().into(),
        ]);
        if result.status.code() == Some(1) {
            let error = String::from_utf8_lossy(&result.stderr);
            assert!(
                error.contains("Windows OCR is unavailable")
                    || error.contains("OCR requires Windows"),
                "{error}"
            );
            assert!(!ocr_path.exists());
            eprintln!("OCR language/platform unavailable; failure left no output");
        } else {
            assert_eq!(result.status.code(), Some(0));
            assert!(
                !engine
                    .text_objects(&engine.open(&ocr_path).unwrap(), 0)
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(std::fs::read(&source).unwrap(), original);
    }
}

#[test]
fn generated_cli_rejects_invalid_arguments_and_rolls_back_multi_outputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let folder = root.join(format!("target/invalid-cli-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let source = root.join("tests/fixtures/generated/multi-page.pdf");
    let output = folder.join("new.pdf");
    for (name, options) in [
        ("rotate", vec!["--degrees", "0"]),
        ("rotate", vec!["--degrees", "NaN"]),
        ("compress", vec!["--quality", "100"]),
        ("compress", vec!["--edge", "0"]),
        ("to-image", vec!["--dpi", "65535"]),
        ("to-image", vec!["--format", "svg"]),
        ("find-replace", vec!["--find", "", "--replace", "AB"]),
        ("bates", vec!["--digits", "0"]),
        ("bates", vec!["--start", "-1"]),
        ("stamp", vec!["--opacity", "NaN"]),
        ("stamp", vec!["--position", "unknown"]),
    ] {
        let mut args = vec![
            name.into(),
            source.clone().into(),
            "--output".into(),
            output.clone().into(),
        ];
        args.extend(options.into_iter().map(OsString::from));
        assert_eq!(invoke(args).status.code(), Some(2));
        assert!(!output.exists());
    }
    assert_eq!(
        invoke(vec![
            "split".into(),
            source.clone().into(),
            "--pages".into(),
            "1;9999".into(),
            "--output".into(),
            folder.clone().into()
        ])
        .status
        .code(),
        Some(1)
    );
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    std::fs::write(
        folder.join("export-2-page-2.png"),
        b"existing generated output",
    )
    .unwrap();
    assert_eq!(
        invoke(vec![
            "to-image".into(),
            source.into(),
            "--pages".into(),
            "1-2".into(),
            "--output".into(),
            folder.clone().into()
        ])
        .status
        .code(),
        Some(1)
    );
    assert!(!folder.join("export-1-page-1.png").exists());
    assert_eq!(
        std::fs::read(folder.join("export-2-page-2.png")).unwrap(),
        b"existing generated output"
    );
}
