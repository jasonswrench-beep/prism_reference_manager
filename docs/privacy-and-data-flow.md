# Privacy and data flow

Written for privacy officers and IT staff reviewing Prism Reference Manager.

## What is stored, and where

| Data | Location | Notes |
|---|---|---|
| Library (sources, projects, notes, annotations, history) | `library.db` in the library folder the person chooses (default `Documents\Prism Reference Libraries\My Library`) | SQLite database |
| Attached files | `files\` inside the library folder (managed) or where they already are (linked) | Originals are never modified |
| Text mirror | `mirror\` inside the library folder | Canonical JSON and Markdown, rebuildable |
| Backups | `backups\` inside the library folder, or a folder the person chooses | Verified zip files |
| Settings | `%APPDATA%\org.prism.referencemanager\settings.json` | Theme, recent libraries, backup schedule, approved local clients (token hashes only) |
| Web view cache | `%LOCALAPPDATA%\org.prism.referencemanager\` | Interface cache only |

Uninstalling removes the program folder and the two settings folders above. Libraries are never
removed by the uninstaller.

## What leaves the computer

Nothing, unless the person uses an optional online lookup or checks for updates. There is no telemetry, no account and
no background synchronization. Each outbound request is listed in Settings › Network (with a
switch per kind) and logged in Tools › Network activity with what was sent.

| Kind | Service | Sent |
|---|---|---|
| DOI lookup | doi.org | The DOI |
| DOI corroboration and notices | api.crossref.org | The DOI |
| Dataset and software DOIs | api.datacite.org | The DOI |
| PubMed and PMC | eutils.ncbi.nlm.nih.gov | The identifier |
| arXiv | export.arxiv.org | The identifier |
| ISBN | openlibrary.org | The ISBN |
| Web page metadata | The site the person entered | A request for that page |
| Free full text (when asked) | api.openalex.org | The DOI |
| Download of a free copy that was found | The repository or publisher hosting it | A request for that file |
| Find missing DOIs (when asked) | api.crossref.org | Title, first author's surname, year |
| Fetch missing abstracts (when asked) | api.crossref.org | The DOI |
| Check web addresses (when asked) | Each source's own site | A request for that address |
| Update check (not set up in this version; see `docs/updates.md`) | The update feed | The program's version and Windows architecture |

PDF contents, notes, annotations and quotations are never sent anywhere. Text recognition for
scanned PDFs uses the recognizer built into Windows on this computer; page images stay in memory
and are not saved or sent. The recognized text is stored in `library.db`, never in the PDF. A project can block all
outbound requests (Settings › Network and privacy). Administrators can turn network use off for
everyone with `policy.json` (`{"network_enabled": false}`).

## Local connections

The optional local API listens on 127.0.0.1 only, is off by default, and requires the person to
approve each program and its permissions. Tokens are stored as SHA-256 hashes. Requests from web
pages are refused.

## Diagnostics

A diagnostic bundle is created only on request, can be read in full before it is saved, and
excludes library content, titles, notes, quotations, document text and tokens. File paths are
redacted unless the person includes them.
