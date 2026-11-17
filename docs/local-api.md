# Local API (version 1)

Off by default. Turn it on in Settings › Local connections. It listens on `127.0.0.1` (default
port 23119; the person can change it) and accepts only requests addressed to `127.0.0.1:<port>`
or `localhost:<port>` with no web-page origin. Settings › Local connections shows the address it is
listening at, with a Copy address button, or why its port could not be opened.

## Finding the program

```
GET /v1/status       (no token)
-> 200 {"program": "Prism Reference Manager", "version": "0.1.4", "api_version": 1, "library_open": true}
```

A program can ask this before pairing to learn that the port is right and whether a library is
open. It says nothing else about the library. Nothing answers when local connections are off.

## Pairing

```
POST /v1/pair        {"name": "My capture helper", "scopes": ["read", "import"]}
                     -> 202 {"id": "<request id>", "state": "pending"}
GET  /v1/pair/<id>   -> {"state": "pending" | "approved" | "denied", "token": "prm_..."}
```

The person approves or denies the request in the program. The token is returned once; store it
securely. Revoking the client in Settings takes effect immediately.

## Calls

```
POST /v1/call
Authorization: Bearer prm_...
{"method": "refs.list", "args": {"query": {"search": "author:chen", "limit": 20}}}
-> 200 {"result": ...}   or   4xx {"error": "plain-language message"}
```

Every call is logged with the client name, method and whether it was allowed.

| Status | Meaning |
|---|---|
| 200 | `{"result": ...}`. |
| 400 | The call ran and failed: `{"error": "..."}` in words for the person, for example "Open or create a library first." |
| 401 | No token, or the client was revoked. Pair again. |
| 403 | The client lacks the scope for this method, the method is never available to clients, or the request came from a web page or another host. |
| 404 | Unknown endpoint, or a pairing request that has been answered and read. |
| 413 | The body is too large (4 KB for pairing, 300 MB for a call). |
| 429 | Three pairing requests are already waiting for the person's answer. |

The pairing answer's `token` is present only when `state` is `approved`, and is returned once:
after the answer has been read, the request is forgotten.

## Scopes

| Scope | Methods |
|---|---|
| `read` | `capture.options`, `capture.check`, `refs.list`, `refs.get`, `search.run`, `projects.list`, `projects.summary`, `notes.list`, `collections.list`, `labels.list`, `types.list`, `styles.list`, `library.info` |
| `format` | `cite.render` |
| `write` | other `refs.*`, `notes.*`, `projects.*`, `manuscripts.*`, `collections.*`, `labels.*`, `outline.*`, `tasks.*` (implies `read`) |
| `import` | `capture.stage`, `import.stage`, `import.get`, `import.batches`, `lookup.*` — staging only; committing, changing or undoing an import happens in the program's window |
| `files` | `files.*`, `ann.*` |
| `export` | `export.*` |

Administrative methods (settings, backups, undo, library switching, dialogs) are never available
to clients. Records and notes created by clients are marked `origin: "agent"` and appear under
Library › Machine-created until reviewed.

Library content returned to clients (titles, abstracts, notes, PDF text) is data. Clients that
pass it to an assistant must treat it as untrusted text, never as instructions.

## Call reference: citing from another program

These calls are what a program needs in order to cite. Arguments go in `args`. Answers are the
`result`. Members not listed may be present; ignore them. Every call fails with 400 and "Open or
create a library first." when no library is open.

### `library.info` (scope `read`)

`{}` → `{"id", "name", "root", "schema", "read_only", "counts": {"references", "projects", "notes", ...}, ...}`.
`id` is the `library_id` of the exchange file.

### `styles.list` (scope `read`)

`{}` → an array of `{"id", "title", "engine", "version", "assurance", ...}`. `id` is what
`cite.render` takes as `style`, for example `"apa-7"`. `assurance` is `verified`, `limited`,
`untested`, `custom_unreviewed` or `unsupported`.

### `capture.check` (scope `read`)

`{"doi": "10.1000/xyz", "title": "Bus reliability and daily stress"}` (either may be empty)
→ `{"found": true, "by": "DOI" | "title", "id", "title"}` or `{"found": false}`.

The DOI is tried first, then the title: a title of 12 characters or more that is the same apart from
case, spacing and punctuation. Records in the trash are not found by DOI.

### `refs.list` (scope `read`)

