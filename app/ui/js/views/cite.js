// Cite: citation workspace, manuscript readiness and changed copies, style checks.

import { call } from "../api.js";
import { h, clear, icon, iconBtn, btn, toast, modal, confirmDialog, select, field, showError, fmtDate, runsHtml, tabs } from "../dom.js";
import { S, registerView, go, manuscript, refresh } from "../app.js";
import { assuranceBadge, localeName, styleCatalog } from "../styles.js";
import * as dialogs from "../dialogs.js";
import { pickSource } from "./notes.js";

const C = { tab: "workspace", items: [], subtab: "copied", checkTab: "readiness", bibSet: "confirmed", font: "", expanded: null, selectedIssue: 0 };

async function labelsFor(ids) {
  if (!ids.length) return {};
  return call("refs.labels", { ids });
}

registerView("cite", {
  async render(el, params) {
    if (params.tab) C.tab = params.tab;
    if (params.refs?.length) { C.items = params.refs.map((id) => ({ ref_id: id, locator_label: "page", locator: "" })); params.refs = null; }
    if (params.ref) { C.items = [{ ref_id: params.ref, locator_label: "page", locator: "" }]; params.ref = null; }
    const m = manuscript();
    const head = h("div", h("h1.view-title", { tabindex: "-1" }, { workspace: "Citation workspace", readiness: "Manuscript readiness", checks: "Style checks" }[C.tab]),
      h("p.view-sub", { workspace: "Preview the exact text before copying", readiness: m ? m.title : "Choose a manuscript", checks: "Know what was checked and what still needs attention" }[C.tab]));
    el.append(h("div.row", { style: { alignItems: "flex-start" } }, h("div.grow", head), tabs([["workspace", "Workspace"], ["readiness", "Readiness"], ["checks", "Style checks"]], C.tab, (k) => go("cite", { tab: k }), { seg: true, label: "Cite sections" })));
    if (C.tab === "workspace") return workspace(el, m);
    if (!m) { el.append(h("div.empty", h("h3", "Choose a manuscript"), h("p", "Readiness and changed copies are tracked per manuscript."), btn("New manuscript", { kind: "primary", onClick: () => dialogs.newManuscript() }))); return; }
    if (C.tab === "readiness") return readiness(el, m);
    return checks(el, m);
  },
});

// ---- workspace ---------------------------------------------------------------------------------

