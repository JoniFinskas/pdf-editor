# Exported fixture provenance

libreoffice.pdf is exported from the synthetic source in tests/sources/libreoffice.fodt
using LibreOffice's headless PDF export. It contains only fabricated text. The PDF
is a read-only test input; normal tests never regenerate or overwrite it.

Reproduction: use a fresh isolated LibreOffice profile, and export the FODT with
`soffice --headless --convert-to pdf --outdir tests/fixtures/exported tests/sources/libreoffice.fodt`.
