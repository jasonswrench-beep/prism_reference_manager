# Requests to the shared core, and what the other tools still need

Prism Reference Manager's development list has two items that other tools depend on:

- **Item 3.** Other tools cite through the local API (`docs/local-api.md`).
- **Item 4.** PrismQDA and PrismStatistics read the exchange file (`prism-family-exchange` 1.0, `docs/library-format.md`).

This file says where each of the other tools stands on those two items and what it still needs. It covers PrismQDA, PrismStatistics and OpenStage Guide. For each need it says who owes it: this program, the shared core (`openstage-core`), or the tool itself.

Checked 2026-11-16 against each tool's current code:

| Tool | Source | Version read |
|---|---|---|
| PrismQDA | its repository | `CORE-REQUESTS.md` R5, decision 195 |
| PrismStatistics | its repository | 0.2.2 (commit 5ca9f9a) |
| OpenStage Guide | its repository | `CORE-REQUESTS.md` G6 |
| Shared core | its repository | commit 27315b9 |

The shared core has started neither the exchange file nor an API client. Its notes and roadmap mention neither.

## Where things stand

| | Item 3: citing through the local API | Item 4: reading the exchange file |
|---|---|---|
| **PrismQDA** | Not built, and not on its list. | **Built** (decision 195, `crates/prismqda-import/src/exchange.rs`). Tested only on hand-made files. |
| **PrismStatistics** | Not built. Its specification asks for method citations and a reference list in exports (STAT-01, OUT-03, OUT-05, OUT-06). The program has none yet. | Not built, and not on its list. Its exchange work is the case table with PrismQDA. |
| **OpenStage Guide** | **Built** (its item 9, `src/prm-client.mjs`, `scripts/prm-link.mjs`). Tested only against a stand-in. | Not needed. |

### What was checked against this program's code

**Guide's API calls match this program's code:**
- Pairing: `POST /v1/pair`, then `GET /v1/pair/<id>`.
- `capture.check` takes `doi` and `title` and returns `found`, `id` and `title`.
- `refs.add` takes `{ref}` with `type`, `creators[].role` and `name.family`/`name.given`, `issued.year`, and `fields.*`.
- `cite.render` takes `{request: {mode: "reference", ref_ids}}` or `{mode: "parenthetical", items: [{ref_id}]}`, plus `style`. It returns `text`, `style` and `style_title`.
- Guide asks for the scopes `read`, `format`, `write` and `import`. The work needs only the first three.
- A record Guide adds is marked `origin: "agent"` and waits under Library › Machine-created until someone reviews it. Guide's `citeSource` can cite it before that review.

**PrismQDA's reader matches what `export_family_exchange` writes:**
- Manifest members, `sources[].id`/`citekey`/`csl`, and `excerpts[]`, including `exact_text_edited` and `annotation_id`.
- It flags `anchor_state: "unresolved"` and `origin: "ai"` and `"agent"`.
- This program also writes `anchor_state: "no-locator"` (the person said the source has no locator). It writes an empty `anchor_state` for kinds not taken from a source (thought, question, task). PrismQDA treats both as confirmed, which is right, but nothing written says so.

## What this program owes (to be done here)

### P1. A written specification of the exchange file

The format is described in five lines, and both readers worked from the code. The specification should cover:
- every member and its type
- the value sets:
  - `type`: quotation, paraphrase, summary, comment, question, task, evidence, thought, note
  - `anchor_state`: resolved, unresolved, no-locator, empty
  - `origin`: user, ai, agent, import
  - locator labels
- what is left out (trashed notes, and sources with no excerpt)
- the version rule (readers accept 1.x and refuse 2 or later)
- the file name (`prism-exchange.json` by default; readers go by `manifest.format`, not the name)

It answers PrismQDA's R5 questions 1 and 2. If the core takes the format (C1), this text moves there.

### P2. A sample file from a real build, and a test

PrismQDA has never seen a file this program made. It needs:
- a test in `prm-core` that builds a small project, writes the file and checks its shape
- the file it writes committed as `docs/examples/prism-exchange.json`

That file is the sample PrismQDA asked for. There is no test of `export_family_exchange` today.

### P3. Sources without excerpts