async function workspace(el, m) {
  const left = h("div.card.pad", { style: { display: "flex", flexDirection: "column" } });
  const mid = h("div", { style: { display: "flex", flexDirection: "column", gap: "12px" } });
  const right = h("div", { style: { display: "flex", flexDirection: "column", gap: "12px" } });
  el.append(h("div", { style: { display: "grid", gridTemplateColumns: "310px minmax(0, 1fr) 320px", gap: "14px" } }, left, mid, right));
  const labels = await labelsFor(C.items.map((i) => i.ref_id));
  const req = (mode, extra = {}) => ({ manuscript_id: m?.id || "", items: C.items, mode, ...extra });

  // Sources and locators
  left.append(h("h2.section-title", "Sources in this citation"));
  const srcList = h("div", { role: "list", "aria-label": "Sources in this citation" });
  C.items.forEach((it, i) => {
    const locLabel = select([["page", "Page"], ["paragraph", "Paragraph"], ["chapter", "Chapter"], ["section", "Section"], ["figure", "Figure"], ["table", "Table"], ["line", "Line"], ["timestamp", "Time stamp"]], it.locator_label || "page", { "aria-label": "Locator type", on: { change: (e) => { it.locator_label = e.target.value; redraw(); } } });
    const loc = h("input", { value: it.locator || "", "aria-label": "Locator value", placeholder: "e.g. 121", on: { change: (e) => { it.locator = e.target.value; redraw(); } } });
    const prefix = h("input", { value: it.prefix || "", placeholder: "Prefix (e.g. see)", "aria-label": "Prefix", on: { change: (e) => { it.prefix = e.target.value; redraw(); } } });
    const suffix = h("input", { value: it.suffix || "", placeholder: "Suffix (e.g. for a review)", "aria-label": "Suffix", on: { change: (e) => { it.suffix = e.target.value; redraw(); } } });
    const sup = h("input", { type: "checkbox", checked: !!it.suppress_author, id: "sa" + i, on: { change: (e) => { it.suppress_author = e.target.checked; redraw(); } } });
    srcList.append(h("div.card", { role: "listitem", style: { padding: "10px", marginBottom: "8px", background: i === 0 ? "var(--primary-soft)" : "var(--surface)" } },
      h("div.row", icon("doc"), h("strong.grow", labels[it.ref_id] || it.ref_id), iconBtn("chevronUp", "Move earlier", () => { if (i > 0) { [C.items[i - 1], C.items[i]] = [C.items[i], C.items[i - 1]]; go("cite"); } }), iconBtn("x", "Remove from citation", () => { C.items.splice(i, 1); go("cite"); })),
      h("div.grid2", { style: { marginTop: "6px" } }, locLabel, loc),
      h("details", h("summary.small", "Prefix, suffix and author"), h("div", { style: { marginTop: "6px" } }, prefix, h("div", { style: { height: "6px" } }), suffix, h("label.row.small", { for: "sa" + i }, sup, "Leave out the author (year only)")))));
  });
  if (!C.items.length) srcList.append(h("p.muted", "Add one or more sources. Several sources become one grouped citation."));
  left.append(srcList, h("div.btn-row", { style: { marginTop: "auto" } }, btn("Add source", { icon: "plus", kind: "outline", onClick: async () => { const id = await pickSource(); if (id) { C.items.push({ ref_id: id, locator_label: "page", locator: "" }); go("cite"); } } })));

  const block = (title, mode, copyLabel, note) => {
    const box = h("div.output-box", { "aria-live": "polite" }, h("span.muted", "…"));
    const extra = h("div");
    const card = h("div.card.pad", h("h2.section-title.bar", title), h("div.row", { style: { alignItems: "flex-start" } }, h("div.grow", box, extra), btn(copyLabel, { kind: "primary", icon: "copy", disabled: !C.items.length, onClick: () => copy(mode) })), note ? h("p.small.muted", note) : null);
    return { card, box, extra };
  };
  const paren = block("Parenthetical", "parenthetical", "Copy parenthetical");
  const narr = block("Narrative", "narrative", "Copy narrative", "Narrative citations are composed for running text, not made by removing parentheses.");
  const ref = block("Reference", "reference", "Copy reference");
  mid.append(paren.card, narr.card, ref.card);
  const bibCard = h("div.card.pad", { style: { background: "var(--surface)" } });
  mid.append(bibCard);

  async function redraw() {
    if (!C.items.length) { [paren, narr, ref].forEach((b) => clear(b.box, h("span.muted", "Add a source to preview."))); drawRight(null); drawBib(); return; }
    try {
      const [p, n, r] = await Promise.all([call("cite.render", { request: req("parenthetical") }), call("cite.render", { request: req("narrative") }), call("cite.render", { request: req("reference", { ref_ids: C.items.map((i) => i.ref_id) }) })]);
      paren.box.innerHTML = runsHtml(p.runs);
      narr.box.innerHTML = runsHtml(n.runs);
      clear(narr.extra, n.locator_fragment ? h("p.small", "For a quotation, place ", h("strong", n.locator_fragment), " after the quoted words.") : null);
      ref.box.innerHTML = r.paragraphs.map((x) => `<p style="margin:0 0 4px;padding-left:.5in;text-indent:-.5in">${runsHtml(x.runs)}</p>`).join("");
      drawRight(p, r);
    } catch (e) { showError(e, mid); }
    drawBib();
  }
  async function drawBib() {
    clear(bibCard);
    const set = select([["confirmed", "Confirmed used in manuscript"], ["manuscript", "All sources in the manuscript"], ["project", "All project sources"], ["selected", "Sources in this citation"]], C.bibSet, { "aria-label": "Bibliography source set", style: { width: "auto" }, on: { change: (e) => { C.bibSet = e.target.value; drawBib(); } } });
    const fmt = select([["docx", "DOCX document (.docx)"], ["rtf", "Rich Text (.rtf)"], ["html", "HTML"], ["md", "Markdown"], ["txt", "Plain text"]], "docx", { "aria-label": "File format" });
    bibCard.append(h("div.row", h("h2.section-title.grow", { style: { margin: 0 } }, "Bibliography"), set));
    if (!m && C.bibSet !== "selected") { bibCard.append(h("p.muted", "Choose a manuscript to build its bibliography.")); return; }
    try {
      const b = await call("cite.render", { request: { manuscript_id: m?.id || "", mode: "bibliography", set: C.bibSet, ref_ids: C.items.map((i) => i.ref_id) } });
      bibCard.append(h("div.output-box.bib", { style: { maxHeight: "240px", overflow: "auto", marginTop: "8px" }, html: b.paragraphs.map((x) => `<p style="padding-left:.5in;text-indent:-.5in">${runsHtml(x.runs)}</p>`).join("") }),
        h("p.small.muted", `${b.source_count} sources • hanging indent and line spacing are real paragraph properties`),
        h("div.btn-row", btn("Copy bibliography", { kind: "primary", icon: "copy", onClick: () => copy("bibliography", { set: C.bibSet, ref_ids: C.items.map((i) => i.ref_id) }) }), fmt,
          btn("Export", { icon: "download", onClick: async () => { const r = await call("cite.export_bibliography", { request: { manuscript_id: m?.id || "", mode: "bibliography", set: C.bibSet, ref_ids: C.items.map((i) => i.ref_id) }, format: fmt.value, font: C.font }); if (r && !r.cancelled) toast("Saved " + r.path, "success"); } })));
    } catch (e) {
      bibCard.append(h("p.muted", C.bibSet === "confirmed" ? "No sources are confirmed as used yet. Mark copies as used in Readiness, or choose another set." : e.message));
    }
  }
  function drawRight(p) {
    clear(right);
    const checksCard = h("div.card.pad", h("h2.section-title", "Output checks"));
    if (p) {
      checksCard.append(h("ul.check-list", p.checks.map((c) => h("li", icon(c.state === "ok" ? "checkCircle" : c.state === "review" || c.state === "unsupported" ? "warn" : "info", 18, c.state === "ok" ? "Passed" : c.state), h("div", h("strong", c.label), h("div.small.muted", c.detail))))),
        h("div.small", h("strong", "Style: "), `${p.style_title} • ${localeName(p.locale)}`),
        h("div.small", h("strong", "Assurance: "), assuranceBadge(p.assurance), " ", h("a", { href: "#", on: { click: (e) => { e.preventDefault(); go("cite", { tab: "checks" }); C.checkTab = "library"; } } }, "View test coverage")),
        p.provisional ? h("div.notice.warn", { style: { marginTop: "8px" } }, icon("info"), h("div.small", p.context_label)) : h("div.small.muted", { style: { marginTop: "6px" } }, p.context_label));
      for (const w of p.warnings.filter((w) => w.level !== "info")) checksCard.append(h("div.small.status-review", icon("warn", 13), " " + w.message));
    } else checksCard.append(h("p.muted", "Checks appear when a source is added."));
    const fmtCard = h("div.card.pad", h("h3", "Format"), field("Font for exported files", select([["", "Receiving document's font"], ["Times New Roman", "Times New Roman"], ["Calibri", "Calibri"], ["Arial", "Arial"], ["Georgia", "Georgia"]], C.font, { on: { change: (e) => (C.font = e.target.value) } }), "Clipboard output uses the receiving document's body font and keeps italics."),
      h("a", { href: "#", on: { click: (e) => { e.preventDefault(); copy("parenthetical", {}, true); } } }, "Copy as plain text"), h("span.small.muted", " (no italics or indents)"),
      h("div.btn-row", { style: { marginTop: "8px" } }, btn("Copy citation key", { small: true, onClick: async () => { if (!C.items.length) return; const t = await call("refs.citekey_copy", { ids: C.items.map((i) => i.ref_id), syntax: "pandoc" }); toast("Copied " + t, "success"); } }),
        btn("LaTeX \\cite", { small: true, onClick: async () => { if (!C.items.length) return; const t = await call("refs.citekey_copy", { ids: C.items.map((i) => i.ref_id), syntax: "latex" }); toast("Copied " + t, "success"); } })));
    const copiedCard = h("div.card.pad", h("div.row", h("h2.section-title.grow", { style: { margin: 0 } }, "Copied citations"), h("span.badge.info", "…")));
    if (m) call("readiness", { manuscript_id: m.id }).then((rd) => {
      copiedCard.querySelector(".badge").textContent = String(rd.copied_items);
      copiedCard.append(rd.need_review ? h("div", { style: { margin: "8px 0" } }, h("a.status-review", { href: "#", on: { click: (e) => { e.preventDefault(); go("cite", { tab: "readiness" }); } } }, icon("warn", 15), ` ${rd.need_review} need review`)) : h("div.small.status-ok", { style: { margin: "8px 0" } }, icon("check", 14), " No copied text has changed"),
        h("p.small.muted", "Copied text does not update automatically in your document."),
        btn("Review changed citations", { kind: "outline", icon: "search", onClick: () => go("cite", { tab: "readiness" }) }));
    }).catch(() => {});
    else copiedCard.append(h("p.small.muted", "Choose a manuscript to track copies."));
    right.append(checksCard, fmtCard, copiedCard);
  }
  async function copy(mode, extra = {}, plain = false) {
    if (!C.items.length && mode !== "bibliography") return;
    if (!m && !(await confirmDialog("No manuscript chosen", "This is a standalone preview: without a manuscript, year suffixes such as 2024a and name disambiguation are provisional. Copy anyway?", { ok: "Copy anyway" }))) return;
    try {
      const r = await call("cite.copy", { request: { ...req(mode, extra), ref_ids: extra.ref_ids || C.items.map((i) => i.ref_id) }, plain });
      toast(`Copied${plain ? " as plain text" : ""}: ${r.output.text.slice(0, 160)}`, "success");
      if (!plain && mode === "bibliography") toast("If your document drops the hanging indent, use Export instead.", "info");
      drawRight((await call("cite.render", { request: req("parenthetical") }).catch(() => null)));
    } catch (e) { showError(e); }
  }
  await redraw();
}

