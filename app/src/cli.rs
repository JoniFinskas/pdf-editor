#[path = "cli_stamps.rs"]
mod cli_stamps;
use pdf_editor_core::{Engine, PageEdit, PageImageFormat, page_ranges};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const HELP: &str = "PDF Editor headless CLI\nCommands:\n  merge INPUT... --output NEW.pdf\n  split INPUT --pages '1-3;4-6' --output EXISTING_FOLDER\n  extract INPUT --pages 1,3-5 --output NEW.pdf\n  compress INPUT --output NEW.pdf [--edge 1600] [--quality 80]\n  rotate INPUT --output NEW.pdf [--pages 1-3] [--degrees 90|180|270]\n  to-image INPUT --output EXISTING_FOLDER [--pages 1-3] [--dpi 144] [--format png|jpg]\n  ocr INPUT --output NEW.pdf [--pages 1-3]\n  find-replace INPUT_OR_FOLDER --find TEXT [--replace TEXT --output PATH] [--dry-run]\n  stamp INPUT_OR_FOLDER --output PATH [--text TEXT] [--pages 1-3] [--position header|footer|absolute] [--date YYYY-MM-DD] [--dry-run]\n  bates INPUT_OR_FOLDER --output PATH [--prefix TEXT] [--start 1] [--digits 6] [--pages 1-3] [--dry-run]\nStamp options: --font Arial --size 12 --color 000000 --opacity 1 --left 30 --margin 24. Tokens: {date}, {page}, {pages}.\nBates folders continue numbering in sorted filename order. --dry-run writes nothing and needs no output.\nFolders are nonrecursive. Outputs must be new; sources are never overwritten.\nExit: 0 success, 1 processing failure, 2 usage error. OCR requires Windows.\nUse -- before positional paths beginning with a dash.\n";
const COMMANDS: &[&str] = &[
    "merge",
    "split",
    "extract",
    "compress",
    "rotate",
    "to-image",
    "ocr",
    "find-replace",
    "stamp",
    "bates",
];
#[derive(Debug)]
struct Command {
    name: String,
    inputs: Vec<PathBuf>,
    options: BTreeMap<String, OsString>,
    dry: bool,
}
impl Command {
    fn text(&self, key: &str, default: &str) -> Result<String, String> {
        self.options
            .get(key)
            .map(|v| {
                v.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{key} requires UTF-8 text"))
            })
            .unwrap_or_else(|| Ok(default.into()))
    }
    fn number<T: std::str::FromStr>(&self, key: &str, default: &str) -> Result<T, String> {
        self.text(key, default)?
            .parse()
            .map_err(|_| format!("Invalid {key}"))
    }
    fn output(&self) -> Result<PathBuf, String> {
        self.options
            .get("--output")
            .map(PathBuf::from)
            .ok_or_else(|| "Missing --output".into())
    }
}
fn parse(args: Vec<OsString>) -> Result<Command, String> {
    let name = args
        .first()
        .and_then(|v| v.to_str())
        .filter(|v| COMMANDS.contains(v))
        .ok_or("Unknown command")?
        .to_owned();
    let allowed: &[&str] = match name.as_str() {
        "merge" => &["--output"],
        "split" | "extract" => &["--output", "--pages"],
        "compress" => &["--output", "--edge", "--quality"],
        "rotate" => &["--output", "--pages", "--degrees"],
        "to-image" => &["--output", "--pages", "--dpi", "--format"],
        "ocr" => &["--output", "--pages"],
        "stamp" => &[
            "--output",
            "--pages",
            "--text",
            "--position",
            "--date",
            "--font",
            "--size",
            "--color",
            "--opacity",
            "--left",
            "--margin",
            "--dry-run",
        ],
        "bates" => &[
            "--output",
            "--pages",
            "--prefix",
            "--start",
            "--digits",
            "--position",
            "--font",
            "--size",
            "--color",
            "--opacity",
            "--left",
            "--margin",
            "--dry-run",
        ],
        _ => &["--output", "--find", "--replace", "--dry-run"],
    };
    let mut c = Command {
        name,
        inputs: vec![],
        options: BTreeMap::new(),
        dry: false,
    };
    let mut it = args.into_iter().skip(1);
    let mut positional = false;
    while let Some(value) = it.next() {
        if !positional && value == "--" {
            positional = true;
            continue;
        }
        if !positional && value.to_string_lossy().starts_with('-') {
            let key = value.to_str().ok_or("Invalid option")?;
            if !allowed.contains(&key) {
                return Err(format!("Unknown option: {key}"));
            }
            if key == "--dry-run" {
                if c.dry {
                    return Err("Duplicate --dry-run".into());
                }
                c.dry = true;
                continue;
            }
            let val = it
                .next()
                .ok_or_else(|| format!("Missing value for {key}"))?;
            if c.options.insert(key.into(), val).is_some() {
                return Err(format!("Duplicate {key}"));
            }
        } else {
            c.inputs.push(PathBuf::from(value));
        }
    }
    if c.inputs.is_empty()
        || (c.name == "merge" && !(2..=101).contains(&c.inputs.len()))
        || (c.name != "merge" && c.inputs.len() != 1)
    {
        return Err("Wrong number of input paths".into());
    }
    if !c.dry {
        c.output()?;
    }
    if matches!(c.name.as_str(), "split" | "extract") && !c.options.contains_key("--pages") {
        return Err("Missing --pages".into());
    }
    if c.name == "find-replace"
        && (c.text("--find", "")?.is_empty() || (!c.dry && !c.options.contains_key("--replace")))
    {
        return Err("Supply nonempty --find and --replace (or --dry-run)".into());
    }
    if c.name == "rotate" && ![90, 180, 270].contains(&c.number::<u16>("--degrees", "90")?) {
        return Err("Degrees must be 90, 180 or 270".into());
    }
    if c.name == "to-image"
        && (!matches!(c.text("--format", "png")?.as_str(), "png" | "jpg")
            || !(36..=600).contains(&c.number::<u16>("--dpi", "144")?))
    {
        return Err("Use png/jpg and DPI 36–600".into());
    }
    if c.name == "compress"
        && (!(128..=8192).contains(&c.number::<u32>("--edge", "1600")?)
            || !(10..=95).contains(&c.number::<u8>("--quality", "80")?))
    {
        return Err("Use edge 128–8192 and quality 10–95".into());
    }
    if matches!(c.name.as_str(), "stamp" | "bates") {
        cli_stamps::options(&c)?;
    }
    Ok(c)
}
/// Recognized headless commands bypass all window creation. Bare PDF paths remain GUI inputs.
pub fn dispatch(args: Vec<OsString>) -> Option<i32> {
    let first = args.first()?.to_str()?;
    if matches!(
        first,
        "shell-open" | "shell-merge" | "shell-compress" | "shell-export"
    ) {
        if args.len() != 2 || args[1].is_empty() {
            eprintln!("Shell command requires one PDF path");
            return Some(2);
        }
        let result = std::env::current_exe().and_then(|exe| {
            let gui = exe.with_file_name("pdf-editor.exe");
            std::process::Command::new(gui)
                .arg(format!("--{first}"))
                .arg(&args[1])
                .spawn()
        });
        return Some(match result {
            Ok(_) => 0,
            Err(_) => {
                eprintln!("Cannot start desktop editor");
                1
            }
        });
    }
    if matches!(first, "--help" | "-h") {
        println!("{HELP}");
        return Some(0);
    }
    if !COMMANDS.contains(&first) {
        return None;
    }
    Some(run(args))
}
pub fn run(args: Vec<OsString>) -> i32 {
    if args.first().is_some_and(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return 0;
    }
    let c = match parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}\n{HELP}");
            return 2;
        }
    };
    let result = (|| {
        let exe = std::env::current_exe().map_err(|_| "Cannot locate executable")?;
        let library = exe
            .parent()
            .ok_or("Cannot locate executable directory")?
            .join(if cfg!(windows) {
                "pdfium.dll"
            } else {
                "libpdfium.so"
            });
        let engine = Engine::new(&library).map_err(|e| e.to_string())?;
        execute(&engine, c)
    })();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
