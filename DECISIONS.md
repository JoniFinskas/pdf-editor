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
