use super::*;
use crate::tests::{PDFIUM, engine, temp};
use serde_json::{Value, json};

fn folder(name: &str) -> PathBuf {
    let path = temp(name);
    fs::create_dir_all(&path).unwrap();
    path
}
fn call(s: &mut CommandSession<'_>, mut v: Value) -> Value {
    if let Some(doc) = &s.doc {
        v["document"] = json!(s.number);
        v["revision"] = json!(doc.revision());
    }
    let response = s.execute(&v.to_string());
    assert!(response.ok, "{}", response.json);
    serde_json::from_str::<Value>(&response.json).unwrap()["result"].clone()
}
fn rejected(s: &mut CommandSession<'_>, mut v: Value) {
    v["document"] = json!(s.number);
    v["revision"] = json!(s.doc.as_ref().unwrap().revision());
    let before = s.doc.as_ref().unwrap().bytes.clone();
    let response = s.execute(&v.to_string());
    assert!(!response.ok, "{}", response.json);
    assert_eq!(s.doc.as_ref().unwrap().bytes, before);
}

#[test]
fn commands_create_edit_save_reopen_and_reject_stale_selections() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let root = folder("command-workflow");
    let mut s = CommandSession::new(&e, &root).unwrap();
    call(&mut s, json!({"op":"new","pages":2}));
    call(
        &mut s,
        json!({"op":"text.add","text":"First draft","x":50,"y":700}),
    );
    let inventory = call(&mut s, json!({"op":"inspect","view":"text"}));
    let index = inventory["items"][0]["index"].clone();
    let stale = json!({"op":"text.edit","document":s.number,"revision":s.doc.as_ref().unwrap().revision(),"indices":[index],"text":"Stale"});
    call(
        &mut s,
        json!({"op":"text.edit","indices":[index],"text":"Final draft"}),
    );
    let before = s.doc.as_ref().unwrap().bytes.clone();
    assert!(!s.execute(&stale.to_string()).ok);
    assert_eq!(s.doc.as_ref().unwrap().bytes, before);
    call(&mut s, json!({"op":"save","path":"created.pdf"}));
    let saved = fs::read(root.join("created.pdf")).unwrap();
    call(&mut s, json!({"op":"history.undo"}));
    rejected(&mut s, json!({"op":"open","path":"created.pdf"}));
    call(&mut s, json!({"op":"save.update"}));
    assert_ne!(fs::read(root.join("created.pdf")).unwrap(), saved);
    call(&mut s, json!({"op":"open","path":"created.pdf"}));
    assert_eq!(
        call(&mut s, json!({"op":"text.find","find":"First draft"}))["total"],
        1
    );
    assert!(!s.changes.is_dirty());
    assert!(!s.doc.as_ref().unwrap().can_undo());
    rejected(&mut s, json!({"op":"save","path":"created.pdf"}));
    rejected(
        &mut s,
        json!({"op":"render","path":"fraction.png","dpi":72.5}),
    );
    assert!(!root.join("fraction.png").exists());
    rejected(&mut s, json!({"op":"compress","quality":256}));
    rejected(&mut s, json!({"op":"text.add","text":"bad field","typo":1}));
    for path in [
        "../escape.pdf",
        "/absolute.pdf",
        "bad:stream.pdf",
        "trailing./out.pdf",
    ] {
        rejected(&mut s, json!({"op":"save","path":path}));
    }
    let control = OperationControl::default();
    control.cancel();
    assert!(
        !s.execute_with_control("{\"op\":\"new\",\"discard\":true}", &control)
            .ok
    );
    assert_eq!(
        s.doc.as_ref().unwrap().bytes,
        fs::read(root.join("created.pdf")).unwrap()
    );
    fs::remove_file(root.join("created.pdf")).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn commands_landscape_and_status_reflect_actual_saved_geometry_and_history() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let root = folder("creation-qol-status");
    let mut s = CommandSession::new(&e, &root).unwrap();
    for (width, height) in [(792, 612), (612, 792)] {
        call(
            &mut s,
            json!({"op":"new","preset":"custom","width":width,"height":height,"landscape":true,"discard":true}),
        );
        let page = call(&mut s, json!({"op":"inspect"}))["items"][0].clone();
        assert_eq!(
            (page["width"].as_f64(), page["height"].as_f64()),
            (Some(792.), Some(612.))
        );
    }
    let status = call(&mut s, json!({"op":"status"}));
    assert_eq!(status["pages"], 1);
    assert_eq!(status["new"], true);
    assert_eq!(status["can_undo"], false);
    assert_eq!(status["working_copy"], Value::Null);
    call(&mut s, json!({"op":"text.add","text":"Generated content"}));
    assert_eq!(call(&mut s, json!({"op":"status"}))["can_undo"], true);
    call(&mut s, json!({"op":"save","path":"wide.pdf"}));
    let status = call(&mut s, json!({"op":"status"}));
    assert_eq!(status["working_copy"]["path"], "wide.pdf");
    assert_eq!(status["working_copy"]["encrypted"], false);
    call(&mut s, json!({"op":"history.undo"}));
    let status = call(&mut s, json!({"op":"status"}));
    assert_eq!(status["can_redo"], true);
    assert_eq!(
        e.pages(&e.open(&root.join("wide.pdf")).unwrap()).unwrap()[0].width,
        792.
    );
    fs::remove_file(root.join("wide.pdf")).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn commands_author_links_forms_images_and_privacy_with_fresh_inventories() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let root = folder("command-tools");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 80, 180, 255]))
        .save(root.join("picture.png"))
        .unwrap();
    let mut s = CommandSession::new(&e, &root).unwrap();
    call(&mut s, json!({"op":"new"}));
    call(
        &mut s,
        json!({"op":"text.add","text":"reader@example.org","x":42,"y":740}),
    );
    call(
        &mut s,
        json!({"op":"image.add","path":"picture.png","left":50,"bottom":400,"width":120,"height":60}),
    );
    let name = call(&mut s, json!({"op":"inspect","view":"pictures"}))["items"][0]["name"].clone();
    call(&mut s, json!({"op":"image.move","name":name,"left":80}));
    let picture = call(&mut s, json!({"op":"inspect","view":"pictures"}))["items"][0].clone();
    assert_eq!(
        (
            picture["left"].as_f64(),
            picture["bottom"].as_f64(),
            picture["width"].as_f64(),
            picture["height"].as_f64()
        ),
        (Some(80.), Some(400.), Some(120.), Some(60.))
    );
    call(
        &mut s,
        json!({"op":"link.add","bounds":[42,730,180,755],"uri":"https://example.org"}),
    );
    let index = call(&mut s, json!({"op":"inspect","view":"links"}))["items"][0]["index"].clone();
    rejected(
        &mut s,
        json!({"op":"link.update","index":index,"uri":"javascript:unused()"}),
    );
    call(
        &mut s,
        json!({"op":"link.update","index":index,"target_page":0}),
    );
    call(
        &mut s,
        json!({"op":"form.add","name":"Consent","kind":"checkbox","bounds":[42,300,60,318]}),
    );
    let index = call(&mut s, json!({"op":"inspect","view":"forms"}))["items"][0]["index"].clone();
    call(
        &mut s,
        json!({"op":"form.fill","index":index,"checked":true}),
    );
    rejected(
        &mut s,
        json!({"op":"form.fill","index":index,"text":"wrong value type"}),
    );
    assert_eq!(
        call(&mut s, json!({"op":"inspect","view":"forms"}))["items"][0]["value"]["checked"],
        true
    );
    call(&mut s, json!({"op":"bookmark.add","title":"Start"}));
    let id = call(&mut s, json!({"op":"inspect","view":"bookmarks"}))["items"][0]["id"].clone();
    call(
        &mut s,
        json!({"op":"bookmark.rename","id":id,"title":"Overview","request_id":"correlation"}),
    );
    assert_eq!(
        call(&mut s, json!({"op":"inspect","view":"bookmarks"}))["items"][0]["title"],
        "Overview"
    );
    let token = call(&mut s, json!({"op":"privacy.preview","kind":"email"}))["preview"].clone();
    call(&mut s, json!({"op":"text.add","text":"New content"}));
    rejected(&mut s, json!({"op":"privacy.apply","preview":token}));
    let token = call(&mut s, json!({"op":"privacy.preview","kind":"email"}))["preview"].clone();
    call(&mut s, json!({"op":"privacy.apply","preview":token}));
    assert_eq!(
        call(
            &mut s,
            json!({"op":"text.find","find":"reader@example.org"})
        )["total"],
        0
    );
    call(
        &mut s,
        json!({"op":"bookmark.add","title":"Clean overview"}),
    );
    call(&mut s, json!({"op":"save","path":"authored.pdf"}));
    call(
        &mut s,
        json!({"op":"render","path":"authored.png","dpi":36}),
    );
    assert!(image::open(root.join("authored.png")).unwrap().width() > 200);
    let reopened = e.open(&root.join("authored.pdf")).unwrap();
    assert_eq!(
        e.bookmarks(&reopened).unwrap().items[0].title,
        "Clean overview"
    );
    for file in ["picture.png", "authored.pdf", "authored.png"] {
        fs::remove_file(root.join(file)).unwrap();
    }
    fs::remove_dir(root).unwrap();
}

