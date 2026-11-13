// Research connections: honest transport labels, file exchange imports, import history with
// batch undo, and the network log. No service is shown as "connected" when the only operation
// is importing a downloaded file or opening a web page.

import { call } from "../api.js";
import { h, clear, icon, btn, toast, modal, select, field, fmtDate, showError } from "../dom.js";
import { S, registerView, go } from "../app.js";
import * as dialogs from "../dialogs.js";

// Service and program names are interoperability labels: they say which export a file comes
// from or which page opens. The transport column states the actual connection honestly.
const SERVICES = [
  { key: "mapping-service", name: "ResearchRabbit", transport: "File exchange", formats: "BibTeX, RIS and CSV", in: true, out: "Seed BibTeX export", note: "Export your collection from ResearchRabbit as BibTeX, RIS or CSV, then import it here. To seed ResearchRabbit, export selected sources as BibTeX and upload that file there." },
  { key: "ai-search", name: "Consensus", transport: "File exchange", formats: "RIS and CSV", in: true, out: null, note: "Export results from Consensus as RIS or CSV, then import them here with duplicate and metadata review." },
  { key: "citation-context", name: "scite", transport: "Browser handoff", formats: "Report page by DOI", in: false, out: null, handoff: true, note: "Opens the scite report for a source's DOI in your web browser. scite's citation classifications are its own dated observations; they are not stored as facts about the source." },
  { key: "reading-assistant", name: "SciSpace", transport: "File exchange", formats: "BibTeX and RIS", in: true, out: "BibTeX or RIS export", note: "Import BibTeX or RIS exported from SciSpace; send sources to it by exporting BibTeX or RIS." },
];

const TRADITIONAL = [
  ["Zotero", "Export as RIS, BibTeX or CSL JSON", "zotero"],
  ["EndNote", "Export as XML (or RIS)", "endnote"],
  ["Mendeley", "Export as RIS or BibTeX", "mendeley"],
  ["Another reference manager", "RIS, BibTeX, CSL JSON, XML or CSV", "desktop-manager"],
];

