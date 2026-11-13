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

**Reading.** Each highlight colour carries a meaning (editable; keys 1 to 6 choose a colour), marks
can be filtered by colour, page range and author and linked to each other, and reading time is
kept per project while a document is open and in use.

**Working with selections and projects.** Project tasks on the dashboard and from a selected
source; rating a selection (per project or library-wide), adding it to a collection, renaming
or exporting its files; a project's evaluation and tags for each source; unlinking versions;
renaming projects, choosing their colour, and renaming the library.

**Export.** Excel workbooks, copying selected sources as BibTeX, biblatex, RIS or CSL JSON,
named export presets, and files kept up to date in any of those formats (not only BibTeX).
Notes export to plain text; annotations export to DOCX and structured JSON.

**Fields and types.** Your own fields (text, number, date, list, yes/no, web link) with
defaults, required values and validation, shown in the editor and details, searchable, and
carried in CSV and CSL JSON. Your own source types as named variants of built-in types, which
citation styles format as their base type. Both can be shared as a file. Fields whose values
repeat across sources suggest earlier values as you type.

**Clean-up.** Tidying rules run whenever a source is saved (spaces, standard DOIs, ISBNs and
ISSNs, final periods in titles, and optionally titles and names typed in capitals). Batch fixes
for selected sources find missing DOIs, fetch missing abstracts and change titles to sentence
case, always as proposals reviewed before anything changes. Web addresses can be checked and
access dates updated. A quick health check can run weekly or monthly.

**Moving from another program.** Groups in an export, including nested groups, can become
collections, and ratings are carried. Each record's identity in the exporting program is kept,
so importing a later export updates only the records that changed there, never overwriting edits
made here. Large imports show progress and can be stopped without adding anything.

**File storage.** Files can be managed (copied into the library) or linked (kept where they
are). Linked files inside a chosen base folder are stored relative to it, and each computer can
say where that folder is, so a synced folder at a different path still works. All files can be
converted either way in one step; conversion copies, never deletes, and can be undone.

**Scanned PDFs.** Text in scanned PDFs can be recognized on this computer, in any language
installed in Windows, for one file or all scanned files at once, with progress and Stop. The
recognized text is searchable, shown beside the page for checking, and used for quotations from
Area marks; a quality estimate warns about poor scans. The PDF is never changed, and an edited
quotation keeps the original recognized text.

**Search.** Results are ranked by relevance while searching (title and author matches count
most). A word that matches nothing is retried with close spellings, and the list says so.
"Mueller" and "Müller" find each other; Chinese, Japanese and Korean words match as words. Date,
added and modified ranges, a search builder, and author, journal, label, collection and project
results in the command palette. PDF text is indexed automatically in the background. Snippets are built only for the results
on screen, which keeps full-text search over 20,000 PDFs under a second.

**Citation styles.** Besides the tested APA 7 profile, about 75 Citation Style Language styles
are bundled and any CSL file can be imported, including dependent styles. A searchable catalog
previews a style with the manuscript's own sources and sets the citation language. Bundled
styles are labelled Untested and imported ones Imported, not reviewed; every citation from them
says so. A manuscript keeps the exact style version it chose.

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
