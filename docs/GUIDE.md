# PDF Editor guide

[Project overview and stack](../README.md).

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

Before distributing a build, check native file dialogs, printing and Windows
integration separately from the automated tests.

Auto-save is off by default. Under Settings -> General -> Auto-save, choose
**After edits (2 seconds)** or a 1, 2, 5 or 10-minute interval. Use **Save As** once
per opened document to create its working copy. Auto-save updates only that copy,
never the original. The two-second mode waits for a pause in applied edits;
minute intervals start with the first unsaved edit. Neither mode applies text
still being typed or forces a draft closed. Clean documents are not rewritten,
and only one automatic save runs at a time across tabs.

Saving runs locally in the background and retains the copy's encryption and
Undo/Redo history. A missing, redirected or externally changed copy pauses
auto-save with a warning; save manually or use Save As to resume. Reopening any
PDF starts a new session, so use Save As again before auto-save can update it.
Turn the setting off at any time; a write already in progress finishes.

Crash recovery is separate. It keeps local copies of applied, unsaved edits in
`%LOCALAPPDATA%\PdfEditor\Recovery`, never in place of the original or a saved
copy. They are deleted after a successful save, after edits are discarded and on a normal
exit; after an unexpected exit the editor offers to restore them. It does not
save the PDF or include text still being typed. Change the interval or turn it
off under Settings → General → Crash recovery. The editor opens no network connections.
Recovery folders are reserved for temporary copies; Save As requires another
folder. Cleanup removes only recognized recovery artifacts, preserving unrelated
files and subdirectories. A restored copy with missing source metadata suggests
`Recovered_edited.pdf` without preselecting its recovery directory.

Use [CORE_API.md](../CORE_API.md) for integration. Create an Engine with the absolute
path of the bundled DLL, open a file, list its text objects, then pass a selected
object to `Engine::edit`. Rendering returns tightly packed RGBA pixels. Undo/redo
includes whole-document font substitutions. Save As requires a new `.pdf` filename.
It creates a working copy owned by this editing session; Ctrl+S can update that
copy while the original stays protected. A missing, redirected or externally
changed working copy requires another Save As. Ctrl+Shift+S or the document menu
creates another copy. Encryption settings are retained in memory for repeated saves.

## Basic editing controls

- Ctrl+G or clicking the current page number opens page navigation.
- Ctrl+F opens Find; Enter advances and Shift+Enter goes back, wrapping through highlighted matches. Escape closes Find.
- Add text: choose a font, size and color, click a baseline on a page, then Place text.
- Double-click existing text to edit it. The text-object toolbar selects individual runs for
  size/color changes or deletion; those operations are undoable.
- Drag empty page space or the gray canvas to pan. Drag text before entering its
  editor to move the selected object; finish or cancel an open edit first. The selected
  text or artwork stays visible in the drag preview, and release commits one undoable move.
  Text snaps to matching baselines so words with different glyph heights align.
- Click once to select and drag logical text blocks,
  images, vector icons or grouped artwork. Drag any of the eight selection handles
  to resize. Text grows proportionally by font size, with live text and point-size feedback; release commits one undo step. Right-click for **Copy**, **Paste on
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
- The Pages sidebar navigates by thumbnail. Batch actions use the selected pages,
  falling back to the current page. Drag selected thumbnails to reorder; Insert PDF adds all source pages
  before the current page. The last page cannot be deleted.
- Ctrl+O opens, Ctrl+S saves the working copy (creating one on first save), Ctrl+Shift+S creates another copy, Ctrl+Z/Ctrl+Y undo/redo, and Ctrl+Plus/Minus zoom.
  Text fields keep their own undo history while focused.
- Recent lists the last ten opened PDFs in the existing local settings file.
  Opening another file creates a separate tab. Closing a dirty tab or the app warns
  about unsaved changes. Each tab has its own undo history.
- `pdf-editor.exe "C:\Documents\My report.pdf"` opens a PDF at startup.

## Daily and extra tools

