# Engine decisions

## Bulk edit ordinal lookups

Index replacement targets once and each page's expected changes once, preserving
the ordered plans for paint order, reverse mutation and cursor repairs. Keep the
first replacement for a duplicate internal target. Indexed verification uses
original run ordinals, including deleted positions, and retains independent
native text and matrix checks. Generated dense pages bound lookup work while
mixed character maps, sparse deletions and duplicate targets preserve behavior.

## Paragraph page coordinates

Validate paragraph placement against PDFium's MediaBox/CropBox intersection in
PDF user space. Display width and height rotate and omit the page origin, so they
cannot bound text matrices. Keep horizontal and font-ascent/descent overflow
checks in the same unrotated coordinates as the paragraph. Invalid or clipped
bounds fail before accepting the candidate, preserving history and document bytes.

## Identical font formatting

Skip a formatting transaction only when every selected run already has the
requested size, visible colors and complete canonical font resource. Compare
decompressed font programs, widths and character maps through bounded indirect
resource traversal; names alone cannot establish font identity. Ignore only
stream compression and indirect numbering. Reuse the native stroke-color check
from size/color styling. Unknown, custom or differing resources retain the normal
verified formatting path. A skipped transaction preserves bytes, recovery state,
redo and preview generations.

## Paragraph encoding reuse

Prepare the template font's decoder once for paragraph measurement and output.
Reuse the same checked encoder for distinct character advances, justified words
and emitted lines, retaining round-trip validation for every string. The parsed
source font remains immutable while the output document is rebuilt separately.
A generated many-character paragraph bounds decoder construction independently
of timing and verifies saved rendering and undo/redo.

## Paragraph font preparation

Require embedded metrics when preparing a paragraph's template run. Keep the
normal line-edit reuse path for standard fonts and Type3 glyphs. Paragraphs use
the existing licensed font fallback when metrics are unavailable, with the same
substitution warning and paragraph-only scope. Shared selection, input and native
round-trip checks remain in place, including preparation for hard line breaks
whose flattened text matches the original run.

## Bulk text encoding

Reuse decoded original font maps within each edited page, keyed by the PDF font
resource rather than its display name. Keep the same encode/decode round-trip
check for every replacement and release borrowed maps before modifying the PDF.
A generated 1,000-object regression reduces decoder construction from 1,002 to
three builds and checks a second case with two same-named faces whose CID maps
differ. Independent native extraction, saved rendering and undo/redo remain checked.

## Identical find/replace matches

Record exact equality with the replacement while planning native text matches,
without retaining duplicate strings. Keep identical matches in occurrence numbering
but skip their mutation. Return the number of changed matches and accept one undo
step only when content changes. This preserves redo and browser glyph-run structure
for identical replacements, including phrases containing generated spaces.

## Empty extracted text versus deletion

An empty replacement deletes a selected PDF text object, even when its extracted
text is already empty. Browser-style space glyphs can have no extracted text
while still owning a real page object and advancing the cursor. Keep the no-op
shortcut for identical nonempty replacements. A generated Type3 regression checks
object removal, unchanged neighboring pixels, saved reopening and undo/redo.

## Unchanged text styles

Compare requested size and color with the current text object after validating
the selection, checking visible stroke color for outlined text. Skip identical
styles before rebuilding the PDF. Worker replies
derive their change count from the document revision, so an unchanged style
does not invalidate previews or mark the document dirty. Generated core and UI
workflows verify original bytes, saved state and redo branches remain intact.

## Standard PDF font reuse

Reuse the twelve standard Latin Type1 faces when they have no custom descriptor,
width table or ToUnicode map and use a supported predefined encoding. Encoding
must round-trip the replacement before reuse. Symbolic fonts, subsets, custom encodings and missing
glyphs retain the existing fallback checks. Generated cases cover all twelve faces,
four page rotations and default, Standard, WinAnsi and MacRoman encodings, including
neighbor pixels, source preservation, saved rendering and undo/redo.

Partial width tables retain the verified font fallback so newly inserted characters
receive usable metrics instead of failing text extraction.

## Cursor anchors inside saved graphics state

Allow absolute text cursor repairs across balanced outer `q`/`Q` pairs when
the existing eligibility checks rule out matrix transforms and line-position
dependencies. Keep the sequential fallback for graphics saves within text
objects, unbalanced nesting and transformed layouts. A generated 150-pair chain
requires two verification passes instead of 59, with neighboring pixels, saved
output and undo/redo checked independently.

## CLI engine startup errors

Report PDF engine initialization failures at the CLI startup boundary, naming
the required library without exposing loader diagnostics or local paths.
Help and version commands remain available without loading the engine.
A generated-PDF test copies the CLI without its library and checks the failure,
source preservation and absence of output files.

## Preview request freshness

Recheck the input channel between worker jobs, including after a bounded reply
send. Append new requests after the remaining batch before using the existing
coalescer, so stale previews are skipped without moving work across a document
mutation. A generated-PDF rendezvous test verifies obsolete native renders are
removed while renders on either side of a page rotation retain their order.

## Zoom anchor coordinates

Zoom page coordinates while keeping canvas padding and page gaps in UI points.
Account for page centering and the width of the page under the pointer, including
documents with mixed page sizes. Compose successive zoom requests with the pending
scroll offset so input received before the next layout does not move the anchor.
Generated-document tests check screen positions, fixed gaps and unchanged document
history across zoom-in and zoom-out transitions.

- Keep Part A headless; no UI or file-dialog dependency is present.
- Pin pdfium-render to 0.9.4 (released 2026-09-06) and disable image adapters;
  page rendering can return pixels without another image dependency.
- Pin the non-V8 Windows x64 PDFium distribution to chromium/8111. Verify its
  published SHA256 before extracting it into the ignored target directory.
  The setup script places the library beside debug and release build outputs.
- Use JoniFinskas as the license copyright holder, matching local Git identity.
- Do not suppress RUSTSEC-2026-0192. The original ttf-parser 0.25.1 dependency is
  unmaintained, and the dependency audit reports no safe upgrade.

## Font parser replacement

Replaced ttf-parser with `skrifa = "=0.48.0"` for font metadata and glyph coverage.
This version was released 2026-10-03, more than 14 days before 2026-10-22, and is
MIT OR Apache-2.0 licensed. The RustSec advisory recommends skrifa as a maintained
alternative. All 115 locked registry packages passed the release-age check on
2026-10-22; their latest eligible versions are retained in Cargo.lock. The
dependency audit requires no advisory exceptions. Fourteen too-recent transitive
versions were replaced with older compatible releases during lockfile preparation.

## PDF content editing

PDFium's public API supports setting text on existing objects, but exposes no
font replacement or character-spacing setter. Font substitution must preserve
spacing, matrix, graphics state, and surrounding objects. A replacement strategy
must be demonstrated by render-diff tests before it is treated as supported.
Use lopdf 0.45.0 (released 2026-09-08) for structured content-stream edits and font
resources. This additional parser is necessary because the public FPDF API cannot
preserve all required text-state properties when replacing a font. Keep PDFium
as the independent renderer and text-object inspection backend. Do not implement
a handwritten general PDF parser.

PDFium content regeneration was rejected after a render-diff fixture demonstrated
that it drops character spacing on neighboring objects. Preserve content streams,
replace the selected show operation, and add measured numeric TJ adjustments to
restore subsequent text cursors. Reject mismatched object mappings transactionally.
Bounding-box render checks use the union of old and new extents with four raster
pixels of padding, since a longer replacement can legitimately extend the box.

## Fonts, history and files

- Use existing embedded TrueType programs whenever glyph coverage permits. Add a
  new Unicode/CID mapping with the same program and font name when encoding alone
  is insufficient. System fallback checks family, bold/italic, glyph coverage and
  editable embedding permissions; TrueType collection faces are reconstructed as
  standalone sfnt programs. CFF-only/Type1 substitution uses a TrueType fallback.
- Keep complete immutable PDF byte snapshots for atomic edits and undo/redo. This
  is simpler than reversing partial PDF resource mutations. Cap undo storage at
  256 MiB and evict the oldest snapshots. This can consume more memory than deltas.
- Require exclusive creation on Save As. Never overwrite any existing output,
  including links; also reject the original path when its file has been removed.
- Persist the small policy through a unique temporary file and rename. The UI
  integration loads and saves the policy using the documented core functions.
- Keep forms, annotations, OCR and images outside editing scope. Ambiguous or
  nested text mappings return a clear error; no speculative stream rewrite occurs.

## Fixture provenance

