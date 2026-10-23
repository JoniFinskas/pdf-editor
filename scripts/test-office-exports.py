"""Validate generated Office fixtures with Python's independent ZIP/XML readers."""
import pathlib
import sys
import zipfile
import xml.etree.ElementTree as ET

folder = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "target/test-output")
files = list(folder.glob("*-table.xlsx")) + list(folder.glob("*-word.docx"))
assert files, "Run generated Office export tests first"
for path in files:
    with zipfile.ZipFile(path) as archive:
        assert archive.testzip() is None, "ZIP checksum mismatch"
        names = set(archive.namelist())
        assert "[Content_Types].xml" in names and "_rels/.rels" in names
        for name in names:
            if name.endswith((".xml", ".rels")):
                root = ET.fromstring(archive.read(name))
                if name.endswith(".rels"):
                    parent = pathlib.PurePosixPath(name).parent.parent
                    for relationship in root:
                        assert relationship.get("TargetMode") != "External"
                        target = str(parent / relationship.attrib["Target"])
                        assert target in names, target
        if path.suffix == ".xlsx":
            root = ET.fromstring(archive.read("xl/worksheets/sheet1.xml"))
            ns = {"s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
            assert not root.findall(".//s:f", ns)
            assert all(cell.get("t") == "inlineStr" for cell in root.findall(".//s:c", ns))
        else:
            root = ET.fromstring(archive.read("word/document.xml"))
            ns = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}
            assert root.findall(".//w:p", ns)
    print(path.name, "ZIP checksums, XML and relationships passed")
