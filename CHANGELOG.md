# Changelog

All notable changes are listed here. Versions follow [VERSIONING.md](VERSIONING.md).

## [Unreleased]
- Apply text edits on click-away, including empty canvas, and retain the draft if applying fails so it can be corrected or retried.
- Add Go to page (Ctrl+G), first/last-page navigation and a clickable page number; reject out-of-range input without changing the PDF.
- Show a searchable All tools panel by default, including tool descriptions and document prerequisites; Ctrl+K focuses its search.
- Add Bring to front and Send to back object actions with stable relative order and undo.
- Stop drawing and area-selection tools when switching back to text editing or object movement.
- Expose text and object movement modes, Compare, and a labelled Tools menu; preserve selection when leaving text editing to move an object.
- Run Open, Save As and Compare file pickers outside the UI frame loop.
- Keep error dismiss buttons stable for wrapped messages; expire notifications automatically and bound repeated errors.
- Allow grouped text editing alongside other page objects and existing page transparency, with render-preservation checks.
- Allow clicking supported grouped PDF content in text mode to ungroup it and open the text at the clicked position. Preserve undo and report unsupported groups.
- Add local batch stamp and Bates CLI commands with dry-run, checked sequences and create-only output.

- Add Bates numbering with a prefix, checked start number and fixed digit count.

- Add ranged headers and footers with editable date and page-number tokens.


### Added

- Added photo-page cleanup with detected edges, draggable corners, perspective correction, deskew, grayscale/black-and-white and optional Windows OCR.

- Added ordered JPG/PNG/HEIC image import to new PDFs, with reorder/remove controls and A4, Letter or image-sized pages.

### Added

- Copy and paste PDF objects between tabs with a private in-memory clipboard that survives closing its source tab.

- Eight resize handles for selected text, images, vector paths and grouped artwork, with a drag preview and one undo step.

### Added

- Ungroup supported Form XObjects into editable page objects, preserving nested
  transforms, clipping and shared instances; supports undo.

- Export PDFs to editable Word documents with paragraph styles, images and simple tables.

- Select a table area, review and edit detected cells, and export Excel or CSV.

- Flatten filled forms into searchable page content while preserving other annotations and undo.

- Add a form designer for text, date, checkbox, radio and dropdown fields with names, required flags, move/resize controls and deletion.

- Add document sanitization with metadata, embedded file, action and supported hidden-text removal, optional comments/links, a removal report and undo.

- Preview and redact literal text or privacy patterns across the document.

### Added

- Local Windows spell checking for existing and new text, with Finnish/English selection, underlines and context-menu suggestions.

### Performance

- Load page-size metadata without parsing every page, cap page texture storage, and bound background-render replies. Add a generated 1,000-page startup benchmark.

### Changed

- Remove intersecting text, image and vector objects directly, including nested
  groups, while preserving unrelated editable content. Add atomic batch redaction
  and remove unused source streams.

- Refresh portable release instructions and manual checks for paragraph editing,
  comparison, command-line tools and Windows integration; document corpus limits.

- Refresh release-build verification, the feature overview and manual checks for
  the current daily/extra features and restyled tool windows.

- Size-focused release profile with thin LTO and stripped symbols; preserve panic recovery.
- PDFium setup reuses verified downloads and includes all bundled native license notices.
- Complete Windows release build instructions and a manual test checklist.

- New look: dark charcoal theme with a blue accent (light theme to match),
  painted icons, a home page with an open card and recent files, a settings
  page with General, Editing, Shortcuts and About sections, floating find and
  text toolbars, and a pages sidebar that can be hidden.
- New tool windows, document tabs, the document menu and the bookmarks
  sidebar now match the rest of the interface.

### Fixed

- Retain page/thumbnail previews during text-edit refresh and preserve the current
  scroll position when search results refresh after an edit.
- Fall back to indexed page loading when metadata sizing fails, preserving loading errors and rejecting invalid geometry.

- Keep the expanded document-tools menu scrollable on smaller windows.

### Added

