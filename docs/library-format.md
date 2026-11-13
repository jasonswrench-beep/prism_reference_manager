# Library folder and file formats (schema version 1)

```
<library folder>/
  library.db            SQLite working store (source of truth)
  library.lock          Present while the library is open (device, process, heartbeat)
  files/                Managed attachments: files/<hash[0:2]>/<hash[0:16]>/<original name>
  mirror/<table>/<id>.json   Canonical JSON for every entity (notes also as .md)
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

A newer schema than the program supports opens read-only. An older schema is migrated after a
copy of the database is written to `backups/`.

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

## Export package

`Export complete library package` writes a zip with `manifest.json` (counts, checksums, journal
watermark), `data/<table>.json` for every entity, `notes/<id>.md`, `references.csl.json`,
`references.bib`, `styles/`, and `files/`. Importing it into an empty folder reproduces the
library; the round trip is tested on every build.

## Exchange file for other Prism tools

`prism-family-exchange` version 1.0: a manifest, `sources` (stable ids, citation keys, CSL JSON)
and `excerpts` (type, source ids, exact text, user text, locator, anchor state, origin). It is a
file export, not live synchronization.

## Annotation sidecar

`<name>.prism-annotations.json` next to an exported annotated PDF: format
`prism-annotation-sidecar` version 1, the attachment hash, and every annotation with project
scope, meaning, links, locators and review state. Each PDF annotation's name equals its id.
