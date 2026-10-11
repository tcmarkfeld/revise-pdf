# Revise

**Edit any PDF like a document.**

Revise opens a PDF and rebuilds it into real paragraphs, lists, columns and
tables, so text wraps and moves as you type — the way it does in Word or
Google Docs. Fix a typo in your resume, add a bullet, insert a table row, and
save it straight back to the same PDF.

It works entirely on your Mac: no uploads, no accounts, no AI guessing at
your content.

## Install

```bash
brew install --cask tcmarkfeld/tap/revise
```

Requires a Mac with Apple silicon (M1 or later) running macOS 12 or newer.

Revise keeps itself up to date: it checks for a new version once a day and
offers to install it (or choose **Revise → Check for Updates…** any time).
To uninstall, run `brew uninstall --cask revise` (add `--zap` to also remove
its settings and backups).

## Opening a PDF

- Launch Revise and click **Open PDF…** (or press ⌘O), or
- Right-click a PDF in Finder and choose **Open With → Revise**.

Revise doesn't replace your default PDF viewer; it just appears as an option.

## Editing

Click anywhere in the text and start typing. Everything you'd expect from a
word processor works: Enter starts a new paragraph (and continues lists),
Tab moves between table cells, and the toolbar at the top covers the rest —
headings, fonts and sizes, bold / italic / underline / strikethrough, text
colour, links, alignment, line spacing, bulleted and numbered lists,
indentation, tables, images and horizontal lines.

With nothing selected, formatting applies to what you type next.

### Tables

Insert one from the toolbar's grid picker. With the cursor in a table, a
second toolbar lets you add or delete rows and columns, merge and split
cells, shade cells or rows, toggle borders, and even out column widths. Drag
a column border to resize it. Right-click a cell for the same options.

### Images

Choose **Insert → Image…**, use the toolbar button, or drag a PNG or JPEG
onto the page. Click an image to select it, drag a corner to resize, and
press Delete to remove it.

### Find, replace and spelling

⌘F finds text, ⌥⌘F finds and replaces. Misspelled words are underlined in
red using the macOS dictionary — right-click one for suggestions. Turn this
off under **Edit → Check Spelling While Typing**.

### Page setup

**File → Page Setup…** (⇧⌘P) changes the paper size (Letter, Legal, A4…),
orientation and margins, for the whole document or just the current page.
Text reflows to fit, and anything that no longer fits flows onto the next
page.

## Saving

⌘S saves your changes back into the same PDF, so it still opens anywhere —
Preview, Acrobat, a browser. ⇧⌘S saves a copy somewhere else.

When you reopen a PDF you saved with Revise, all of its structure (tables,
lists, styles) comes back exactly as you left it.

### Your files are safe

- **Unsaved changes** — Revise asks before closing, quitting or opening
  another file.
- **Original backup** — the first time you save over a PDF, Revise keeps a
  copy of the untouched original. **File → Revert to Original…** brings it
  back at any time.
- **Crash recovery** — unsaved edits are autosaved every 15 seconds. If
  Revise ever quits unexpectedly, it offers to recover them next time you
  open the file.

## Keyboard shortcuts

| Action | Shortcut |
| --- | --- |
| Open / Save / Save As | ⌘O / ⌘S / ⇧⌘S |
| Undo / Redo | ⌘Z / ⇧⌘Z |
| Delete to start of line | ⌘⌫ |
| Bold / Italic / Underline | ⌘B / ⌘I / ⌘U |
| Strikethrough | ⇧⌘X |
| Normal text / Heading 1–3 | ⌥⌘0 / ⌥⌘1–3 |
| Bulleted / Numbered list | ⇧⌘8 / ⇧⌘7 |
| Indent / Outdent | ⌘] / ⌘[ (or Tab / ⇧Tab in a list) |
| Align left / center / right / justify | ⇧⌘L / ⇧⌘E / ⇧⌘R / ⇧⌘J |
| Add link | ⌘K |
| Clear formatting | ⌘\ |
| Find / Find and Replace | ⌘F / ⌥⌘F |
| Find next / previous | ⌘G / ⇧⌘G |
| Page Setup | ⇧⌘P |
| Zoom in / out / actual size / fit width | ⌘+ / ⌘− / ⌘0 / ⌘9 |

Light and dark mode follow your system setting, or pick one under
**View → Appearance**.

## What works best

Revise is tuned for text-based business documents: resumes, letters,
reports, forms and simple multi-column layouts. It reads the PDF's own text,
so scanned documents (photos of paper) can't be edited. Very
design-heavy PDFs such as magazines or posters may not rebuild perfectly.

## Feedback

Found a PDF that doesn't come out right, or a bug? Please
[open an issue](https://github.com/tcmarkfeld/revise-pdf/issues) — attaching
the PDF (if you can share it) helps a lot.

Building from source or contributing? See
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