Inter 4.1 comes from the official rsms release; the source archive hash is recorded
in tests/fonts/README.md and the OFL is committed. The renamed, physically subsetted
fixture is generated using a checksum-pinned fontTools 4.60.1 wheel, released
2025-09-29. It is a fixture-regeneration tool only, stored under ignored target,
with no installation or import in the Rust runtime or ordinary test suite.

The LibreOffice PDF is exported from a committed FODT containing fabricated text
using a fresh isolated profile. Tests treat exported files as read-only.

## Desktop UI

- Converted the repository to a virtual workspace with `core` and `app` members.
  cargo-deny analyses only the root package of a non-virtual manifest, so the UI
  dependency graph was not being checked. The core manifest moved to
  core/Cargo.toml; its test helper now resolves the repository root from the crate
  directory. No core API changed.
- Pinned eframe 0.36.2 (released 2026-09-08). 0.37.0 was released 2026-10-15 and
  is below the 14-day floor. The glow renderer is used instead of wgpu to keep the
  binary smaller; egui's default fonts are disabled because Inter is embedded.
  AccessKit stays enabled for screen readers.
- Pinned rfd 0.17.2 (released 2026-01-12) with default features disabled; those
  features select Linux portal backends that a Windows build does not use.
- Embedded Inter 4.1 Regular and SemiBold from the official release with the OFL
  (app/assets/fonts/README.md records hashes). Segoe UI and Segoe UI Symbol are
  loaded at runtime from the Windows font directory, when present, as glyph
  fallbacks for non-Latin text in the edit box. They are not redistributed.
- All core calls run on one worker thread that owns the Engine and Document.
  Queued render and object requests for the same page are coalesced. Replies carry
  a document generation; the UI drops replies from an older generation.
- Hover and click hit testing use cached `text_objects` results plus an affine
  page-to-raster mapping derived from three `page_to_pixel` samples per page.
  Each core call reparses the PDF bytes, so calling `hit_test` on every pointer
  move would block the worker. Bounds are axis-aligned and page rotation is a
  multiple of 90 degrees, so the mapped rectangles select the same last-painted
  object as `hit_test`.
- 100% zoom maps one PDF point to 96/72 interface points. Render scale follows
  zoom and display scaling, quantized downward to 1/64 and capped below the
  core's 16 megapixel limit and an 8192 pixel texture side. Higher zoom levels
  upscale the capped texture. Textures of pages more than two pages outside the
  viewport are released.
- The edit box uses the run's font size scaled by its text matrix, because
  PDFium bounds are glyph-tight (text without descenders is shorter than the
  em box). Bounds height is the fallback when the matrix disagrees with them.
  The box widens with the text and uses the run's color, in the Inter UI font.
- Enter or moving focus elsewhere applies an edit, and only Escape cancels it,
  matching common editors so typed text is not lost silently. Unchanged text
  sends no edit. Clicking another line applies the open edit first, then opens
  the clicked line once the page's objects are refreshed, because the earlier
  selection becomes stale. Open, Save As, Settings and closing the window apply
  an open edit first; with uncommitted typing, Undo discards the typing only.
- Save As is the only save path (Ctrl+S also opens Save As). A missing `.pdf`
  extension is appended. Opening another file or closing with unsaved edits asks
  for confirmation; the original file is never modified in either case.
- Settings load at startup through `Substitution::config_path` and
  `Substitution::load`; a read failure shows a warning banner and uses the
  default. Changes are saved immediately and applied to the open document.
- Banners stack at the top of the canvas and are dismissible. Font warnings and
  errors persist until dismissed; save confirmations expire after four seconds.
  Render failures are shown inside the affected page instead of as banners.

## UI dependency audit

- Locked transitive versions newer than the 14-day floor were replaced with the
  newest older compatible releases (arboard, combine, litrs, memmap2,
  redox_syscall, smallvec, toml_edit, toml_parser, uuid, zbus_names, zerocopy,
  zerocopy-derive, zvariant, zvariant_derive).
- clipboard-win is an exception to the 14-day floor: eframe requires arboard 3.6,
  which requires clipboard-win ^5.3.1. On 2026-10-23 every 5.2.0 to 5.4.1 release
  was yanked upstream after an out-of-bounds read in `raw::get_html`
  (upstream issue 42), and 5.4.2 and 5.4.3 were published that day. Pinned 5.4.2,
  the release that contains only that fix. Its source diff replaces an unchecked
  `copy_nonoverlapping` with a bounds-checked slice. Revisit when a later release
  passes the floor.
- Allowed BSL-1.0 (Boost Software License 1.0) for clipboard-win and error-code.
  It is OSI-approved and permissive, and it requires no notice in binary
  distributions.
- Ignored RUSTSEC-2026-0370, -0371 and -0372 (zvariant, zbus_names,
  zvariant_derive unmaintained). They are transitive Linux-only dependencies of
  AccessKit's AT-SPI backend through eframe, with no safe upgrade until AccessKit
  moves to zbus 6. The Windows build does not compile them. deny.toml records
  the justification.
- The private app crate (`publish = false`) uses an unversioned path dependency
  on core, allowed by `allow-wildcard-paths`, so the version is set in one place.

## Versioning

- Semantic Versioning with one version for the workspace, set only in
  `[workspace.package]` of the root manifest and inherited by both crates. A
  shared version suits two crates that are released together; the rules are in
  VERSIONING.md. The interface reads it from `CARGO_PKG_VERSION`, and debug
  builds are labelled so test reports identify them.

## Browser-printed PDFs

- Chrome print output uses Type3 fonts without BaseFont, glyph names in
  /Differences plus a ToUnicode CMap, and one show operator per glyph. The page
  mapping rejected such pages: lopdf ignores ToUnicode when /Encoding exists,
  so its text disagreed with PDFium's. Decoding now prefers ToUnicode, matching
  PDFium and the PDF text-extraction rules.
- Every show operator with glyph bytes is part of the mapping, because PDFium
  creates an object for each one. Whitespace is not compared, since PDFium
  reports space-only glyph runs as empty text. A run whose own decoding is only
  private-use code points (icon fonts) may differ from PDFium's text. Such a run
  keeps its place in the mapping, but editing it returns an error. Counts and
  all other text must still match.
- Type3 glyphs are reused when every replacement character maps to an existing
  glyph procedure. Otherwise the normal fallback search applies with a warning.
  Fonts without BaseFont or Name are identified by a hash of their glyph table
  for whole-document substitution.
- Added `Engine::edit_group` instead of a UI loop of single edits, so a line
  of many glyph objects becomes one atomic edit and one undo step. `edit` now
  calls it with one object. Td positioning in such output is relative to the
  line start, so removing later glyph runs does not move other text; any cursor
  change is still repaired by the existing measured correction.

## Line editing, settings page and auto-save

- The UI groups consecutive objects with the same font, size, color and
  horizontal matrix, on one baseline and at most 0.8 em apart, into one line.
  Space-only objects never begin or end a line. The line text adds a space where
  a space object or a gap above 0.2 em separates runs. Editing replaces the
  whole line; the first run receives the text. Runs in other PDFs that share a
  line and format are therefore edited together as well.
- Settings moved from a modal to a full page with Font substitution, Auto-save
  and About sections. Three option groups and their explanations need more room
  than a small modal provides. Escape or Back returns to the document.
- Auto-save writes recovery copies only. Writing to the original or to a Save As
  target would require overwriting files, which the core forbids by design.
  Copies go to a per-session folder under `%LOCALAPPDATA%\PdfEditor\Recovery`
  through `Document::save_as` with new file names. The previous copy is deleted
  after each write, and the folder is removed on a normal exit. Copies are also
  removed after Save As, after undo returns to the saved state, and after edits
  are discarded.
- Each session holds an exclusively opened lock file. At start, folders whose
  lock can be opened belong to editors that are no longer running. Those with a
  copy are offered for restore and empty ones are removed. A restored copy is
  deleted only after its edits are protected again by a new copy or a save, or
  are discarded.
- Auto-save defaults to every 2 minutes and can be set to off or 1, 2, 5 or 10
  minutes. The setting is stored in `%APPDATA%\PdfEditorpp-settings.txt`
  because the core's settings file accepts only the substitution policy. An
  unreadable value falls back to the default.

## Part D: Find text

Use PDFium Unicode search across adjacent runs, case-insensitive, with bounded query/result counts. Search runs on the existing worker and highlights precise match rectangles; navigation wraps. Refresh results after document changes.

## Part D: New text

New text uses a clicked baseline and a single-line input, with installed TrueType faces, point size and RGB picker. Font scanning runs on the worker. Unsupported glyphs or embedding permissions fail explicitly instead of silently replacing the chosen face. Content is appended in isolated graphics state; existing streams are preserved.

