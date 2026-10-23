# Container corpus harness

Build the image from the repository root using `scripts/corpus/Dockerfile`, using the pinned chromium/8111 Linux x64 archive SHA256 default. Override only after independent verification. The standard archive must be the build without V8/XFA; exported unsafe initialization symbols are additionally checked.

Download with a non-root container, `--cap-drop ALL --security-opt no-new-privileges --cpus 2 --memory 4g --pids-limit 128`, mounting only the local ignored `corpus` directory at `/corpus`, and command `python3 /opt/pdf-editor/download.py`. Network is enabled for this download only. It records URLs and archive hashes inside ignored `corpus/SOURCES.md`. It never follows corpus links or extracts filenames as paths. The PDFium resource archive and PDF-only GitHub repository archives are attempted separately; redirects beyond exact source hosts fail closed.

After download, stop the container and run Microsoft Defender against the corpus folder. A completed clean scan is mandatory before any corpus run. Never inspect PDFs on the host. Downloaded file hashes serve as IDs; corpus content stays out of model context.

Run with the same limits plus `--network none --read-only --tmpfs /tmp:rw,noexec,nosuid,size=128m`, mounting only corpus at `/corpus:ro` and a separate ignored `corpus-results` at `/results:rw`. Each file runs in a separate process with a 30-second timeout. Parser messages and panic payloads are suppressed. Results contain hashes, fixed error categories and timing only. Saved PDFs are temporary and removed after each file. No PDFs or per-file results may be committed.

The harness exercises opening, rendering all pages, enumerating text, finding, an actual edit per eligible page, render differences outside padded edit bounds, saving and reopening. Unsupported documents or edits are explicit failure categories; they are never silently counted as passes. Aggregate result output contains no file identifiers. This on-demand harness is not part of cargo test.

No full corpus run or download is implied by the presence of these scripts. Execution evidence belongs in TEST_REPORT.md.

Upstream feature verification: https://github.com/bblanchon/pdfium-binaries/blob/master/steps/05-configure.sh sets both pdf_enable_v8 and pdf_enable_xfa to ENABLE_V8 (default false). build.yml names enabled archives pdfium-v8-*. Standard archive SHA256: bdc60126399f39fe768566a18d4be055a7b3681bc9e248e8a6656b4f6cae0aa2. FPDF_LoadXFA is exported even without XFA and is not a valid presence test. V8 initialization exports are checked additionally.
