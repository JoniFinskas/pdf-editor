# Backlog

Open work and known limitations, highest priority first. Move an item to
CHANGELOG.md when it ships, and record design choices in DECISIONS.md.

## P1: Next

- **Interactive release acceptance.** Complete physical drag/drop from Explorer
  and real monitor-scale transitions. Opt-in generated-data tests cover native
  Open/Save As selection, editor workflows, OLE drop delivery, close
  cancellation, repeated edits and 100–200% rendered scales; physical gestures and
  extended sessions remain separate manual acceptance.

## P3: Later

- **Dependency exceptions to revisit.**
  - The fixed clipboard-win 5.4.2 remains below the 14-day age floor. Its release
    was 2026-10-23; recheck with the date-based audit on 2026-11-07. The cached
    5.4.3 release is also too recent at the current review date.
  - RUSTSEC-2026-0370, -0371 and -0372 are ignored for Linux-only AccessKit
    dependencies; remove the ignores once eframe's AccessKit moves to zbus 6.

## Current limitations

Icon text requires a verifiable Unicode or accessibility-label mapping. Shared
labels must be selected in full; ambiguous or unmapped symbols remain protected.
Graphics groups that affect text positioning retain conservative cursor repair.

Supported nested Form text can be exposed for editing, including supported
transparency groups with render verification. Ambiguous mappings and unsupported
compositing still fail safely. Objects mode moves/deletes grouped artwork as a whole;
unsupported native copies fail cleanly. Ungroup expands supported ordinary Forms
for individual editing. Transparency groups, optional-content forms and inline
images remain unsupported by ungrouping. Word/table conversion remains best
effort; native Office layout acceptance is still on the manual checklist.

CVs use one editable, single-column layout. Draft files are saved and opened
explicitly; there is no automatic draft persistence. Text review checks the PDF's
extracted text, images and size, and cannot predict an employer's ATS decisions.
Local editing commands are limited to their selected workspace, but are not an
operating-system sandbox against concurrent hostile filesystem changes.
