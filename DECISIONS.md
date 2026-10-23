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
