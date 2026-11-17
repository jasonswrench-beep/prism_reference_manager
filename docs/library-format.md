# Library folder and file formats (schema version 2)

```
<library folder>/
  library.db            SQLite working store (source of truth)
  library.lock          Present while the library is open (device, process, heartbeat)
  files/                Managed attachments: files/<hash[0:2]>/<hash[0:16]>/<original name>
  mirror/<table>/<id>.json   Canonical JSON for every entity (notes also as .md)
  mirror/_external/          Outside edits kept for review when the program had to rewrite a file
  backups/              Scheduled backups (zip) and pre-migration copies (.db)
  backups/before-changes/    Copies taken before merges, bulk edits, imports and deletions
```

## Store

Every entity table has `id TEXT PRIMARY KEY, data TEXT` where `data` is canonical JSON (keys
sorted). Tables: `refs`, `aliases`, `projects`, `project_sources`, `manuscripts`,
`manuscript_sources`, `copy_events`, `snapshots`, `collections`, `labels`, `saved_searches`,
`duplicate_decisions`, `tasks`, `attachments`, `annotations`, `notes`, `outline_sections`,
`import_batches`, `styles`, `integration_runs`, `custom_fields`. Field definitions are in
`crates/prm-core/src/model.rs`.

`events` holds one row per change: `seq, id, grp, time, device, actor, tbl, entity_id, before,
after`. `undo_groups` holds undo steps. `fts` and `page_text` are rebuildable search caches.
`meta` holds `schema_version`, `library_id`, `name` and `mirror_seq` (the mirror checkpoint).

## The text mirror

The mirror is written from committed events behind the `mirror_seq` checkpoint. `mirror_files`
records the SHA-256 of every file as the program wrote it, so a file whose content differs was
changed outside the program. Such changes are never taken in silently (Tools › Text mirror): each
is checked by reading it into its record type, shown field by field, and applied only when
chosen, as one undoable step; a deleted source file moves the source to the trash. Note Markdown
edits change the title, quotation (`> ` lines) and body. Records the program keeps for itself
(import batches, copy events, snapshots and similar) can only be put back. If the program has to
rewrite a file that was changed outside, the outside version is first copied to
`mirror/_external/` and listed for review.

A library rebuilt from the mirror alone has the same records as the original
(`prm_core::mirror::reconstruct`; Tools › Text mirror › Check that the mirror is complete).
Search caches, page text and the change history are rebuilt, not part of the comparison.

## Format versions and upgrades

`meta.schema_version` is the format version; `schema_history` records each version a library
reached (`version, applied, label, backup, app_version`).

| Version | Change |
|---|---|
| 1 | First format. |
| 2 | Adds `schema_history`. Additive tables and columns of this release (`ocr_words`, new JSON fields) need no step: fields absent from older records take their defaults. |

Opening a library in an older format: the database is copied to
`backups/pre-migration-schema<old>-<time>.db` and the copy is checked (SQLite integrity check and
the same row count in every table); then every upgrade step runs in one transaction. If the copy
fails its check, or any step fails, nothing is changed and the library is not opened. A library in
a newer format than the program writes opens read-only.

Reversal (Settings › Library and backup › This library › Format history) closes the library, keeps
the current file as `backups/before-revert-<time>.db`, and puts the pre-upgrade copy back. It is
for returning to an earlier program version; this version upgrades the library again when it next
opens it. It cannot make an older program understand fields added later.

New upgrade steps go in `prm_core::store::MIGRATIONS` together with a raised `SCHEMA_VERSION`, a
row in the table above and a test in `crates/prm-core/tests/migrations.rs`.

## Key rules

- A reference keeps its id through edits and export. A merge keeps an alias from each retired id.
- Names are structured (family, given, particle, suffix, literal organization or single name).
- Dates are partial (year, month, day, season, range, approximate, status such as "in press").
- `confirmed_absent` lists fields the person confirmed do not exist (different from unknown).
- Article numbers are stored separately from page ranges.
- Unmapped imported fields are kept in `overflow` under a namespace such as `ris:M3`.
- Each annotation stores the attachment version hash, physical page index, page label,
  confirmed citation locator, page-relative geometry and text anchors.
- An attachment's `mode` is `managed` (its `path` is relative to `files/`) or `linked`. A linked
  `path` is either a full path or `base:` followed by a path relative to the base folder, with
  forward slashes. The library's base folder is the `linked_base` key in `meta`; each computer
  may override it in its own settings.
- An attachment whose text was recognized from page images has `text_source` `ocr`, with
  `ocr_lang`, `ocr_quality` (0 to 1, an estimate) and `ocr_date`. Its page text is in
  `page_text` like any other; the `ocr_words` table holds each page's words with page-relative
  positions so an Area mark can take the words inside it. The PDF file is not changed.
- An annotation's `text_origin` says where its text came from (empty for the PDF's text layer,
  `ocr`, or `typed`). When the person edits extracted text, `extracted_text` keeps the original and
  `text_edited` is true.

## Export package

`Export complete library package` writes a zip with `manifest.json` (counts, checksums, journal
watermark), `data/<table>.json` for every entity, `notes/<id>.md`, `references.csl.json`,
`references.bib`, `styles/`, and `files/` (each file named by its content hash,
`files/<hash>.<ext>`, so no name can climb out of a folder when the package is unpacked elsewhere;
packages from earlier builds used the stored path and still import). Importing it into an empty folder reproduces the
library; the round trip is tested on every build.

## Exchange file for other Prism tools

`prism-family-exchange` version 1.1: a manifest, `sources` (stable ids, citation keys, CSL JSON)
and `excerpts` (type, source ids, exact text, user text, locator, anchor state, origin). It is a
file export, not live synchronization. The full specification is in
[`exchange-format.md`](exchange-format.md), with a sample in `examples/prism-exchange.json`.

## Annotation sidecar

`<name>.prism-annotations.json` next to an exported annotated PDF: format
`prism-annotation-sidecar` version 1, the attachment hash, and every annotation with project
scope, meaning, links, locators and review state. Each PDF annotation's name equals its id.

## Data from outside the program

Export packages and restored backups are made elsewhere, so their contents are checked before use:

- Attachment rows are kept only when a managed path is relative with plain name parts (no `..`,
  drive or root) and a linked path is on a lettered drive or under the base folder. Network paths
  (`\\server\share`, `\\?\`) are refused, since opening one would make Windows offer the
  person's sign-in to that server. Refused rows are listed in the import result or the restore log.
- Files unpacked from a package or backup are marked as downloaded (a `Zone.Identifier` stream),
  so Windows warns before anything runs them. "Open in default app" opens only document types whose
  content still matches the recorded hash.
- On every open, the database runs in SQLite's defensive mode with an untrusted schema, and any
  trigger or view is removed (the program makes none). The mirror ignores change records that name
  an unknown table.
- Before deleting, renaming or copying into the files folder, the program checks the real location
  is inside it.
