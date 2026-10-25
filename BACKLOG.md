# Backlog

Open work and known limitations, highest priority first. Move an item to
CHANGELOG.md when it ships, and record design choices in DECISIONS.md.

## P1: Next

- **Manual checks of untested paths.** Test the native Open and Save As
  dialogs, drag and drop from Explorer, and the close confirmation from the
  title bar. Also check display scales other than 150% and a long editing
  session on generated documents.
- **Unusual graphics-state edit latency.** Dense pages use one-pass character
  indexing; cursor repairs batch predictable translations and anchor eligible
  text chains. Generated 150-repair cases improved from 456–565 ms median to
  25–29 ms. Uncertain propagation across graphics-state changes retains a
  conservative fallback; profile a generated reproducer before extending it.

## P2: Soon

- **Closer fallback fonts.** When a browser-printed Type3 font lacks a typed
  character, the line falls back to the closest installed TrueType font,
  usually Arial. Existing text can choose an explicit family/style; automatic
  fallback matching could additionally rank visual width and weight.
- **Rotated and skewed text.** The edit box is always horizontal, so rotated
  runs are edited in an axis-aligned box. Rotate the box to the text matrix.
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
- **Interactive release acceptance.** Portable 0.x releases have automated
  verification. Complete MANUAL_TEST.md before claiming full interactive
  acceptance or promoting the app to 1.0.0.

## Out of scope for v1

Paragraph reflow, forms, annotations, image editing and OCR are implemented.
Supported nested Form text can be exposed for editing, including supported
transparency groups with render verification. Ambiguous mappings and unsupported
compositing still fail safely. Objects mode moves/deletes grouped artwork as a whole;
unsupported native copies fail cleanly. Ungroup expands supported ordinary Forms
for individual editing. Transparency groups, optional-content forms and inline
images remain unsupported by ungrouping. Word/table conversion remains best
effort; native Office layout acceptance is still on the manual checklist.
