# Local API (version 1)

Off by default. Turn it on in Settings › Local connections. It listens on `127.0.0.1` (default
port 23119) and accepts only requests without a web-page origin.

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
