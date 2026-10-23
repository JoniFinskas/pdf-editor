# Core API

Crate: `pdf_editor_core`. All fallible operations return `Result<T, Error>`.
Errors contain user-facing descriptions without document contents or filesystem
paths. Error implements Display and std::error::Error; standard conversions from
I/O, lopdf and PDFium errors intentionally discard internal diagnostic details.

## Functions

| Function | Inputs | Output / effect |
| --- | --- | --- |
| `Engine::new` | Absolute trusted PDFium DLL path | Runtime handle; reuses the process-wide runtime for the same canonical path. |
| `Engine::open` | Source path | Document with immutable original bytes and empty history; rejects encrypted/invalid/oversized PDFs. |
| `Engine::pages` | Document | Page dimensions in PDF points, in page order. |
| `Engine::text_objects` | Document, zero-based u16 page | Page-level text objects in paint order, with revision, native index, text, font, size, color, matrix and bounds. |
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
| `Document::save_as` | Document, new PDF path | Writes and synchronizes a new file; rejects original path, existing files and alternate data-stream names. |
| `Substitution::config_path` | None | `%APPDATA%/PdfEditor/settings.txt`, or an error if APPDATA is unavailable. |
| `Substitution::load` | Config path | Line or Document policy; absent file defaults to Line, invalid content returns an error. |
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
  binding is synchronized by pdfium-render. Do not initialize another independent
  PDFium binding in the same process.

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
