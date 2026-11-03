# Changelog

All notable changes are listed here. Versions follow [VERSIONING.md](VERSIONING.md).

## [Unreleased]

- Add optional local auto-save for session-owned working copies, with a two-second edit delay or minute intervals; preserve originals, encryption and unfinished drafts.
- Reject working-copy saves when a parent folder has been redirected.

- Keep resized text previews sharp with bounded high-resolution foreground layers and mipmap filtering.
- Drag document tabs to reorder them, with a moving preview, insertion marker and edge scrolling; preserve the active editing session.

- Clarify that crash recovery creates local backups rather than saving the PDF; correct the one-minute interval label.

- Resize selected text by changing its font size proportionally, with live text and size feedback; keep the original font shape.
- Preserve character and word spacing when transforming clipped text, including neighboring runs.
- Keep the original left text origin across repeated paragraph edits and avoid copying paragraph data on each width-drag frame.

- Reflow text by dragging its writable width without scaling the font; join likely soft wraps, preserve axis-aligned text scale and reject new overlaps with nearby text.
- Snap dragged text to real baselines and shorten paragraph editing hints.

- Edit, delete and style supported accessibility-labelled icon text; update local labels without changing shared properties or unrelated symbols.
- Batch text-cursor repairs across closed styling and painting groups without text-position changes.
- Save and reopen unfinished CV drafts from the builder, undo loading a draft, and exchange draft data through local commands. Draft copies protect existing files.

- Refresh documented limitations and release acceptance guidance.

- Keep custom landscape dimensions wide across desktop and command-line creation.
- Validate CV drafts before opening another tab; retain failed font/layout drafts for retry, restore removed sections and keep editor state when reordering.
- Inspect command-session document, save-copy and undo/redo state with `status`.

- Create blank PDFs with standard or custom page sizes, orientation and page count.
- Build editable, selectable-text CVs with wrapping, page breaks and extracted-text review.
- Author PDFs through a bounded local JSON-line editing interface with discoverable schemas and revision checks.

- Add, edit and remove web, email and document-page hyperlinks from Markup and links; select link areas directly on the page.
- Keep annotation selection after updates and render only visible rows in long annotation lists.
- Run local OCR across selected pages as one undo step, with cancellation and explicit skips for existing text or unrecognized pages.
- Release PDF rendering locks during OCR recognition and reuse page coordinate mapping for every recognized word.

- Reject text edit, style and paragraph selections whose current content or geometry no longer matches, including handles from other documents.

- Keep the full tool list in the searchable All tools panel; simplify the document menu to Save As and recent files.

- Split PDFs at top-level bookmarks from the existing page organizer, retaining leading pages and refusing existing output files.

- Index bulk text replacements and deletion checks by original run position instead of repeatedly scanning every target.

- Edit paragraphs on rotated and cropped pages with shifted origins, while rejecting true page overflow without changing the document.

- Remove a repeated comparison source label and clarify difference navigation and counts.

- Preserve redo, previews and saved state when applying identical font formatting; verify the full embedded font resource before skipping a change.

- Reuse the paragraph font's encoding map for measuring and writing reflowed text.

- Prepare usable font metrics for paragraph editing in standard-font PDFs, including manual line breaks, while preserving neighboring content.

- Reuse each page font's encoding map during bulk text edits, keeping distinct character maps separate.

- Skip identical Find/Replace matches without clearing redo or marking the PDF as edited; keep occurrence numbering stable.

- Delete blank glyph objects in browser-style PDFs even when they extract as empty text.

- Keep redo history, previews and the saved state when applying an unchanged text size and color.

- Preserve built-in Helvetica, Times and Courier fonts with standard metrics when editing supported characters; retain font fallback for custom width tables.

- Speed up long text edits inside balanced saved graphics states without moving neighboring text.

- Explain missing or incompatible PDF engine libraries in the CLI without disclosing local paths.

- Skip superseded previews that arrive while a render batch is running, keeping edits and other document actions in order.

