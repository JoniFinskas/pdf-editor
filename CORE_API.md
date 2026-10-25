# Core API

Crate: `pdf_editor_core`. All fallible operations return `Result<T, Error>`.
Errors contain user-facing descriptions without document contents or filesystem
paths. Error implements Display and std::error::Error; standard conversions from
I/O, lopdf and PDFium errors intentionally discard internal diagnostic details.

## Functions

| Function | Inputs | Output / effect |
| --- | --- | --- |
| `Engine::page_items` | Document, page | Top-level text/image/path/form objects with revision, native index, kind and PDF-point bounds; capped at 20,000 objects. |
| `Engine::edit_page_items` | Mutable document, same-page PageItems, ObjectAction | Atomic move by PDF-point delta, resize one object to PDF-point bounds, duplicate with offset, delete, or move selected objects to the front/back of the page paint order; rejects stale selections and unsupported native operations; one undo step. |
| `Engine::copy_page_items` | Document, same-page PageItems | Opaque in-memory ObjectClipboard snapshot independent of subsequent edits and source-document lifetime; may be pasted into another document. |
| `Engine::paste_page_items` | Mutable document, ObjectClipboard, target page | Copy snapshot objects onto the target page with a 12-point right/down offset; copied text/images/paths remain directly editable, source Form groups retain resources and clipping; atomic and undoable. |
| `Engine::new` | Absolute trusted PDFium DLL path | Runtime handle; reuses the process-wide runtime for the same canonical path. |
| `Engine::open` | Source path | Document with immutable original bytes and empty history; rejects encrypted/invalid/oversized PDFs. |
| `Engine::pages` | Document | Page dimensions in PDF points, in page order; uses fast PDFium metadata, with indexed page-loading fallback on metadata failure; rejects invalid dimensions and propagates loading errors. |
| `Engine::text_objects` | Document, zero-based u16 page | Page-level text objects in paint order, with revision, native index, text, font, size, color, matrix and bounds. Uses one character-ownership pass; excludes generated spacing and nested objects. |
| `Engine::hit_test` | Document, page, PDF-point x/y | Last-painted text object containing the point, or None. |
| `Engine::render` | Document, page, positive finite pixels-per-point scale | Image with width, height and tightly packed RGBA bytes, top-left raster origin. |
| `Engine::pixel_to_page` | Document, page, render scale, integer pixel x/y | Corresponding PDF-point x/y, including page rotation. |
| `Engine::page_to_pixel` | Document, page, render scale, PDF-point x/y | Corresponding integer raster coordinates, including page rotation. |
| `Engine::edit` | Mutable document, current selected TextObject, single-run replacement | Atomic edit, EditResult warning and changed-object count; stale selections and failed round trips are rejected. |
| `Engine::edit_group` | Mutable document, current TextObjects from one page, single-run replacement | One atomic, undoable edit: the first object in paint order receives the text, the others are removed. |
| `Bounds::contains` | Bounds, PDF-point x/y | Inclusive axis-aligned hit-test boolean. |
| `Document::can_undo` | Document | Whether a previous snapshot exists. |
| `Document::can_redo` | Document | Whether a redo snapshot exists. |
| `Document::undo` | Mutable document | Restores previous bytes; true on change, false on empty history. |
| `Document::redo` | Mutable document | Restores next bytes; true on change, false on empty history. |
| `Document::suggested_save_path` | Document | Original directory plus `<original-stem>_edited.pdf`. |
| `Document::save_as` | Document, new PDF path | Flushes a temporary file before publishing the complete PDF; rejects original path, existing files and alternate data-stream names. |
| `Substitution::config_path` | None | `%APPDATA%/PdfEditor/settings.txt`, or an error if APPDATA is unavailable. |
| `Substitution::load` | Config path | Line or Document policy; absent file defaults to Document, invalid content returns an error. |
| `Substitution::save` | Policy, config path | Writes a small config through an exclusively created temporary file and rename. |

## Types and integration rules

- `Document::substitution` is the active `Line` (default) or `Document` fallback
  policy. Load the saved policy after opening, and call Substitution::save when
  a setting changes. It applies only when the same font cannot be reused or found.
- `TextObject::index` is a native object index, not an index into the returned
  text-only vector. Pass the complete object to edit. Refresh selections after
  edit/undo/redo, which invalidate their revision. Empty text deletes the run.
- Browser print output often places each glyph in its own text object. Use
  `edit_group` to replace such a line at once. Space-only glyph objects report
  empty text. Icon-font glyphs whose text differs between parsers cannot be
  edited and return an error.
