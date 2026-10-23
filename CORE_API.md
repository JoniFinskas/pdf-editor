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
