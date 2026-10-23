# Backlog

Open work and known limitations, highest priority first. Move an item to
CHANGELOG.md when it ships, and record design choices in DECISIONS.md.

## P1: Next

- **Manual checks of untested paths.** Test the native Open and Save As
  dialogs, drag and drop from Explorer, and the close confirmation from the
  title bar. Also check display scales other than 150% and a long editing
  session on real documents.
- **Edit latency.** An edit takes about one second on pages with hundreds of
  text objects, because every core call reparses the PDF bytes, including once
  per cursor correction. Profile a release build, then cache the
  parsed document per revision or use a text-state interpreter.

## P2: Soon

- **Closer fallback fonts.** When a browser-printed Type3 font lacks a typed
  character, the line falls back to the closest installed TrueType font,
  usually Arial. Match on a size and weight hint, or offer a font choice in the
  warning banner.
- **Rotated and skewed text.** The edit box is always horizontal, so rotated
  runs are edited in an axis-aligned box. Rotate the box to the text matrix.
- **Undo history after restore.** A restored recovery copy opens with unsaved
  edits but no undo history. Store the snapshot history with the recovery copy
  if restored sessions need undo.
- **Icon glyphs.** Runs whose text differs between the two parsers (icon fonts)
  keep their place in the page mapping but cannot be edited.

## P3: Later

- **Sharp rendering at high zoom.** Textures are capped below the core's
  16 megapixel render limit, so very large zoom levels upscale. Render visible
  tiles instead of whole pages.
- **Dependency exceptions to revisit.**
  - clipboard-win 5.4.2 was below the 14-day age floor when pinned; replace it
    once a later release passes the floor.
  - RUSTSEC-2026-0370, -0371 and -0372 are ignored for Linux-only AccessKit
    dependencies; remove the ignores once eframe's AccessKit moves to zbus 6.
- **First versioned release.** Complete MANUAL_TEST.md and follow VERSIONING.md
  before changing the version or tagging a release.

## Out of scope for v1

Paragraph reflow, forms, annotations, image editing and OCR are implemented.
Nested Form XObject text and ambiguous text mappings remain unsupported for text
rewriting. Objects mode moves/deletes top-level grouped artwork as a whole;
unsupported native copies fail cleanly. Ungrouping, cross-tab object clipboard,
and arbitrary nested-object editing remain unimplemented.