- `TextObject::matrix` is `[a,b,c,d,e,f]`; size is the unscaled font size in points;
  color is `[r,g,b,a]`; bounds use PDF coordinates. They are axis-aligned extents,
  including for rotated text. Font warnings are display data, never markup.
- `Image::rgba` has exactly width × height × 4 bytes. Use the same scale for
  rendering and coordinate conversion; scrolling offsets belong to the UI.
- `EditResult::objects_changed` is zero for a no-op. A new edit clears redo;
  oldest undo snapshots are evicted when their total exceeds 256 MiB.
- Save As leaves the original path and history unchanged. On an I/O failure a
  newly created destination can be partial; the original remains untouched.
- Call engine work off the UI thread for large PDFs. The process-wide PDFium
  runtime serializes complete core transactions across threads, including nested
  core calls and native versioned saves. Do not initialize another independent
  PDFium binding in the same process or bypass the core with native calls.

The README records supported font formats, unsupported mappings and resource
limits. The core has no UI, dialog, network or telemetry dependencies.

- `Engine::find_text(document, query)` -> case-insensitive literal `SearchMatch` list in page order; each match has page index and PDF-point bounds, empty query returns none.

- `Engine::system_fonts()` -> sorted installed TrueType face names for the picker.
- `Engine::add_text(document, NewText)` -> atomically append a single run at a PDF-point baseline with selected system font, size and RGB color; undoable, rejects unavailable glyphs/embedding restrictions.

- `Engine::delete_text(document, selected)` -> removes exactly one current text object, preserves neighboring positions and returns `EditResult`; undoable.

- `Engine::style_text(document, selected, size, rgb)` -> atomically change one current object’s point size and fill/stroke RGB; preserve font, encoded glyphs and neighboring state; undoable.

- `Engine::thumbnail(document, page, edge)` -> aspect-preserving RGBA page preview; longest edge 16–512 pixels (rounding up at most one pixel).

- `Engine::edit_pages(document, PageEdit)` -> undoable clockwise 90° rotation, deletion, move to final zero-based index, or insertion of all source pages before an index; rejects empty documents and invalid positions.

Keyboard commands are UI dispatch only: Open/Save As use `Engine::open`/`Document::save_as`, undo/redo use `Document` history, Find uses `Engine::find_text`, and zoom uses `Engine::render`.

- `Settings::load(path)` -> substitution policy and up to ten recent paths; accepts legacy policy-only files.
- `Settings::remember(path)` -> move a path to the front, deduplicate and retain ten.
- `Settings::save(path)` -> atomically persist settings; paths use bounded UTF-8 hex encoding. `Substitution::save` preserves recent paths.

- `ChangeState::default()` / `ChangeState::recovered()` -> clean or unsaved initial history marker.
- `ChangeState::edited()` / `undone()` / `redone()` -> track a successful history transition, invalidating an abandoned save point.
- `ChangeState::mark_saved()` -> mark the current position saved after successful Save As.
- `ChangeState::is_dirty()` -> whether the current position differs from the reachable saved position.

- `startup_path(arguments)` -> optional OS-native PDF path from arguments after the executable, accepting an optional `--`; errors on empty paths or extra arguments. Opening still uses `Engine::open`.

- `Engine::prepare_print(document)` -> temporary raster `PrintJob`, at up to 300 DPI within render/spool limits; dropping it removes the job.
- `PrintJob::directory()` / `page_count()` -> private spool path and number of pages for native print integration.

## Pictures and signatures

- `Picture::open(path) -> Result<Picture>`: bounded PNG/JPG decode to RGBA.
- `signature_image(strokes) -> Result<Picture>`: normalized drawing strokes to transparent signature pixels.
- `store_signature(folder, picture) -> Result<PathBuf>`: save a reusable PNG under a new exclusive filename.
- `signature_files(folder) -> Result<Vec<PathBuf>>`: list saved signature PNGs.
- `Engine::place_picture(document, picture, Placement) -> Result<()>`: embed transparent image at PDF-point position and size as undoable page content.

## Forms

- `Engine::form_fields(document) -> Result<Vec<FormField>>`: list PDFium text, checkbox, radio and dropdown widgets with names, values, read-only flags and revision identifiers.
- `Engine::fill_field(document, field, value) -> Result<()>`: apply a revision-checked value using native form interactions, regenerate appearances, verify persisted values, and record one undo step.

## Find and replace