- Keep zoom anchored to the same page point across centered layouts, mixed page sizes and fixed gaps; combine zoom requests received before the next frame.

- Shorten the project overview, document the technology stack, and move detailed controls and setup notes to a separate guide.

- Match installed named font faces and intermediate weights correctly, ignore subset codes when inferring style, and allow formatting weighted families such as Segoe UI Semibold without false substitution warnings.

- Show selected text's PDF font in the status bar, text toolbar and formatting window; distinguish mixed or unknown fonts and retain full subset names on hover.

- Preserve clipping when moving or resizing mixed text and artwork selections; ignore edge-handle drags that leave object bounds unchanged.

- Keep clipped text visible when moved, preserving original clipping, neighboring content, undo and saved output.
- Show real selected content while dragging or resizing, track the pointer directly and retain the preview until the updated page is ready; remove alignment text badges.

- Reset previous-document form fields and designer state when opening a PDF; load current fields automatically when the Forms panel is open.

- Refresh open annotation and image lists after edits, undo/redo and document replacement; retain unchanged selections and annotation drafts while clearing stale handles.

- Ignore delayed search results after scope, selection or matching options change, preventing stale highlights and unexpected page jumps.

- Refresh scoped search after page and selection updates, keeping results valid when deleting the last page or using undo/redo.

- Keep pending text drafts visible and preserve close warnings after worker disconnection; prevent automatic retries and advise copying before restart.

- Reject exports into reserved recovery storage and keep busy state clear after engine shutdown, including standalone image conversion.

- Improve keyboard control of page navigation and password dialogs; retain focus after invalid input and discard closed password-field undo history.

- Reduce temporary allocations when painting rotated/skewed text while preserving clipping, texture coordinates and colors.

- Wait for pending text edits before exports, printing and common document tools; cancel queued actions when edits fail or the document changes.

- Keep rotated/skewed text selections active beyond the edit box and prevent accidental applies on selection release.

- Add Sanitize & save with on-disk verification before atomic publication, image-only desktop defaults, protected-source acknowledgement and cooperative cancellation.
- Reject retained content aliases and forged/unused payloads during sanitization and verification; write deterministic sanitized copies and check rebuilt image metadata.
- Rank fallback fonts using run width, weight and slant; align rotated/skewed text editing, selection, caret and IME with the page geometry.
- Render high-zoom viewport tiles at display resolution with bounded queues/caches, preserving forms and annotations across pan, edits and display-scale changes.
- Add opt-in isolated native desktop acceptance for generated-document workflows, OLE drops, scaled rendering and close cancellation; normalize native dialog directories.


- Compare current unsaved edits against an immutable opening original; retain explicit originals and encrypted-input passwords across refreshes.
- Remember originals for app-saved unencrypted copies in private local settings; offer older filename matches for confirmation and reject identical comparison files.
- Simplify comparison, tool panels, editor chrome and settings while retaining actionable warnings and errors.
- Verify editable and visible-page sanitization across comment/link options, edited content, saved output, appearance and undo/redo; clarify visible edits and local recovery retention.


- Protect saved PDFs from recovery cleanup: reject recovery-folder Save As destinations, preserve unrecognized files and directories, and suggest a safe filename when source metadata is missing.

- Select words, lines and text ranges across pages for Unicode copying and one-step highlighting.
- Shift-click or marquee-select objects; move, resize, copy, delete, nudge, align and distribute selections together, including rotated pages.
- Select multiple thumbnails for batch page actions, drag reordering, duplication and blank-page insertion.
- Show long-operation progress with cooperative cancellation and export cleanup.
- Apply installed font families, bold/italic, size and color to existing text with preservation checks.
- Optionally remember unencrypted tabs and reading positions; add tab shortcuts, reopen closed tabs and restore recovery undo/redo history.
- Add search case/whole-word options, page/text scopes and result excerpts, with consistent replacement counting.
- Save a session-owned working copy with Ctrl+S while protecting the original, retaining encryption and refusing detected external changes; Ctrl+Shift+S creates another copy.
- Retry failed recovery writes and remove discarded recovered snapshots.