registerView("connections", {
  async render(el) {
    el.append(h("h1.view-title", { tabindex: "-1" }, "Research connections"), h("p.view-sub", "Bring discoveries into your project with their sources intact"));
    const left = h("div", { style: { display: "flex", flexDirection: "column", gap: "14px" } });
    const right = h("div", { style: { display: "flex", flexDirection: "column", gap: "14px" } });
    el.append(h("div", { style: { display: "grid", gridTemplateColumns: "minmax(0, 1fr) 360px", gap: "16px" } }, left, right));
    left.append(h("div.card", h("table.table", h("thead", h("tr", h("th", "Service"), h("th", "Available workflow"), h("th", "Action"))),
      h("tbody", SERVICES.map((s) => h("tr", h("td", h("strong", s.name), h("div.small.muted", s.note), h("div.small.muted", "API: not built in. See API access below.")), h("td.nowrap", `${s.transport} • ${s.formats}`),
        h("td.nowrap", s.in ? btn("Import files", { small: true, icon: "download", onClick: () => dialogs.importFile(s.key) }) : null,
          s.out ? btn("Export for it", { small: true, icon: "upload", onClick: () => dialogs.exportRefs([]) }) : null,
          s.handoff ? btn("Open for a source…", { small: true, icon: "external", onClick: () => handoff() }) : null,
          s.handoff ? btn("Change address", { small: true, kind: "ghost", onClick: () => changeHandoffAddress() }) : null)))))),
      h("p.small", "File exchange works without an account and never shows as a live connection."));

    // API access: allowed through the local API, never built in, costs belong to the person.
    left.append(h("div.card.pad", h("h2.section-title", "API access"),
      h("p", "Prism Reference Manager does not include API connections to these services. If you have API access to one of them, you can use your own script or tool to fetch records and send them here through the local API. They arrive as an import batch and wait for your review, like any other import."),
      h("p", h("strong", "Costs are yours to understand. "), "API access is offered (or not) by each provider under its own terms, and is often billed by request or by plan. Check the provider's current pricing, usage limits and terms before you use it. Prism Reference Manager does not track, limit or pay for API use, and cannot see what a provider charges."),
      h("p.small.muted", "The local API is off by default and only accepts programs on this computer that you approve. Give such a script the import scope only."),
      h("div.btn-row", btn("Local API settings", { small: true, onClick: () => go("settings", { tab: "api" }) }))));

    // Import history
    const batches = await call("import.batches");
    left.append(h("div.card", h("div.card-head", h("h2.section-title", { style: { margin: 0 } }, "Import review and history"), btn("Import a file", { small: true, kind: "primary", icon: "download", onClick: () => dialogs.importFile() })),
      batches.length ? h("table.table", h("thead", h("tr", h("th", "File"), h("th", "From"), h("th", "Records"), h("th", "State"), h("th", "When"), h("th", ""))),
        h("tbody", batches.slice(0, 50).map((b) => h("tr", h("td", h("span.row", icon("doc", 16), b.file_name)), h("td.small", serviceLabel(b.service)), h("td", String(b.records.length)),
          h("td", h("span.badge." + ({ committed: "success", staged: "info", undone: "neutral", cancelled: "neutral", partial: "attention" }[b.state] || "neutral"), { committed: "Imported", staged: "Waiting for review", undone: "Undone", cancelled: "Cancelled (nothing committed)", partial: "Partly undone" }[b.state] || b.state)),
          h("td.small", fmtDate(b.committed || b.created)),
          h("td.nowrap", b.state === "staged" ? btn("Review records", { small: true, kind: "primary", onClick: async () => dialogs.importReview(await call("import.get", { id: b.id })) }) : null,
            b.state === "committed" || b.state === "partial" ? btn("Undo import", { small: true, onClick: () => dialogs.undoImport(b.id) }) : null))))) : h("div.empty", "Nothing imported yet.")));

    right.append(h("div.card.pad", h("h2.section-title", "Traditional tools"),
      h("div", TRADITIONAL.map(([label, how, key]) => h("button.proj-row", { type: "button", style: { gridTemplateColumns: "28px 1fr", padding: "10px 12px" }, "aria-label": `Import from ${label}: ${how}`, on: { click: () => dialogs.importFile(key) } }, icon("doc"), h("span", h("div", { style: { fontWeight: 600 } }, label), h("div.small.muted", how))))),
      btn("Import exported library", { kind: "primary", icon: "download", onClick: () => dialogs.importFile("desktop-manager") }),
      h("p.small.muted", "Start with a dry run: the migration scan shows what an export file carries and what it cannot (annotations inside another program's database, smart groups, files without the companion folder). Files and annotations need their own migration checks.")),
      h("div.card.pad", h("h3", "Network"), h("p.small.muted", "Every request is listed with what it sent. Nothing runs in the background."), h("div.btn-row", btn("Network activity", { small: true, onClick: () => dialogs.network() }), btn("Network settings", { small: true, onClick: () => go("settings", { tab: "network" }) }))),
      h("div.card.pad", h("h3", "Other Prism research tools"), h("p.small", "Export source-linked notes and evidence as a versioned exchange file from Notes. Direct sending requires a compatible receiving version and is planned for later."), btn("Go to Notes", { small: true, onClick: () => go("notes") })));
  },
});

// The scite report page for a DOI. Changeable under Connections if the service moves it.
const REPORT_ADDRESS = "https://scite.ai/reports/{doi}";

async function changeHandoffAddress() {
  let cur = REPORT_ADDRESS;
  try { cur = localStorage.getItem("handoff:citation-context") || REPORT_ADDRESS; } catch (_) {}
  const p = await import("../dom.js").then((m) => m.promptDialog("scite report address", "Web address for a DOI. Write {doi} where the DOI goes.", cur, { help: "Only needed if scite changes its address. Clear the box to restore the built-in address.", ok: "Save" }));
  if (p === null) return;
  if (p.trim() && !p.includes("{doi}")) return toast("Include {doi} in the address.", "error");
  try { if (p.trim() && p.trim() !== REPORT_ADDRESS) localStorage.setItem("handoff:citation-context", p.trim()); else localStorage.removeItem("handoff:citation-context"); } catch (_) {}
  toast("Saved", "success");
}

function serviceLabel(k) {
  return ({ file: "File", "mapping-service": "ResearchRabbit", "ai-search": "Consensus", "reading-assistant": "SciSpace", zotero: "Zotero", endnote: "EndNote", mendeley: "Mendeley", "desktop-manager": "Reference manager export", browser: "Browser capture", lookup: "Identifier lookup", paste: "Pasted text", pdf: "PDF files" })[k] || k;
}

async function handoff() {
  const { pickSource } = await import("./notes.js");
  const id = await pickSource();
  if (!id) return;
  const d = await call("refs.get", { id });
  const doi = d.ref.fields.doi;
  if (!doi) return toast("This source has no DOI to hand off.", "info");
  let pattern = REPORT_ADDRESS;
  try { pattern = localStorage.getItem("handoff:citation-context") || REPORT_ADDRESS; } catch (_) {}
  call("system.open_url", { url: pattern.replace("{doi}", doi.split("/").map(encodeURIComponent).join("/")) });
}
