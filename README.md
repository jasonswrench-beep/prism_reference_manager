# Prism Reference Manager

A local-first desktop reference manager for Windows, part of the Prism research family.
It collects sources, reads and marks PDFs, organizes evidence by project, and produces
citations that can be checked before they leave the program.

- **Projects over one library.** A source is one canonical record; each project keeps its own
  reading decisions, notes, outline and manuscripts.
- **Deterministic citations.** A tested APA 7 composer renders parenthetical and narrative
  citations, references and bibliographies from structured metadata. Output carries four
  separate checks: source identity, metadata readiness, style assurance and output context.
- **Changed-copy review.** Every successful clipboard copy is recorded with its exact text and
  context. When a later edit or a new same-author work changes the output (for example
  2024 → 2024b), the copy is flagged with the old and current text.
- **Durable annotations.** Marks are stored in the library with the file-version hash, physical
  page, page label, confirmed citation locator, geometry and text anchors. Originals are never
  modified; annotated copies use standard PDF annotations plus a sidecar.
- **Recoverable by design.** Transactional store with a change event per edit, persistent
  undo, a deterministic text mirror, verified backups, and an open export package.
- **Private.** No account, no telemetry. Optional lookups are listed and logged.

## Repository layout

| Path | Contents |
|---|---|
| `crates/prm-core` | Domain core: model, store, rendering, formats, search, PDF, backup, services |
| `crates/prm-core/tests` | APA golden fixtures, workflow and robustness tests, scale benchmark |
| `app/src-tauri` | Desktop shell: window, dispatcher, clipboard, settings, local API, installer hooks |
| `app/ui` | Interface (HTML, CSS, JavaScript modules; PDF rendering library vendored at build) |
| `assets` | Approved logo and icon artwork, plus the simplified small-size icon |
| `docs` | Design decisions, data and privacy notes, file formats, local API |
| `tools` | Release build, icon assembly, third-party notices and SBOM generation |

## Building

Requirements: Rust (stable, MSVC toolchain), Node.js 20 or later, Python 3, WebView2 runtime.

```
cd app
npm install
npm run vendor
cd ..
cargo test -p prm-core
powershell -ExecutionPolicy Bypass -File tools\build-release.ps1
```

The release script runs the tests, regenerates `THIRD-PARTY.md` and `sbom.json` (stopping on an
unreviewed license), builds the per-user installer, and assembles a portable zip. Output goes to
`dist\`. Builds are unsigned until a signing certificate is approved.

## Running tests

```
cargo test -p prm-core                     # fixtures, workflows, robustness
cargo test -p prm-core --release -- --ignored --nocapture   # 100,000-reference benchmark
```

Debug builds of the desktop program accept `PRM_TEST_SCRIPT`, `PRM_TEST_LOG` and
`PRM_CONFIG_DIR` to run an end-to-end script inside the real window with isolated settings.
These hooks are compiled out of release builds.

## License

Apache License 2.0. See `LICENSE`, `NOTICE` and `THIRD-PARTY.md`.