- Serialize PDF engine transactions across tabs and comparison jobs to prevent concurrent native saves from corrupting processing results.
- Preserve editable fields, appearances and default fonts when inserting, merging, splitting or extracting form pages; keep repeated imports independent.

- Select glyph-based text as a logical block for movement, resizing, copying and deletion, preserving relative glyph positions and atomic undo.

- Keep close confirmation cancellable during pending work, including Escape before underlying editors consume it.

- Dismiss the floating text toolbar when clicking away from an unchanged draft.

- Show drag grips, a page preview and animated before/after insertion guides when reordering thumbnails.

- Offer explicit visible-page rebuilding for sanitizing complex PDFs, with clear loss-of-editability warnings, preserved page sizes and atomic undo.

- Preview signatures under the pointer and place them with a page click; support Escape cancellation and normal object movement after placement.

- Verify replacements consistently when PDFs retain empty text objects; cover glyph-by-glyph, spaced, transformed, rotated and paragraph layouts.

- Export safe transparency groups as editable Word content and preserve unsupported complex pages as images instead of aborting the whole document.

- Use direct object selection and movement without a mode toggle; double-click text to type, with clear grab cursors and functional selection handles. Dismiss the text toolbar when editing ends.

- Navigate Find matches with Enter and Shift+Enter without losing search focus; Escape closes Find.

- Stop failed text drafts from resubmitting on focus changes and trapping document operations in a busy loop.

- Add labeled alignment guides, exact page centering, equal-gap snapping and stable drag locks for text, images and page objects.

- Default font substitution to the whole document while preserving saved preferences.
- Stabilize logical window size across DPI changes, bound native resize retries, and defer PDF redraws while moving or resizing the window.
- Start comparisons from the open PDF, compare automatically after selecting the other file, and reserve a full-height review area for results.

- Publish Save As and recovery PDFs only after their contents have been written and flushed.
- Find tools by familiar terms such as checkbox, JPEG, spreadsheet and DOCX; close the catalog after choosing a tool.
- Review comparison changes with before/after excerpts, clickable page highlights and previous/next change controls; reuse page renders when switching comparison modes.

## [0.3.1] - 2026-10-24
- Remap compiler paths in portable builds.
- Limit portable packages to the application, runtime, public README and license notices.

## [0.3.0] - 2026-10-24
- Show recovery status and link directly to its settings; verify crash restoration with real edited PDFs.
- Add edge/center snapping and alignment guides for object moves and resize handles, with an Alt bypass.
- Keep explicitly selected short pages active when scrolling cannot center them.
- Deduplicate queued thumbnails and prioritize text selection data without crossing document changes.
- Group tools by task, filter categories, search aliases such as diff, and open results with arrow keys and Enter.
- Exercise editing controls through real PDF workers, including cross-tab paste and save/reopen.

## [0.2.3] - 2026-10-24
- Batch predictable text cursor repairs and anchor eligible text chains to avoid repeated whole-page reparsing.

## [0.2.2] - 2026-10-24
- Speed up dense-page text extraction and editing with one-pass character indexing, reused font decoding and selected-page data.

## [0.2.1] - 2026-10-24
- Discard late Compare picker results after closing and clear cancelled comparison previews immediately.
- Wait for pending text edits before saving; cancel the save and retain the draft when an edit fails.

## [0.2.0] - 2026-10-24
- Add headless `--version` output and checked portable release packaging with build details, licenses and SHA-256 checksums.
- Make image import, image-queue and signature messages dismissible using consistent tool controls.
- Match unchanged comparison pages around insertions and deletions, show actual original/revised page numbers, and offer explicit page-number matching.
- Run all import/export and page-insertion file pickers in background jobs; capture input options, prevent duplicate jobs, and discard cancelled or closed-tool results.
- Keep tool windows within the viewport, with scrollable content and an always-accessible close button on smaller screens.
- Fit comparison pages side by side, label Original/Revised, number matching changes, add changed-page navigation, and retain the preview while switching comparison modes.
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
