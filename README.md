# PDF Editor

A lightweight Windows PDF editor for editing existing text in place. The `core`
crate is a headless Rust library; the `app` crate is the egui desktop editor
built on its public API.

Implementation requirements and scope are documented in [SPEC.md](SPEC.md).

## Features

- Edit existing text with font preservation; add, delete and restyle text objects.
- Move and resize text, images and vector artwork with selection handles; copy
  editable objects between tabs and ungroup supported Form XObjects.
- Edit compatible paragraphs as a block with alignment-preserving reflow; ambiguous
  layouts keep line editing.
- Find all matches, navigate results, and replace one or all editable matches.
- Work in document tabs with separate undo/redo, unsaved-change warnings, recent
  files and optional recovery copies.
- Navigate page thumbnails and bookmarks; rotate, delete, reorder, insert, merge,
  split, extract and crop pages.
- Fill existing PDF forms, add reusable drawn or imported signatures, and insert,
  move and resize PNG/JPG images.
- Design text/date fields, checkboxes, radio groups and dropdowns; move, resize,
  rename, require or delete fields, and flatten filled forms into page content.
- Check Finnish and English spelling with installed Windows dictionaries,
  underlines and right-click suggestions in existing and new text.
- Add editable highlights, underlines, strikethroughs and notes; draw freehand,
  rectangles, ellipses, lines and arrows as PDF annotations.
- Open password-protected PDFs and optionally protect a new saved copy with AES-256.
- Recognize scanned pages with Windows OCR; redact entire intersecting text, image and
  vector objects while retaining other editable page content.
- Create PDFs from ordered JPG/PNG/HEIC images. Clean photo scans with detected
  edges, draggable corners, perspective correction, deskew, grayscale or black
  and white, then optional OCR. HEIC uses installed Windows codecs.
- Preview and redact literal text, email addresses, phone numbers, IBANs,
  Finnish personal identity codes or credit-card patterns. Sanitize metadata,
  invisible text, attachments and actions, with optional comments/link removal.
- Compress supported images, add page numbers/watermarks, export PNG/JPG pages at
  a chosen DPI, and print through the Windows print dialog.
- Compare two saved PDFs with shared scrolling/zoom, text highlights, a visual
  pixel overlay and a page-change sidebar (the **Compare PDFs** toolbar button). Comparison
  matches unchanged pages around insertions/deletions and bounds previews to one megapixel per side.
- Use headless commands for batch PDF operations, optional per-user Windows shell
  integration, and single-instance document handoff.
- Select a table area, correct the detected grid, and export Excel (`.xlsx`) or CSV.
  Extraction uses text alignment and ruling lines; complex tables need review.
- Export editable Word documents with paragraphs, font styling, images and simple ruled tables (best-effort layout).
- Add ranged headers/footers with date/page tokens and Bates numbering; batch
  stamp PDF folders from the CLI with dry-run and separate output files.
- Use light/dark themes, a grouped document menu and movable tool windows.

See the controls and limitations below, especially redaction, encrypted-source
recovery and supported text/image formats.

Press **Ctrl+G** or click the current page number to jump to any page, the first page or the last page.

## Build on Windows

Requirements: Windows x64, PowerShell, stable Rust with the
`x86_64-pc-windows-msvc` toolchain, and Visual Studio Build Tools with the
**Desktop development with C++** workload and a Windows SDK. Run these commands
from the repository root:

```powershell
./scripts/setup-pdfium.ps1
cargo build --release --workspace --locked
.\target\release\pdf-editor.exe
# Or open a document immediately:
.\target\release\pdf-editor.exe "C:\Documents\My report.pdf"
```

Keep `pdf-editor.exe`, `pdf-editor-cli.exe` and `pdfium.dll` together for the
complete desktop, command-line and Explorer workflows. `corpus-runner.exe` is
development tooling and is not part of the portable app. The desktop embeds its Inter
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
- Drag empty page space or the gray canvas to pan. Drag text before entering its
  editor to move the line/paragraph; finish or cancel an open edit first. A moving
  outline previews placement, and release commits one undoable move.