fn pdf_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = vec![];
    for entry in std::fs::read_dir(folder).map_err(|_| "Cannot read input folder")? {
        let entry = entry.map_err(|_| "Cannot read folder entry")?;
        if entry
            .file_type()
            .map_err(|_| "Cannot inspect folder entry")?
            .is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("pdf"))
        {
            files.push(entry.path());
        }
        if files.len() > 10_000 {
            return Err("Folder exceeds 10000 PDF limit".into());
        }
    }
    files.sort();
    if files.is_empty() {
        return Err("Folder contains no PDFs".into());
    }
    Ok(files)
}
fn execute(engine: &Engine, c: Command) -> Result<(), String> {
    if matches!(c.name.as_str(), "stamp" | "bates") {
        return cli_stamps::execute(engine, &c);
    }
    let err = |e: pdf_editor_core::Error| e.to_string();
    if c.name == "find-replace" {
        let folder = c.inputs[0].is_dir();
        let inputs = if folder {
            pdf_files(&c.inputs[0])?
        } else {
            c.inputs.clone()
        };
        let output = if c.dry { None } else { Some(c.output()?) };
        if folder && output.as_ref().is_some_and(|p| !p.is_dir()) {
            return Err("Output must be an existing directory".into());
        }
        let mut failed = false;
        let mut total = 0;
        for (index, path) in inputs.iter().enumerate() {
            let result = (|| {
                let mut doc = engine.open(path).map_err(err)?;
                let count = engine
                    .replacement_count(&doc, &c.text("--find", "")?)
                    .map_err(err)?;
                if let Some(output) = &output {
                    let destination = if folder {
                        output.join(path.file_name().ok_or("Invalid filename")?)
                    } else {
                        output.clone()
                    };
                    engine
                        .replace_text(
                            &mut doc,
                            &c.text("--find", "")?,
                            &c.text("--replace", "")?,
                            None,
                        )
                        .map_err(err)?;
                    doc.save_as(&destination).map_err(err)?;
                }
                Ok::<usize, String>(count)
            })();
            match result {
                Ok(count) => {
                    total += count;
                    println!("file={} matches={count}", index + 1);
                }
                Err(e) => {
                    failed = true;
                    eprintln!("file={} error={e}", index + 1);
                }
            }
        }
        println!("files={} matches={total}", inputs.len());
        return if failed {
            Err("One or more files failed; successful outputs are retained".into())
        } else {
            Ok(())
        };
    }
    let mut doc = engine.open(&c.inputs[0]).map_err(err)?;
    let output = c.output()?;
    let count = engine.pages(&doc).map_err(err)?.len();
    let pages = if matches!(c.name.as_str(), "rotate" | "to-image" | "ocr" | "extract") {
        page_ranges(&c.text("--pages", &format!("1-{count}"))?, count).map_err(err)?
    } else {
        vec![]
    };
    match c.name.as_str() {
        "merge" => {
            engine.merge_pdfs(&mut doc, &c.inputs[1..]).map_err(err)?;
            doc.save_as(&output).map_err(err)?;
        }
        "split" => {
            let outputs = engine
                .split_pdf(&doc, &c.text("--pages", "")?, &output)
                .map_err(err)?;
            println!("files={}", outputs.len());
        }
        "extract" => engine.extract_pages(&doc, &pages, &output).map_err(err)?,
        "compress" => {
            let r = engine
                .compress_images(
                    &mut doc,
                    c.number("--edge", "1600")?,
                    c.number("--quality", "80")?,
                )
                .map_err(err)?;
            doc.save_as(&output).map_err(err)?;
            println!(
                "before={} after={} images={} skipped={}",
                r.before, r.after, r.images, r.skipped
            );
        }
        "rotate" => {
            for page in pages {
                for _ in 0..c.number::<u16>("--degrees", "90")? / 90 {
                    engine
                        .edit_pages(&mut doc, &PageEdit::Rotate { page })
                        .map_err(err)?;
                }
            }
            doc.save_as(&output).map_err(err)?;
        }
        "to-image" => {
            let format = if c.text("--format", "png")? == "jpg" {
                PageImageFormat::Jpeg
            } else {
                PageImageFormat::Png
            };
            let outputs = engine
                .export_page_images(&doc, &pages, c.number("--dpi", "144")?, format, &output)
                .map_err(err)?;
            println!("files={}", outputs.len());
        }
        "ocr" => {
            let mut words = 0;
            for page in pages {
                words += engine.ocr_page(&mut doc, page).map_err(err)?;
            }
            doc.save_as(&output).map_err(err)?;
            println!("words={words}");
        }
        _ => return Err("Unknown command".into()),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }
    #[test]
    fn rejects_ambiguous_usage() {
        for a in [
            vec!["compress", "a.pdf"],
            vec!["rotate", "a.pdf", "--output", "b.pdf", "--degrees", "45"],
            vec!["extract", "a.pdf", "--output", "b.pdf"],
            vec!["compress", "a.pdf", "--output", "b.pdf", "--dry-run"],
            vec!["find-replace", "a.pdf", "--dry-run", "--find", ""],
            vec!["merge", "a.pdf", "--output", "b.pdf", "--output", "c.pdf"],
        ] {
            assert!(parse(args(&a)).is_err(), "{a:?}");
        }
    }
    #[test]
    fn supports_dry_run_and_native_paths() {
        let c = parse(args(&[
            "find-replace",
            "C:/documents ä",
            "--find",
            "needle",
            "--dry-run",
        ]))
        .unwrap();
        assert!(c.dry);
        assert_eq!(c.inputs[0], PathBuf::from("C:/documents ä"));
    }
    #[test]
    fn double_dash_protects_filenames() {
        let c = parse(args(&[
            "compress",
            "--output",
            "new.pdf",
            "--",
            "-original.pdf",
        ]))
        .unwrap();
        assert_eq!(c.inputs[0], PathBuf::from("-original.pdf"));
    }
    #[test]
    fn gui_inputs_are_not_commands() {
        assert_eq!(dispatch(args(&["document.pdf"])), None);
        assert_eq!(run(args(&["unknown"])), 2);
    }
    #[test]
    fn generated_fixture_commands_preserve_sources_and_refuse_overwrite() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let engine = Engine::new(&root.join("target/debug/pdfium.dll")).unwrap();
        let source = root.join("tests/fixtures/generated/multi-page.pdf");
        let before = std::fs::read(&source).unwrap();
        let out = root.join(format!(
            "target/cli-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&out).unwrap();
        let invoke = |name: &str, options: Vec<(&str, OsString)>| {
            let mut a = vec![OsString::from(name), source.as_os_str().to_owned()];
            for (key, value) in options {
                a.push(key.into());
                a.push(value);
            }
            execute(&engine, parse(a).unwrap())
        };
        let rotated = out.join("rotated.pdf");
        invoke(
            "rotate",
            vec![
                ("--output", rotated.clone().into()),
                ("--pages", "1".into()),
            ],
        )
        .unwrap();
        assert!(invoke("rotate", vec![("--output", rotated.into())]).is_err());
        invoke(
            "extract",
            vec![
                ("--output", out.join("extract.pdf").into()),
                ("--pages", "1".into()),
            ],
        )
        .unwrap();
        invoke(
            "compress",
            vec![("--output", out.join("compress.pdf").into())],
        )
        .unwrap();
        invoke(
            "split",
            vec![("--output", out.clone().into()), ("--pages", "1;2".into())],
        )
        .unwrap();
        invoke(
            "to-image",
            vec![
                ("--output", out.clone().into()),
                ("--pages", "1".into()),
                ("--dpi", "36".into()),
            ],
        )
        .unwrap();
        execute(
            &engine,
            parse(vec![
                "merge".into(),
                source.clone().into(),
                source.clone().into(),
                "--output".into(),
                out.join("merge.pdf").into(),
            ])
            .unwrap(),
        )
        .unwrap();
        let mut dry = vec![OsString::from("find-replace"), out.as_os_str().to_owned()];
        dry.extend(args(&["--find", "unchanged", "--dry-run"]));
        let entries = std::fs::read_dir(&out).unwrap().count();
        execute(&engine, parse(dry).unwrap()).unwrap();
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), entries);
        invoke(
            "find-replace",
            vec![
                ("--output", out.join("replace.pdf").into()),
                ("--find", "Subset".into()),
                ("--replace", "test".into()),
            ],
        )
        .unwrap();
        let replaced = engine.open(&out.join("replace.pdf")).unwrap();
        assert_eq!(engine.replacement_count(&replaced, "Subset").unwrap(), 0);
        assert_eq!(engine.replacement_count(&replaced, "test").unwrap(), 4);
        assert_eq!(std::fs::read(&source).unwrap(), before);
    }
}
