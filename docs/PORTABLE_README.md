# PDF Editor

Extract the whole ZIP to a folder, then open `pdf-editor.exe`. Keep `pdfium.dll`,
the command-line companion and the license files beside the application.

## Getting started

- Open a PDF with **Ctrl+O** or drag it into the window.
- Click text to edit. **Enter** or clicking away applies; **Escape** cancels.
- Choose **Move / resize objects** to move text, images and vector objects.
  Drag the handles to resize. **Snap** aligns edges and centers; hold **Alt**
  for free placement.
- **Ctrl+Z** undoes a change; **Ctrl+Y** redoes it.
- **Ctrl+S** opens Save As. The original PDF is never overwritten.
- **Ctrl+K** searches all tools. Use arrow keys to select and Enter to open.
  Search terms include `diff`, `OCR`, `forms`, `watermark` and `recovery`.

## Tools

Edit and find text; organize pages; copy objects between tabs; compare PDFs;
fill or design forms; add signatures, images, notes and shapes; redact and
sanitize; merge, split and extract; crop; add headers, footers and page numbers;
compress images; export images, tables and Word documents; clean up scans;
recognize text with installed Windows OCR languages; print.

Recovery copies protect applied, unsaved edits after an unexpected exit.
The recovery button shows their status and opens interval settings. Password-
protected documents do not create recovery copies. A draft still being typed
is not included until it is applied. Use Save As for a permanent copy.

Imported artwork and fonts vary. Unsupported edits produce an error rather than
silently changing unrelated content. Word and table conversion is best effort.
Check important results before sharing or printing.

## Command line

Run `pdf-editor-cli.exe --help` for command-line options.

## Licenses

PDF Editor is licensed under GNU GPL version 3 only (GPL-3.0-only); see LICENSE.
Distributing binaries requires providing corresponding source under GPLv3.
The included notices retain the licences of PDFium and bundled third-party
components. Use current releases
so PDFium receives security updates.
