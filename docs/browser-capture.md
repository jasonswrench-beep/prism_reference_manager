# Browser capture extension

The extension in `extension/` saves the paper on the page you are reading, with its details and
PDF, into Prism Reference Manager on the same computer. Everything it sends is staged in the
program's import review; nothing joins the library until the person commits it there.

## Supported browsers

Chromium-based browsers, version 116 or later (Manifest V3 with module service workers). Other
browser families need their own qualification and are not supported yet.

## Installing

The extension ships inside the program's install folder (`extension\`). Until it is published
in a browser extension store:

1. In Prism Reference Manager, turn on Settings › Local connections › Allow local connections.
   The same section has a button that opens the extension folder.
2. In the browser, open the extensions page, switch on developer mode, choose **Load unpacked**
   and pick that folder.
3. Click the extension's button and choose **Connect**; choose **Allow** in the program.

## What it does

| Page | What is offered |
|---|---|
| An article page with citation metadata (`citation_*`, Dublin Core, PRISM, schema.org) | That paper, with its PDF when the page links one |
| A PDF open in the browser | That PDF; details come from identifiers in the file |
| A results or reference-list page | A checklist of papers found in embedded citation spans (COinS) and DOI, arXiv, PubMed and PMC links |
| Any other page | The page itself, as a web page with today's access date |

The popup shows whether the paper is already in the library (by DOI, then exact title), and lets
the person choose a project, a collection, labels and a note. The keyboard shortcut is
Alt+Shift+P.

## Permissions and privacy

- **On-click page access** (`activeTab`, `scripting`): the extension reads a page only when its
  button is clicked. It has no standing access to sites.
- **Local connection** (`http://127.0.0.1/*`): the only address it sends to.
- **PDF sites (asked each time)**: to fetch a PDF with the browser's session, the extension asks
  for access to that one site when Save is clicked. If declined, the record is saved without the
  PDF.
- **Storage**: the port and the connection token, kept in the browser's extension storage.

The program accepts the extension only after the person approves it, gives it the `read` and
`import` permissions (stage for review, never commit), and refuses any request carrying a web
page's origin. Revoking it in the program's settings takes effect at once.

## In the program

A capture appears as a notification with a **Review** button, and under Connections › Import
review and history. Committing the batch attaches the PDF (kept in the library's `inbox`
folder until then), applies the labels and collection, and adds the note to the chosen project.

## Tests

- `tools/extension-tests/article.html` and `results.html` run the page reader and record builder
  in a real browser page against sample markup (serve the repository root over HTTP).
- `tools/extension-tests/popup-preview.html` renders the popup with stand-ins for the browser
  APIs and the program's replies.
- The end-to-end check (pairing, permission scopes, PDF capture, commit, labels, note, inbox
  cleanup) runs against a debug build of the program over the real local connection.
