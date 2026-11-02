# Local document commands

The desktop app and console companion use the same PDF engine. No server,
account or network connection is needed. Keep the bundled PDFium library beside
the executable.

```powershell
pdf-editor-cli.exe new --output draft.pdf --size letter --pages 2
pdf-editor-cli.exe commands --schema
pdf-editor-cli.exe commands --root WORKSPACE --script build.jsonl
```

`new` accepts A4, A5, Letter or custom dimensions in PDF points, `--landscape`
and 1–1000 pages. Landscape keeps an already-wide custom page wide. Outputs must be new files. For custom dimensions use
`--width POINTS --height POINTS`.

Without `--script`, `commands` reads UTF-8 JSON objects from stdin and flushes
one JSON response per command. An interactive client can inspect each response
and correct errors. Scripts stop at the first failed command. The process exits
with code 1 if a command failed, or 2 for invalid startup options.

## Create, inspect, edit, save

Put this fictional example in `WORKSPACE/build.jsonl`:

```jsonl
{"op":"new","preset":"a4"}
{"op":"text.add","document":1,"revision":0,"text":"Example heading","font":"Arial","size":20,"x":42,"y":760}
{"op":"inspect","view":"text","page":0}
{"op":"save","document":1,"revision":1,"path":"example.pdf"}
```

Every response includes `ok`, `result` or `error`, `document`, `revision`,
`dirty` and the optional correlation `request_id`. Read the returned revision:
an unchanged operation may leave it unchanged. Mutations require both expected
`document` and `revision`; reads and exports also check them when supplied.
Object indices and IDs come from current inventories, not from guesses.
Bookmarks and designed fields use their returned `id` string, independently of
`request_id`.

Coordinates are PDF points with a bottom-left origin. Page numbers and native
object indices are zero-based. Bounds are `[left,bottom,right,top]`. Colors are
RGB bytes. Paragraph indices come from `inspect` with `view:"paragraphs"`.
Stamp/Bates page-range strings alone are one-based (`"1,3-5"`; empty means all).

`commands --schema` returns every operation with its accepted fields, types and
required inputs. Unknown operations and misspelled fields fail. Tool groups:

| Task | Commands |
| --- | --- |
| Create or load | `new`, `cv.create`, `images.create`, `open` |
| Inspect | `status`, `inspect`, `fonts`, `text.characters`, `text.find`, `text.spelling`, `cv.check` |
| Edit text | `text.add/edit/delete/style/format/replace/highlight`, `paragraph.edit`, `document.policy` |
| Arrange objects | `objects.edit/layout/copy/paste/ungroup/prepare_text`, `image.add/move`, `draw` |
| Organize pages | `page.blank/rotate/delete/move/duplicate/import/merge/crop/extract/split/split_bookmarks` |
| Mark up | `annotation.add/update/delete`, `link.add/update/delete` |
| Forms and navigation | `form.add/update/delete/fill/flatten`, `bookmark.add/rename/delete/move` |
| Process | `stamp`, `bates`, `ocr`, `compress`, `scan.preview/cleanup` |
| Privacy | `privacy.preview/apply`, `redact`, `sanitize`, `export.sanitized` |
| Export and compare | `render`, `export.images/word`, `table.detect/edit/export`, `compare.load`, `compare` |
| Save and history | `save`, `save.update`, `history.undo/redo` |

For example, `objects.edit` supports move, duplicate, resize, delete, front and
back. `draw` supports freehand, rectangle, ellipse, line and arrow. Images and
freehand drawing can place a signature without using a persistent signature
library. OCR uses installed Windows languages and processes a numeric `pages`
array. `compare` uses the opening snapshot by default; null `page` or
`revised_page` represents an inserted or removed page.

`status` reports actual page/byte counts, new-document and password-protection
flags, undo/redo availability, font substitution policy and this session's
workspace-relative working-copy path/encryption flag. It never returns a password.

## CV input

```jsonl
{"op":"cv.create","name":"Avery Example","headline":"Software developer","contact":["reader@example.org"],"sections":[{"heading":"Experience","body":"Built a fictional project using Rust."},{"heading":"Education","body":"BSc Computing | Example University"}]}
{"op":"cv.check"}
{"op":"save","path":"CV.pdf"}
```

`letter:true` selects Letter instead of A4. See [CV formatting](CV.md) for
limits and ATS guidance. `cv.check` reads the current PDF, including applied
edits; it does not predict hiring decisions.

## State and file safety

Use a dedicated, trusted workspace. All command paths, including the script,
must be relative to `--root`. Parent traversal, absolute paths, redirected paths
and existing export files are rejected. Output folders must already exist;
`"."` means the workspace root. This is not an OS sandbox against another
process maliciously changing the filesystem while a command runs.

`save` creates a working copy exclusively. `save.update` updates only the copy
created by this session and refuses detected external changes. A protected
source requires an explicit output `password`; an empty string means unencrypted.
Passwords are never echoed. Never place real credentials in command scripts.
Replacing a dirty document requires `discard:true`; object clipboard contents
can survive a document replacement for cross-document paste.

`privacy.preview` returns a token. `privacy.apply` requires that token and
fresh document/revision values; any intervening document edit invalidates it.
`table.detect` caches a table at the current revision; `table.edit` edits that
export grid, not the PDF. `table.export` writes CSV or XLSX. Scan preview writes
a PNG and returns detected corners when available. `form.fill` requires the
matching `text`, `checked` or `selected` value; `selected:null` clears a choice.

Limits: 1 MiB per command, 10,000 lines per CLI session, 8 MiB per response,
bounded inventories (`offset`/`limit`) and existing per-tool limits. Text search
reports `total` and `truncated` when over 10,000 hits; narrow pages/regions to
retrieve a smaller result. CV input is limited to 64 KB, 20 sections and 20 pages.
Failures preserve the current document when an edit cannot be applied.

The interface does not execute shell commands, open PDF actions or links,
access the network, print, configure Windows, or manage persistent signature
and recovery folders. The caller chooses its own command/output logging; the
session does not write a transcript.