**Compare PDFs** opens inside the current tab. **Back to PDF** returns to the
document view. Original versus Current edits includes applied unsaved changes.
Opening it applies pending text first; rejected edits must be corrected
or discarded. Saving does not reset the opening original. Copies saved by the
app remember their original in local settings for later comparisons, except
protected or encrypted documents. Older `_edited.pdf` copies can offer a sibling
filename suggestion; confirm it or choose the original with the folder button.
A filename match alone does not establish ancestry. **Two files** compares saved
PDFs and rejects the same file on both sides.

Review numbered text outlines and before/after excerpts, click a highlight, or
use the change arrows. **All pages** includes unchanged pages. **Text** and
**Visual** switch overlays; **Fit** resets zoom. Switching modes reuses the page
render. Unchanged pages anchor automatic page matching around insertions and
deletions. Repeated pages and adjacent modified/inserted pages can be ambiguous;
choose **Match page numbers** in options when appropriate.

Import/export file selection runs in the background. The tool shows its pending
state and a Cancel pending action control. Closing a tool discards its late
selection; reopen it to start again. Export options are captured when selection
starts. Image decoding also stays off the UI thread.

Use **All tools** or **Ctrl+K** to search tool names, descriptions and subfeatures
such as flattening forms, Excel export and Bates numbering. Tools that need an
open PDF stay disabled until one is opened. Close the panel for more page space;
**All tools** reopens it. The small document arrow holds **Save As** and recent
files.

Tool windows have a labeled title row, can be moved beside the page, and close
with their X button. Long content scrolls within the screen while the close button
stays visible. They stay open while interacting with the page. The password
prompt opens centered. Tool positions and dimensions use PDF points (72 per inch);
page numbers in controls start at one. Drawing uses shape icon toggles, while
markup type and image export format use segmented selectors.

The active document tab has an accent line. Unsaved tabs show a dot; hover a tab
to reveal its close button. Closing a dirty tab asks whether to discard changes.
Drag a tab before or after another to reorder it. The active document stays selected;
hold near an edge to scroll through tabs, or press Escape to cancel the drag.

- Print opens the Windows print dialog and prints rendered pages, including edits.
- Signatures supports drawing, PNG/JPG import, and a reusable local library.
  Choose Place signature, move its preview onto the desired page, then click.
  Escape cancels placement. Select the placed signature to drag or resize it.
  Signatures are flattened page graphics, not digital certificates.
- Forms lists existing text, checkbox, radio and dropdown fields. Edit a value and
  choose **Apply**; the undo arrow reverts that field's draft. Unchanged fields
  keep their drafts when another field is applied or refreshed. Apply or revert
  pending entries before saving or flattening. Closing the PDF warns about
  unapplied entries; drafts stay in memory only. JavaScript and external form
  actions are disabled.
- Replace counts editable matches and replaces one or all while retaining the
  existing font where possible. Unsupported text mappings produce an error.
- Encrypted PDFs prompt for a password. Save As optionally encrypts the new copy
  with AES-256. Recovery copies and remembered sessions exclude encrypted sources
  and encrypted working copies. A blank Save As
  password deliberately exports an unencrypted copy.
- Merge appends PDFs in the chosen order. Extract accepts ranges such as `1-3,5`;
  Split accepts semicolon-separated ranges such as `1-3;4-6`, or **Top-level
  bookmarks**. Bookmark destinations start new PDFs; leading pages are kept in a
  separate file. Exports are unencrypted and refuse existing files. Encrypted
  merge inputs need a separately decrypted copy.
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

## Selection, search and session controls

Choose **Select text** and drag across words or pages; double-click selects a word,
triple-click selects a line. **Copy text** or Ctrl+C copies Unicode text with line
breaks. **Highlight selection** adds editable highlights in one undo step. Text uses
the PDF's native reading order; complex layouts can differ from visual order.

**Selection tools** offers box selection, alignment and equal-gap distribution.
Shift-click adds/removes logical text blocks or other objects. Drag the selection
or its handles to move/resize together; arrows nudge by one PDF point and
Shift+arrows by ten. Copy, duplicate and delete act on the whole selection.
Alignment labels follow the displayed page orientation.