// ---- readiness ---------------------------------------------------------------------------------

async function readiness(el, m) {
  let rd;
  try { rd = await call("readiness", { manuscript_id: m.id }); } catch (e) { return showError(e, el); }
  const main = h("div", { style: { display: "flex", flexDirection: "column", gap: "12px" } });
  const side = h("div.card.pad");
  el.append(h("div", { style: { display: "grid", gridTemplateColumns: "minmax(0, 1fr) 340px", gap: "14px" } }, main, side));
  main.append(tabs([["overview", "Overview"], ["copied", "Copied citations"], ["bibliography", "Bibliography"]], C.subtab, (k) => { C.subtab = k; go("cite"); }, { seg: true, label: "Readiness sections" }));
  main.append(h("div.card", h("div.row", { style: { padding: "12px 18px", justifyContent: "space-around", gap: "20px" } },
    h("span.row", icon("layers", 22), h("strong", rd.project_sources), " project sources"), h("span", "|"),
    h("span.row", icon("quote", 22), h("strong", rd.copied_items), " copied items"), h("span", "|"),
    h("span.row", icon("check", 22), h("strong", rd.confirmed), " confirmed used"), h("span", "|"),
    rd.need_review ? h("span.badge.attention", { style: { fontSize: "16px" } }, icon("warn", 18), `${rd.need_review} need review`) : h("span.badge.success", icon("check", 16), "Nothing changed"))));

  if (C.subtab === "overview") {
    main.append(h("div.card.pad", h("h2.section-title.bar", "Manuscript context"),
      h("p", `${rd.manuscript_sources} sources are in this manuscript's reference set. That set controls year suffixes (2024a, 2024b) and name disambiguation, and differs from the ${rd.project_sources} sources in the project.`),
      h("p", `Style: ${S.styles.find((s) => s.id === m.style)?.title || m.style} • citation language ${localeName(m.locale)} • context revision ${m.context_revision}`),
      h("h3", "Saved snapshots"), rd.snapshots.length ? h("table.table", h("tbody", rd.snapshots.map((s) => h("tr", h("td", s.label), h("td.small", fmtDate(s.created)), h("td", `${s.sources} sources`), h("td", btn("Verify", { small: true, onClick: async () => toast((await call("snapshot.verify", { id: s.id })) ? "Snapshot is intact." : "The snapshot does not match its checksum.", "info") })))))) : h("p.muted", "No snapshots yet.")),
      await manuscriptSources(m));
  }
  if (C.subtab === "copied") {
    const changed = rd.changed;
    main.append(h("div.card", h("div.card-head", h("h2.section-title.bar", { style: { margin: 0 } }, "Changed since copying")),
      changed.length ? h("table.table", h("thead", h("tr", h("th", ""), h("th", "Source"), h("th", "Why it changed"), h("th", "Status"))),
        h("tbody", changed.flatMap((c, i) => {
          const open = C.expanded === c.copy.id || (C.expanded === null && i === 0);
          const row = h("tr", { "aria-selected": String(open), tabindex: "0", on: { click: () => { C.expanded = open ? "" : c.copy.id; go("cite"); }, keydown: (e) => e.key === "Enter" && (C.expanded = open ? "" : c.copy.id, go("cite")) } },
            h("td", icon(open ? "chevronDown" : "chevronRight")), h("td", h("strong", c.sources.join("; "))), h("td", c.copy.change_reason), h("td", h("span.badge.attention", icon("warn", 14), "Review")));
          if (!open) return [row];
          const detail = h("tr", h("td", { colspan: 4 }, h("div", { style: { padding: "10px" } },
            h("div.diff", h("div.side", h("h4", "Previously copied"), h("div", { html: c.copy.payload_html ? c.copy.payload_html.replace(/<p[^>]*>/g, "<p>") : c.copy.payload_text })), h("div.arrow", icon("arrowRight", 24)), h("div.side.after", h("h4", "Current output"), h("div", { html: c.copy.current_html ? c.copy.current_html.replace(/<p[^>]*>/g, "<p>") : c.copy.current_text }))),
            h("div.notice", { style: { margin: "10px 0" } }, icon("info"), "Check every affected citation and its matching reference in your document."),
            h("div.btn-row", { style: { justifyContent: "center" } },
              btn("Copy updated text", { kind: "primary", icon: "copy", onClick: async () => { await call("cite.copy_text", { text: c.copy.current_text, html: c.copy.current_html }); toast("Updated text copied. Mark it as replaced once it is in your document.", "success"); } }),
              btn("Mark as replaced", { kind: "outline", icon: "check", onClick: async () => { await call("copies.set_status", { id: c.copy.id, status: "replaced" }); toast("Marked as replaced", "success"); C.expanded = null; go("cite"); } })),
            h("p.small.muted", { style: { textAlign: "center" } }, "Copying does not confirm that text was pasted or replaced."))));
          return [row, detail];
        }))) : h("div.empty", h("h3", "No copied citations have changed"), h("p", "When a source, the style or this manuscript's reference set changes, affected copies appear here with the old and new text."))),
      h("div.card", h("div.card-head", h("h3", `All copies (${rd.copies.length})`)), h("table.table", h("thead", h("tr", h("th", "Copied text"), h("th", "Mode"), h("th", "When"), h("th", "Status"), h("th", ""))),
        h("tbody", rd.copies.slice(0, 200).map((c) => h("tr", h("td", { html: c.payload_html ? c.payload_html.replace(/<p[^>]*>/g, "").replace(/<\/p>/g, " ") : c.payload_text }), h("td.small", c.mode.replace(/_/g, " ")), h("td.small", fmtDate(c.created)),
          h("td", c.imported_unknown ? h("span.badge.neutral", "History unknown") : c.changed ? h("span.badge.attention", "Changed") : h("span.badge." + (c.status === "copied" ? "info" : c.status === "discarded" ? "neutral" : "success"), { copied: "Copied (not confirmed)", used: "Confirmed used", replaced: "Replaced", discarded: "Discarded" }[c.status])),
          h("td.nowrap", c.status === "copied" ? btn("Mark used", { small: true, onClick: async () => { await call("copies.set_status", { id: c.id, status: "used" }); go("cite"); } }) : null, c.status !== "discarded" ? btn("Discard", { small: true, kind: "ghost", onClick: async () => { await call("copies.set_status", { id: c.id, status: "discarded" }); go("cite"); } }) : null)))))));
  }
  if (C.subtab === "bibliography") {
    const fmt = select([["docx", "DOCX document (.docx)"], ["rtf", "Rich Text"], ["html", "HTML"], ["md", "Markdown"], ["txt", "Plain text"]], "docx");
    try {
      const b = await call("cite.render", { request: { manuscript_id: m.id, mode: "bibliography", set: "confirmed" } });
      main.append(h("div.card.pad", h("h2.section-title.bar", `Submission bibliography (${b.source_count} confirmed used)`),
        h("div.output-box.bib", { html: b.paragraphs.map((x) => `<p style="padding-left:.5in;text-indent:-.5in;line-height:2">${runsHtml(x.runs)}</p>`).join("") }),
        b.warnings.filter((w) => w.level !== "info").map((w) => h("div.small.status-review", icon("warn", 13), " " + w.message)),
        h("div.btn-row", { style: { marginTop: "10px" } }, btn("Copy bibliography", { kind: "primary", icon: "copy", onClick: async () => { await call("cite.copy", { request: { manuscript_id: m.id, mode: "bibliography", set: "confirmed" } }); toast("Copied", "success"); } }), fmt,
          btn("Export", { icon: "download", onClick: async () => { const r = await call("cite.export_bibliography", { request: { manuscript_id: m.id, mode: "bibliography", set: "confirmed" }, format: fmt.value }); if (r && !r.cancelled) toast("Saved " + r.path, "success"); } }))));
    } catch (e) {
      main.append(h("div.card.pad", h("h2.section-title.bar", "Submission bibliography"), h("p.muted", "No sources are confirmed as used yet. The default submission bibliography uses only confirmed sources; confirm them in the Overview, or mark copies as used.")));
    }
  }
  // Before you submit
  const steps = [
    ["Review metadata", "Check titles, authors, years and other details for accuracy.", () => go("cite", { tab: "checks" })],
    ["Confirm cited sources", "Make sure every in-text citation matches a source in this manuscript.", () => { C.subtab = "overview"; go("cite"); }],
    ["Review changed copies", rd.need_review ? `Check ${rd.need_review} items that need review (year suffix, spelling, numbering).` : "No copied text has changed.", () => { C.subtab = "copied"; go("cite"); }],
    ["Export final bibliography", "Create the formatted reference list.", () => { C.subtab = "bibliography"; go("cite"); }],
    ["Save manuscript snapshot", "Freeze the citation data and style used for this submission.", null],
  ];
  let checked = {};
  try { checked = JSON.parse(localStorage.getItem("submit:" + m.id) || "{}"); } catch (_) {}
  side.append(h("h2.section-title.bar", "Before you submit"), h("ul.check-list", steps.map(([t, d, fn], i) => {
    const cb = h("input", { type: "checkbox", id: "st" + i, checked: !!checked[i], on: { change: (e) => { checked[i] = e.target.checked; try { localStorage.setItem("submit:" + m.id, JSON.stringify(checked)); } catch (_) {} } } });
    return h("li", cb, h("div", h("label", { for: "st" + i }, h("strong", t)), h("div.small.muted", d), fn ? h("a.small", { href: "#", on: { click: (e) => { e.preventDefault(); fn(); } } }, "Open") : null));
  })),
    h("div", { style: { display: "flex", flexDirection: "column", gap: "8px", marginTop: "14px" } },
      btn("Export bibliography", { kind: "primary", icon: "download", onClick: async () => { const r = await call("cite.export_bibliography", { request: { manuscript_id: m.id, mode: "bibliography", set: "confirmed" }, format: "docx" }).catch((e) => showError(e)); if (r && !r.cancelled) toast("Saved " + r.path, "success"); } }),
      btn("Save snapshot", { kind: "outline", icon: "save", onClick: async () => { const l = await import("../dom.js").then((d) => d.promptDialog("Save snapshot", "Label", `Submitted ${new Date().toLocaleDateString()}`)); if (l === null) return; await call("snapshot.save", { manuscript_id: m.id, label: l }); toast("Snapshot saved. It is a citation-context package, not a copy of your document.", "success"); go("cite"); } })));
}

