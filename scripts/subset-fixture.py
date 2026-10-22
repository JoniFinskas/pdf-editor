"""Rebuild the licensed fixture subset using fontTools (development only)."""
from pathlib import Path
import hashlib
import sys

root = Path(__file__).resolve().parent.parent
wheel = root / "target/fonttools-4.60.1-py3-none-any.whl"
if wheel.exists():
    if hashlib.sha256(wheel.read_bytes()).hexdigest() != "906306ac7afe2156fcf0042173d6ebbb05416af70f6b370967b47f8f00103bbb":
        raise RuntimeError("fontTools checksum mismatch")
    sys.path.insert(0, str(wheel))
from fontTools import subset
from fontTools.ttLib import TTFont

font = TTFont(root / "tests/fonts/Inter-Regular.ttf", recalcTimestamp=False)
options = subset.Options()
options.recalc_timestamp = False
subsetter = subset.Subsetter(options=options)
subsetter.populate(text="Subset test äöå€ AB unchanged")
subsetter.subset(font)
for record in font["name"].names:
    if record.nameID in (1, 3, 4, 6, 16):
        record.string = "PdfEditorFixture".encode(record.getEncoding())
font.save(root / "tests/fonts/PdfEditorFixture.ttf")