- Choose **Move / resize objects** in the toolbar to select and drag individual text runs,
  images, vector icons or grouped artwork. Drag any of the eight selection handles
  to resize; text scales without reflowing, with one undo step on release. Right-click for **Copy**, **Paste on
  this page**, **Duplicate** or **Delete**; Ctrl+C, Ctrl+V, Ctrl+D and Delete also
  act on the selection when a text field is not focused. Copy/paste uses a private
  in-memory clipboard shared between document tabs and pages. A copy survives closing
  its source tab and stays available until replaced or the application closes.
  Copied text, images and paths remain directly editable; grouped artwork stays grouped.
  Fonts, resources and clipping are preserved. Unsupported native operations fail
  without changing the document. Annotations retain their separate editing tools.
- Text edits retain page and thumbnail previews while replacements render in the
  background. Search refreshes preserve the current viewing position. Complex
  PDF edits can still take time; this is not a guarantee of zero processing latency.
- The Pages sidebar navigates by thumbnail. Rotate and Delete act on the current
  page. Drag a thumbnail onto another to reorder; Insert PDF adds all source pages
  before the current page. The last page cannot be deleted.
- Ctrl+O opens, Ctrl+S saves a new copy, Ctrl+Z/Ctrl+Y undo/redo, and Ctrl+Plus/Minus zoom.
  Text fields keep their own undo history while focused.
- Recent lists the last ten opened PDFs in the existing local settings file.
  Opening another file creates a separate tab. Closing a dirty tab or the app warns
  about unsaved changes. Each tab has its own undo history.
- `pdf-editor.exe "C:\Documents\My report.pdf"` opens a PDF at startup.

## Daily and extra tools

**Compare PDFs** fits Original and Revised side by side. Numbered text outlines
match the change list. Read the original (−) and revised (+) excerpts, click a
highlight on either page, or use Previous/Next change to bring it into view.
Include unchanged pages when reviewing the whole document. Previous/Next changed
page skips unchanged pages. Text changes and Visual changes switch the overlay;
Fit both pages resets zoom. Switching modes reuses the current page render.
Unchanged pages anchor the automatic page matching, so inserted/deleted pages do
not shift later matches. The labels show actual Original/Revised page numbers.
Repeated pages, reordered pages and adjacent modified/inserted pages can be
ambiguous; use **Match page numbers only** when that is a better fit.

Import/export file selection runs in the background. The tool shows its pending
state and a Cancel pending action control. Closing a tool discards its late
selection; reopen it to start again. Export options are captured when selection
starts. Image decoding also stays off the UI thread.

The **All tools** panel opens by default on the home screen and beside documents. Press **Ctrl+K** to search names and descriptions, including subfeatures such as flattening forms, Excel export and Bates numbering. Tools that need an open PDF are visible but disabled until a document is opened. Close the panel for more page space; **All tools** reopens it. The small arrow retains the compact document menu. Its **Edit** section holds
Find and replace, Fill form, Signatures, Images, Markup and notes, Draw and shapes,
Redact, and OCR current page. **Document** holds Bookmarks sidebar, Merge/split/
extract, Crop page, Page numbers and watermark, Export pages as images, Compress
images, and Print. Recent files appear below these sections; scroll the menu when
needed.

Tool windows have a labeled title row, can be moved beside the page, and close
with their X button. Long content scrolls within the screen while the close button
stays visible. They stay open while interacting with the page. The password
prompt opens centered. Tool positions and dimensions use PDF points (72 per inch);
page numbers in controls start at one. Drawing uses shape icon toggles, while
markup type and image export format use segmented selectors.

The active document tab has an accent line. Unsaved tabs show a dot; hover a tab
to reveal its close button. Closing a dirty tab asks whether to discard changes.

- Print opens the Windows print dialog and prints rendered pages, including edits.
- Signatures supports drawing, PNG/JPG import, and a reusable local library. Place
  and size a signature before adding it; the Images tool can adjust inserted image
  positions and dimensions. Signatures are flattened page graphics, not digital
  certificates.