async function manuscriptSources(m) {
  const ms = await call("manuscripts.sources", { id: m.id });
  const ids = ms.map((x) => x.ref_id);
  const labels = await labelsFor(ids);
  const card = h("div.card", h("div.card-head", h("h3", "Sources in this manuscript"), btn("Add from project…", { small: true, onClick: async () => { const id = await pickSource(); if (id) { await call("manuscripts.set_use", { id: m.id, ref_ids: [id], state: "candidate" }); go("cite"); } } })));
  const STATES = [["candidate", "Candidate"], ["copied", "Copied (not confirmed)"], ["confirmed", "Confirmed used"], ["excluded", "Not in manuscript"]];
  card.append(ms.length ? h("table.table", h("tbody", ms.sort((a, b) => (labels[a.ref_id] || "").localeCompare(labels[b.ref_id] || "")).map((x) => h("tr", h("td", labels[x.ref_id] || x.ref_id),
    h("td", select(STATES, x.state, { "aria-label": "Use state for " + (labels[x.ref_id] || ""), on: { change: async (e) => { await call("manuscripts.set_use", { id: m.id, ref_ids: [x.ref_id], state: e.target.value }); go("cite"); } } })))))) : h("p.muted", { style: { padding: "12px" } }, "Sources join the manuscript when you copy a citation for it, or add them here."));
  return card;
}

