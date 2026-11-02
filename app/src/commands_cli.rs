use pdf_editor_core::{BlankPdf, CommandSession, Engine};
use std::{
    ffi::OsString,
    io::{BufRead, Read, Write},
    path::PathBuf,
};

const HELP: &str = "Local document creation\n  new --output NEW.pdf [--size a4|a5|letter|custom] [--width POINTS --height POINTS] [--landscape] [--pages 1]\n  commands --root EXISTING_WORKSPACE [--script RELATIVE.jsonl]\n  commands --schema\nWithout --script, commands reads JSON objects from stdin and writes one JSON response per line.\nOnly relative workspace files are accessible. Scripts stop on the first failed command; stdin sessions can correct errors.\nMutations require the document and revision returned by the previous response. No commands open dialogs, run shell commands, use a printer or access the network.\n";

pub(super) fn run(args: Vec<OsString>) -> i32 {
    if args.iter().skip(1).any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return 0;
    }
    if args.first().is_some_and(|a| a == "commands") && args.get(1).is_some_and(|a| a == "--schema")
    {
        if args.len() != 2 {
            eprintln!("--schema takes no other arguments");
            return 2;
        }
        println!("{}", pdf_editor_core::command_schema());
        return 0;
    }
    let result = (|| -> Result<(), (i32, String)> {
        let mut values = std::collections::BTreeMap::new();
        let mut landscape = false;
        let mut i = 1;
        while i < args.len() {
            let key = args[i].to_str().ok_or((2, "Invalid option".into()))?;
            if key == "--landscape" {
                if landscape {
                    return Err((2, "Duplicate --landscape".into()));
                }
                landscape = true;
                i += 1;
                continue;
            }
            let allowed = if args[0] == "new" {
                &["--output", "--size", "--width", "--height", "--pages"][..]
            } else {
                &["--root", "--script"][..]
            };
            if !allowed.contains(&key)
                || i + 1 >= args.len()
                || values.insert(key.to_owned(), args[i + 1].clone()).is_some()
            {
                return Err((2, "Unknown, duplicate or incomplete option".into()));
            }
            i += 2;
        }
        if args[0] == "commands" && landscape {
            return Err((2, "--landscape applies only to new".into()));
        }
        let exe = std::env::current_exe().map_err(|_| (1, "Cannot locate executable".into()))?;
        let library = exe.with_file_name(if cfg!(windows) {
            "pdfium.dll"
        } else {
            "libpdfium.so"
        });
        if args[0] == "new" {
            let output = values
                .get("--output")
                .ok_or((2, "Missing --output".into()))?;
            let size = values
                .get("--size")
                .map(|s| s.to_str().ok_or((2, "Invalid page size".into())))
                .transpose()?
                .unwrap_or("a4");
            let mut spec = match size {
                "a4" => BlankPdf::default(),
                "a5" => BlankPdf {
                    width: 419.528,
                    height: 595.276,
                    pages: 1,
                },
                "letter" => BlankPdf {
                    width: 612.,
                    height: 792.,
                    pages: 1,
                },
                "custom" => BlankPdf::default(),
                _ => return Err((2, "Page size must be a4, a5, letter or custom".into())),
            };
            for (key, target) in [("--width", &mut spec.width), ("--height", &mut spec.height)] {
                if let Some(value) = values.get(key) {
                    *target = value
                        .to_str()
                        .and_then(|v| v.parse().ok())
                        .ok_or((2, "Invalid page dimension".into()))?;
                }
            }
            if let Some(value) = values.get("--pages") {
                spec.pages = value
                    .to_str()
                    .and_then(|v| v.parse().ok())
                    .ok_or((2, "Invalid page count".into()))?;
            }
            spec = spec.with_landscape(landscape);
            let engine=Engine::new(&library).map_err(|_|(1,"Cannot start PDF engine; keep the bundled PDFium library beside the executable".into()))?;
            engine
                .blank_document(spec)
                .and_then(|d| d.save_as(&PathBuf::from(output)))
                .map_err(|e| (1, e.0))?;
            return Ok(());
        }
        let root = values.get("--root").ok_or((2, "Missing --root".into()))?;
        let engine = Engine::new(&library).map_err(|_| {
            (
                1,
                "Cannot start PDF engine; keep the bundled PDFium library beside the executable"
                    .into(),
            )
        })?;
        let mut session =
            CommandSession::new(&engine, &PathBuf::from(root)).map_err(|e| (1, e.0))?;
        let stdin = std::io::stdin();
        let mut reader: Box<dyn BufRead + '_> = if let Some(script) = values.get("--script") {
            let path = session
                .input_file(
                    script
                        .to_str()
                        .ok_or((2, "Script path must be UTF-8".into()))?,
                )
                .map_err(|e| (1, e.0))?;
            Box::new(std::io::BufReader::new(
                std::fs::File::open(path).map_err(|_| (1, "Cannot read command script".into()))?,
            ))
        } else {
            Box::new(stdin.lock())
        };
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        let mut had_error = false;
        for line_number in 0..=10000 {
            let mut bytes = Vec::new();
            let count = reader
                .by_ref()
                .take(1_048_578)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| (1, "Cannot read command input".into()))?;
            if count == 0 {
                return if had_error {
                    Err((1, "Command session completed with errors".into()))
                } else {
                    Ok(())
                };
            }
            if line_number == 10000 {
                return Err((1, "Command session reached its 10000-line limit".into()));
            }
            if count > 1_048_577 {
                return Err((1, "Command line exceeds limit".into()));
            }
            let line = std::str::from_utf8(&bytes)
                .map_err(|_| (1, "Commands must be UTF-8".into()))?
                .trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() {
                continue;
            }
            let response = session.execute(line);
            writeln!(output, "{}", response.json)
                .and_then(|_| output.flush())
                .map_err(|_| (1, "Cannot write command response".into()))?;
            if !response.ok {
                had_error = true;
                if values.contains_key("--script") {
                    return Err((1, "Command script stopped after an error".into()));
                }
            }
        }
        Err((1, "Command session reached its 10000-line limit".into()))
    })();
    match result {
        Ok(()) => 0,
        Err((code, message)) => {
            eprintln!("{message}");
            code
        }
    }
}
