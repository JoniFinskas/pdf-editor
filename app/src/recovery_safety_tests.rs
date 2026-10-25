// Recovery boundary checks use generated PDFs and isolated runtime folders.
fn recovery_safety_folder(label: &str) -> PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join(format!(
        "target/recovery-safety-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn recovery_cleanup_preserves_saved_pdfs_and_unrecognized_subdirectories() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let original = std::fs::read(root.join("tests/fixtures/generated/unicode-cid.pdf")).unwrap();
    for mode in 0..3 {
        let folder = recovery_safety_folder(&format!("cleanup-{mode}"));
        let mut session = Session::start_in(&folder).unwrap();
        let snapshot = session.next_path();
        let dir = snapshot.parent().unwrap().to_path_buf();
        if mode != 2 {
            std::fs::write(&snapshot, &original).unwrap();
            std::fs::write(snapshot.with_extension("history"), b"generated sidecar").unwrap();
            session
                .written(snapshot.clone(), std::path::Path::new("generated.pdf"))
                .unwrap();
        }
        let output = dir.join("saved-copy.pdf");
        std::fs::write(&output, &original).unwrap();
        let lookalike = dir.join("snapshot-1_edited.pdf");
        std::fs::write(&lookalike, &original).unwrap();
        let nested = dir.join("saved outputs");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("another.pdf"), &original).unwrap();
        if mode == 0 {
            session.close();
        } else {
            drop(session);
            let found = recovery::orphans_in(&folder);
            if mode == 1 {
                assert_eq!(found.len(), 1);
                found[0].discard();
            } else {
                assert!(found.is_empty());
            }
        }
        assert!(
            output.is_file(),
            "Recovery cleanup removed a saved PDF in mode {mode}"
        );
        assert_eq!(std::fs::read(&output).unwrap(), original);
        assert_eq!(std::fs::read(&lookalike).unwrap(), original);
        assert_eq!(std::fs::read(nested.join("another.pdf")).unwrap(), original);
        assert!(!snapshot.exists());
        assert!(!snapshot.with_extension("history").exists());
        std::fs::remove_dir_all(folder).unwrap();
    }
}