// ---- style checks --------------------------------------------------------------------------------

async function checks(el, m) {
  el.append(tabs([["readiness", "Readiness"], ["library", "Style library"], ["preview", "Preview"], ["custom", "Custom style"]], C.checkTab, (k) => { C.checkTab = k; go("cite"); }, { label: "Style checks" }));
  const body = h("div", { style: { marginTop: "14px" } });
  el.append(body);
  const style = S.styles.find((s) => s.id === m.style) || S.styles[0];
  const cov = style.coverage;
  if (C.checkTab === "readiness") {
    const rd = await call("readiness", { manuscript_id: m.id });
    const issues = rd.issues;
    const left = h("div.card.pad", h("h2.section-title.bar", style.title), h("div.row", icon("globe"), style.id === "apa-7" ? "English (US)" : localeName(m.locale)), h("hr"),
      h("div.row", icon(cov.assurance === "verified" ? "checkCircle" : "info"), h("strong", "Assurance: "), assuranceBadge(cov.assurance)),
      h("p.small.muted", cov.assurance === "verified" ? "Coverage applies to the tested source types and cases." : cov.cases ? `${cov.passed} of ${cov.cases} golden cases pass; ${cov.reviewed} have been reviewed by a qualified person. Until all are reviewed the profile is shown as Limited testing.` : cov.note),
      btn("Change style or language…", { small: true, onClick: () => styleCatalog() }),
      h("a", { href: "#", on: { click: (e) => { e.preventDefault(); C.checkTab = "library"; go("cite"); } } }, "View test coverage"));
    const sel = issues[C.selectedIssue] || issues[0];
    const table = h("div.card", issues.length ? h("table.table", h("thead", h("tr", h("th", ""), h("th", "Source"), h("th", "Check"), h("th", "Suggested action"))),
      h("tbody", issues.map((i, k) => h("tr", { "aria-selected": String(sel === i), tabindex: "0", on: { click: () => { C.selectedIssue = k; go("cite"); } } }, h("td", icon("warn", 18, "Needs review")), h("td", h("strong", i.source)), h("td", i.message),
        h("td", btn(i.action, { small: true, kind: "outline", onClick: (e) => { e.stopPropagation(); act(i); } })))))) : h("div.empty", h("h3", "No issues found"), h("p", "The checks look for missing or conflicting information, likely mistakes and changed copies. A missing DOI alone is not an issue.")));
    const detail = sel ? h("div.card.pad", { style: { marginTop: "12px" } }, h("h2.section-title.bar", sel.message),
      sel.before || sel.after ? h("div.diff", h("div.side", h("h4", "Before"), h("div", sel.before)), h("div.arrow", icon("arrowRight", 24)), h("div.side.after", h("h4", "After"), h("div", sel.after))) : null,
      sel.code === "article-number-as-page" ? h("p", "Keep article numbers distinct from page ranges.") : null,
      h("div.btn-row", { style: { marginTop: "10px" } },
        ["article-number-as-page", "possible-organization", "all-caps-name"].includes(sel.code) ? btn("Apply reviewed change", { kind: "primary", icon: "check", onClick: async () => { await call("style.apply", { ref_id: sel.ref_id, code: sel.code }); toast("Change applied with provenance; undo is available.", "success"); go("cite"); } }) : null,
        btn(["article-number-as-page", "possible-organization", "all-caps-name"].includes(sel.code) ? "Keep current value" : "Open source", { onClick: () => (["article-number-as-page", "possible-organization", "all-caps-name"].includes(sel.code) ? toast("Kept as it is.", "info") : go("library", { select: sel.ref_id })) }))) : null;
    const right = h("div.card.pad", h("h2.section-title.bar", "Check details"),
      h("ul.check-list",
        h("li", icon("doc"), h("div", h("strong", "Metadata checks"), h("div.small.muted", "Looks for inconsistencies, missing fields and format issues."))),
        h("li", icon("beaker"), h("div", h("strong", "Style test coverage"), h("div.small.muted", style.id === "apa-7" ? "Checks core APA 7 formatting rules for references and in-text citations in this project." : "This style has no test cases in this program; the metadata checks below still apply."))),
        h("li", icon("layers"), h("div", h("strong", "Manuscript context"), h("div.small.muted", `Uses your project and manuscript settings (${style.title}) to guide relevant checks.`))),
        h("li", icon("info"), h("div.small", "Missing DOI does not automatically mean an incomplete reference."))),
      h("p.small.muted", { style: { fontStyle: "italic" } }, "Custom edits need their own review."));
    body.append(h("div", { style: { display: "grid", gridTemplateColumns: "260px minmax(0, 1fr) 320px", gap: "14px" } }, left, h("div", table, detail), right));
    async function act(i) {
      if (i.code === "copy-changed") { C.tab = "readiness"; C.subtab = "copied"; go("cite", { tab: "readiness" }); }
      else if (i.code === "conflict") go("library", { select: i.ref_id });
      else { C.selectedIssue = issues.indexOf(i); go("cite"); }
    }
  }
  if (C.checkTab === "library") {
    body.append(h("div.card.pad", h("div.card-head", h("h2.section-title.bar", { style: { margin: 0 } }, `Installed styles (${S.styles.length})`), btn("Browse, preview or import…", { small: true, kind: "primary", onClick: () => styleCatalog() })),
      h("table.table", h("thead", h("tr", h("th", "Style"), h("th", "Source"), h("th", "Engine"), h("th", "Licence"), h("th", "Assurance"))), h("tbody", S.styles.map((s) => h("tr", h("td", s.title, s.class === "note" ? h("span.small.muted", " (notes)") : null), h("td.small", s.source === "builtin" ? "Built in" : s.source === "imported" ? "Imported" : "Bundled"), h("td.small", s.engine === "builtin" ? "Tested composer" : "CSL " + s.version), h("td.small", s.license || ""), h("td", assuranceBadge(s.assurance)))))),
      h("p.small.muted", "Bundled CSL styles are Untested until test cases for them are run and reviewed. An imported style is its own version and never inherits verification from a parent style.")),
      h("div.card.pad", { style: { marginTop: "12px" } }, h("h2.section-title.bar", `Test coverage: ${style.title}`),
        h("p", `${cov.cases} golden cases • ${cov.passed} pass • ${cov.reviewed} reviewed by a qualified person • processor ${cov.processor} ${cov.processor_version} • style ${cov.style_version}`),
        h("div.notice" + (cov.assurance === "verified" ? "" : ".warn"), icon("info"), cov.note),
        h("h3", "By category"), h("table.table", h("tbody", cov.categories.map((c) => h("tr", h("td", c.category), h("td", `${c.passed} / ${c.cases} pass`))))),
        h("details", h("summary", "Every case"), h("table.table", h("thead", h("tr", h("th", "Case"), h("th", "Expected"), h("th", "Result"), h("th", "Reviewed"))), h("tbody", cov.results.map((r) => h("tr", h("td.small", h("strong", r.id), h("div.muted", r.reason)), h("td.small", r.expected), h("td", r.passed ? h("span.status-ok", "Pass") : h("span.status-review", "Fail: " + r.actual)), h("td", r.reviewed ? "Yes" : h("span.muted", "Pending")))))))));
  }
  if (C.checkTab === "preview") {
    const ids = (await call("manuscripts.sources", { id: m.id })).filter((x) => x.state !== "excluded").map((x) => x.ref_id);
    if (!ids.length) { body.append(h("div.empty", "Add sources to the manuscript to preview its output.")); return; }
    const b = await call("cite.render", { request: { manuscript_id: m.id, mode: "bibliography", set: "manuscript" } });
    body.append(h("div.card.pad", h("h2.section-title.bar", "Every manuscript source in this style"),
      h("div.output-box.bib", { html: b.paragraphs.map((x) => `<p style="padding-left:.5in;text-indent:-.5in">${runsHtml(x.runs)}</p>`).join("") }),
      b.warnings.map((w) => h("div.small" + (w.level === "info" ? ".muted" : ".status-review"), icon(w.level === "info" ? "info" : "warn", 13), " " + w.message))));
  }
  if (C.checkTab === "custom") {
    body.append(h("div.card.pad", h("h2.section-title.bar", "Custom styles"),
      h("p", "Editing a style creates a new, separately versioned style with the status “Custom, not reviewed”. It never overwrites the installed profile and never inherits its verification."),
      h("div.notice.warn", icon("info"), "The visual style editor is planned for a later version. You can import any CSL style file now; it is marked Imported, not reviewed."),
      btn("Import CSL style file…", { onClick: () => styleCatalog() })));
  }
}
