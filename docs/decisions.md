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

Open: evaluate a CSL processor for the wider style catalog (other styles would show Limited
testing). A CSL processor must pass the same fixtures before it can replace or join the
composer for APA. Owner input needed if the candidate's license or size affects distribution.

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

Research services and other reference managers are described generically in the program and
repository ("literature-mapping service", "another reference manager's export"). Their file
formats are supported; no service account is connected.

## D7. Local API

Off by default, loopback only, per-client tokens with scopes, approval in the window, immediate
revocation, and an activity log. Requests carrying a web-page origin are refused. Items created
by API clients are marked as machine-created.

## D8. Administrator policy and portable mode

`policy.json` next to the program or in `%ProgramData%\Prism Reference Manager` locks setting
values (for example `network_enabled`, `api_enabled`, `auto_backup`). A `portable.txt` file next
to the program keeps settings and the default library beside it.