- Drag-to-pan, draggable text and an Objects selection mode with move, copy/paste,
  duplication and deletion of supported top-level PDF artwork; all changes undoable.
- Add an isolated, on-demand corpus downloader and validator with Defender pre-scan, bounded container execution and aggregate-only reporting.

- Headless merge, split, extract, compress, rotate, image export, Windows OCR, and
  font-preserving find/replace commands with folder dry-run counts and exit codes.

- Compare two saved PDFs side by side with shared scrolling and zoom, text highlights, visual difference overlays and a page-change sidebar.

- Single-instance document forwarding and explicit per-user Windows default-app and Explorer menu integration.

- Click compatible multiline paragraphs to edit and reflow a block with left, right, centered or justified alignment; Ctrl+Enter applies and Escape cancels.
- Preserve paragraph font metrics and spacing, warn on downward expansion, and keep conservative line editing for ambiguous layouts.

- Bookmarks sidebar with nested navigation, add, rename, delete and sibling reordering.

- Draw freehand, rectangles, ellipses, lines and arrows directly on pages as annotations.

- Crop pages using validated visible page bounds.

- Export selected pages as PNG or JPG with a DPI choice.

- Page-number templates and text watermarks with position, size, color and opacity.

- Image downscaling and recompression with before/after document sizes.

- Secure rectangle redaction through a fresh image-only document rebuild.

- Windows OCR for scanned pages with an invisible searchable text layer.

- Multiple document tabs with independent undo history, unsaved state and recovery.

- Insert PNG/JPG images and edit their positions and dimensions after insertion.

- Editable PDF highlights, underlines, strikethroughs and sticky notes.

- Merge multiple PDFs, split by page ranges and extract selected pages.

- Open password-protected PDFs and optionally encrypt Save As with AES-256.

- Find and replace one or all editable text matches with font preservation.

- Fill text, checkbox, radio and dropdown PDF form fields with preserved widgets.

- Windows print dialog with page range and printer options for the edited document.

- Validated command-line PDF opening, preserving spaced and Unicode paths.

- Recent menu with the last ten successfully opened PDFs, persisted in settings.txt.

- Find toolbar action and tested keyboard commands, including shifted plus layouts.

- Rotate, delete, drag to reorder, and insert PDF pages with undo/redo.

- Scrollable page thumbnail sidebar with click navigation.

- Change the size and color of an existing text object.

- Delete individual PDF text objects from the selected-line toolbar.

- Place new text with a system font picker, point size and color.

- Find text with Ctrl+F, match highlights and wrapping next/previous navigation.

- Version number shown in the status bar and Settings.
- Versioning rules in VERSIONING.md; the workspace version is set in one place.
- Settings page with font substitution, auto-save and About sections, replacing
  the settings popup.
- Auto-save as recovery copies (off, or every 1, 2, 5 or 10 minutes; default 2)
  with a restore prompt after an unexpected exit. Originals and saved copies
  are never overwritten.
- Whole-line editing: adjacent runs on one baseline are selected and edited as
  one line (`Engine::edit_group`).

### Changed

- Clicking away from the edit box applies the edit, like Enter; only Escape
  cancels. Open, Save As, Settings and closing the window apply an open edit
  first.

### Fixed

- Fall back to indexed page loading when metadata sizing fails, preserving loading errors and rejecting invalid geometry.

- Keep all toolbar actions accessible in narrow windows and isolate background search failures from document operations.

- Keep unsaved-change warnings after a replacement open fails or is canceled; wait for pending edits before discard.

- PDFs printed from Chrome and other browsers could not be edited: text using
  Type3 fonts with a ToUnicode map was decoded differently from PDFium, and
  space glyph runs and unnamed fonts were rejected.

## [0.1.0] - Unreleased

First development version.

### Added

- Headless core: open, render, list and hit-test text objects, in-place text
  edits with embedded-font reuse, system font lookup and substitution warnings,
  substitution setting, undo/redo and Save As.
- Desktop editor: toolbar, page canvas with zoom, hover and click editing,
  font warning banners, settings modal, light and dark themes.

- Draw, import, save and place reusable transparent signatures.