Ctrl-click thumbnails to toggle pages and Shift-click for a range. Sidebar actions
rotate, delete, duplicate or extract those pages. Drag a selected thumbnail to move
the batch in document order. **Blank page** accepts dimensions and insertion position.
A batch is one undo step. Deleting all pages or breaking bookmark destinations is
refused without changing the document.

Find's **Options & results** opens case/whole-word controls, scope choices and
clickable result excerpts. Scopes include the current page, selected thumbnails
and selected text. Find and replace uses the same options and counts only safely
editable matches; nested text can be searchable without being replaceable.
**Font family and style** changes existing text using installed embeddable fonts,
including bold/italic. Unsupported styles or unsafe layout changes are refused.

Long multi-step operations show progress. **Cancel** takes effect at the next safe
step, after an active native call finishes. Cancelled mutations preserve the prior
document and history; batch exports remove outputs they created before cancellation.

Settings offers **Remember open tabs and reading position**, off by default. It
stores unencrypted saved file paths and page/zoom/scroll locally, without passwords.
Ctrl+Tab and Ctrl+Shift+Tab cycle tabs; Ctrl+T opens a tab, Ctrl+W closes it and
Ctrl+Shift+T reopens a saved closed tab. Explicitly discarded edits are not reopened.
Recovery snapshots also keep bounded undo/redo history. Recovery remains separate
from saving a working copy and is unavailable for protected documents.

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
or fixture-generation downloads. See [DECISIONS.md](../DECISIONS.md) and [the threat model](../repo-starter-threat-model.md).

Set `PDF_EDITOR_GENERATED_ONLY=1` before `cargo test --workspace --locked --offline`
to restrict fixture traversal to generated PDFs. Stress regressions exercise
concurrent document workers, mixed feature sequences at every page rotation,
editable form-page transfers, repeated rejected operations, tool dismissal and
the console commands. Test outputs belong under ignored `target/` storage.
Set `PDF_EDITOR_TEST_CLI` to an absolute portable console executable path to
repeat the feature integration matrix against a release build.
Native printer dialogs, Explorer integration and desktop acceptance remain
separate interactive checks.

To regenerate synthetic PDFs, first regenerate the renamed subset with
`python scripts/subset-fixture.py` if needed (fontTools 4.60.1), then run
`cargo test generate_fixtures -- --ignored`. The subset script accepts the
checksum-pinned wheel documented in its source from ignored target storage.

## License

Copyright (c) 2026 JoniFinskas.

Licensed under the [GNU General Public License, version 3](../LICENSE)
(GPL-3.0-only). Third-party components retain their original licences.
When distributing binaries, provide the corresponding source and build scripts
as required by GPLv3, and retain all required third-party notices.

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

Double-click a compatible multiline paragraph to edit the block. Enter adds a line break;
**Ctrl+Enter** applies and **Escape** cancels. **Alt+click** selects one line instead.
The editor preserves the detected width, line spacing, alignment and supported
font metrics. Drag the side grip to change the writable width without scaling the font.
Likely wrapped rows edit as one sentence. Longer text can use more lines; edits
that would overlap other text are rejected so the draft can be adjusted. The editor
never moves neighboring content. Unsupported metrics, overwide words and page overflow
fail without modifying the document. Mixed styles, rotated text and unclear
columns retain line editing with a short explanation. Font substitution is scoped
to the paragraph to preserve its neighbors.

## Isolated corpus checks

The on-demand [corpus harness](../scripts/corpus/README.md) runs downloaded PDFs only
in restricted Linux containers. Scan downloaded inputs with
`scripts/scan-corpus.ps1` before each changed corpus is processed. Corpus bytes,
provenance and per-file results remain ignored local data.

The harness reports unsupported fonts and text layouts as failures. Its presence
does not imply exhaustive compatibility testing or a completed fuzz campaign.
The native desktop itself is not container-sandboxed.

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

