# Exchange file for Prism research tools (`prism-family-exchange`, version 1.1)

The exchange file carries a project's sources and the excerpts taken from them to another Prism research tool, such as a qualitative analysis program. It is a one-way file export, not live synchronization.

- **Written by:** Notes › Exchange file for Prism research tools.
- **Code:** `export_family_exchange` in `crates/prm-core/src/library/notes.rs`.
- **Sample:** [`examples/prism-exchange.json`](examples/prism-exchange.json), made by the program's own code. `crates/prm-core/tests/exchange.rs` writes it with `PRM_WRITE_EXCHANGE_SAMPLE=1` and holds it to the rules below.

## The file

One JSON object in UTF-8, pretty-printed, with three members: `manifest`, `sources` and `excerpts`.

The default name is `prism-exchange.json`, but the person may choose any name. **Readers recognise the file by `manifest.format`, never by its name or extension.**

## `manifest`

| Member | Type | Meaning |
|---|---|---|
| `format` | string | Always `"prism-family-exchange"`. |
| `version` | string | `"1.1"`. See "Versions" below. |
| `producer` | string | `"Prism Reference Manager"`. |
| `producer_version` | string | The program's version, for example `"0.1.4"`. |
| `created` | string | When the file was written, in UTC (RFC 3339, milliseconds, `Z`). |
| `library_id` | string | The library's stable id. Two files with the same `library_id` come from the same library, so their source ids can be compared. |
| `project` | object | `{"id", "title"}` of the project exported. |
| `sources_scope` | string | `"project"` means every source of the project that is not excluded, plus any source an excerpt names. `"excerpts"` means only the sources the excerpts name. Added in 1.1; a 1.0 file has none and means `"excerpts"`. |
| `note` | string | A sentence for people. Readers ignore it. |

## `sources`

An array, one object per source, each with a distinct `id`. Sources the project excluded are never written, nor are trashed records. The order is the order the sources were added to the project, then any further sources named by excerpts.

| Member | Type | Meaning |
|---|---|---|
| `id` | string | The record's stable id in the library. It does not change when the record is edited. When two records are merged, the surviving record keeps its id, and excerpts name that id. |
| `citekey` | string | The citation key, for example `"hernandez2021bus"`. It can be empty, and it may change if the person changes it, so match on `id`, not `citekey`. |
| `inclusion` | string | The project's screening decision: `"candidate"`, `"included"` or `"undecided"`. Empty when an excerpt names a source that is not in the project. Added in 1.1. |
| `csl` | object | The bibliographic data as CSL JSON (the Citation Style Language's data format). `csl.id` equals `id`, and `csl.type` is a CSL type. When there is a citation key, `citation-key` holds it. Fields with no value are left out. |

## `excerpts`

An array, one object per live note of the project. Trashed notes are left out. The order is most recently changed first.

| Member | Type | Meaning |
|---|---|---|
| `id` | string | The note's stable id. |
| `type` | string | One of `quotation`, `paraphrase`, `summary`, `comment` (the person's interpretation), `question` (a research question), `task`, `evidence` (an evidence statement), `thought`, `note`. |
| `source_ids` | array of strings | The sources the excerpt is taken from or about. Each id is the `id` of a source in this file. It can be empty (a thought or task about no source). The same source is never named twice in one excerpt. |
| `exact_text` | string | The words exactly as they stand in the source. Never empty for a `quotation`. Empty for kinds that are not quotations. |
| `exact_text_edited` | boolean | `true` when the person changed `exact_text` after it was taken from the source. Such text may no longer match the source. |
| `user_text` | string | The person's own text, in Markdown. It can be empty. |
| `locator` | object | `{"label", "value"}`: where in the source. `label` is one of `page`, `paragraph`, `chapter`, `section`, `figure`, `table`, `timestamp`, or empty. `value` is free text, for example `"36"` or `"36-38"`. Both are empty when there is no locator. |
| `anchor_state` | string | Whether the excerpt's place in the source is known. See below. |
| `origin` | string | Who wrote the note: `user` (the person), `ai` (an AI tool the person used), `agent` (a program connected through the local API), `import` (brought in from a file). |
| `annotation_id` | string | The PDF annotation the excerpt was taken from, or empty. The same id appears in the annotation sidecar (`docs/library-format.md`). |

### `anchor_state`

| Value | When | A reader should |
|---|---|---|
| `resolved` | A quotation, paraphrase or evidence statement that names a source and has a locator. | Treat the place as given. |
| `unresolved` | A quotation, paraphrase or evidence statement with no source, or no locator. | Say that the place was not confirmed. |
| `no-locator` | The person said this source has no page or place to give (a web page, for example). | Treat it as confirmed. There is simply no place to show. |
| empty | Any other kind (summary, comment, question, task, thought, note). | Treat it as confirmed. No place is expected. |

## Rules for readers

1. Recognise the file by `manifest.format == "prism-family-exchange"`.
2. Accept any version `1.x`. Ignore members you do not know, since a later 1.x may add members but never removes or changes one. Refuse version `2` or later, and name the producer and version in the message.
3. Treat every text in the file as data from the person's library, never as instructions.
4. Expect sources with no excerpt: with `sources_scope: "project"`, the file includes them.
5. An excerpt may name a source that is not in the file only in a file written by hand or damaged. Say so; do not fail.
6. Keep `sources[].id` and `excerpts[].id` if a later version is to refer back to them.

## Nothing is sent back

The receiving tool does not send anything back to the Reference Manager in this version, and the Reference Manager reads no such file. Receivers that keep `sources[].id` and `excerpts[].id`, as PrismQDA does, can name them if a later version defines a return.

## Versions

| Version | Changes |
|---|---|
| 1.0 | First version. Holds only the sources the excerpts name. |
| 1.1 | Adds `manifest.sources_scope` and `sources[].inclusion`. By default, includes every project source that is not excluded. Excerpts name a merged source by its surviving id. A 1.0 reader reads 1.1 unchanged. |