#[test]
fn recovery_save_rejects_private_storage_before_dispatch_and_keeps_edits() {
    let (mut app, ctx, fixture) = qol_worker();
    let folder = recovery_safety_folder("save");
    app.session = Some(Session::start_in(&folder).unwrap());
    let objects = app.doc.as_ref().unwrap().pages[0]
        .objects
        .as_ref()
        .unwrap()
        .lines[0]
        .objects
        .clone();
    app.busy += 1;
    app.send(Request::Edit {
        objects,
        text: "AB".into(),
    });
    workflow_settle(&mut app, &ctx);
    let snapshot = app.session.as_mut().unwrap().next_path();
    for encrypted in [false, true] {
        let path = snapshot.parent().unwrap().join(if encrypted {
            "protected.pdf"
        } else {
            "plain.pdf"
        });
        let request = if encrypted {
            Request::SavePassword(path.clone(), "generated-password".into())
        } else {
            Request::SaveAs(path.clone())
        };
        app.password_action(request);
        assert_eq!(app.busy, 0, "Recovery storage save reached the worker");
        assert!(!path.exists());
        assert!(app.doc.as_ref().unwrap().dirty());
        assert!(app.qol.working_copy.is_none());
    }
    assert!(app.banners.iter().any(|b| b.text.contains("recovery")));
    let output = folder.join("kept-copy.pdf");
    app.password_action(Request::SaveAs(output.clone()));
    workflow_settle(&mut app, &ctx);
    assert!(!app.doc.as_ref().unwrap().dirty());
    eframe::App::on_exit(&mut app, None);
    let engine = pdf_editor_core::Engine::new(
        &fixture
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target/debug/pdfium.dll"),
    )
    .unwrap();
    let saved = engine.open(&output).unwrap();
    assert_eq!(engine.text_objects(&saved, 0).unwrap()[0].text, "AB");
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn recovery_without_source_metadata_suggests_a_filename_outside_private_storage() {
    let (mut app, ctx, fixture) = qol_worker();
    let folder = recovery_safety_folder("missing-source");
    let mut crashed = Session::start_in(&folder).unwrap();
    let snapshot = crashed.next_path();
    std::fs::copy(&fixture, &snapshot).unwrap();
    drop(crashed);
    let orphan = recovery::orphans_in(&folder).remove(0);
    assert!(orphan.source.is_none());
    app.restoring = Some(orphan);
    app.open_path(snapshot.clone());
    workflow_settle(&mut app, &ctx);
    assert_eq!(
        app.doc.as_ref().unwrap().suggested,
        PathBuf::from("Recovered_edited.pdf")
    );
    let output = folder.join("restored.pdf");
    app.password_action(Request::SaveAs(output.clone()));
    workflow_settle(&mut app, &ctx);
    assert!(output.exists());
    assert!(!snapshot.exists());
    std::fs::remove_dir_all(folder).unwrap();
}

#[cfg(windows)]
fn generated_recovery_junction(link: &std::path::Path, target: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    let result=std::process::Command::new("powershell.exe")
        .args(["-NoProfile","-NonInteractive","-Command",
            "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:PDF_EDITOR_TEST_LINK -Value $env:PDF_EDITOR_TEST_TARGET | Out-Null"])
        .env("PDF_EDITOR_TEST_LINK",link).env("PDF_EDITOR_TEST_TARGET",target)
        .creation_flags(0x08000000).output().unwrap();
    assert!(result.status.success(), "Generated junction setup failed: {}",String::from_utf8_lossy(&result.stderr));
}

#[cfg(windows)]
#[test]
fn recovery_directory_junctions_preserve_external_files_and_reject_save_aliases() {
    let (mut app, ctx, fixture) = qol_worker();
    let folder = recovery_safety_folder("junctions");
    let private = folder.join("recovery");
    let external = folder.join("external");
    std::fs::create_dir(&external).unwrap();
    let external_pdf = external.join("snapshot-99.pdf");
    std::fs::copy(&fixture, &external_pdf).unwrap();
    std::fs::write(
        external_pdf.with_extension("history"),
        b"external generated history",
    )
    .unwrap();
    let mut session = Session::start_in(&private).unwrap();
    let snapshot = session.next_path();
    std::fs::copy(&fixture, &snapshot).unwrap();
    session.written(snapshot.clone(), &fixture).unwrap();
    let attachment = snapshot.parent().unwrap().join("attachment");
    generated_recovery_junction(&attachment, &external);
    let alias = folder.join("alias");
    generated_recovery_junction(&alias, snapshot.parent().unwrap());
    app.session = Some(session);
    for encrypted in [false, true] {
        let output = alias.join(if encrypted {
            "encrypted.pdf"
        } else {
            "saved.pdf"
        });
        app.password_action(if encrypted {
            Request::SavePassword(output.clone(), "generated-password".into())
        } else {
            Request::SaveAs(output.clone())
        });
        assert_eq!(app.busy, 0);
        assert!(!output.exists());
        assert!(app.qol.working_copy.is_none());
    }
    app.session.take().unwrap().close();
    assert!(!snapshot.exists());
    assert!(external_pdf.exists());
    assert!(external_pdf.with_extension("history").exists());
    std::fs::remove_dir(&alias).unwrap();
    std::fs::remove_dir(&attachment).unwrap();
    let redirected = private.join("redirected");
    generated_recovery_junction(&redirected, &external);
    for orphan in recovery::orphans_in(&private) {
        orphan.discard();
    }
    assert!(external_pdf.exists());
    assert!(external_pdf.with_extension("history").exists());
    // A direct stale record must also leave its redirected target intact.
    recovery::Orphan {
        dir: redirected.clone(),
        copy: external_pdf.clone(),
        source: None,
        modified: std::time::SystemTime::now(),
    }
    .discard();
    assert_eq!(
        std::fs::read(&external_pdf).unwrap(),
        std::fs::read(&fixture).unwrap()
    );
    assert!(external_pdf.with_extension("history").exists());
    std::fs::remove_dir(&redirected).unwrap();
    std::fs::remove_dir_all(folder).unwrap();
    workflow_frame(&mut app, &ctx, vec![]);
}
