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
converted either way in one step; conversion copies, never deletes, and can be undone. Files
that were moved or renamed outside the program are found again by their content and relinked
together. A storage report shows the space used, the same file attached more than once (with a
one-click fix when it is one source) and files no source uses. Files copied into the library can
be named by a pattern such as author and year; the original name is kept.

**Scanned PDFs.** Text in scanned PDFs can be recognized on this computer, in any language
installed in Windows, for one file or all scanned files at once, with progress and Stop. The
recognized text is searchable, shown beside the page for checking, and used for quotations from
Area marks; a quality estimate warns about poor scans. The PDF is never changed, and an edited
quotation keeps the original recognized text.

**Where values came from.** Every value records its source and time, and values filled in
automatically also record how sure that source is: a record found by its DOI is near certain, a
web page's tags or a pasted reference less so, a guess from a PDF's properties least. Values below
70% are highlighted in the editor and marked for review in the list until you check or correct
them. A value two resolvers disagree on is marked as a conflict.

**Library format upgrades.** A library made by an earlier version is upgraded on opening, after
a copy is saved and checked; if anything fails, nothing changes. Each upgrade is recorded, and the
last one can be reversed to go back to an earlier program version. A library from a newer version
opens read-only with an explanation.

**Text mirror.** The plain-text copy of the library (JSON per record, Markdown for notes) can be
checked: a library rebuilt from it alone must match. Files changed outside the program are listed
with their changes field by field and taken in only when you apply them, as one undoable step;
broken files are explained and put back. An outside edit is never overwritten without being kept
for review.

**Keyboard and quick entry.** Every shortcut can be changed (a key already in use is moved, and
plain letters are refused so typing is never taken), and the list can be printed. Quick add
(Ctrl+Shift+A) takes a pasted identifier, web address or reference: a clean lookup is added in one
step, anything uncertain goes to review. A special-character palette (Ctrl+Shift+U) inserts
accents, punctuation, symbols, Greek and Cyrillic at the cursor, and Alt+X turns a character code
into its character. A short getting-started tour follows the first library.

**Collections and background jobs.** Sources can be dragged onto a collection (Shift moves them out
of the collection being shown), and collections onto each other to nest them; every drag has a
keyboard way (Add to collection, Move to collection, a collection's Move into…, Alt+Left). Long
tasks (reading PDF text, recognizing text, imports) run as jobs with progress, a log and a
completed count; reading and recognizing can be paused, resumed, cancelled and retried, and an
import can be stopped safely.

**Labels, links and administration.** Labels can be created several at a time, recoloured,
renamed, merged and deleted in one place (Edit › Manage labels). Sources, collections, marks and
notes have a Copy link button; the prism-rm:// link opens the item in the program from a document
or another program. Administrators can lock settings with Group Policy (template included) as
well as with policy.json.

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
