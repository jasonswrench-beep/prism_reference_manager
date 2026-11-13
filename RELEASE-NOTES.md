# Release notes

## 0.1.0 (first build, not released)

First complete build for internal testing. Unsigned; not published.

**Projects and library.** Projects with manuscripts, separate reading decisions, notes and
outlines over one shared library. Collections, smart collections, labels, ratings, trash with
retention, persistent undo and redo, per-field history with restore, duplicate review with
merge, aliases and version linking, bulk edits and find-and-replace with preview, term lists.

**Collecting.** Lookup by DOI, PubMed, PMC, arXiv, ISBN and web page; import of RIS,
BibTeX/biblatex, CSL JSON, XML records, mapped CSV and pasted reference lists; PDF import with
identifier detection. Everything is staged for review with duplicates, conflicts and unmapped
fields shown, and whole imports can be undone.

**Updates.** An updater with signature-verified downloads is built in but not set up in this
build, so it never contacts the network. Once set up, automatic checks stay off until turned on,
follow the network settings, are logged, and never install without a click.

**Browser capture.** A capture extension for Chromium-based browsers saves the paper on the
current page (citation metadata, structured data, embedded citation spans, identifier links)
with its PDF, or several papers from a results page, into the import review with a project,
collection, labels and a note. It reads a page only when clicked and connects only to the
program on the same computer after you approve it.

**Reading.** Built-in PDF reader with tabs, thumbnails, outline, search, continuous, single and
two-page views, light, dark and sepia pages. Highlight, underline, strikeout, comment, text box,
drawing, area and ellipse marks with confirmed citation pages. Annotated and flattened copies,
annotation import from other readers, file replacement with re-anchoring.

**Notes.** Typed notes keeping quotations separate from interpretation, a manuscript outline,
exports to DOCX, RTF, HTML, Markdown and JSON, and an exchange file for other Prism tools.

**Citing.** APA 7 parenthetical, narrative, reference, bibliography and quotation output with
four separate checks; clipboard text, HTML and RTF together; a ledger of copies that flags
changed output; bibliography export; frozen submission snapshots; style checks with reviewed
fixes; automatically refreshed BibTeX files.

**Safety.** Verified scheduled backups, restore into a new folder, a complete open export
package, safety copies before large changes, a lock against two writers, no telemetry, a network
log, an optional local API with approval and scopes, administrator policy file, portable mode.
