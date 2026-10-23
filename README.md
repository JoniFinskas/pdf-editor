# PDF Editor

A lightweight Windows PDF editor for editing existing text in place. The `core`
crate is a headless Rust library; the `app` crate is the egui desktop editor
built on its public API.

Implementation requirements and scope are documented in [SPEC.md](SPEC.md).

## Build on Windows

Requirements: Windows x64, PowerShell, stable Rust with the
`x86_64-pc-windows-msvc` toolchain, and Visual Studio Build Tools with the
**Desktop development with C++** workload and a Windows SDK. Run these commands
from the repository root:

```powershell
./scripts/setup-pdfium.ps1
cargo build --release --locked
.\target\release\pdf-editor.exe
# Or open a document immediately:
.\target\release\pdf-editor.exe "C:\Documents\My report.pdf"
```

Keep `pdf-editor.exe` and `pdfium.dll` together. The executable embeds its Inter
UI fonts; no font download or Rust installation is needed to run the built app.
The release profile uses size optimization, thin LTO, one code-generation unit
and stripped symbols. Panic unwinding remains enabled for recoverable parser
errors. The build does not create an installer, register file associations or
publish a release.

The setup script uses the non-V8 PDFium chromium/8111 release, verifies the
committed archive SHA256 on every run, and places the DLL, `pdfium-LICENSE.txt`
and `pdfium-licenses/` in `target/debug` and `target/release`. A verified cached
archive is reused. An invalid cached archive causes an error; remove that archive
and rerun the script to download it again. Retain these notices, the Inter
`app/assets/fonts/OFL.txt`, the project licenses and applicable dependency notices
when redistributing binaries. Do not substitute an arbitrary DLL from a download site.

If Windows reports that the executable is in use, close that copy before
rebuilding, or use `cargo build --release --locked --target-dir target/check` and
copy the verified DLL beside the executable in `target/check/release`.
No download occurs while opening or editing a document.

## Automated checks

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo install cargo-deny --version 0.20.2 --locked
cargo deny check
```

Use `cargo run -p pdf-editor --locked` for a debug build. Exact dependency versions
are pinned in the crate manifests and Cargo.lock. Python is optional for supply
chain verification: `python scripts/check-crate-age.py YYYY-MM-DD` checks the
publication dates of every locked registry package against that date's 14-day
floor. Normal tests need neither Python nor fixture downloads.

Before distributing a build, complete [MANUAL_TEST.md](MANUAL_TEST.md). Automated
checks and their limits are recorded in [TEST_REPORT.md](TEST_REPORT.md).

Auto-save keeps recovery copies of unsaved edits in
`%LOCALAPPDATA%\PdfEditor\Recovery`, never in place of the original or a saved
copy. They are deleted after Save As, after edits are discarded and on a normal
exit; after an unexpected exit the editor offers to restore them. Auto-save can
be turned off in Settings. The editor opens no network connections.

Use [CORE_API.md](CORE_API.md) for integration. Create an Engine with the absolute
path of the bundled DLL, open a file, list its text objects, then pass a selected
object to `Engine::edit`. Rendering returns tightly packed RGBA pixels. Undo/redo
includes whole-document font substitutions. Save As requires a new `.pdf` filename;
existing files and the original path are never overwritten.

## Basic editing controls

- Ctrl+F opens Find; all matches are highlighted and Previous/Next wrap through them.
- Add text: choose a font, size and color, click a baseline on a page, then Place text.
- Click existing text to edit it. The text-object toolbar selects individual runs for
  size/color changes or deletion; those operations are undoable.
- The Pages sidebar navigates by thumbnail. Rotate and Delete act on the current
  page. Drag a thumbnail onto another to reorder; Insert PDF adds all source pages
  before the current page. The last page cannot be deleted.
- Ctrl+O opens, Ctrl+S saves a new copy, Ctrl+Z/Ctrl+Y undo/redo, and Ctrl+Plus/Minus zoom.
  Text fields keep their own undo history while focused.
- Recent lists the last ten opened PDFs in the existing local settings file.
  Opening another file or closing warns about unsaved changes.
- `pdf-editor.exe "C:\Documents\My report.pdf"` opens a PDF at startup.

## Editing behavior

Each edit replaces one text run without paragraph reflow. Existing embedded
TrueType fonts are retained when their glyphs cover the new text. Where necessary,
the engine extends the PDF encoding using the same embedded font program. Otherwise
it searches Windows and per-user font directories for matching family and style,
then an embeddable substitute. Missing fonts produce a warning. Substitution can
affect one line or every matching missing-font resource in the document.

Character spacing, word spacing, horizontal scaling, text rise, color, and transforms
remain in the content stream. Numeric text-cursor adjustments keep neighboring runs
in place. A failed edit leaves the document and history unchanged.

The current text editor targets page-level text runs. Nested Form XObject text,
unsupported encodings, and ambiguous native/parser object mappings fail safely
rather than being rewritten heuristically. Replacement system fonts support
TrueType files and TrueType collections; CFF-only/Type1 font substitution may use
a TrueType fallback. Complex-script shaping, paragraph reflow, OCR, annotations,
form editing, and image editing are not provided.

Limits: 64 MiB source/result PDFs, 16 MiB per decoded content/font stream,
10,000 pages, 16 megapixels per render, 16,384 replacement characters, and
256 MiB undo snapshots. Edits requiring more than 1,024 cursor corrections on a
page return an error. Full-font embedding can increase output size. A substitute
is never used if it lacks required glyphs or disallows editable embedding.

## Security and fixtures

PDFs and embedded fonts are untrusted input. Update pdfium.dll when security fixes
ship, updating its pinned release and verified checksum together. The native PDFium
parser runs in-process; Rust error handling and size limits are not a native-code
sandbox or a hard process-memory limit. Load the DLL only from a trusted install
directory. No JavaScript engine, network service, or form-action execution is used.

Generated PDFs and their generator are committed. Tests also use a synthetic PDF
exported by LibreOffice; exported inputs are never modified. Inter is obtained only
from its official rsms release, with its OFL license. Normal tests need no Python
or fixture-generation downloads. See [TEST_REPORT.md](TEST_REPORT.md),
[DECISIONS.md](DECISIONS.md), and [the threat model](repo-starter-threat-model.md).
Open work and known limitations are tracked in [BACKLOG.md](BACKLOG.md).

To regenerate synthetic PDFs, first regenerate the renamed subset with
`python scripts/subset-fixture.py` if needed (fontTools 4.60.1), then run
`cargo test generate_fixtures -- --ignored`. The subset script accepts the
checksum-pinned wheel documented in its source from ignored target storage.

## License

Copyright (c) 2026 JoniFinskas.

Licensed under the [GNU General Public License, version 3](LICENSE)
(GPL-3.0-only).