- Forms lists existing text, checkbox, radio and dropdown fields. Select a field,
  edit its value, and Apply. JavaScript and external form actions are disabled.
- Replace counts editable matches and replaces one or all while retaining the
  existing font where possible. Unsupported text mappings produce an error.
- Encrypted PDFs prompt for a password. Save As optionally encrypts the new copy
  with AES-256. Recovery copies are disabled for encrypted sources. A blank Save As
  password deliberately exports an unencrypted copy.
- Merge appends PDFs in the chosen order. Extract accepts ranges such as `1-3,5`;
  Split separates groups with semicolons, such as `1-3;4-6`. These exports are
  unencrypted. Encrypted merge inputs need a separately decrypted copy.
- Markup creates editable PDF highlights, underlines, strikethroughs and sticky
  notes. Select an entry in its list to update or delete it.
- OCR recognizes an image-only current page with installed Windows OCR languages
  and adds invisible searchable text. It needs a supported Windows OCR language;
  there is no bundled engine or cloud service.
- Redaction removes whole intersecting text runs, images, vectors and form groups,
  including portions outside the box. Other page content stays editable. It drops
  forms, annotations, bookmarks, attachments and metadata throughout the document.
  Tagged properties, Type 3 fonts, patterns, shadings and soft-mask graphics states
  that cannot be verified are refused. Save As writes the sanitized copy; Undo and
  the source file retain the original.
- Compress downsizes and JPEG-compresses supported RGB images, showing before/after
  byte sizes. Unsupported image encodings remain unchanged, and a larger result
  is not accepted.
- Page numbers and watermarks accept `{page}` and `{pages}`, font, position, size,
  color and opacity. They are undoable text content on all pages.
- Export creates PNG/JPG files for selected page ranges at 36–600 DPI, within the
  render limit. Crop changes visible page bounds; hidden content is retained.
- Draw and shapes supports freehand, rectangle, ellipse, line and arrow. Enable
  Draw on page and drag; every stroke is a real PDF annotation and one undo step.
- Bookmarks opens a sidebar for navigation and nested entries. Add targets the
  current page; Rename, Delete and Move up/down act on the selected entry. Delete
  includes children; moving preserves the selected subtree.

## Editing behavior

Single-line editing replaces one text run; compatible paragraphs support
alignment-preserving reflow as described below. Existing embedded
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
a TrueType fallback. Complex-script shaping is not provided. Paragraph reflow
requires compatible horizontal text and supported font metrics; ambiguous layouts
retain line editing.

Limits: 64 MiB source/result PDFs, 16 MiB per decoded content/font stream,
10,000 pages, 16 megapixels per render, 16,384 replacement characters, and
256 MiB undo snapshots. Edits requiring more than 1,024 cursor corrections on a
page return an error. Full-font embedding can increase output size. A substitute
is never used if it lacks required glyphs or disallows editable embedding.

Page dimensions use fast metadata with an indexed loading fallback. Visible pages
render in the background; retained full-page textures are capped at 128 MiB per
document. This cap excludes native working memory, thumbnails and undo history.

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

## Headless command line

`pdf-editor-cli.exe` is the console companion; place the verified `pdfium.dll`
next to it. The desktop executable also recognizes these commands before creating
windows, but use the console companion when capturing stdout, stderr or exit codes.
No new dependencies are required.

```powershell
pdf-editor-cli merge first.pdf second.pdf --output merged.pdf
pdf-editor-cli split source.pdf --pages "1-3;4-6" --output existing-folder
pdf-editor-cli extract source.pdf --pages "1,3-5" --output selected.pdf
pdf-editor-cli compress source.pdf --output smaller.pdf --edge 1600 --quality 80
pdf-editor-cli rotate source.pdf --output rotated.pdf --pages "1-3" --degrees 90
pdf-editor-cli to-image source.pdf --output existing-folder --dpi 144 --format png
pdf-editor-cli ocr scanned.pdf --output searchable.pdf --pages "1-2"
pdf-editor-cli find-replace source.pdf --find "old" --dry-run
pdf-editor-cli find-replace input-folder --find "old" --replace "new" --output output-folder
```

