# Security policy

## Reporting a vulnerability

Please report security problems privately, not in a public issue. Once the repository is
published, use its private vulnerability reporting ("Report a vulnerability" under the
Security tab). Include the version (Settings › About), what you did, what happened, and, if
you can, a diagnostic bundle (Help › Diagnostic bundle). The bundle contains no library
content.

Reports are acknowledged within seven days. A fix, or a plan with a date, follows within 30
days for problems that put library data or the computer at risk. The release notes credit the
reporter unless they ask otherwise.

## Supported versions

Only the latest release receives security fixes.

## What the program exposes

- **Local API.** Off by default. When on, it listens only on `127.0.0.1`, refuses requests from
  web pages, and needs a token the person approves in the window. Tokens are stored only as
  hashes. Scopes limit each client; administrative methods are never available. Records that
  clients create are marked as machine-created. One call, `GET /v1/status`, needs no token: it
  gives only the program's name and version, the API version, and whether a library is open.
- **Browser capture extension.** Reads a page only when the person clicks it, and connects only
  to the local API on `127.0.0.1`. It asks for access to a PDF's site when a PDF is saved.
- **Network.** Every outbound request is optional, listed in Settings › Network with a switch for
  each kind, and recorded in the network log with what was sent. Projects can block all outbound
  requests, and administrators can turn network use off with `policy.json`.
- **Files.** Imported PDFs and other files are copied into the library and never modified.
  The reader runs pdf.js with code evaluation and XFA forms disabled, and PDF scripts are not
  run.
- **Updates.** Downloads are verified against an update-signing key built into the program
  before anything runs. Checks go to the project's release page only when the person asks,
  or once a day if they turn that on. Versions before 0.1.3 cannot update themselves.

## Scope

In scope: the desktop program, its installer and uninstaller, the local API, the browser
capture extension and the library file format. Out of scope: problems that need an attacker
who already controls the person's Windows account, and issues in third-party services the
program can contact.