**Sanitize & save…** creates a new copy, reopens and verifies the written bytes,
then publishes it. The open document and its undo history stay unchanged.
Image-only rebuilding is selected by default. Protected inputs require an explicit
**Export without password** choice; originals and existing destinations are never
replaced. Cancelled or failed verification leaves no published copy.


All tools → Sanitize document removes PDF metadata, embedded files, executable actions
and text using invisible rendering or zero opacity. Options also remove comments
and link annotations. A report lists removed entries; Save As writes a fresh copy,
and Undo restores the original. Recovery snapshots can still contain original data.
Layers, soft masks, clipping and mixed hidden/visible text blocks are rejected
without changes. Same-color, occluded and off-page text are not detected; this tool
is not a complete visual-concealment audit.

Editable cleanup preserves text and forms, and verifies the supported PDF cleanup.
It does not assess visible content, embedded-image metadata or arbitrary opaque
payloads. Use rebuilding when preparing a copy after removing private data.
Rebuilt exports also reject image metadata and unexpected document extras.

Sanitization also rejects aliased page-content streams, XFA forms, retained annotation appearances, unused retained Form
streams, Forms without their own resources, and aliased attachment/script/metadata payloads.
These cases leave the original document unchanged. For complex documents,
**Rebuild visible pages** creates image-only pages from the current appearance
(up to 100 pages, at most 2048 pixels per side). No original text layer, metadata,
scripts, forms or attachments are copied. Page dimensions are preserved, but text
is no longer searchable or editable and interactive links are removed. This mode is
selected by default in the desktop tool; Undo restores the original. Comment/drawing and link options
control whether their visible appearances are included in the rebuilt images;
interactive annotation data is always removed.

Visible edits remain in the page images. Rebuilding removes underlying edit
objects and metadata from the saved copy, not visual evidence of an edit. The app
has no edit-activity log. Undo/redo stays in memory, while optional recovery
PDFs/history can retain earlier content. Recent files, optional restored tabs and
comparison-origin settings keep local paths. Sanitizing does not delete the
source, existing recovery copies or operating-system backups.

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
text, images and other objects. Thin lines and gap marks show alignment with
other objects and the page center. Centers align to
centers; moving an edge near the middle of a page does not center the object.
Snapping starts within six screen points and holds slightly farther to avoid
jitter between nearby targets. Hold **Alt** for free placement or turn **Snap**
off. The released object keeps the position shown by its preview.

Settings → General has separate working-copy auto-save and crash-recovery settings.
Recovery is enabled every two minutes by default for unprotected documents; it
never replaces Save As or writes over the original.

Portable distribution builds use `scripts/build-release.ps1` to remap compiler
paths. `scripts/package-release.ps1` collects executables, licence notices
and checksums. The portable README covers
application usage; development reports and fixtures are not distributed.


Object interaction: click text, an image or artwork once to select it. Drag the
selection to move it, use its square handles to resize it, and press Delete to
remove it. Double-click text to type; Delete then edits characters normally.
Click outside the text editor to apply changes and dismiss its toolbar.


Page ordering: drag a thumbnail by its preview. The floating thumbnail identifies
the moving page; an insertion line shows whether it will go before or after the
target. Release commits one undoable move.

Word export keeps supported text, tables and images editable. Complex compositing
that cannot be expanded safely is preserved as a page image instead of aborting
the export. Those image-only pages are not text-editable in Word.

### Text geometry and high zoom

Automatic font fallback considers the original run width, family category, weight
and slant after checking glyph coverage and embedding permissions. Exact installed
families still take priority; an explicit family remains available in font settings.
Rotated and skewed text uses an aligned editor, including pointer selection, caret,
copy/paste, text undo and IME positioning. Paragraphs keep Ctrl+Enter to apply.

High zoom renders visible page tiles at display resolution, including form values
and annotations. A smaller whole-page preview remains while tiles load. Cache and
batch limits bound memory; edits, pan, zoom and display-scale changes invalidate
stale results.
