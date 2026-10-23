use std::{ffi::OsString, path::Path, process::Command};

#[test]
fn stamping_cli_batch_exit_codes_and_outputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let base = root.join(format!(
        "target/stamp-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input = base.join("input");
    let output = base.join("output");
    std::fs::create_dir_all(&input).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    let source = std::fs::read(root.join("tests/fixtures/generated/multi-page.pdf")).unwrap();
    for name in ["a.pdf", "b.pdf"] {
        std::fs::write(input.join(name), &source).unwrap();
    }
    let invoke = |args: Vec<OsString>| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdf-editor-cli"));
        cmd.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        cmd.output().unwrap()
    };
    let result = invoke(vec![
        "stamp".into(),
        input.as_os_str().into(),
        "--text".into(),
        "Header {date} {page}/{pages}".into(),
        "--date".into(),
        "2026-10-23".into(),
        "--position".into(),
        "header".into(),
        "--dry-run".into(),
    ]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
    let args: Vec<OsString> = vec![
        "bates".into(),
        input.as_os_str().into(),
        "--output".into(),
        output.as_os_str().into(),
        "--pages".into(),
        "2".into(),
        "--prefix".into(),
        "B-".into(),
        "--start".into(),
        "9".into(),
        "--digits".into(),
        "4".into(),
    ];
    let result = invoke(args.clone());
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let engine = pdf_editor_core::Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
    for (name, label) in [("a.pdf", "B-0009"), ("b.pdf", "B-0010")] {
        let doc = engine.open(&output.join(name)).unwrap();
        assert_eq!(engine.find_text(&doc, label).unwrap().len(), 1);
        assert_eq!(std::fs::read(input.join(name)).unwrap(), source);
    }
    let before = std::fs::read(output.join("a.pdf")).unwrap();
    assert_eq!(invoke(args).status.code(), Some(1));
    assert_eq!(std::fs::read(output.join("a.pdf")).unwrap(), before);
    assert_eq!(
        invoke(vec![
            "bates".into(),
            input.as_os_str().into(),
            "--digits".into(),
            "0".into(),
            "--dry-run".into()
        ])
        .status
        .code(),
        Some(2)
    );
    std::fs::write(input.join("broken.pdf"), b"not a PDF").unwrap();
    let partial = base.join("partial");
    std::fs::create_dir_all(&partial).unwrap();
    assert_eq!(
        invoke(vec![
            "stamp".into(),
            input.as_os_str().into(),
            "--output".into(),
            partial.as_os_str().into()
        ])
        .status
        .code(),
        Some(1)
    );
    assert!(partial.join("a.pdf").is_file() && partial.join("b.pdf").is_file());
    assert!(!partial.join("broken.pdf").exists());
}
