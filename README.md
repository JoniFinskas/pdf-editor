# PDF Editor

An offline Windows PDF editor built in Rust. Edit existing text, move objects
and organize pages in a native desktop app, or use the CLI for batch operations.

## What it does

- Create blank PDFs or single-column CVs with selectable text.
- Edit text with font matching, add text and images, and move or resize objects.
- Work across tabs with undo/redo, search and replace, and optional crash recovery.
- Reorder, rotate, crop, merge, split and extract pages.
- Fill and design forms; add signatures, notes, highlights and drawings.
- Compare the original with current edits, including applied unsaved changes.
- Redact content, sanitize a copy, recognize scans with Windows OCR, and export
  images, tables or Word documents. Conversion quality depends on the PDF.

Use **Ctrl+N** for a blank PDF or **Build CV** in All tools.
See [CV formatting and ATS](docs/CV.md) and the [local command interface](docs/COMMANDS.md).

## Stack

| Part | Technology |
| --- | --- |
| Language | Rust, edition 2024 |
| Desktop UI | eframe / egui, OpenGL via glow, embedded Inter fonts |
| PDF rendering and editing | PDFium through pdfium-render; lopdf for PDF structure |
| Font inspection | skrifa |
| Images | image crate |
| Local command protocol | serde_json |
| File dialogs | rfd |
| Platform integration | Windows APIs for OCR, printing and shell integration |

The `core` crate contains PDF logic without UI dependencies. The `app` crate
provides the desktop editor and console companion.

## Build and run

Requires Windows x64, PowerShell, stable Rust (MSVC), and Visual Studio Build
Tools with the C++ desktop workload and Windows SDK.

```powershell
.\scripts\setup-pdfium.ps1
.\scripts\build-release.ps1
.\target\release\pdf-editor.exe
```

Setup downloads a pinned PDFium build and verifies its checksum. Editing runs
offline. Keep `pdfium.dll` beside the executables. Use
[the packaging script](scripts/package-release.ps1) to prepare a portable copy
with the required licence notices.

## Before sharing a PDF

The original stays protected. **Ctrl+S** creates a working copy on first save,
then updates that copy; **Ctrl+Shift+S** creates another copy.

Crop only hides content. To prepare a copy after removing private data, use
**Sanitize & save** with **Rebuild visible pages**. This creates image-only pages
and removes the original text layer, metadata, forms and attachments from the
export. Check the visible result. Originals, undo history, recovery copies and
backups can still contain earlier content.

PDFium parses files inside the app process. Keep its pinned DLL and checksum
updated when security fixes ship; the parser is not sandboxed.

## Development

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny check
```

The last check requires cargo-deny. See the [guide](docs/GUIDE.md) for controls,
CLI examples, setup details and limitations, the [core API](CORE_API.md) for
integration, and the [changelog](CHANGELOG.md) for recent changes.

## Licence

Copyright (c) 2026 JoniFinskas. Licensed under the
[GNU General Public License, version 3](LICENSE) (GPL-3.0-only).
Third-party components retain their own licences.
