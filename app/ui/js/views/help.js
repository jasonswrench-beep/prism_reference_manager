// Offline help with search.

import { h, clear, btn } from "../dom.js";
import { registerView } from "../app.js";
import * as dialogs from "../dialogs.js";

const TOPICS = [
  ["Getting started", `
<p>A <strong>library</strong> is a folder on your computer holding every source, file, note and backup. A <strong>project</strong> is a workspace inside it: its own reading decisions, notes, outline and manuscripts, sharing sources with the rest of the library without copying them. A <strong>manuscript</strong> is the article, chapter or dissertation whose citations a project produces.</p>
<ol><li>Create a project (Ctrl+Shift+N).</li><li>Add sources: by identifier (Ctrl+I), by hand (Ctrl+N), from a file (Ctrl+O), or by importing PDFs.</li><li>Read and mark PDFs in the Reader.</li><li>Organize evidence in Notes.</li><li>Copy citations in Cite, with the manuscript chosen in the context bar.</li></ol>`],
  ["Copying citations", `
<p>Cite offers separate actions: <strong>Copy parenthetical</strong>, <strong>Copy narrative</strong>, <strong>Copy reference</strong>, <strong>Copy bibliography</strong>, <strong>Copy quote with citation</strong> and <strong>Copy as plain text</strong>. Each copy puts plain text, HTML and rich text on the clipboard together so the receiving program can keep italics and hanging indents. A program's "paste as plain text" removes formatting by design; if formatting is lost, use <em>Export</em> to save a DOCX, RTF or HTML file.</p>
<p>The manuscript's reference set decides year suffixes such as 2024a and when names must be spelled out. Without a manuscript, output is a <em>standalone preview</em> and may change once one is chosen.</p>
<p>A copy is recorded only after the clipboard write succeeds. The program never assumes you pasted it. When later edits change the output, the copy appears under <strong>Readiness › Copied citations</strong> with the old and new text. <em>Copy updated text</em> and <em>Mark as replaced</em> are separate: mark it replaced only after you change it in your document.</p>`],
  ["What the checks mean", `
<ul><li><strong>Source identity</strong>: whether the record describes the intended work. A DOI confirms which work it is, not what it shows.</li>
<li><strong>Metadata readiness</strong>: whether the information this source type needs is present, conflicting, or confirmed absent. A missing DOI alone is not a problem.</li>
<li><strong>Style assurance</strong>: how thoroughly the style was tested. <em>Verified profile</em> means every approved test case passed and was reviewed by a qualified person; <em>Limited testing</em> means the cases pass but review is pending or coverage is partial.</li>
<li><strong>Output context</strong>: whether the output was made from the current manuscript set.</li></ul>`],
  ["Reading and marking PDFs", `
<p>Choose a tool (or press its key: V select, H highlight, U underline, S strikeout, C comment, T text box, D draw, A area, E ellipse) and select text or drag on the page. Marks are stored in the library, never in the original PDF.</p>
<p>Each mark records the PDF page, the page label the file defines, and a <strong>citation page</strong> you confirm. PDF page 7 and printed page 121 are different values: set the page mapping once per file, then confirm each quotation's page before copying it with its citation.</p>
<p><strong>Export annotated PDF</strong> writes a copy with standard annotations that other readers can show, plus a sidecar file for project and link information. A <em>flattened copy</em> draws marks permanently into a display copy. <strong>Import marks from another reader</strong> previews additions and conflicts and never duplicates marks it already has.</p>
<p>When you replace a file with a new version, the old version is kept and marks are matched by their text. Marks that moved or could not be found are listed for review, never moved silently.</p>`],
  ["Notes and the outline", `
<p>Notes are typed: direct quotation, paraphrase, summary, my interpretation, question, task, evidence statement and thought. A quotation keeps the exact source text separately from your interpretation. Source-derived notes need a source and locator, or they are marked unresolved. New notes belong to the active project; a library-wide note is a deliberate choice shown on the note.</p>
<p>The outline groups notes by manuscript section. One note can be filed under several sections without being copied. Drag notes onto sections or use <em>File in section</em>; move sections with Alt+arrow keys.</p>`],
  ["Imports and duplicates", `
<p>Every import is staged for review: new sources, possible duplicates, records needing metadata review and included files are shown before anything is committed. Fields without a direct mapping are kept in each record's overflow area. Merges fill missing fields and record conflicting values for review; they never overwrite your edits. A whole import can be undone from Connections.</p>
<p>Duplicate review compares records field by field. Merging keeps the retired record's id as an alias, so links still work. <em>Not duplicates</em> and <em>Keep both</em> are remembered. A preprint and its published version can be linked as versions of one work instead of merged.</p>`],
  ["Search syntax", `
<p><code>reflection teaching</code> all words • <code>"reflective practice"</code> phrase • <code>"faculty support"~5</code> nearby words • <code>-survey</code> exclude • <code>(a OR b) c</code> grouping • <code>title: author: journal: doi: tag: abstract: note: annotation: fulltext:</code> fields • <code>year:2020..2024</code>, <code>rating:&gt;=3</code> ranges • <code>type:book label:Theory collection:Methods has:pdf has:conflict</code> filters • <code>re:/pattern/</code> regular expression over titles and abstracts.</p>
<p>Accents and capitals do not matter; results show the original spelling. PDFs are indexed for full-text search when opened, or all at once from Settings › Library.</p>`],
  ["Backups, moving and recovery", `
<p>A verified backup is written once a day by default. <strong>Restore</strong> always goes into an empty folder after checking checksums, counts, links and files; your current library is never overwritten. To move to a new computer, restore a backup there, then relink attachments if their folder changed (Tools › Check attached files › Relink all).</p>
<p>Never open the same library on two computers at once. The program refuses a second writer, but synchronizing folders can still conflict; keep the library in a local folder.</p>
<p><strong>Export complete library package</strong> writes everything as open JSON, Markdown and files with a checksum manifest, readable without this program.</p>`],
  ["Privacy and network", `
<p>No telemetry and no account. Online lookups are optional, listed in Settings › Network with what each sends, and logged in Network activity. Each project can forbid all outside requests. Local connections for other programs on this computer are off by default and require your approval per program.</p>`],
];

registerView("help", {
  async render(el) {
    const q = h("input", { type: "search", placeholder: "Search help", "aria-label": "Search help", "data-search": "1", style: { maxWidth: "420px" } });
    const body = h("div.help-doc");
    const draw = () => {
      const s = q.value.trim().toLowerCase();
      clear(body);
      for (const [t, html] of TOPICS) {
        if (s && !(t + html).toLowerCase().includes(s)) continue;
        body.append(h("section.card.pad", { style: { marginBottom: "12px" } }, h("h2", t), h("div", { html })));
      }
      if (!body.children.length) body.append(h("p.muted", "No help topics match."));
    };
    q.addEventListener("input", draw);
    el.append(h("h1.view-title", { tabindex: "-1" }, "Help"), h("p.view-sub", "Works offline."), h("div.row", { style: { marginBottom: "12px" } }, q, btn("Keyboard shortcuts", { onClick: () => dialogs.shortcuts() }), btn("Print shortcuts", { onClick: () => { dialogs.shortcuts(); setTimeout(() => window.print(), 300); } })), body);
    draw();
  },
});
