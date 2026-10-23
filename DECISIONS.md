# Engine decisions

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
