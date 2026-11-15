# Release notes

## 0.1.2 (pre-release, 2026-11-15)

Fixes for problems found while refreshing the user guide for 0.1.1. None of them lost data.

- **Keyboard.** On a row of tabs (Settings, Cite), the arrow keys now move along the whole row,
  and Home and End go to the first and last tab. Before, the focus fell to the page heading after
  one move.
- **Notes.** After File in section, the number beside the section updates at once.
- **Compare records.** The first rows are named Type, Authors and other people, and Date, and the
  type shows its name (for example Journal article). Refresh from the online record shows dates
  as dates.
- **Opening a library.** When the library is still marked open by this computer, for example after
  the program stopped suddenly, the message says so and says the mark clears by itself within two
  minutes. A library open on another computer gets its own message.

## 0.1.1 (pre-release, 2026-11-15)

Fixes for faults found while writing the user guide for 0.1.0. None of them lost data. The
installer and the program are signed and timestamped (Jason Wrench). Automatic updates are off
in this build.

- **Screen readers.** The Folder box in File › New library has a name. The page behind an open
  dialog is no longer read or reachable. The Settings › Network tick boxes have plain names
  ("Allow DOI cross-check") with the short code beside them for the network log.
- **Keyboard.** Escape closes only the dialog on top, not every open dialog. Tab in an open menu
  closes it and moves on; Home and End jump to the first and last item.
- **Layout and wording.** The action buttons above the library table wrap onto a second row
  instead of overlapping. The status bar shows the version instead of "Interface concept build".
  Settings › About shows a build code and date, and in portable mode says the settings are next
  to the program. The PDF import review says whether an identifier was found and whether it was
  looked up, instead of the same note for every file. Compare records shows dates as dates and
  one person per line.
- **Dark theme.** Primary buttons use a dark label, so their text is easy to read (6:1).
- **Behaviour.** The same notice shown again replaces the earlier one with a count, and at most
  five notices show at once. The program starts on the page it was on when it closed. Archive on
  a project card asks first.

## 0.1.0 (pre-release, 2026-11-14)

First public pre-release, for testing. The installer and the program are signed and timestamped
(Jason Wrench). Automatic updates are off in this build.

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

**Export.** Spreadsheets (.xlsx), copying selected sources as BibTeX, biblatex, RIS or CSL JSON,
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

**Journal names and several libraries.** Journal abbreviations are kept with each source and in
every export format, learned into a journal list (which can also import a list file), filled in
by a reviewed batch fix, and shown full or abbreviated per manuscript or by default. Ctrl+Shift+L
switches library; selected sources can be copied or moved to another library with their files,
marks, notes and labels, never as duplicates. Once releases are published, the previous version
can be reinstalled from Settings.

**Side by side and quick copy.** Two open PDFs can be read side by side (the second scrolls and
zooms on its own and shows its marks; Swap makes it the one you mark). An optional system-wide
shortcut opens a small Quick copy box over any program: find a source, choose parenthetical,
narrative or the full reference, press Enter and paste. It uses the clipboard; nothing is typed
into the other program.

**Security.** After an independent review: policy counts only when an administrator made it and
can only restrict; export packages and restored backups cannot reach files outside the library or
network shares; "Open in default app" opens only unchanged documents; a database from elsewhere
cannot run its own SQL; programs connected through the local connection cannot open, move or
delete files; CSV cells cannot become formulas; online lookups start off and each request follows
its own switch; requests cannot reach this computer or the local network; only web addresses
become links in copied text. The PDF and XML readers were updated to versions without known
issues, so a crafted PDF cannot crash the program by nesting objects deeply.

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
