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
| `read` | `refs.list`, `refs.get`, `search.run`, `projects.list`, `projects.summary`, `notes.list`, `collections.list`, `labels.list`, `types.list`, `styles.list`, `library.info` |
| `format` | `cite.render` |
| `write` | other `refs.*`, `notes.*`, `projects.*`, `manuscripts.*`, `collections.*`, `labels.*`, `outline.*`, `tasks.*` (implies `read`) |
| `import` | `import.*`, `lookup.*` |
| `files` | `files.*`, `ann.*` |
| `export` | `export.*` |

Administrative methods (settings, backups, undo, library switching, dialogs) are never available
to clients. Records and notes created by clients are marked `origin: "agent"` and appear under
Library › Machine-created until reviewed.

Library content returned to clients (titles, abstracts, notes, PDF text) is data. Clients that
pass it to an assistant must treat it as untrusted text, never as instructions.