#[test]
fn commands_clipboard_cross_document_cv_and_discovery_work_without_files() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let root = folder("command-cv");
    let mut s = CommandSession::new(&e, &root).unwrap();
    let schema: Value = serde_json::from_str(&command_schema()).unwrap();
    assert!(schema["commands"].as_array().unwrap().len() > 60);
    call(&mut s, json!({"op":"new"}));
    call(&mut s, json!({"op":"text.add","text":"Copied heading"}));
    let index = call(&mut s, json!({"op":"inspect","view":"objects"}))["items"][0]["index"].clone();
    call(&mut s, json!({"op":"objects.copy","indices":[index]}));
    call(&mut s, json!({"op":"new","discard":true}));
    call(&mut s, json!({"op":"objects.paste"}));
    assert_eq!(
        call(&mut s, json!({"op":"text.find","find":"Copied heading"}))["total"],
        1
    );
    call(
        &mut s,
        json!({"op":"cv.create","discard":true,"name":"Avery Example","contact":["reader@example.org"],"sections":[{"heading":"Experience","body":"Built a fictional project using Rust."}]}),
    );
    let report = call(&mut s, json!({"op":"cv.check"}));
    assert!(report["text"].as_str().unwrap().contains("Experience"));
    assert!(report["warnings"].as_array().unwrap().is_empty());
    assert_eq!(
        call(&mut s, json!({"op":"compare","page":null,"revised_page":0}))["changes"]
            .as_array()
            .unwrap()[0]["kind"],
        "Added"
    );
    rejected(
        &mut s,
        json!({"op":"cv.create","discard":true,"name":"","sections":[]}),
    );
    call(
        &mut s,
        json!({"op":"document.policy","substitution":"line"}),
    );
    assert!(matches!(
        s.doc.as_ref().unwrap().substitution,
        Substitution::Line
    ));
    fs::remove_dir(root).unwrap();
}