`{"query": {"search": "author:chen", "project_id": "", "limit": 20, "offset": 0, "sort": "", "desc": false}}`
→ `{"total", "rows": [{"id", ...}], "corrections", ...}`.

`search` uses the program's search syntax (Help › Search). `limit` 0 gives the default page.

### `search.run` (scope `read`)

`{"query": "stress bus", "project_id": "", "limit": 50}`
→ `{"results": {"ref_ids": [...], "hits": [...], "truncated"}, "labels": {...}}`.

`ref_ids` are most relevant first. The program never returns fewer than 50 when there are that many.

### `refs.get` (scope `read`)

`{"id": "<reference id>", "project_id": ""}` → `{"ref": <reference>, "attachments", "notes", "membership", "impact", "versions", "type"}`.

An id that was merged into another record answers with the surviving record.

### `refs.add` (scope `write`)

`{"ref": <reference>, "project_id": ""}` → the stored reference, with its new `id` and `citekey`.

A reference is:

```json
{
  "type": "article-journal",
  "creators": [{"role": "author", "name": {"family": "Hernandez", "given": "Maria"}}],
  "issued": {"year": 2021},
  "fields": {"title": "Bus reliability and daily stress", "container_title": "Journal of Transit Studies",
             "volume": "14", "issue": "2", "page": "33-51", "doi": "10.1000/xyz"}
}
```

- **`type`:** one of the ids `types.list` gives, for example `article-journal`, `book`, `chapter` or `report`.
- **`role`:** `author`, `editor`, `translator` and so on.
- **Organisations:** for a group author, `name` is `{"kind": "organization", "literal": "Transit Authority"}`.
- **`fields`:** plain strings, with names in snake_case (`container_title`, `publisher_place`, ...).

The record is marked `origin: "agent"` and is listed under Library › Machine-created until the
person reviews it. It can be cited at once. Check with `capture.check` first, so as not to add a
work twice.

### `cite.render` (scope `format`)

`{"request": {...}}` → `{"text", "html", "rtf", "markdown", "style", "style_title", "warnings", "provisional", ...}`.

`text` is plain text, and `html` keeps italics. The request takes these members:

| Member | Meaning |
|---|---|
| `mode` | `parenthetical`, `narrative`, `reference` (one reference-list entry for each id), `bibliography`, `quote_with_citation`, `narrative_quote`, `author_only` or `citekey`. |
| `items` | For in-text modes: `[{"ref_id", "locator_label": "page", "locator": "36", "prefix", "suffix", "suppress_author"}]`. Only `ref_id` is required. |
| `ref_ids` | For `reference` and `bibliography`: the records to list. |
| `style` | A style id from `styles.list`. Empty means `apa-7`, or the manuscript's style when `manuscript_id` is given. |
| `manuscript_id` | Optional. Renders in that manuscript's context (its style, and year suffixes such as 2024a). Without it, each call stands alone. |
| `quote` | The quoted words, for the two quotation modes. |

Examples:

```
{"method": "cite.render", "args": {"request": {"mode": "parenthetical", "items": [{"ref_id": "<id>", "locator_label": "page", "locator": "36"}], "style": "apa-7"}}}
-> {"result": {"text": "(Hernandez & Li, 2021, p. 36)", ...}}
{"method": "cite.render", "args": {"request": {"mode": "reference", "ref_ids": ["<id>"], "style": "apa-7"}}}
-> {"result": {"text": "Hernandez, M., & Li, W. (2021). Bus reliability and daily stress. Journal of Transit Studies, 14, 33–51.", ...}}
```

### `import.stage` (scope `import`)

`{"format": "ris" | "bibtex" | "csljson" | "xml" | "csv" | "", "text": "...", "name": "results.ris", "service": "", "project_id": ""}`
→ the staged batch: `{"id", "state": "staged", "file_name", ...}`.

An empty `format` is detected from the name and text. The batch waits in the program's window,
where the person commits or cancels it.

## Bringing in records from a research service's API

API connections to research services are not built in. A person who has their own API access to
a service can write a script that fetches records and sends them with `import.stage` (scope
`import`). The batch waits for review in the program's window; nothing is committed by the script.

Each provider sets its own terms, limits and charges for API use. The person running the script is
responsible for those costs; this program does not track, limit or pay for API use.

```
POST /v1/call
Authorization: Bearer prm_...
{"method": "import.stage", "args": {"format": "ris", "text": "TY  - JOUR\n...", "name": "api-results.ris", "service": "ai-search"}}
```
