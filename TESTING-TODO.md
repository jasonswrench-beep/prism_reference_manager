# Testing to do (needs a person)

Everything here needs a human: judgement against a style manual, real receiving programs, a
screen reader, a second computer, or a decision only the owner can make. Automated checks that
already pass are listed at the end for reference.

## 1. Decisions for the owner

- [ ] **Shipping license.** The repository uses Apache-2.0 like the other suite tools. Approve it,
      or name another, before anything is published (spec LIC-001, LIC-009).
- [ ] **Code signing.** Installer and portable builds are unsigned. Approve a certificate and the
      signing step before a release (spec PLT-001, SEC-009). Windows SmartScreen will warn on
      unsigned builds.
- [ ] **Update channel.** No automatic update check exists yet. It needs a signed update feed and
      a place to host it, which means publishing. Decide whether and where (spec PLT-009).
- [ ] **Service names in the interface.** Research services and other reference managers appear
      under generic labels ("Literature-mapping service", "AI research search engine",
      "Citation-context service", "AI reading assistant", "another reference manager's
      export"). Confirm this wording works for users.
- [ ] **Citation-context hand-off.** The program asks the person to paste the service's report
      address pattern once (with `{doi}`) rather than shipping a product address. Confirm.

## 2. Citation correctness (release gate G01, G02)

- [ ] **Review the 58 APA golden cases** in `crates/prm-core/tests/fixtures/apa/fixtures.json`
      against the APA Publication Manual (7th ed.). For each correct case put the reviewer's
      name in `reviewed_by`. Correct any expected text that is wrong, rerun
      `cargo test -p prm-core`, and fix the composer if a case then fails. Until every case is
      reviewed, the program shows APA as *Limited testing* rather than *Verified profile*.
      Cases worth a careful look: preprint italics and the absence of a `[Preprint]` label;
      20 and 21+ author lists; `(n.d.-a)` forms; four-author et al. ambiguity (the program spells
      out every name when only one would be replaced by et al.); retrieval dates for web pages;
      thesis with and without an archive.
- [ ] Have an academic editor or librarian review output for every source type in Cite ›
      Style checks › Preview using real records.

## 3. Real paste matrix (release gate G04, G05; spec CIT-018)

For each host, record the exact build, OS and paste mode, and whether italics, punctuation,
non-Latin characters, links and the bibliography's hanging indent survive. Use Copy
parenthetical, Copy narrative, Copy reference and Copy bibliography.

| Host | Ordinary paste | Keep source formatting | Merge formatting | Paste as plain text |
|---|---|---|---|---|
| Desktop word processor (Windows) | [ ] | [ ] | [ ] | [ ] |
| Same word processor, web version | [ ] | [ ] | [ ] | [ ] |
| Online document editor | [ ] | [ ] | [ ] | [ ] |
| Open-source office suite writer | [ ] | [ ] | [ ] | [ ] |

- [ ] Open the exported `references.docx` and `references.rtf` in each host: hanging indent
      0.5 inch, double spacing, italics, links.
- [ ] Confirm plain-text paste has no stray control characters.

## 4. PDF annotations with other readers (release gate G08; spec ANN-008)

- [ ] Export an annotated copy (Reader › Export annotated PDF) with every mark type and open it in
      two independent desktop PDF readers. Record which marks show and which attributes are lost
      (publish this as the compatibility table).
- [ ] In each reader add a highlight, a sticky note and a text box, save, then use Import marks
      from another reader. Check: additions previewed, changed comments shown as conflicts,
      nothing duplicated on a second import, highlighted text recovered.
- [ ] Check the flattened copy displays correctly and has no editable marks.
- [ ] Rotate pages in the reader view and zoom: marks stay on their text.
- [ ] Replace a file with a revised version (Files › Replace with new version): moved quotations
      are marked "Check position", missing ones "Not found", nothing moves silently.
- [ ] A password-protected PDF: the password prompt appears, marks save to the library layer,
      and the annotated-copy export explains why it is unavailable if the file is encrypted.
- [ ] A scanned (image-only) PDF: the warning appears; Area, Draw and Comment work.

## 5. Accessibility (release gate G11; spec ACC-001 to ACC-004)

- [ ] With a screen reader (NVDA and Narrator), complete: import a file, mark a PDF passage,
      copy a citation, export a bibliography. Note any unlabelled control or focus trap.
- [ ] Keyboard only: every view, menus (Alt-less menu bar: Tab to it, arrows inside), command
      palette (Ctrl+K), dialogs (focus returns after closing), the library table (arrows, Space,
      Shift+arrows, Enter), the outline (Alt+arrows to move sections), reader tool keys.
- [ ] Windows text scaling at 200 percent and display scaling at 150 and 200 percent: no clipped
      controls in Library, Reader, Cite.
- [ ] High contrast theme in Windows and the program's own high-contrast theme.
- [ ] Run an automated WCAG checker over each view (for example by opening the debug build's
      developer tools) and record results.
- [ ] Known gap: selecting PDF text with the keyboard alone is limited; marks can be created with
      a mouse selection then edited by keyboard. Decide whether a caret-browsing mode is needed.

## 6. Installation and recovery (release gate G10, G12, G13)

- [ ] Install with the per-user installer on a clean Windows account without administrator rights.
      (Automated: `tools	est-installer.ps1` passed on the development machine — silent install
      into a chosen folder, per-user registration, uninstall removes only the program folder and
      its settings, neighbouring folders and the library folder untouched.)
- [ ] Silent install `Prism-Reference-Manager-0.1.0-setup.exe /S` and uninstall `/S` from a script.
- [ ] Uninstall: confirm `%LOCALAPPDATA%\Prism Reference Manager`,
      `%APPDATA%\org.prism.referencemanager` and `%LOCALAPPDATA%\org.prism.referencemanager` are
      gone, and the library folder under Documents is untouched.
- [ ] Portable zip: unzip to a USB drive, run, confirm settings and the default library live in
      `data\` next to the program.
- [ ] Restore a backup on a second computer (Restore backup → empty folder), relink attachments
      with Tools › Check attached files › Relink all.
- [ ] Open the same library from two computers through a synchronized folder: the second must be
      refused while the first is open.
- [ ] Pull the power (or end the process in Task Manager) during an import and during a backup;
      reopen: no committed records lost, mirror repaired, an interrupted backup leaves only a
      `.part` file.
- [ ] Turn off networking entirely: library, reader, notes, citations, exports and backups work.
- [ ] `policy.json` with `{"network_enabled": false, "api_enabled": false}` in
      `%ProgramData%\Prism Reference Manager`: settings show "Managed" and cannot be changed.

## 7. Online lookups and research-service files (release gate G09)

- [ ] Look up a real DOI, PubMed ID, PMC ID, arXiv ID, ISBN and a journal article web page.
      Check corroboration messages, the network log, and the per-field Refresh dialog.
- [ ] Import a real export from each research service type (BibTeX, RIS, CSV) and from another
      reference manager (RIS and XML). Check the migration scan, mapping report, duplicates and
      batch undo.
- [ ] CSV import with column mapping from a spreadsheet saved by a spreadsheet program
      (UTF-8 with BOM, and semicolon-separated).

## 8. Performance (spec PRF-001 to PRF-015)

- [ ] Record the reference machine (4 cores, 16 GB, SSD, OS build, power plan) and rerun
      `cargo test -p prm-core --release --test robustness benchmark_100k_references -- --ignored --nocapture`.
      Measured on the development machine, 100,000 references, release build:

      | Measure | Result | Target | Status |
      |---|---|---|---|
      | Insert 100,000 records (one batch) | 27.6 s | — | |
      | Text mirror catch-up after that batch | 523 s (runs in the background) | — | slow but nonblocking |
      | Metadata search | median 328 ms, p95 919 ms | p95 ≤ 250 ms | **missed** |
      | List and sort a page | 1.0 s | ≤ 500 ms | **missed** |
      | Citation render, 200-source context | 42 ms | ≤ 500 ms | met |
      | Duplicate scan | 116 s | ≤ 60 s | **missed** |

      At 20,000 references search p95 was 229 ms and list 189 ms, both within target. The misses
      at 100,000 need work before release: paging the list in SQL instead of in memory, a
      lighter search result path, and finer blocking in the duplicate scan.
- [ ] Open a 500-page PDF and time the first page (target 2 seconds).
- [ ] Index 20,000 PDFs (Settings › Library › Index all PDFs) and time it.
- [ ] Warm start with a 100,000-reference library (target 3 seconds); idle memory (target 500 MB).

## 9. Usability pilot (spec UIX-016)

- [ ] Observe faculty, graduate and undergraduate users, a librarian and a keyboard or
      screen-reader user: collect a paper, resolve a metadata conflict, mark a PDF, reuse a
      source in another project, copy both citation forms, recover an earlier version. Record
      misunderstood status labels.

## 10. Not built yet (first-release items still open)

These are specified for the first release but not implemented in this build. Each needs either
work that could not be verified here or an owner decision.

- Browser capture extension (spec CAP-001 to CAP-003). The local API it would use is built.
- Optional local OCR for scanned PDFs (FIL-010).
- Journal abbreviation lists (QLT-009); custom reference types and fields editor (LIB-002, DAT-008).
- Global quick-copy launcher with a system-wide shortcut (WRT-019).
- Side-by-side reading of two documents (part of RDR-001); configurable colour-to-type scheme
  (RDR-003); annotation filters by colour and author (RDR-007).
- Resizable and saved pane layouts, list and card views, customizable shortcuts (UIX-001,
  LIB-009, UIX-012). Navigation can be collapsed (Ctrl+B).
- A searchable catalog of other citation styles (STY-002) and a style editor (P2).
- Copying references between two libraries (LIB-017); link scheme for opening items from other
  programs (EXT-013); scheduled export presets (EXP-006); a spreadsheet (.xlsx) export (CSV works).
- Watched folders (CAP-009); tasks interface (KNW-012; the data model exists).

## Automated checks that pass in this build

- 58 APA golden cases (text and italics), run on every build.
- Workflow tests: context change flags copies and isolates other manuscripts (G03); project
  isolation (G06); crash between commit and mirror write is repaired on open, undo survives a
  restart (G10); concurrent writer refused; full export package round trip compares equal
  (G13); backup verify and restore; import staging, duplicates and batch undo; merge with
  aliases and undo; accent-insensitive, field, range, phrase and regex search; quotation anchor
  rules; PDF annotation export, reimport without duplicates, flattened copy (G08, within this
  program).
- Robustness: 3,500 mutated and random inputs through every importer and the query parser
  without a crash (found and fixed two crashes); malformed and placeholder PDFs reported.
- End-to-end in the real window (debug build): library and project creation, import, citation
  rendering, clipboard copy with text, HTML and RTF all present, changed-copy detection, style
  issue fix and undo, PDF attach with page labels, highlight, quotation note, outline, search,
  health report, every view renders without errors, command palette.
