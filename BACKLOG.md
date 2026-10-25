# Backlog

Open work and known limitations, highest priority first. Move an item to
CHANGELOG.md when it ships, and record design choices in DECISIONS.md.

## P1: Next

- **Interactive release acceptance.** Complete physical drag/drop from Explorer
  and real monitor-scale transitions. Opt-in generated-data tests cover native
  Open/Save As selection, editor workflows, OLE drop delivery, close
  cancellation, repeated edits and 100–200% rendered scales; physical gestures and
  extended sessions remain separate manual acceptance.
- **Unusual graphics-state edit latency.** Dense pages use one-pass character
  indexing; cursor repairs batch predictable translations and anchor eligible
  text chains. Generated 150-repair cases improved from 456–565 ms median to
  25–29 ms. Uncertain propagation across graphics-state changes retains a
  conservative fallback; profile a generated reproducer before extending it.

## P2: Soon

- **Icon glyphs.** Runs whose text differs between the two parsers (icon fonts)
  keep their place in the page mapping but cannot be edited.

## P3: Later

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