## Part D: Delete text

The selected-line toolbar exposes its individual PDF runs so deletion targets exactly one object, including glyph-by-glyph browser PDFs. Deletion reuses the verified empty-text edit transaction and cursor correction; stale selections are rejected.

## Part D: Text size and color

Style changes retain the original encoded glyph operators, font resource and spacing. Scoped graphics state restores surrounding style, and existing cursor correction preserves subsequent positions. Size is bounded to 1–1000 points. The individual-run picker identifies the exact object being styled.

## Part D: Page thumbnails

A virtualized sidebar renders visible 150-pixel thumbnails on the worker, keeps a small nearby cache, and navigates by click. Edits invalidate thumbnails. This bounds texture memory even for long PDFs.

## Part D: Page operations

Page controls operate on the current page; dragging a thumbnail onto another moves it to that index. Insert imports all pages before the current page. Deleting the last page is rejected. Reordering materializes inherited page attributes before rebuilding the page tree. Native PDFium import retains page contents without content regeneration. All operations share snapshot history and refresh page dimensions and caches on undo/redo.

## Part D: Shortcuts

Retain text-field undo ownership while a field is focused. Document shortcuts are tested using real egui key events, including shifted plus/equal keyboard layouts. Add a visible Find action for discoverability. No PDF API is needed for keyboard dispatch; existing headless history and rendering tests cover its core operations.

## Part D: Recent files

Extend the existing settings.txt with ten hex-encoded UTF-8 paths after its original policy line. Legacy files remain readable and changing substitution preserves recents. Record only successful opens, exclude recovery copies, deduplicate case-insensitively for Windows, and route Recent through the same unsaved-change guard as Open.

## Part D: Unsaved changes

Keep the existing close/open discard prompt. Its actions wait for outstanding edits, and failed or canceled replacement opens preserve dirty state and recovery copies. Save-point tracking is now a headless tested core type, including redo-branch abandonment and recovered documents. Only successful Save As marks a document saved.

## Part D: Command-line opening

Retain OS-native argument parsing for spaced and Unicode paths. Accept a single PDF path with optional --, show a usage error for extra arguments, and route the result through the normal worker open and recent-file flow. Missing or malformed PDFs use existing UI errors.

## Part D: Integration verification

Use a separate target/part-d build directory while the normal debug executable is
locked by a running editor. Keep the toolbar responsive by wrapping its actions
at narrow widths. Cache search transforms once per page, isolate background errors
from document lifecycle state, and retain a visible outline for the selected run.
New font resource names avoid collisions, and insertion verifies that existing
text metadata and transforms are unchanged. UI pointer/keyboard tests supplement
core round-trip tests without requiring desktop automation or new dependencies.

## Part E: Restyle

The reference images in design/refs/ are kept out of the repository
(`/design/refs/` in .gitignore) because they are not ours to redistribute.

Match them with egui only, without new dependencies:

- Icons are painted line shapes (app/src/icons.rs) instead of an icon font,
  so no extra font or license is added. Shared controls (icon buttons, keycaps,
  segmented control, settings rows) live in app/src/widgets.rs and carry stable
  ids, which the UI tests use to click icon-only buttons.
- Palette: charcoal surfaces with a blue accent, yellow find matches and a blue
  current match. The light theme uses the same structure, since the editor
  follows the system theme.
- The document toolbar shows icons with labels and drops the labels below
  1120 points, instead of wrapping onto a second row. The minimum window width
  still fits every action.
- Find and the text-object tools float over the canvas instead of taking a
  panel row. The text tools sit above the selected object or the new text point.
- The home page (top bar with Home and Recent, open card, recent list) replaces
  the old empty state. Recent rows show the folder; no open time is stored, so
  none is shown.
- Settings use a left navigation with General, Editing, Shortcuts and About.
  Rows from the references with no behavior behind them are left out: theme
  switch, default zoom, recent-file count, save behavior, warning toggles and
  default font. Fixed behaviors (discard prompts, Save As only) are shown as
  "Always on" rather than as toggles that would do nothing.
- Page menus offer Rotate right only, because the core rotates pages clockwise
  in 90 degree steps. There is no separate Select tool; clicking text selects it.
- Size and color still need "Apply style": each change is an undoable core
  edit, and applying on every drag step would flood the history.
- The status bar keeps the edit hint and the version next to the page count,
  the pages toggle and the zoom menu. The pages sidebar can be hidden.