Page ranges are one-based; omitted ranges select every page. Rotation is clockwise
90, 180 or 270 degrees. Image export accepts PNG/JPG and 36–600 DPI. OCR uses
installed Windows OCR languages. Password-protected files must first be opened
and exported through the desktop UI. CLI commands inherit core resource limits.

Find/replace is case-insensitive and uses the same font-preserving edit engine.
Folder mode processes immediate regular `.pdf` files only (no recursion or
symlink traversal), in sorted order, up to 10,000 files. It prints numeric file
indices and match counts, never extracted text. `--dry-run` writes nothing and
requires no output path. A folder replacement requires an existing output folder;
it preserves basenames and processes failures independently. Successfully written
outputs remain if a later file fails. Reports count matches only for successful
files. Single-file replacement uses an explicit new output filename.

All outputs refuse existing files and never overwrite source PDFs. Export and split
folders must already exist. Use `--` before input filenames beginning with a dash.
Exit status is **0** for success, **1** for processing/I/O failures (including any
folder member failure), and **2** for invalid command syntax. `--help` lists syntax.
Explorer integration uses the console companion's `shell-open`, `shell-merge`,
`shell-compress` and `shell-export` wrappers to open the corresponding desktop
workflow; these return after launching the GUI, not after completing an export.

### Windows integration

The desktop app uses one instance per Windows user. Opening another local PDF forwards
it to a new tab in the running window. The local mailbox has bounded requests and
uses an operating-system lock; it opens no network port. Network/device paths are
not accepted through this integration.

Settings → General offers **Make default PDF app**. This registers this installation
for the current user and opens Windows Default Apps, where the `.pdf` choice must
be confirmed. **Remove** deletes only PDF Editor's per-user registration and menu;
it does not modify Windows' protected default-app selection.

Enable the Explorer PDF menu in the same pane after building/installing
`pdf-editor-cli.exe` beside `pdf-editor.exe`. The menu offers Open in tab, Merge
selected, Compress and Export as images. Windows 11 may put it under **Show more
options**. Commands pass filenames directly to the CLI wrapper, then open the relevant
editor tool so output paths and changes can be reviewed. Merge selections collect in one tab's ordered list until Merge is clicked or the
organizer is closed (up to 100 additional PDFs); review the order before merging. No source file is overwritten. Removing or moving the portable folder
requires removing/re-registering its shell entries from Settings.

## Paragraph editing

Click a compatible multiline paragraph to edit the block. Enter adds a line break;
**Ctrl+Enter** applies and **Escape** cancels. **Alt+click** selects one line instead.
The editor preserves the detected width, line spacing, alignment and supported
font metrics. Longer text extends downward with an overlap warning; it never
moves neighboring content. Unsupported metrics, overwide words and page overflow
fail without modifying the document. Mixed styles, rotated text and unclear
columns retain line editing with a short explanation. Font substitution is scoped
to the paragraph to preserve its neighbors.

## Isolated corpus checks

The on-demand [corpus harness](scripts/corpus/README.md) runs downloaded PDFs only
in restricted Linux containers. Scan downloaded inputs with
`scripts/scan-corpus.ps1` before each changed corpus is processed. Corpus bytes,
provenance and per-file results remain ignored local data. See [BLOCKED.md](BLOCKED.md)
and [TEST_REPORT.md](TEST_REPORT.md) for actual coverage and outstanding requirements.

The latest 340-file PDFium corpus run passed 175 files (51.47%); 165 returned
guarded errors, most often unavailable fallback fonts or unsupported text layouts.
No crashes, panics, timeouts or render-diff failures were observed. This is not
full Part H acceptance: the 3,000-file Govdocs minimum and fuzz testing remain
incomplete. The native desktop itself is not container-sandboxed.

## Additional editing tools

**Objects → Ungroup** expands supported grouped artwork into independently
editable text, images and paths. Transparency groups, optional-content forms and
inline images are rejected without changes. Undo restores the original group.