- `Engine::replacement_count(document, query) -> Result<usize>`: count case-insensitive literal occurrences in editable text, including adjacent runs.
- `Engine::replace_text(document, query, replacement, occurrence) -> Result<usize>`: replace one zero-based occurrence or all (`None`), preserve fonts through the existing edit engine, and commit a single atomic undo step.

## Passwords

- `Engine::open_with_password(path, password) -> Result<Document>`: decrypt a bounded PDF into editable memory; incorrect passwords return `PASSWORD_REQUIRED`.
- `Document::is_password_protected() -> bool`: identify encrypted sources for recovery policy.
- `Document::save_as_with_password(path, password) -> Result<()>`: exclusive AES-256 encrypted export, or ordinary Save As for an empty password.

## Merge and extraction

- `page_ranges(input, page_count) -> Result<Vec<u16>>`: parse one-based page lists and ascending ranges into validated zero-based indices.
- `Engine::merge_pdfs(document, paths) -> Result<()>`: append multiple PDFs in order as one atomic undoable operation.
- `Engine::extract_pages(document, pages, destination) -> Result<()>`: export selected pages in the supplied order to a new PDF.
- `Engine::split_pdf(document, ranges, folder) -> Result<Vec<PathBuf>>`: export semicolon-separated page groups as split-1.pdf, split-2.pdf, etc., refusing existing destinations.

## PDF annotations

- `Engine::annotations(document) -> Result<Vec<Annotation>>`: list supported real PDF markup and sticky notes with revision-bound identifiers.
- `Engine::add_annotation(document, markup) -> Result<()>`: create highlight, underline, strikethrough or note with geometry, color, opacity and comment.
- `Engine::update_annotation(document, selected, markup) -> Result<()>`: update the selected annotation and its appearance.
- `Engine::delete_annotation(document, selected) -> Result<()>`: remove the selected annotation as an undoable edit.

## Placed images

- `Engine::placed_pictures(document) -> Result<Vec<PlacedPicture>>`: list images inserted by the editor with revision identifiers and point geometry.
- `Engine::move_picture(document, selected, placement) -> Result<()>`: change an inserted image position and dimensions in one undoable operation.

## Multiple documents

Each `Document` owns its byte snapshots, revision, undo/redo stacks and source identity. Multiple documents may share one `Engine`; mutations and history operations affect only the supplied document. The UI owns one worker/session per tab.

## Windows OCR

- `Engine::ocr_page(document, page) -> Result<usize>`: recognize a scanned page with installed Windows OCR languages and add an invisible, searchable text layer as one undo step; returns the number of recognized words.

## Secure redaction

- `Engine::redact(document, page, bounds) -> Result<()>` and `Engine::redact_regions(document, &[(page, bounds)]) -> Result<()>`: atomically remove entire intersecting text runs, images, paths and form groups, prune unused resources and old streams, verify extraction, then draw black rectangles. Other page content stays editable. Batch redaction creates one undo step. A fresh page tree drops forms, annotations, bookmarks, attachments, actions, tags and metadata. Marked content with properties, retained Type 3 fonts, patterns, shadings, function resources, forms with inherited resources, soft-mask graphics states and unverifiable geometry fail closed. Nested encoding and resource parameter dictionaries discard nonstandard auxiliary fields. Save As writes a fresh file; the original and in-session undo history remain unchanged.

## Image compression

- `Engine::compress_images(document, maximum_edge, jpeg_quality) -> Result<CompressionReport>`: downscale/recompress supported RGB XObjects while preserving text and alpha masks; report byte sizes and changed/skipped counts, retaining the original when no bytes are saved.

## Page numbers and watermarks

- `Engine::stamp_pages(document, TextStamp) -> Result<()>`: add positioned text to every page with font, size, color and opacity; expand `{page}` and `{pages}` placeholders and record one undo step.

## Page image export

- `Engine::export_page_image(document, page, dpi, format, destination) -> Result<()>`: render one page at 36–600 DPI to a new PNG/JPG within raster and file-size limits.
- `Engine::export_page_images(document, pages, dpi, format, folder) -> Result<Vec<PathBuf>>`: export an ordered page selection to new numbered image files, cleaning up outputs if the batch fails.

## Page cropping

- `Engine::crop_page(document, page, bounds) -> Result<()>`: set a validated CropBox inside the inherited MediaBox as one undoable change, preserving underlying content.

## Drawing annotations

