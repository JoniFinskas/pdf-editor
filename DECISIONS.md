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