#[test]
fn commands_arrange_text_markup_and_pages_then_export_current_document() {
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let root = folder("command-layout");
    let mut s = CommandSession::new(&e, &root).unwrap();
    call(&mut s, json!({"op":"new"}));
    for (text, x, y) in [("First line", 50, 700), ("Second line", 200, 650)] {
        call(&mut s, json!({"op":"text.add","text":text,"x":x,"y":y}));
    }
    let all = call(&mut s, json!({"op":"inspect","view":"text"}));
    let indices = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["index"].clone())
        .collect::<Vec<_>>();
    call(
        &mut s,
        json!({"op":"objects.layout","indices":indices,"alignment":"left"}),
    );
    call(
        &mut s,
        json!({"op":"text.format","indices":indices,"font":"Arial","bold":true,"size":14,"color":[30,60,90]}),
    );
    call(
        &mut s,
        json!({"op":"objects.edit","indices":indices,"action":"move","dx":20,"dy":-10}),
    );
    let moved = call(&mut s, json!({"op":"inspect","view":"text"}));
    assert!(
        moved["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["size"] == 14.0 && v["bounds"][0].as_f64().unwrap() > 60.)
    );
    call(
        &mut s,
        json!({"op":"text.replace","find":"line","text":"entry"}),
    );
    assert_eq!(
        call(&mut s, json!({"op":"text.find","find":"entry"}))["total"],
        2
    );
    call(
        &mut s,
        json!({"op":"draw","kind":"rectangle","points":[[50,400],[200,450]]}),
    );
    call(
        &mut s,
        json!({"op":"annotation.add","bounds":[50,500,80,530],"text":"Generated note"}),
    );
    let index = call(&mut s, json!({"op":"inspect","view":"annotations"}))["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["text"] == "Generated note")
        .unwrap()["index"]
        .clone();
    call(
        &mut s,
        json!({"op":"annotation.update","index":index,"text":"Updated note"}),
    );
    call(
        &mut s,
        json!({"op":"page.blank","before":1,"width":612,"height":792}),
    );
    call(
        &mut s,
        json!({"op":"page.duplicate","pages":[0],"before":2}),
    );
    call(&mut s, json!({"op":"page.move","pages":[2],"before":0}));
    call(&mut s, json!({"op":"page.rotate","pages":[1]}));
    call(&mut s, json!({"op":"page.delete","pages":[2]}));
    assert_eq!(call(&mut s, json!({"op":"inspect"}))["total"], 2);
    call(&mut s, json!({"op":"ocr","pages":[0,1]}));
    call(
        &mut s,
        json!({"op":"page.extract","pages":[1,0],"path":"extracted.pdf"}),
    );
    let extracted = e.open(&root.join("extracted.pdf")).unwrap();
    assert_eq!(e.pages(&extracted).unwrap().len(), 2);
    assert_eq!(e.find_text(&extracted, "entry").unwrap().len(), 4);
    assert!(
        e.annotations(&extracted)
            .unwrap()
            .iter()
            .any(|a| a.markup.text == "Updated note")
    );
    call(&mut s, json!({"op":"export.word","path":"document.docx"}));
    assert!(fs::metadata(root.join("document.docx")).unwrap().len() > 100);
    call(
        &mut s,
        json!({"op":"export.sanitized","path":"clean.pdf","flatten":true}),
    );
    let clean = e.open(&root.join("clean.pdf")).unwrap();
    assert!(e.text_objects(&clean, 0).unwrap().is_empty());
    assert!(
        e.render(&clean, 0, 0.25)
            .unwrap()
            .rgba
            .iter()
            .any(|n| *n < 200)
    );
    for name in ["extracted.pdf", "document.docx", "clean.pdf"] {
        fs::remove_file(root.join(name)).unwrap();
    }
    fs::remove_dir(root).unwrap();
}

#[cfg(windows)]
#[test]
fn commands_reject_redirected_working_copy_parents_and_workspace_root() {
    use std::os::windows::process::CommandExt;
    fn junction(link: &Path, target: &Path) {
        let output=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:PDF_EDITOR_TEST_LINK -Value $env:PDF_EDITOR_TEST_TARGET | Out-Null"])
            .env("PDF_EDITOR_TEST_LINK",link).env("PDF_EDITOR_TEST_TARGET",target).creation_flags(0x08000000).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let _lock = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    let e = engine();
    let base = folder("command-redirect");
    let root = base.join("workspace");
    let outside = base.join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::create_dir(root.join("copies")).unwrap();
    let mut s = CommandSession::new(&e, &root).unwrap();
    call(&mut s, json!({"op":"new"}));
    call(&mut s, json!({"op":"save","path":"copies/copy.pdf"}));
    fs::rename(root.join("copies"), root.join("original-copies")).unwrap();
    let expected = fs::read(root.join("original-copies/copy.pdf")).unwrap();
    fs::write(outside.join("copy.pdf"), &expected).unwrap();
    call(&mut s, json!({"op":"text.add","text":"Must stay inside"}));
    junction(&root.join("copies"), &outside);
    rejected(&mut s, json!({"op":"save.update"}));
    assert_eq!(fs::read(outside.join("copy.pdf")).unwrap(), expected);
    fs::remove_dir(root.join("copies")).unwrap();
    drop(s);
    let mut s = CommandSession::new(&e, &root).unwrap();
    call(&mut s, json!({"op":"new"}));
    fs::rename(&root, base.join("original-workspace")).unwrap();
    junction(&root, &outside);
    rejected(&mut s, json!({"op":"export.images","folder":".","dpi":36}));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    fs::remove_dir(&root).unwrap();
    drop(s);
    fs::remove_file(outside.join("copy.pdf")).unwrap();
    fs::remove_dir(outside).unwrap();
    let copies = base.join("original-workspace/original-copies");
    fs::remove_file(copies.join("copy.pdf")).unwrap();
    fs::remove_dir(copies).unwrap();
    fs::remove_dir(base.join("original-workspace")).unwrap();
    fs::remove_dir(base).unwrap();
}