- `Engine::add_drawing(document, Drawing) -> Result<()>`: create a freehand, rectangle, ellipse, line or arrow as a standard PDF annotation with color, width, opacity and page-space geometry.

## Bookmarks

- `Engine::bookmarks(document) -> Result<BookmarkSet>`: list nested outline titles, local page destinations, sibling positions and a document revision; remote actions are never executed.
- `Engine::edit_bookmarks(document, revision, BookmarkEdit) -> Result<()>`: atomically add a root/child bookmark, rename, delete a subtree or move within siblings, rejecting stale revisions.
- `BookmarkSet`: revision and flattened depth-annotated entries; `Bookmark`: object identifier, title, optional zero-based page, depth and sibling positions.
- `BookmarkEdit`: Add(title, page, optional parent), Rename(id, title), Delete(id), or Move(id, final sibling position).

## Command-line integration

The console companion uses the existing public Engine/Document methods directly;
no new public core API is introduced. Its parser validates per-command arguments,
and batch find/replace counts through `replacement_count` before `replace_text`.
All destinations retain the core exclusive-create and original-file protections.

## PDF comparison

- `PagePair { left: Option<u16>, right: Option<u16> }`: original/revised zero-based page indices; `None` means an inserted or deleted page.
- `Engine::comparison_fingerprint(doc, page)` -> session-only `u64` fingerprint of page dimensions, a 64-pixel preview and text. Used for matching, not security or persistent identity.
- `align_comparison_pages(left_fingerprints, right_fingerprints)` -> ordered `Vec<PagePair>`. Preserves exact-match anchors, pairs edited gaps by order and emits missing sides. LCS memory is limited to four million `u16` cells; larger documents use 64-page lookahead. Accepts at most 32,767 pages per file.
- `Engine::compare_page_pair(left, right, pair, overlay)` -> `ComparePage` for explicit aligned indices. Rejects invalid indices and two missing sides; does not mutate documents.

- `compare_text(left, right)` -> bounded ordered-run `TextChange` list (Added, Removed, Changed), with optional original PDF-point bounds on each side; exact Unicode text equality and 32-run lookahead. Limits: 20,000 runs on either side and 8 MiB combined text per page pair. This is run alignment, not semantic paragraph matching.
- `compare_pixels(left, right, overlay)` -> differing pixel count; optionally tint differences magenta in each optional RGBA image. Missing pixels differ, equal pixels are unchanged. Validates raster buffers and limits each raster to 1.1 megapixels and the union canvas to 2.2 megapixels.
- `Engine::compare_page(left, right, page, overlay)` -> `ComparePage` containing optional side images, text changes, changed-pixel count and raster-space highlight rectangles. Uses a shared scale capped at one megapixel and 4096 pixels per edge, permits missing pages on either side, rejects an index missing on both. Documents remain unchanged.

Comparison UI runs loading, page matching, text extraction, rendering and raster comparison on a background thread with bounded queues, cancellable between pages. It retains one displayed page pair, one pristine worker preview and numeric summaries, and compares saved files rather than unsaved editor state. TextChange includes optional before/after excerpts limited to 160 characters plus an ellipsis per side. Excerpts remain in memory. Switching overlay modes reuses the pristine preview rather than reparsing the PDFs. Encrypted inputs accept transient passwords. Matching uses unchanged-page anchors by default; page-number mode is available explicitly and is the fallback if fingerprinting fails. Repeated pages, reordered pages and modified gaps remain best effort. Raster previews may omit changes smaller than their resolution. No source text is written to logs or disk.

### Windows shell boundary

Single-instance IPC and per-user shell registration live in the desktop application.
They introduce no core API: incoming paths use existing document open operations,
while merge/compress/export use the existing public operations after UI confirmation.

## Paragraph editing

- `paragraphs(objects) -> Vec<Paragraph>`: group current page text into compatible lines and paragraphs with bounds, text, spacing, alignment and line-edit fallback reason; dense, mixed, rotated and ambiguous column layouts stay conservative.
- `Engine::edit_paragraph(document, paragraph, text) -> Result<EditResult>`: revision-checked, atomic reflow within the detected width using PDF glyph widths and embedded font metrics, original line spacing and alignment; extends downward with an overlap warning and rejects page overflow or words wider than the block.
- `ParagraphAlignment`: Left, Right, Center or Justified; justified reflow expands word gaps except on the final line.
- `Paragraph`: current objects, PDF-point bounds, text, line spacing/count, detected alignment and optional fallback reason. Callers must refresh after edits/history operations. Unsupported font encodings/metrics fail without altering the document.

