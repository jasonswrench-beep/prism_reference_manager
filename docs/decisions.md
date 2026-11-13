# Design decisions

Recorded at the first build. Each decision lists what was chosen, why, and what would change it.

## D1. Desktop toolkit: Tauri 2 with the system WebView2

The program is its own Windows application with a native window, built the same way as the
other suite tools. The interface is HTML and JavaScript rendered by the WebView2 runtime that
ships with Windows; no browser or separate browser window is involved. The Rust core runs in
process. Installer: NSIS, per-user (no administrator rights).

Accessibility: WebView2 exposes the interface to Windows screen readers through UI Automation.
Keyboard and screen reader conformance still needs the manual audit listed in TESTING-TODO.md.

## D2. Store: SQLite, one table per entity, change event per edit

Each entity is canonical JSON in its own table; generated columns index the members that need
lookups. Every edit commits the entity and its change event (before and after) in one
transaction. Events drive persistent undo and redo, per-field history, the text mirror, and
later synchronization. Search uses an FTS5 index over folded text; it is a rebuildable cache.

The text mirror (`mirror/` in the library folder) is written from committed events behind a
durable checkpoint, with atomic file replacement. A crash between commit and mirror write is
repaired on the next open by replaying events after the checkpoint.

A lock file with a heartbeat prevents two computers or processes from writing one library. A
library inside a synchronized folder shows a warning.

## D3. Citation processor: a built-in APA 7 composer for the first release

The specification asks for a deterministic processor chosen by testing real fixtures, narrative
output, Unicode, performance and licensing. For the first release the program uses its own
composer for APA 7 (`crates/prm-core/src/render/apa.rs`): narrative and parenthetical forms are
separate operations; disambiguation (year suffixes, et al. ambiguity, first-author initials) is
computed over the manuscript's reference set; output is structured runs consumed unchanged by
every serializer (clipboard HTML, RTF and text; DOCX; RTF; HTML; Markdown).

Assurance is computed from the golden fixture corpus at run time. The APA profile reports
**Limited testing** until every case has been reviewed by a qualified person (the
`reviewed_by` field in `tests/fixtures/apa/fixtures.json`); it reports **Verified profile** only
when all approved cases pass and are reviewed.

### D3a. Other styles: CSL through an embedded processor (2026-11-13)

Every style other than APA 7 is rendered from a Citation Style Language file by an embedded
CSL processor (hayagriva 0.10.1, MIT or Apache-2.0), through `render/csl.rs`. Records go to the
processor as CSL JSON; its formatted output (italic, bold, small caps, super- and subscript,
links) comes back as the same runs every serializer already uses.

Evaluation against the 57 non-quotation APA golden cases, using the processor's own APA style:
36 matched exactly. The misses include year-suffix order, "&" where APA's narrative form needs
"and", an ellipsis character instead of ". . .", the publisher repeated when it is the author,
no first-initial disambiguation, missing suffixes on "n.d.", and an empty title in one
narrative case. A 200-entry reference list rendered in about 150 ms. The processor therefore
does **not** replace the composer for APA, and the APA CSL styles are hidden.

Consequences:

- About 75 bundled styles from the CSL style repository (CC BY-SA 3.0) are shown as
  **Untested**, and every citation from them carries a review warning. Imported .csl files are
  shown as **Imported, not reviewed**. Neither inherits verification from a parent style.
- Bundled styles that the processor renders wrongly in a way a person could miss are withheld
  (`KNOWN_BROKEN` in `render/csl.rs`); currently the two NLM citation-sequence styles, which drop
  author names. The test `every_bundled_style_renders_a_citation_and_reference` guards this.
- Imported styles are stored in the library's `styles` folder under their content hash, so a
  manuscript keeps rendering with the exact file it chose; importing a changed file adds a new
  version. Dependent styles resolve their parent from imported or bundled styles.
- Citation language applies to CSL styles through the CSL locale files; APA 7 stays US English.
- Prefixes and suffixes inside a citation are not placed by the processor; output says so.
- Literal dates (for example "in press") are passed as the CSL `status` variable, and date ranges
  are reduced to their start date, because the processor's CSL JSON reader rejects both.
- Release builds now unwind on panic so a failure inside the processor is caught and reported
  as a warning instead of closing the program.

A style becomes **Limited testing** or **Verified profile** only when it has its own golden
cases, as APA 7 does.

