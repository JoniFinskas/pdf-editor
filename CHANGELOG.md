# Changelog

All notable changes are listed here. Versions follow [VERSIONING.md](VERSIONING.md).

## [Unreleased]

### Added

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