Paragraph fallback substitution stays inside the paragraph even when the ordinary line-edit policy is document-wide, so unrelated content remains unchanged. Newlines request hard line breaks; Ctrl+Enter applies a paragraph in the canvas editor; Alt+click selects an individual line.

## Corpus validation executable
The opt-in corpus-runner binary exercises existing core APIs inside Docker only. It adds no public library functions and suppresses parser diagnostics and panic text.

### Ranged headers and footers
`RangeStamp` combines a `TextStamp`, one-based `pages` ranges (empty selects all), a literal date and `StampPlacement::{Header,Footer,Absolute}`. `Engine::stamp_page_range` expands `{page}`, `{pages}` and `{date}` in one atomic undo step. Header/footer offsets follow the visible cropped/rotated page; y is the edge margin. Invalid ranges, margins and control characters fail without modifying the document. `stamp_date_today()` supplies a UTC date; callers capture it once and may override it.

### Images to PDF
- `import_picture(path) -> Result<Picture>`: bounded PNG/JPEG decoding; HEIC uses installed Windows image codecs with orientation applied.
- `Engine::images_to_pdf(paths, ImagePageSize) -> Result<Document>`: creates 1–100 image pages in input order, with A4, Letter or image-size (96 DPI) pages.

### Ungroup grouped artwork

`Engine::ungroup_page_item(document, item)` expands a selected Form XObject and
its nested forms into independent page objects in one undoable transaction.
Selection uses the current revision and page object index. Existing transforms,
resource bindings, paint order and form bounding-box clips remain intact; other
instances sharing the same form remain grouped. Unsupported transparency groups,
optional-content forms, reference forms, inline images, excessive nesting and
ambiguous selections return an error without changing document history.

`Engine::prepare_grouped_text(document, item)` exposes a clicked group's text.
Preserve existing page compositing, or promote the selected group's compositing
dictionary to page level, and compare full-page renders at 72 and 144 DPI before
accepting the undoable change. Reject fractional alpha, non-normal blending,
soft masks and knockout groups. Surrounding artwork is retained and checked by
the same render comparison.
Allow only compositor rounding of at most one channel level in under 1% of
channel values; larger differences reject the transaction.

## Table export

- `Engine::detect_table(document, page, area) -> Result<TableGrid>`: infer a bounded grid from PDF text positions and transformed horizontal/vertical ruling lines inside a PDF-point selection.
- `TableGrid::export(path, TableFormat::{Xlsx,Csv}) -> Result<()>`: export edited cell values to a new local file; XLSX uses explicit string cells, CSV neutralizes formula prefixes.

## Word export

- `Engine::export_word(document, path) -> Result<()>`: create a new local `.docx`
  containing paragraph flow, font names and sizes, bold/italic runs, inline images
  and simple ruled tables. Ordinary Form XObjects are expanded in a temporary
  document. Unsupported transparency groups return an error before output is
  written. The source bytes and undo history are unchanged.

## Form designer

- `DesignFieldKind`: Text, Date, Checkbox, Radio with options, or Dropdown with options.
- `NewFormField`: zero-based page, unique name, required flag, PDF-point rectangle and kind. Radio group rectangles are divided vertically between their options.
- `Engine::designed_fields(document)`: bounded widget list with object IDs, revision, page, name, required flag, kind and rectangle.
- `Engine::design_form(document, FormDesignEdit)`: atomically add a field, update a current widget's rectangle/name/required flag, or delete it. A radio child's name/required flag belongs to its group. Empty ancestors are pruned. History supports undo/redo; stale references and invalid rectangles fail without changes.

Date fields are ordinary text widgets with a YYYY-MM-DD tooltip, without JavaScript or validation actions. Dropdowns and radio buttons use native PDF fields. The Forms window's Design tab provides a draggable rectangle, resize corner and exact PDF-point coordinate controls; Apply commits the draft. Refresh retrieves current fields after edits.

## Form flattening

`Engine::flatten_forms(document) -> Result<usize>` paints each widget's selected
normal appearance into page content, removes widget annotations and the AcroForm
field tree, and returns the widget count. Other annotations remain editable.
Appearance BBox/Matrix and widget Rect determine placement and clipping. Hidden
widgets are removed without painting. Missing or malformed appearances fail
atomically: fill those fields before flattening. The operation preserves vector
text and images, supports undo/redo, and is a no-op when no widgets exist.
The Forms window exposes Flatten forms alongside Fill and Design.