The file holds only sources that an excerpt names, so a project's sources with no excerpt never reach PrismQDA. A literature review coded in PrismQDA usually wants every source in the project. PrismQDA's reader already handles such a source and makes a document of its title, details and abstract.

Proposed: an option, on by default, to include every source in the project. This needs no version change, since 1.0 readers already accept it.

### P4. What "return links" means

The manifest note says receivers "return links as separate data". Nothing in this program reads such data, and no reader writes it.

Proposed answer for 1.0: nothing is returned. Receivers keep `sources[].id` and `excerpts[].id` so that a later version can name them. Remove the sentence or say this in the note. PrismQDA already keeps both ids.

### P5. A status call before pairing, and a way to find the port (Guide G6)

Today a tool cannot tell whether this program is running, which version it is, or whether a library is open, without a token. Guide probes by sending an unsigned call and reading the 401.

Proposed:
- `GET /v1/status` with no token, returning the API version, the program version and whether a library is open. It returns no library content.
- The port shown and copyable in Settings › Local connections. Alternatively, a small file in the program's data folder naming the port while the API is on.

The security limits stay as they are: 127.0.0.1 only, no web origin, and token hashing.

### P6. Request and answer shapes for every call (Guide G6)

`docs/local-api.md` lists the methods by scope but not their arguments or answers. Guide took them from `dispatch.rs`.

Proposed: one entry for each method a client may call, with its arguments, its answer and its errors. Start with the citing path: `capture.check`, `refs.add`, `refs.get`, `cite.render`, `styles.list`.

### P7. Guide against the real program

This needs a person, because pairing has to be approved in the window. It is added to `TESTING-TODO.md`.

## What the shared core is asked for

### C1. The exchange file as a core format (PrismQDA R5)

The format should be held to one text with a conformance set, as PSTAT-1 is:
- the specification from P1, moved to `specs/exchange`
- the sample from P2
- files a reader must accept or refuse

This program would test its writer against the set. PrismQDA would test its reader against it.

### C2. One client of the local API (Guide G6)

A shared client covering:
- pairing, with the token kept by the calling program and never by a page
- `call`
- the citing path: find by DOI or title, add if missing, render the entry and the in-text citation

Guide's `prm-client.mjs` is the starting point and would be deleted once it moves. PrismStatistics is Rust, so a Rust form is needed too if it cites (below).

## What each tool still needs

### PrismQDA

1. **For item 4:** the sample file and specification (P1, P2), then a test against them (C1). Until then its reader rests on hand-made files.
2. **For item 4:** with P3, sources without excerpts arrive. The reader already handles them, so no work is needed there.
3. **For item 3:** nothing on its list. If its reports or literature-review memos should carry formatted citations, it would use the shared client (C2) with `read` and `format`. It already keeps each document's library id and citation key from the exchange file, so it can name the work without a lookup.

### PrismStatistics

1. **For item 3:** its specification wants each method's citations and a methods/reference list in DOCX and HTML exports (STAT-01, OUT-06). Export checks should flag missing method citations (OUT-05). None of this is built.

   Two ways to supply the citations:
   - **Option a:** keep the method references in its own method registry and format them itself.
   - **Option b:** send them to this program for formatting through the API, using `capture.check`, `refs.add` and `cite.render`, with scopes `read`, `format` and `write`.

   Option b gives the person's chosen style and one library. It needs the Rust client (C2) and must still work when this program is not running. The reference list then falls back to the registry's own text.

   This is PrismStatistics's decision and is recorded here, not made.

2. **For item 4:** nothing. The exchange file holds sources and excerpts, not data, and PrismStatistics's spec (INT-01 to INT-04) links to PrismQDA through the case table, which is built. The joint displays of INT-04 could later show an excerpt's source as a citation; that would come through PrismQDA, not this file.

   Proposed: drop PrismStatistics from item 4 of this program's list, unless its author wants the excerpts for another use.

### OpenStage Guide

1. **For item 3:**
   - a run against the real program (P7)
   - the status call and port (P5)
   - the documented shapes (P6), so its client stops relying on this program's code
   - moving onto the shared client when the core has one (C2)

   One small point: it asks for the `import` scope and never uses it. Asking for `read`, `format` and `write` would show the person a shorter list at pairing.

2. **For item 4:** nothing.