- Canonicalized Windows paths are shown without the `\?\` prefix.

## Part C: Release build and cleanup

Use size optimization (`s`), thin LTO, one code-generation unit and stripped
symbols for the Windows release executable. Keep panic unwinding because the
core catches recoverable parser panics; abort-on-panic would remove that behavior.
Keep AccessKit accessibility support and the existing non-V8 PDFium binary.

The dependency audit found all direct dependencies in active use: eframe for UI,
rfd for native dialogs, the internal core crate, pdfium-render for native PDF
operations, skrifa for font metadata/glyphs, and lopdf for content editing.
Compiler and strict Clippy checks found no unused private code. Keep documented
public core APIs even when they are used only by library consumers or tests.
No dependency removals or new exceptions are needed.

PDFium setup now copies the archive's third-party notices and verifies a reused
cached archive. Identical installed DLLs are left untouched to allow setup while
a matching editor is running. The manual checklist remains unchecked until a
human run; a release-mode build alone does not justify a version bump or tag.

## Part F: Windows printing

Use the native Windows Forms print dialog and Windows print spooler through the installed Windows PowerShell runtime; no new package or PDF file association is required. The worker prepares private BMP pages at up to 300 DPI (16-megapixel page and 512-MiB spool bounds), fits them within printer margins, and removes temporary files on cancel, success or failure. Printing uses the edited document and does not alter its saved state. Physical printer checks remain separate from automated raster/spool tests.

## Part F: Signatures

Signatures use bounded PNG/JPEG decoding and a transparent drawing pad. Reusable copies live in the config folder's `signatures` directory with exclusive filenames. Placement uses PDF-point controls and an on-page preview; width and height remain independently adjustable before placement. Embedded image and alpha-mask streams are ordinary page content, so Save As flattens signatures without external file dependencies. Undo removes a placement.

Promoted the existing locked `image = 0.25.10` crate to an exact direct dependency with only PNG and JPEG features. Native image decoding is needed for imported signatures and pictures. Release-date evidence in the dependency-age audit: image 2026-03-10, zune-core 0.5.3 2026-08-07, zune-jpeg 0.5.15 2026-03-26; all precede the 2026-10-09 cutoff.

## Part F: Form filling

Use PDFium form interaction APIs for focus, text replacement, button clicks and dropdown selection. The high-level setters only write dictionary values and do not regenerate appearances. A second binding to the same trusted DLL is retained before runtime initialization to access raw APIs; both bindings use the library's shared thread marshaller. Native handles and the callback table are lifetime-bound and released in order. No JavaScript, external-action, upload or network callbacks are installed.

Treat the explicit Off appearance as unchecked: the wrapper's radio fallback compares the group value with its appearance and can otherwise classify Off/Off as selected. Persisted values and visible appearance changes are tested for every supported field type. Form controls remain editable PDF widgets.

## Part F: Find and replace

Replacement searches editable text runs with Unicode-aware case folding and original byte boundaries. Adjacent runs can share matches; separate lines and distant columns are not concatenated. The replacement appears in the first affected run and unaffected prefixes/suffixes retain their run. All page changes are staged and independently parsed before one history update. Unsupported content mapping fails the entire operation. The replacement dialog counts editable occurrences separately from general PDF text search, which can also find noneditable nested content.

## Part F: Password-protected PDFs

Decrypt authenticated inputs into in-memory editing snapshots. Preserve the encrypted-source flag through edits, undo and redo. Passwords are not stored in configuration, document history, diagnostics or recovery files. Disable recovery snapshots for encrypted sources. An empty user password can open automatically.

Encrypted Save As uses PDF AES-256 revision 6, a fresh 32-byte Windows CNG random key, and the supplied password for both owner and user authentication. Password confirmation prevents accidental mismatches. Existing files and the original remain protected by exclusive creation. Encryption changes only the exported copy; the working document remains editable.

## Part F: Merge, split and extract

Use native PDFium page import into a fresh document for extraction, keeping selected-page rendering intact. Merge stages all imports before one undo step. Split validates every range and destination before creating output, and removes outputs created by that operation if a later group fails. Range order is explicit; duplicate pages within a group are rejected. Exports are unencrypted new files, clearly identified in the dialog, and never alter working state. Password-protected merge inputs currently require a separately decrypted export; failure leaves the destination unchanged.

## Part F: Real markup annotations

Store markup in page Annots arrays with standard Highlight, Underline, StrikeOut and Text subtypes, QuadPoints where applicable, UTF-16 comments and explicit appearance streams. Opacity uses an appearance graphics state. Existing editable annotations retain their indirect identity when updated. Selection revisions prevent edits after another document mutation. Selecting a text object before opening the markup tool initializes its page and bounds; precise point controls handle other placements.

## Part F: PNG/JPG insertion

Reuse the bounded PNG/JPEG decoder and transparent image embedding API. Mark each inserted image with standard marked-content operators, so placement identifiers survive content-stream recomposition during text edits and save/reopen. Position and dimensions are editable in PDF points. Existing arbitrary PDF image transforms are left intact; the image tool lists editor-inserted images and signatures.

## Part F: Document tabs

Each tab owns its entire editor state and worker, including undo history, unsaved/savepoint state, view, selection, tools and recovery lock. Native PDFium calls remain serialized by the shared bindings. Background tabs continue polling completed work and scheduling recovery. Open creates a new tab instead of discarding the active document. Closing one tab checks its pending edits; closing the window checks all tabs, including inactive ones. Limit open tabs to 32 to bound worker and document allocation.

## Part G: Windows OCR

Use the installed Windows.Media.Ocr recognizer through the exact existing `windows = 0.62.2` crate. It and the activated Windows support crates were released 2025-10-06, well before the 14-day cutoff. No OCR engine, model download or cloud request is bundled. Initialize a scoped WinRT apartment on the worker and recognize a bounded raster. Missing Windows language support returns an actionable error.

Write recognized words as embedded Unicode text with invisible rendering mode 3. Map OCR pixel geometry back into PDF coordinates, including page rotation, and use actual font advances for horizontal fitting. Refuse pages with an existing text layer to avoid duplicate text. Recognition and layer insertion are one atomic history operation.

## Part G: True redaction

Use a conservative raster rebuild for secure redaction. All pages are freshly encoded at up to 200 DPI, with a bounded pixel budget; selected pixels are replaced with black before encoding. No source objects, resource dictionaries, attachments, form values, metadata, annotation dictionaries or text layers are copied. This avoids recoverable content in unused image resources, nested forms or incremental history.

The tradeoff is explicit in the UI: the whole document becomes image-only, with interactive features and bookmarks removed. Require acknowledgment of flattening before application. The original file remains untouched; in-memory undo can restore the pre-redaction snapshot, but Save As serializes only sanitized current bytes. A one-pixel margin covers edge antialiasing. Large outputs fail atomically at the existing size limit.

## Part G: Image compression

Compress standard 8-bit DeviceRGB images using bounded JPEG decoding/encoding and proportional downscaling. Preserve transparency through independently resized soft masks; preserve text and page geometry. Unsupported color spaces, predictors, masks or decode transforms are skipped. Retain the current bytes if the complete candidate would be larger. Report input/output byte counts and changed/skipped images, with one undo step for an accepted result.

## Part G: Page numbers and watermarks

Use one shared embedded font and opacity graphics state across all stamped pages. Treat page numbers as text templates with `{page}` and `{pages}` substitutions; a literal template is a watermark. Coordinates are PDF points from the bottom-left, with an explicit baseline. Preserve original page content in a graphics-state scope. The operation is atomic and undoable.

## Part G: PNG/JPG page export

Render page content, forms and annotations at the requested DPI with the existing native raster limits. Encode lossless PNG or quality-90 JPEG; JPEG also records the requested density. Pixel dimensions follow the DPI for both formats. Export uses exclusive filenames and leaves document history unchanged. Batch failure removes only files created by that batch.

## Part G: Cropping

Use the standard PDF CropBox and validate finite, positive dimensions within the inherited MediaBox. Native rendering and page geometry refresh after the edit. The UI explains that cropping hides rather than removes content, and directs removal needs to redaction.

## Part G: Freehand drawing and shapes

Capture page drags through the inverse native page transform so drawing respects zoom, rotation and crop offsets. Store freehand strokes as Ink annotations, rectangles as Square, ellipses as Circle and lines/arrows as Line with standard endpoint styles. Explicit appearance streams preserve visuals across readers. Each completed drag is one history operation; drawing mode suppresses text-selection clicks until finished.

### Bookmarks

Decided to preserve nested outline trees and destination dictionaries while rewriting sibling links after edits. New bookmarks target the current page with Fit; existing local direct and named destinations resolve for navigation. External actions remain inert. Reordering stays within the current sibling group, deletion includes descendants, and undo restores the complete tree. Traversal rejects cycles, more than 10,000 entries and nesting beyond 64 levels. Titles use Unicode PDF text strings. The sidebar is refreshed after document changes to prevent stale identifiers from modifying another revision.

### Final interface review

Decided to make the expanded document-tools menu scroll within the viewport so Print and recent files remain reachable at the minimum window height. A pointer-and-wheel interaction regression test covers reaching and dispatching Print at 640×420. Updated the usage guide and manual checklist for all daily and extra tools, including encrypted-source recovery restrictions and whole-document redaction flattening.

## Restyle of the Part F and G screens

The tools added in Parts F and G now use the Part E style, with visual changes only:

- Tool windows share one frame (app/src/widgets.rs `ToolWindow`): the menu
  surface and border, a title row with a painted icon and a close button, and
  no egui title bar. They open beside the page, below the find bar, and stay
  movable and non-modal, because drawing and signature placement need the page
  while the window is open. The password prompt opens centered.
- Inside the windows, fields have a caption above them, number fields line up
  on a four-column grid, explanations are muted text, warnings and errors use
  tinted notices, and the main action is a filled button at the bottom right.
  Redaction uses the destructive button style.
- Filled buttons switch to a neutral fill while disabled, so their labels stay
  readable.
- Checkboxes in the new windows get a 4 point radius; the shared 8 point widget
  radius would draw them as circles.
- Sliders draw their rail in the border color, which stays visible on the
  white light-theme window.
- The document menu under the Open button groups the tools under Edit and
  Document, with an icon for each entry, ahead of the recent files. It keeps
  its place, so the toolbar still fits the minimum window width.
- Document tabs are drawn as tabs: the active one joins the toolbar with an
  accent line on top, and an edited document shows a dot in place of the close
  button until the pointer is over the tab.
- The bookmarks sidebar uses the Pages sidebar header and selectable list rows.
- Shape choice in Draw and shapes is a row of icon toggles, and the markup kind
  and export format use the segmented control.
- Twenty more line icons were added for the tools, still painted with egui
  shapes.

## Part C refresh for the current application

Decided to retain the existing size-optimized release profile and all current direct dependencies after checking their use and compiler diagnostics. The current build includes the tool-window restyle and Parts F/G. Updated the feature overview and manual checks to use the grouped Edit/Document menu and current tab/window controls. Kept version 0.1.0 and the Unreleased changelog because producing a local release-mode build does not complete the manual acceptance requirements for a versioned release.

## Large-document rendering

- Enumerate page dimensions through PDFium page-size metadata, avoiding a full page-content load during document startup. Rendering and text extraction remain lazy background work.
- Bound each worker reply channel to one result and consume at most four results per UI poll. This applies backpressure when the UI is slower than rendering, preventing an unbounded queue of RGBA images.
- Cap retained full-page textures at 128 MiB per document, accounting for replacement textures and evicting pages farthest from the incoming page. Retain existing viewport-distance eviction and on-demand virtualized thumbnails. Drop text-object caches for distant pages as well. The texture cap excludes native renderer working memory, document/history bytes, small visible thumbnails, and temporary upload buffers.
- Benchmark a locally generated 1,000-page text PDF, including engine initialization, file read/validation and page metadata. Report first-page raster time separately; the benchmark does not claim a frame-rate measurement or worst-case performance on arbitrary PDFs.

### Headless commands

Decided to provide a console companion binary for reliable Windows standard streams
and exit codes while allowing the desktop executable to dispatch headless commands.
Commands reuse existing core operations without dependencies. Folder replacement is
bounded, nonrecursive, skips symlinks, reports numeric indices/counts, and retains
successful exclusive outputs when another input fails. Shell wrappers launch the
sibling desktop binary through argument arrays without command-shell interpretation.

## Part K: read-only PDF comparison

- Compare explicit saved files in a dedicated styled tool window; separate password fields support encrypted input without writing passwords or comparison data to disk.
- Align corresponding page numbers and ordered text runs with bounded lookahead. This handles ordinary insertions/deletions while keeping memory linear; automatic page realignment and semantic paragraph matching are outside this comparison model.
- Scan page summaries in the background, prioritize requested previews between pages, and retain only one rendered page pair. Shared scroll and zoom keep the two panes aligned. The change sidebar includes text-only and visual-only changed pages and visible per-page errors.
- Use a one-megapixel, 4096-pixel-edge preview bound with exact raster pixel comparison. Fine differences below preview resolution may be lost. Text extraction remains independent of raster downscaling. Queue capacities and text budgets bound retained work; cancellation is checked between native page operations.
- Comparison remains read-only, including undo history and original files. Generated fixtures verify unchanged inputs and detect text/raster changes; pure tests cover Unicode insertion/deletion, missing rasters, unchanged pixels and resource limits.

## Per-user Windows integration

- Use an OS-held exclusive file lock and bounded file mailbox under LOCALAPPDATA.
  This introduces no dependency or network listener. Requests contain a known action
  and one absolute local PDF path, at most 64 KiB. Reject redirects/reparse points,
  malformed/oversized input and stale requests from previous sessions. The existing
  profile ACL is the boundary; processes already running as the same user remain trusted.
- Register capabilities and a private ProgID only under HKCU, then open Windows
  Default Apps for the protected user choice. Never edit UserChoice or HKLM.
- Explorer commands invoke the adjacent console CLI with individually quoted paths,
  without cmd.exe or PowerShell. The CLI routes actions into reviewable GUI tools.
  Merge launches collect into one ordered list until Merge is clicked or the organizer closes, so slow Explorer launches retain their selection. The list is capped at 100 additional PDFs with a visible overflow error.
- Do not automatically register or remove anything at startup. Only Settings actions
  change registration. Generated registry records are tested without live mutation.

## Paragraph reflow

- Detect conservative horizontal paragraphs with compatible font/style, overlapping extents, stable baselines and aligned edges or centers. Rotated/scaled text, mixed-style lines, ambiguous columns and dense pages retain line editing and an inline explanation.
- Reuse the existing text transaction to select/embed the replacement font. Layout prefers explicit PDF glyph widths, uses embedded font advances where widths are absent, preserves character/word spacing, and adds justified gaps through TJ adjustments. Unsupported mappings or render modes produce a clean error.
- Scope substitutions to the paragraph to preserve all neighboring objects. Reflow never moves other content; added lines warn about overlap and reject overflow beyond the page. Overwide unbreakable words require a space or ordinary line editing.
- Replace the paragraph as a single undo snapshot, verify text/neighbor state before accepting, and test all alignments with generated Finnish fixtures and outside-box raster comparisons.

## Corpus isolation
Added an on-demand process-isolated corpus harness and allowlisted Govdocs archive downloader. Only fixed error categories and timing cross the parser boundary. Linux PDFium chromium/8111 SHA256 is pinned from release metadata; corpus and output remain ignored. Defender scan is mandatory between download and execution. No new public core API is introduced.

## Integrated local validation

Kept all branches, merges, builds and results local. No remote was added and no repository content was published. Downloaded corpus bytes and per-file outputs remain ignored; only aggregate outcomes enter project documentation. Defender scanning precedes container parsing; unchanged scanned inputs are reused read-only for validation. Exact-host redirects fail closed and failed sources remain documented rather than being replaced. Fuzz target/run coverage and the minimum corpus size remain incomplete instead of being inferred from smaller regression tests.

## Page geometry fallback

Keep metadata-only sizing for the common startup path. If PDFium cannot supply metadata, load pages by explicit index and validate finite positive geometry. Never use the page iterator for validation: it stops silently on a page-loading error and can return a truncated page list as success. Generated rotated-page tests compare fallback and metadata dimensions. Corpus success counts from the former iterator are historical rather than complete-coverage evidence.

## Current portable release verification

Rebuild the desktop and console companion with the existing size-focused release
profile and verified cached PDFium runtime. Keep version 0.1.0: this is local build
verification, not a versioned release or publication. Preserve active dependencies;
source usage review and the existing strict lint pass found no removal warranted.
Correct obsolete paragraph limitations and extend manual checks without marking
interactive behavior as tested. Reuse unchanged Docker corpus evidence rather than
opening downloaded PDFs on the host or repeating the same corpus run.

## Editing continuity and page-object interactions

Keep old page and thumbnail rasters during content-only text changes, invalidate
hit-test objects immediately, and replace previews only with matching-generation
results. Structural page edits still rebuild page views. Automatic Find refreshes
must not jump to the first match. Native core work stays on the background worker;
this removes blanking without claiming to eliminate processing latency.

Enable mouse drag scrolling on the canvas. A drag beginning on text moves the
selected line/paragraph; Objects mode targets one top-level native object. Convert
screen deltas through the inverse page transform for rotated pages. Preview only
an outline while dragging, then commit one atomic operation on release.

Use PDFium object transforms/removal/copy with bounded input and candidate
validation. Preserve grouped Form objects as groups and fail unsupported copies
without accepting partial changes. Copy/paste keeps one private in-memory PDF
snapshot per tab, never writes the operating-system clipboard or disk, and remains
valid after the source is edited/deleted. Annotations keep their dedicated tools.

### Ranged stamping
Headers and footers use the visible page crop and rotation, with an edge margin in PDF points. Date expansion uses one explicit value per operation for reproducibility. Existing absolute watermark placement remains supported. A single candidate is validated before one undo snapshot is accepted.

### Image document creation
Images are decoded one at a time and embedded as JPEG at quality 95 against white, preserving order without retaining all decoded inputs. A4 and Letter center the image without cropping; image-size pages use 96 DPI. HEIC uses the existing Windows imaging API and installed codecs; no engine or codec download occurs. Imports are limited to 100 images, 16 million pixels per image and a 64 MiB resulting PDF. Output uses exclusive creation.

## Object-level redaction

Remove whole intersecting native PDF objects instead of clipping text runs or rasterizing every page. This conservatively removes complete images and nested form groups when partially intersected. Rebuild the catalog and page tree with only page geometry, content and cleaned resources; discard document extras that can retain removed data. Reject marked content with properties rather than preserving unverified alternate text. Validate extraction and object bounds before drawing covers. Redaction is one atomic undo step; source files and session history are not erased.

## Object resizing

Decided to scale native page-object transforms from eight selection handles in Objects mode. Text keeps its font and content without paragraph reflow. Screen bounds are converted through the page transform, including rotation and zoom. Preview is local; release dispatches one atomic operation. Degenerate, non-finite and excessive sizes are rejected before mutation. Grouped Form artwork scales as a whole.

### Expand grouped artwork through content streams

Decided to inline selected Form XObject invocations with isolated graphics state,
original matrices and bounding-box clips, and uniquely renamed resource bindings.
This preserves embedded fonts and shared form instances without rebuilding glyphs
or rasterizing artwork. Nested forms expand recursively with depth and operation
limits. Transparency groups and optional-content/reference forms are rejected
because removing their group boundary can change compositing or visibility.
Inline-image forms are rejected until their resource bindings can be rewritten
safely. The Objects context menu exposes Ungroup for Form XObjects.

Generated fixtures cover nested shared forms, four page rotations, exact rendered
pixel identity, independent move/delete, undo, stale selection and save/reopen.

## Redaction resource validation

Clean nested encoding, CID system, decoder parameter and transparency group dictionaries by semantic role, including indirect dictionaries without a Type entry. This prevents auxiliary references from retaining removed data. Reject retained PDF function resources and forms that inherit resources: unsupported function serialization can change colors, and binding a shared form to one parent's resources can change other pages. Both cases retain the original bytes and undo state on failure.

## Cross-tab object clipboard

Decided to share one immutable clipboard snapshot among the workers in a window. A copy owns bounded source PDF bytes, so source edits and closing a tab cannot invalidate it. Workers clone only a shared handle and release the lock before native PDF work. Paste affects only the destination document and its undo history. PDF data never enters the operating-system clipboard or a clipboard file.

Copy reserves its result when queued, and paste captures that exact result slot when queued. A destination worker waits for an earlier pending copy without blocking the interface or holding a native PDF lock. Later copies cannot replace an already-queued paste. Failed or cancelled copies wake waiting pastes with an error.

Copies use PDFium's group-to-Form import against the private snapshot. Temporary wrappers around individual text, image and path objects are expanded on the candidate before accepting one undoable change, keeping them directly editable. Genuine source Form groups remain grouped. This retains nested resources, transformations and clipping without flattening or altering the source document. Source native handles are captured before any copy because restoring a temporary group can change object indices.

## Table extraction and Office packaging

Table extraction uses ruling-line boundaries when present and conservative text alignment otherwise. The editable preview is mandatory because PDF content does not encode table semantics reliably. Merged cells and irregular layouts require manual correction.

Office exports use a bounded stored-ZIP Open XML writer: no new dependencies, macros, external links, formulas or embedded executable content. XLSX cells are inline strings. CSV values beginning with =, +, -, @, tab or carriage return are prefixed with an apostrophe to prevent formula execution. Existing export files are never overwritten.

## Word conversion layout

Word export reconstructs semantic paragraph flow rather than fixed PDF positions.
Simple ruled grids become editable Word tables; ambiguous grids remain best
effort. Images are rasterized to at most 2048 pixels per side and retained as
inline PNGs. Font names are retained without embedding font programs. Ordinary
groups are expanded only in a temporary document; unsupported groups fail
without creating partial output. ZIP output remains bounded to 64 MiB.

## Form designer

Create standard AcroForm widgets with bounded names/options and explicit appearances.
Use real parent/Kids radio groups so PDFium enforces exclusive selection. Preserve
existing field values and appearances while moving or resizing widget rectangles.
Date fields remain script-free text with a date-format tooltip. The designer uses
a separate draft canvas and coordinate controls; an explicit Apply commits one
undoable change. Refresh replaces stale selections after document mutations.

## Form flattening

Reuse the selected normal appearance stream as a page XObject, with its original
resources and transform. This preserves filled text, checkmarks and dropdown
appearances without rasterizing or rebuilding them. Preserve non-widget
annotations. Reject missing appearances atomically rather than dropping values.
Remove the AcroForm tree after all widgets are flattened; normal candidate
pruning removes detached field objects. Generated fixture checks cover PDFium
extraction, rendered appearance, save/reopen and history restoration.

### Photo page cleanup
Scan cleanup works on image-only pages without annotations, preventing removal of existing text or displacement of markup. Preview and application use the same bounded 180 DPI raster (2500-pixel maximum side). A bright connected region estimates paper edges; four draggable corners allow correction on difficult backgrounds. A projective transform rectifies the quadrilateral, with a separate ±20-degree deskew adjustment and grayscale or thresholded black-and-white output. Page dimensions retain the source raster's physical scale. Windows OCR is optional; cleanup and OCR succeed or fail together in one undo step. Corrected photos use the image creation encoder, JPEG quality 95. Source files remain unchanged.

## Sanitize document

Sanitization uses a fresh, pruned object graph so removed payloads do not survive
as unused objects or incremental revisions. Existing snapshot undo remains available.
Safe ordinary link actions are reconstructed from their destination only; chained
or executable actions are discarded. Optional annotation removal preserves form
widgets. Unsupported visibility constructs fail atomically rather than reporting
successful hidden-text removal. Removal counts describe entries, not unique files.

Reject shared inherited-resource Forms, unprocessed retained Forms/appearances and
aliased attachment/script/metadata payloads to prevent visible-content loss or residual data.
Generated regressions cover each rejection and require unchanged bytes/history.

### Search redaction previews
Use PDFium page-text search for rectangle mapping across text runs. Pattern
recognition remains dependency-free and deliberately reports candidates for
review rather than promising account or identity validation. Keep an exact
source snapshot with the preview so switching documents cannot apply stale
rectangles even when revision numbers coincide. Reuse canvas search highlights
and provide clickable page/rectangle rows before applying true redaction.

## Windows spelling feedback

Use the built-in Windows spell checking API and installed Finnish or English dictionaries. No dictionary downloads, cloud requests or automatic corrections. Debounce edits for 300 ms and run checks off the UI thread. Discard stale results after text or language changes. Underline returned ranges and offer replacements from the text field context menu. Missing dictionaries show an actionable message. Limit input to 64 KiB, issues to 256 and suggestions to eight per issue. Enable COM and Globalization features on the existing pinned Windows dependency; no new package.

### Bates numbering
Bates labels have a strict digit width (1–20), a literal prefix and checked u64 arithmetic. Selected pages are numbered in document order regardless of range input order. Sequence validation precedes all content changes.

### Batch stamping
Batch operations use nonrecursive sorted input enumeration and create-only destinations. Bates numbering continues across successfully written files, preventing duplicate labels within one batch; failed files do not consume sequence numbers. Dry-run uses the identical core transformation in memory. Explicit dates are reproducible, and the default UTC date is captured once for all files.

## Editing grouped text

Text-mode clicks on Form XObjects prepare supported groups in an undoable
operation and reopen text at the clicked position after the refresh.
A sole page-wrapping transparency group retains its compositing dictionary at
page level; compare complete page renders at 72 and 144 DPI. Permit at most one
channel level of compositor rounding in under 1% of channel values, since PDFium
rounds Form and page group edges differently. Reject larger differences atomically.
Fractional alpha, non-normal
blend modes, soft masks and knockout groups remain rejected before mutation.
Keep the original file unchanged and preserve group preparation as an undo step.

## Groups alongside existing page artwork

Preserve an existing page transparency dictionary when exposing another group's
text. When absent, promote the selected group's dictionary. Surrounding artwork
is not itself a reason to reject editing: the opacity restrictions and complete
page render comparison remain the acceptance checks. Cover both layouts in the
generated edit/save/undo regression.

## Interaction and tool access

- Keep native Open, Save As and Compare pickers on bounded background tasks. Each editor tab owns its pending result; cancellation is distinct from a pending result, and duplicate launches are ignored.
- Apply pending text before switching to object movement. Clear drawing and table-selection modes so they cannot consume the same drag. Preserve the clicked text location across the worker refresh.
- Keep the existing grouped, scrollable Tools menu and expose it with a labelled toolbar control. Put Edit text, Move / resize objects and Compare PDFs beside it so common actions do not depend on discovering a chevron.
- Object layer ordering preserves the relative order within the selected and unselected sets. Native object removal/reinsertion occurs only in a candidate document and is committed as one undo step.
- Error banners expire after twelve seconds, warnings after ten and success messages after five; all retain an explicit close button. Top-align the close button so wrapped text cannot shift its hit target between pointer events.


## Tool discovery and page navigation

Expose the complete tool catalog in a default-visible, searchable side panel.
Keep document prerequisites visible, and search descriptions as well as names
so subfeatures such as form flattening and Excel export can be found. Ctrl+K
focuses search; closing the panel restores page space.

Go to page uses one-based input and bounds checks, updates only the viewport,
and preserves horizontal scroll. Ctrl+G and the current page indicator open it.

Native picker acceptance controls only dialogs owned by the test process and
records numeric timing data without window titles or screenshots. Opening and
cancellation are covered; automated file selection remains unverified because
the attempted native button activation did not complete selection.

## Editing and comparison usability

Apply existing-text drafts on a primary click outside the editor, including
non-focusable canvas space. Keep spelling menus active without applying early.
Retain the submitted draft until the worker confirms success; restore it after
a failed edit. These changes affect the open document and undo history. File
output remains explicit Save As, preserving the original.

Fit the original and revised comparison previews together by default. Pair
numbered outlines with change-list entries, distinguish added/removed/changed
text by labels as well as color, and navigate directly between changed pages.
Keep a same-page preview while a different overlay is being computed, with a
loading indicator; never present an old page as the newly selected page.

Constrain shared tool windows to the viewport. Scroll their content independently
of the title and close button so long forms and export controls remain reachable
at the minimum supported window size.

## Background actions and comparison page matching

Keep every import/export file dialog in a tool-owned background job, including
multi-file selection and page insertion. Capture options at launch, queue a
completed result while another document operation is active, and deliver once.
Closing or cancelling a tool discards a late result. Cancellation does not write
an output; the native chooser itself remains under interactive control.
Decode imported images on the same background job. Pending actions and tool
errors have explicit cancel/dismiss controls. No dependency changes are needed.

Match unchanged page fingerprints before comparison. Fingerprints include page
dimensions, text and a small raster so image-only and grouped pages can anchor
alignment. They are session-only matching hints, not security hashes. Bound LCS
storage to eight MB and use 64-page lookahead for larger documents. Pair modified
gaps in order; repeated/reordered pages remain ambiguous. Provide explicit
page-number mode and report fallback if fingerprints cannot be computed.

Package versioned portable releases from a clean checkout with a matching CLI
version, source commit, bundled license notices and SHA-256 manifests. The
packaging script never overwrites an existing version or opens a GUI. Keep
packages under ignored dist/; distribution and installation remain separate.

## Save sequencing and cancelled comparison sessions

Defer Save As until pending edits acknowledge success. Cancel the deferred save
on a blocking operation failure, preserving the edit draft for correction.
Normal and password-protected saves use the same ordering rule. Each tab owns
its deferred save; no output request is sent on a rejected edit.

Use the shared cancellable background picker for Compare. Closing the tool
immediately cancels its session, discards late picker results, clears password
inputs and releases previews. Reopening cannot adopt a cancelled selection.

## Dense-page text indexing

Replace per-object scans of every page character with a single PDFium character
ownership index keyed by native top-level object handles. Nested objects and
PDFium-generated spacing keep the same filtering as the former extraction path.
A separate short-lived native document supplies the index because the binding's
public object API does not expose handles. RAII closes text, page and document
handles on success, error and unwinding; no cache crosses document revisions.

Decode each page font map once per content traversal and reuse selected-page
operations and text during edit planning. Retain all mapping, cursor, transform
and independent round-trip verification. Cursor repair itself is unchanged.

## Batched cursor repairs

Plan multiple TJ translations from one independently measured page. Carry the
planned displacement until a text-line reset; defer propagation across graphics
state saves/restores and transformations until an absolute text reset. Insert
repairs in reverse operation order, serialize once and verify every text run.
Keep the existing transform/origin tolerance and bounded sequential fallback.

For CID/TrueType chains with neutral horizontal scale and rise, no graphics
transforms and no relative line-position operators, place explicit Tm anchors at
verified original matrices. No subsequent operation consumes the changed line
matrix in this restricted grammar. This avoids accumulated relative-position
rounding without weakening verification. Other layouts use measured TJ repairs.
Arrays with a leading TJ advance also use measured repairs because their native
origin may already include that advance.

Generated regressions check replacement and deletion, long chains, quotation
operators, rotated/scaled text, rotated pages and graphics boundaries. A test-only
pass counter prevents reintroducing one reparse per run in these supported cases.

## Task-oriented tool discovery

Keep the compact document menu and provide seven task categories in All tools.
Search labels, descriptions and a small explicit alias list. Keyboard selection
uses the same enabled state as pointer activation; navigation retains search focus.
Expose existing recovery settings without introducing a second recovery system.

## Preview scheduling

Coalesce only contiguous preview requests. Keep the newest render scale, load
text selection data before rasters and thumbnails, and preserve every edit/open/
save boundary. Queue tests reproduce duplicate thumbnails and cross-edit
cancellation in the previous implementation. Existing in-flight work is not
interrupted; no document mutation is reordered.

## Object alignment

Compute snapping in displayed page coordinates using a six-screen-point radius.
Exclude dragged objects, choose the nearest edge/center per axis, and preserve
fixed resize edges and minimum size. Keep raw pointer displacement separate from
the snapped preview to prevent sticky accumulation. Apply the preview bounds only
on release; reuse existing object transforms and undo. Alt and the Snap checkbox
provide unsnapped placement. No dependency or document format changes are needed.

## Recovery visibility

Reuse the existing locked per-session recovery system and interval preference.
Show applied-edit snapshot state separately from uncommitted text drafts; avoid
claiming an active draft is protected by an older snapshot. Provide a direct
settings entry. Test directory injection avoids accessing installed settings or
recovery sessions during generated-PDF crash simulation.

## Complete save publication

Write PDFs to exclusively created temporary files in the destination directory,
flush them, then publish without replacing existing files. Windows uses a
same-directory move without replacement; other platforms use an exclusive hard
link. Failed writes remove their temporary file. Interrupted writes cannot appear
as completed recovery snapshots, and the previous snapshot remains available.

## Tool search handoff

Include common file formats and control names in tool searches. Close the
catalog when a command is selected so its document or tool window is visible.
Disabled entries explain whether a document or a completed operation is needed.

## Comparison review controls

Pair numbered page highlights with short before/after text excerpts and cyclic
change navigation. Retain one pristine page pair in the worker so changing
overlay mode avoids PDF parsing and rendering. Keep summaries numeric and do not
persist extracted text. Show comparison failures separately from unchanged pages.

## Comparison setup and layout

Preselect the active document's saved source and start after choosing the second
file. Keep free selection for both sides and group passwords and page matching
under options. Use a compact setup view and a full-height results window so the
page highlights remain reachable. Verify both identical and changed generated
PDFs through the picker result, worker and rendered UI flow.

## Display transitions

Anchor a normal window's logical dimensions before a DPI transition instead of
using an already resized physical size as the next baseline. Bound corrections
to two attempts, constrain them to the destination monitor, and leave maximized,
fullscreen and minimized states under native control. Do not query monitor
geometry unless scale changes. Recompute the physical minimum size for the
destination DPI and available monitor bounds. Delay replacement PDF renders until the surface
has been stable for 180 ms; retain existing previews and discard superseded
render replies. Tests cover repeated scale changes, ignored native requests,
restore behavior, motion debounce and stale preview responses. Native behavior
on mixed-DPI hardware requires an interactive acceptance check.

### Stable object alignment

Moving objects align centers to centers and edges to edges. Page centering uses
the object's center, while resizing may align an active edge to any guide.
A six-screen-point acquisition radius and a 1.7-times release radius prevent
rapid switching between nearby alignments without accumulating snapped deltas.
Equal-gap alignment uses the nearest non-overlapping neighbors in the same row
or column. Guide labels distinguish these cases. Alt and the Snap toggle clear
the current lock. Preview geometry is reused when committing the edit.

### Preserve glyph resources during text geometry edits

Logical text selections use a shared affine transform for all member objects.
Text movement and resizing modify content-stream graphics state around the
selected shows, preserving original glyph encodings and font resources.
Verification checks decoded content and every native object transform. Native
object operations also check extracted text before accepting a candidate, since
regenerating Type3 content can lose text. Unsafe results leave the document intact.

### PDF engine transaction serialization

Serialize complete fallible core operations with a process-wide mutex and a
thread-local nesting marker. The pinned binding does not lock every native save
entry point. Concurrent extraction can otherwise interfere with document loading,
text extraction and editing. Nested core calls reuse their outer transaction;
panic recovery restores the nesting marker and releases the lock.

### Form-aware page transfers

Native page import copies widget annotations without their AcroForm field tree
and default resources. Restore the selected field ancestry, clone reachable form
resources and map widget/page references to the imported pages. Filter Kids to
selected widgets, alias imported default fonts, and make colliding root names
unique so repeated imports remain independent. Reject XFA, repeated form pages,
unsupported resource conflicts and references to unselected pages atomically.
Verify the editable field count before accepting or writing the result.


### Selection and safe batch transactions

Native character indices provide reading-order copying and precise search ranges;
editable object identifiers remain separate. Batch page and object changes build
and verify a candidate before one undo commit. Arrangement labels map through the
page transform so rotated documents retain visual directions. Multi-step jobs use
thread-local cooperative controls; cancellation never interrupts native calls.

### Session continuity and working-copy authority

Optional session files store only local unencrypted document references and view
positions. Bounded recovery sidecars carry immutable current/undo/redo snapshots,
validate every PDF on restore, and never persist passwords. Explicit discard removes
owned snapshots; failed recovery writes retain their prior success marker and retry.

Save As creates an exclusive new file and an opaque in-memory working-copy token.
Only that token can authorize repeated atomic replacement, after checking the last
written bytes and rejecting redirected files. Original document identity is retained
throughout the session. Encryption passwords remain in the token and are cleared
when it is dropped; reopening a saved file starts with Save As again.


### Recovery storage ownership

Recovery directories are reserved for runtime snapshots, not saved working copies.
Resolve output parents before dispatching Save As, encrypted saves or working-copy
updates so directory aliases cannot bypass that boundary. Missing source metadata
uses a filename-only suggestion. Cleanup removes only numbered snapshots, their
history sidecars, metadata/lock files and recognized interrupted temporary writes;
it never recursively removes unknown entries or traverses redirected directories.
This prevents recovery retirement from deleting unrelated saved documents.

## Current-edit comparison and minimal tool copy

Capture an immutable opening snapshot on the document worker and compare it
with current applied bytes. Keep both versions in memory without history copies
or plaintext temporary PDFs. Queue capture after successful pending edits and
reject stale results. Saving never changes the opening baseline. Preserve an
explicitly chosen original and transient encrypted-input passwords across refreshes.

Remember source/copy relationships only after successful unencrypted saves from
unprotected sources, in a private bounded local file. Validate the whole store
and copy size/modification time before resolving a relationship. Treat filename
matching only as a suggestion requiring confirmation; the PDF itself receives no
ancestry metadata. Identical canonical paths are invalid in saved-file mode.

Keep tool inputs, useful results and irreversible-operation warnings visible;
remove repeated tutorials and static status prose. Generated sanitization tests
cover both modes, all comment/link combinations, visible edits, fresh saves,
removed payloads and exact undo/redo. Rebuilt output is image-only; visible edits
remain, and local undo/recovery can retain source content.

## Verified exports and desktop editing fidelity

Export sanitization from an independent snapshot and publish only after the flushed
file has been reopened and verified. Keep the editor savepoint/history unchanged.
New outputs are unencrypted; protected inputs require an explicit desktop choice.
Default the desktop tool to page rebuilding for sharing; retain editable cleanup
with its supported-content limits. Verification certifies the selected cleanup,
not visible content or every possible opaque payload in editable documents.

Write sanitized documents with a fixed PDF header, classic cross-reference table,
Root/Size trailer, pruned and renumbered objects. Compare canonical bytes on
verification, rejecting raw extras and unused objects instead of exempting streams
by a claimed type. Validate rebuilt JPEG metadata and minimal document roles.
Count page-content graph edges once per page object and indirect container so
legitimate sharing remains editable while unrelated aliases fail closed.

Use native quadrilateral bounds and the page transform for affine editing. Keep
the native TextEdit keyboard/history/IME behavior, map pointer selection into local
coordinates and transform clipped paint meshes back to the page. Rank non-exact
font candidates by category, width, weight and slant; preserve glyph, licensing and
exact-family checks.

Render display-resolution viewport tiles with a shared page load per bounded
batch. Keep small whole-page previews while tiles arrive, share the raster cache
budget and drop stale replies before they can evict current data.

Native acceptance bypasses normal startup, instance forwarding, settings and
recovery. It owns only generated inputs and its own windows. Declare the already
resolved pinned winit version as a test-only dependency for its platform event-loop
extension; add no runtime packages or features. Real foreground keyboard testing is
opt-in, guarded by window ownership; normal native workflow checks use owned-window
messages and the registered OLE drop target. Raw reports and captures stay local.

Allow the portable build script to select either executable while preserving its
default workspace build and path-remapping flags. A locked desktop executable must
not prevent rebuilding the console binary or require closing a running editor.

## Affine text selection capture

Capture selection gestures that start inside the transformed text editor until
button release, including movement beyond its bounds. Checking only the current
pointer position leaves the selection at the previous caret when an event jumps
past an edge. Keep capture scoped to the editor ID and ignore drags started outside.
Do not interpret a captured release as a click-away, even when its short travel
remains below the toolkit's drag threshold.
Verify both directions across rotated/skewed planes and replacement/undo through
the real worker with a generated transformed PDF.

## Ordered document actions

Reuse the save intent barrier for document tools, exports and printing. Keep
one pending action until preceding work succeeds; cancel it on failure, document
replacement or engine shutdown. Reject unchanged failed drafts rather than
running tools against the previous document contents. Keep preview requests and
explicit undo/discard behavior separate, and leave independent image conversion
available without an open document. Generated real-worker extracts verify both
successful edits and rejected drafts repeatedly without changing the source.

## Affine paint allocation

Reuse two small polygon buffers throughout affine mesh clipping. Preserve the
existing clipping/interpolation algorithm and output ordering. The generated-text
debug benchmark measured 5,537 to 3,587 microseconds median over 20 samples with
identical mesh fingerprints; this measures the painting loop, not whole-app latency.
Check clipped area, texture coordinates and vertex colors independently.

## Keyboard dialog flow

Keep native text editors for page numbers and passwords. Select the current page
number on opening and retain focus after invalid Enter input. Password dialogs
focus the first field, accept Enter only when idle and valid, move to confirmation
when needed, and treat Escape as cancellation. Scope field IDs to the calling tab;
discard field text and widget undo state when closing, submitting or reopening.
Do not interpret the opening key event as submission. Generated encrypted PDFs
verify the complete keyboard-to-worker open flow without native dialogs.

## Export destinations and stopped-engine dispatch

Apply the desktop recovery-storage guard to every file and folder export before
dispatch, including extraction, split, images, Word, tables and image conversion.
Probe folder outputs as children so the existing canonical-parent check also
rejects recovery-root and junction-alias destinations. A generated regression
confirmed an extracted file could otherwise be deleted during recovery cleanup.
Keep image conversion independent of an open document, while requiring a live
engine. Always clear busy/opening/pending state after a failed worker send, even
when shutdown was already reported; emit the restart notice only once.

## Unacknowledged drafts after worker disconnection

Reuse normal failed-edit restoration when a worker send or reply channel fails.
Restore the pending draft with focus and suppress unchanged automatic retries,
preserving close/open warnings without claiming the PDF mutation succeeded.
Keep the original generation and savepoint; advise copying the draft before
restart. Generated-document tests cover send failure and missing acknowledgement.

## Search refresh after document changes

Refresh search through the existing scoped request dispatcher after page counts,
current page and invalidated selections have been updated. Capturing the scope
before these updates could search a deleted page or keep results for a selection
no longer shown. Generated-document deletion and repeated undo/redo tests verify
that result pages remain valid and the source stays unchanged.

## Search reply scope validation

Carry the existing search options in scoped worker replies and compare them with
the current controls before installing matches or navigating. Plain search replies
are valid only for default options. Generation and query alone cannot distinguish
results from an earlier page, selection, case or whole-word setting. Generated
worker replies held across control changes verify both rejection of stale results
and acceptance of current results, without timing-dependent assertions.

## Annotation and image list freshness

Refresh open annotation and placed-image panels after acknowledged document
changes and replacement. Keep closed panels idle and avoid refreshes for unchanged
generations. Preserve a selection only when its identity and displayed source
content still match; keep unapplied draft controls intact. Clear old document
handles before refreshing a replacement. Generated workflows verify repeated
updates, image moves, deletion, undo/redo and reopening an unchanged source.

## Form state on document replacement

Clear cached fill fields and designer state after a successful document open.
Reload fields only for an open Forms panel. Separate documents can have identical
field indices and revision numbers, so the previous panel must not remain usable
for a new source. Generated forms with different values verify replacement,
closed-panel lazy loading and switching to a document without forms. Preserve the
existing explicit refresh behavior for edits within the same document.

## Visible object dragging

Move clipping paths with selected clipped text through the existing native page
transaction. Keep the content-stream path for ordinary text and verify encoded
text and native matrices before accepting a candidate. Generated cropped and full
text, native and embedded fonts, all page rotations and saved reopening verify
visible ink and unchanged neighbors.

Prepare one bounded native background/transparent foreground pair for the current
selection in the worker. Cache at most two million pixels per layer; release
native page/bitmap resources before preparing the foreground. Pointer frames move
cached textures directly, with an immediate existing-page crop while preparing.
Use absolute press-to-pointer displacement, preserve release coordinates and keep
the preview until the matching updated page render arrives. Ignore stale replies,
support Escape cancellation and commit a drag as one undoable mutation. Alignment
lines and gap marks remain; omit overlapping text badges.
Cache stationary snap targets once per gesture using selected-index lookup;
reuse cloned rectangle iterators for both axes instead of allocating and checking
every selected index again on each pointer frame.

## Object gesture clipping and unchanged bounds

Reuse the native object-and-clip transformation transaction for mixed selections as well as clipped text. Keep text-only stream preservation and validate candidate text before accepting mixed edits. Compare effective preview bounds before dispatching drag edits so motion along an inactive resize axis does not create an undo or dirty-state entry. Generated raster oracles cover text with clipped paths and images at all page rotations; pointer workflows cover multiple selections, zoom, release position and save/history round trips.

## Selected text font readout

Display the BaseFont names already read by PDFium for selected text objects. Remove six-uppercase-letter subset prefixes only from compact labels; retain exact PDF names on hover and report differing or missing faces as mixed or unknown. Keep the current PDF font separate from unapplied replacement choices, clear inactive selections and reuse the current selection iterator without cloning text objects for display. Hash selected object identities before lookup so large selections avoid repeated linear membership scans. Font inspection does not guess from page pixels or change document content.

## Exact font face and weight matching

Strip subset prefixes before inferring weight or slant, and normalize face-name separators before comparing PostScript names. An installed named face is an exact match when glyph coverage and embedding permissions pass. Family-based exact matches require the requested numeric weight and slant instead of grouping all heavy weights as Bold. Normalize compound weight names and distinguish Extra Light and Semi Light. If explicit formatting cannot resolve a weighted family through the normalized name, use the installed face's actual legacy family metadata before applying style modifiers; never accept a different fallback family as explicit formatting. Preserve existing width/category fallback ranking, coverage checks and license guards.

Generated tests cover named regular/bold-italic/black/semibold faces, misleading subset codes, compound weight aliases, embedded and nonembedded edits across four page rotations and affine text transforms, Nordic/Euro replacements, deletion, unsupported-glyph rejection, neighbour pixels, exact undo/redo history and saved raster equality.