## D4. PDF reading: pdf.js in the window, lopdf in the core

Pages, text layers and thumbnails are rendered with pdf.js (Apache-2.0), vendored at build so the
program works offline; scripts in PDFs are never run. The core uses lopdf to inspect files (page
count, page labels, encryption, damaged structure, image-only pages), to write annotated and
flattened copies, and to read annotations made in other readers.

Annotations use page-relative geometry of the unrotated page, so zoom and rotation do not move
them. Exported annotations carry the library annotation id as their PDF name, which is how
re-importing an exported copy avoids duplicates.

## D5. Clipboard: text, HTML and RTF together, confirmed before recording

The shell writes Unicode text, the Windows HTML clipboard format and RTF in one clipboard
transaction, reads the text back to confirm, and only then records a copy event. Bibliography
paragraphs use real hanging-indent paragraph properties in HTML and RTF.

## D6. Interchange formats and naming

Importers: RIS, BibTeX/biblatex, CSL JSON, the XML record format several desktop reference
managers export, mapped CSV/TSV, and pasted reference lists (heuristic, always reviewed).
Exporters: RIS, BibTeX, biblatex, CSL JSON, CSV, Prism XML, and the complete package.

Owner decision (2026-11-13): interoperability labels in the interface (import sources, the
Connections page, the citation-context hand-off) name the service or program a file comes from.
Code, comments, commits, documentation and release notes stay generic, and nothing compares
this program with others. Service APIs are not built in; file exchange is the built-in method.

## D7. Local API

Off by default, loopback only, per-client tokens with scopes, approval in the window, immediate
revocation, and an activity log. Requests carrying a web-page origin are refused. Items created
by API clients are marked as machine-created.

## D2a. Confidence of automatically filled values (DAT-002)

Each field's provenance holds its source, time, a user-edited flag, a review state and, for values
filled in automatically, a confidence from 0 to 1 (`prm_core::provenance`). The scale is coarse
and rule-based rather than a statistical estimate: 0.95 for a registration record found by its
identifier, 0.85 for a book catalogue record, 0.5 to 0.8 for a web page depending on whether it
carries scholarly citation tags, the parser's own estimate for pasted references, and 0.4 to 0.5
for guesses from a PDF. A value a second resolver contradicts drops to 0.6. Typed values, values
copied from another program's export file and tidying rules carry none. Below 0.7 a value asks to
be checked; checking keeps the source and confidence and records the review, and editing makes the
value the person's own.

## D7a. Text recognition (OCR)

Scanned PDFs are recognized with the optical character recognizer that ships with Windows 10 and
11: it is local, needs no download or account, and adds no third-party code. Languages are the
ones installed in Windows. pdf.js renders each page at about 200 dots per inch; the recognizer
returns lines and word boxes. It reports no confidence, so the program estimates quality as the
share of tokens that look like words or numbers and warns below 75%. Recognized text never goes
into the PDF; it is stored with the attachment and can be removed or redone. Quotations taken
from recognized words are labelled as such, and an edit keeps the original extraction.

## D8. Administrator policy and portable mode

`policy.json` next to the program or in `%ProgramData%\Prism Reference Manager` locks setting
values (for example `network_enabled`, `api_enabled`, `auto_backup`). A `portable.txt` file next
to the program keeps settings and the default library beside it.

Group Policy: values under `Software\Policies\Prism Reference Manager` lock the setting of the
same name, in HKCU and HKLM (computer policy wins over user policy, and both over
`policy.json`). Each value takes the type of its setting (a DWORD 0 or 1 for a yes/no setting, a
number, text, or JSON text for a list); names that are not settings and values that do not fit
are ignored. `docs/policy/PrismReferenceManager.admx` and `en-US/PrismReferenceManager.adml` are
the administrative template for the common settings.

## D8a. Secrets

The program stores no secrets. Local connection tokens are shown once and kept only as hashes;
service API keys are not built in (file exchange is the built-in method). A test fails if the
settings file ever gains a password, secret, key or token field. Any future secret goes in the
Windows credential store, never in settings or the library.

## D8b. Links

`prism-rm://open/<source|collection|annotation|note|project>/<id>?library=<library id>` opens an
item. The installer registers the scheme for the current user and the uninstaller removes it.
A link only shows an item; anything else is refused. A link to another library on this computer
asks before switching. A second start of the program (from a link) hands the link to the running
window.
