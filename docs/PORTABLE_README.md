# PDF Editor

Extract the whole ZIP to a folder, then open `pdf-editor.exe`. Keep `pdfium.dll`,
the command-line companion and the license files beside the application.

## Getting started

- **Ctrl+N** creates a blank PDF. **Build CV** in All tools lays out a single-column CV.
  Review extracted text before sharing; no format guarantees an ATS result.
- Open a PDF with **Ctrl+O** or drag it into the window.
- Click text to edit. **Enter** or clicking away applies; **Escape** cancels.
- Choose **Move / resize objects** to move text, images and vector objects.
  Drag the handles to resize. **Snap** aligns edges and centers; hold **Alt**
  for free placement.
- **Ctrl+Z** undoes a change; **Ctrl+Y** redoes it.
- **Ctrl+S** saves the working copy; the first save opens Save As. The original stays unchanged.
- Optional auto-save in **Settings -> General** updates that working copy after edits or
  at a minute interval. It is off by default and requires Save As once per opened document.
- **Ctrl+K** searches all tools. Use arrow keys to select and Enter to open.
  Search terms include `diff`, `OCR`, `forms`, `watermark` and `recovery`.

## Tools

Edit and find text; organize pages; copy objects between tabs; compare PDFs;
fill or design forms; add signatures, images, notes and shapes; redact and
sanitize; merge, split and extract; crop; add headers, footers and page numbers;
compress images; export images, tables and Word documents; clean up scans;
recognize text with installed Windows OCR languages; print.

## Compare two versions

Choose **Compare PDFs** in the toolbar. The opening version is compared with current applied edits.
Choose other saved PDFs with **Change file** when needed. Apply any draft text
before comparing. Passwords and page
matching are under **Options and passwords**.
Use the changed-page list to move through the document. Within a page, click a
numbered highlight or use **‹ Change / Change ›**. The list shows removed text
with **−** and added text with **+**; long excerpts end with an ellipsis.
**Visual changes** highlights changed pixels in magenta, including image and
layout changes. **Fit both pages** resets the zoom. Matching inserted or removed
pages is automatic; **Match page numbers only** is available when needed.

Recovery copies protect applied, unsaved edits after an unexpected exit.
The recovery button shows their status and opens interval settings. Password-
protected documents do not create recovery copies. A draft still being typed
is not included until it is applied. Use Save As for a permanent copy.

Imported artwork and fonts vary. Unsupported edits produce an error rather than
silently changing unrelated content. Word and table conversion is best effort.
Check important results before sharing or printing.

## Command line

Run `pdf-editor-cli.exe --help` for command-line options.
Use `new --output NEW.pdf` for blank pages or `commands --schema` to discover
the JSON-line interface. `commands --root WORKSPACE` reads commands from stdin;
all paths must be relative to that existing folder. Exports refuse existing files.
`save.update` updates only this session's working copy.

## Licenses

PDF Editor is licensed under GNU GPL version 3 only (GPL-3.0-only); see LICENSE.
Distributing binaries requires providing corresponding source under GPLv3.
The included notices retain the licences of PDFium and bundled third-party
components. Use current releases
so PDFium receives security updates.


### serde_json 1.0.150 and zmij 1.0.21

These components are used under their MIT terms:

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