**Search and redact** previews matches before applying. Patterns are detection
heuristics: review the marked regions. Redaction removes whole intersecting
objects, so a match inside a long text run or group can remove surrounding content.
It also removes document extras that could retain private data. Save As creates
the cleaned copy; undo and recovery snapshots may still retain the original.

Headers and footers are available in the stamping tool. Select Header or Footer,
enter page ranges such as `1-3,5` (blank means all), and use `{date}`, `{page}` and
`{pages}` tokens. The date defaults to UTC and is editable. Bates numbering adds a
prefix and fixed-width sequence in document order; overflow aborts the operation.

### Sanitize document

Document → Sanitize document removes metadata, embedded files, executable actions
and text using invisible rendering or zero opacity. Options also remove comments
and link annotations. A report lists removed entries; Save As writes a fresh copy,
and Undo restores the original. Recovery snapshots can still contain original data.
Layers, soft masks, clipping and mixed hidden/visible text blocks are rejected
without changes. Same-color, occluded and off-page text are not detected; this tool
is not a complete visual-concealment audit.

Sanitization also rejects XFA forms, retained annotation appearances, unused retained Form
streams, Forms without their own resources, and aliased attachment/script/metadata payloads.
These cases leave the original document unchanged.

### Batch headers, footers and Bates numbering

```powershell
pdf-editor-cli stamp C:\docs --output C:\stamped --text "Report {date} - {page}/{pages}" --position header --pages "1-3"
pdf-editor-cli bates C:\docs --output C:\numbered --prefix "CASE-" --start 1 --digits 6 --position footer
pdf-editor-cli stamp C:\docs --text "Reviewed {date}" --dry-run
```

Both commands accept a PDF or a folder. Folders are nonrecursive, processed in sorted filename order, and contain at most 10,000 PDFs. The output folder must already exist; each destination must be new. Originals are never overwritten. Bates numbering continues across successful files. Each operation supports `--pages`, `--position header|footer|absolute`, `--font`, `--size`, `--color RRGGBB`, `--opacity`, `--left` and `--margin`. Header/footer margins are measured from the visible page edge; absolute mode uses a bottom baseline. Text stamping also supports `--text` and `--date`; date defaults to UTC captured once for the batch. Blank page ranges select every page. `--dry-run` validates the complete operation without writes and needs no output path. Failed files are reported individually; successful outputs are retained. Exit codes: 0 success, 1 processing failure, 2 invalid usage.

### Dense-page editing benchmark

Run `cargo run -p pdf-editor-core --example dense_edit_benchmark --release --locked --offline -j 1`
after preparing the pinned PDFium DLL. It creates only generated 100-, 500- and
1,000-object pages under `target/performance/`, measures text listing and one edit,
and verifies unchanged neighboring text and transforms. No input PDFs or GUI
are used. Timings depend on hardware and workload; cursor-dependent layouts may
still require repeated verification passes.

For repeated cursor repairs, run
`cargo run -p pdf-editor-core --example cursor_repair_benchmark --release --locked --offline -j 1`.
It generates independent lines and continuous text chains, replaces text across
10, 50 and 150 pairs, and verifies every unchanged run's text and transform.

Tool discovery: Ctrl+K opens All tools. Search names, descriptions or terms such as
`diff`, `OCR`, `flatten`, `watermark` and `recovery`. Choose a category to browse;
arrow keys select a result and Enter opens it. Reset clears search and category.

Enable **Snap** above the document for alignment guides while moving or resizing
text, images and other objects. Edges and centers align with nearby objects and
the page within six screen points. Hold **Alt** for free placement.

The recovery button shows whether copies are off, unavailable, pending, writing
or saved. Unapplied text is marked separately. Click it for interval settings.
Recovery is enabled every two minutes by default for unprotected documents; it
never replaces Save As or writes over the original.

Portable distribution builds use `scripts/build-release.ps1` to remap compiler
paths. `scripts/package-release.ps1` validates an explicit file set and binary
strings before promoting a package from staging. The portable README covers
application usage; development reports and fixtures are not distributed.