### Scan cleanup
- `detect_scan_corners(picture) -> Result<[[f32; 2]; 4]>`: estimates clockwise normalized paper corners from the largest bright region; defaults to the full image when detection is uncertain.
- `clean_scan(picture, options) -> Result<Picture>`: perspective correction, deskew up to ±20 degrees and color/grayscale/black-and-white conversion with bounded output.
- `Engine::scan_preview(document, page) -> Result<Picture>`: renders an image-only, annotation-free page at up to 180 DPI and 2500 pixels per side.
- `Engine::cleanup_scan(document, page, options) -> Result<()>`: replaces the photo page, optionally adds Windows OCR and commits one undo snapshot; any failure leaves the document unchanged.

## Document sanitization

`Engine::sanitize(&mut Document, SanitizeOptions) -> Result<SanitizeReport>` performs
one undoable transaction. Metadata, embedded files and active actions are removed
recursively; ordinary HTTP(S)/mail links and internal destinations can be retained.
Options separately remove comments/drawing annotations and link annotations.
The report counts removed entries and hidden text blocks, not unique payload files.

Invisible text rendering (mode 3) and zero fill/stroke opacity are supported,
including form XObjects. Text clipping, optional layers, soft masks, inherited
non-default visibility in forms and mixed visible/hidden blocks fail without
changing the document. This is not a guarantee against every visual concealment
technique (such as same-color text, occlusion or off-page text). Save As writes a
fresh pruned object graph. Undo/recovery retain the original until discarded.

Sanitization also rejects XFA forms, retained annotation appearances, unused retained Form
streams, Forms without their own resources, and aliased attachment/script/metadata payloads.
These cases leave the original document unchanged.

### Search and redact
`PrivacyQuery` selects literal case-insensitive text or email, phone, IBAN,
Finnish personal identity code and credit-card patterns. `preview_redactions`
returns page-space rectangles across adjacent text runs. Presets are pattern
candidates, not identity/account validation. `apply_redaction_preview` rejects
changed or different documents and applies all rectangles in one undo step.
Intersecting text runs and grouped artwork may be removed in full.
Redaction rebuilds a clean page graph, dropping document-wide annotations, forms, bookmarks, attachments, tags, metadata and actions; unsupported retained content fails without changing the document.

## Spell checking

- `check_spelling(text, language)` returns bounded spelling issues using installed Windows dictionaries (`en-US` or `fi-FI`); UTF-16 results are converted to UTF-8 byte ranges. Unsupported platforms/dictionaries return a readable error. Run on a background thread.
- `SpellingIssue` contains a byte range, original word and up to eight suggestions.
- `replace_spelling(text, issue, replacement)` checks the current word and Unicode boundaries before replacement; stale results fail without changing text.

`BatesNumber { prefix, start, digits }` formats fixed-width decimal labels. `label(offset)` rejects invalid prefixes, widths outside 1–20, overflow or a number wider than the chosen width. `Engine::stamp_bates(doc, range_stamp, bates)` numbers selected pages in document order, preserving leading zeroes in one atomic undo step; the text template is ignored.

### Batch stamping CLI
`stamp` and `bates` accept a PDF or a nonrecursive folder of PDFs. They use the same core range-stamping methods. Folder outputs retain the source basename in an existing output folder and never overwrite. Bates sequences continue across successful files in sorted filename order. A date is captured once per batch. Dry-run performs the same in-memory validation, returns counts and writes nothing. Per-file failures continue, retain successful outputs and produce exit code 1; usage errors return 2.


Word export expands groups only when compositing can be preserved. Other complex
pages are rendered from the original document into bounded page images; remaining
pages retain editable text and tables. Export never modifies source history.


SanitizeOptions.flatten_visible rebuilds a document of up to 100 pages using
bounded rendered images, retaining page dimensions and one undo step. It copies
no original objects, text layer, scripts or metadata. SanitizeReport.rasterized_pages
identifies this mode. Editable sanitization remains the default and fails closed
when visibility cannot be verified.


ObjectAction::Resize accepts a selection on one page. Its common bounding box
maps to the requested bounds; each object's transform uses the same scale and
translation, preserving relative placement. The operation remains atomic.

Page insertion/merge and extraction/split preserve AcroForm field ancestry,
normal appearances, values and default fonts. Imported fields with colliding root
names receive a numeric suffix. Shared groups retain only selected widgets during
extraction. XFA, repeated form-page selections, conflicting non-font default
resources and form references to unselected pages return errors without changing
the destination or creating an output file.
