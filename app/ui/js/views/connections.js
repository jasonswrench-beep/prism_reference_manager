// Research connections: honest transport labels, file exchange imports, import history with
// batch undo, and the network log. No service is shown as "connected" when the only operation
// is importing a downloaded file or opening a web page.

import { call } from "../api.js";
import { h, clear, icon, btn, toast, modal, select, field, fmtDate, showError } from "../dom.js";
import { S, registerView, go } from "../app.js";
import * as dialogs from "../dialogs.js";

const SERVICES = [
  { key: "mapping-service", name: "Literature-mapping service", transport: "File exchange", formats: "BibTeX, RIS and CSV", in: true, out: "Seed BibTeX export", note: "Export your collection from the service as BibTeX, RIS or CSV, then import it here. To seed the service, export selected sources as BibTeX and upload that file there." },
  { key: "ai-search", name: "AI research search engine", transport: "File exchange", formats: "RIS and CSV", in: true, out: null, note: "Export results as RIS or CSV from the service, then import here with duplicate and metadata review. An API connection would need its own authorization and has not been set up." },
  { key: "citation-context", name: "Citation-context service", transport: "Browser handoff", formats: "Source page by DOI", in: false, out: null, handoff: true, note: "Opens the service's page for a DOI in your web browser. Its classifications are its own dated observations; they are not stored as facts about the source. An API adapter needs separate authorization." },
  { key: "reading-assistant", name: "AI reading assistant", transport: "File exchange", formats: "BibTeX and RIS", in: true, out: "BibTeX or RIS export", note: "Import its BibTeX or RIS exports; send sources to it by exporting BibTeX or RIS. Direct extraction would require confirmed account access and is not set up." },
];

const TRADITIONAL = [
  ["Desktop reference manager (RIS)", "ris"], ["Desktop reference manager (XML records)", "xml"], ["Open-source reference manager (BibTeX or CSL JSON)", "bibtex"], ["Plain BibTeX or biblatex file", "bibtex"],
];

registerView("connections", {
  async render(el) {
    el.append(h("h1.view-title", { tabindex: "-1" }, "Research connections"), h("p.view-sub", "Bring discoveries into your project with their sources intact"));
    const left = h("div", { style: { display: "flex", flexDirection: "column", gap: "14px" } });
    const right = h("div", { style: { display: "flex", flexDirection: "column", gap: "14px" } });
    el.append(h("div", { style: { display: "grid", gridTemplateColumns: "minmax(0, 1fr) 360px", gap: "16px" } }, left, right));
    left.append(h("div.card", h("table.table", h("thead", h("tr", h("th", "Service"), h("th", "Available workflow"), h("th", "Action"))),
      h("tbody", SERVICES.map((s) => h("tr", h("td", h("strong", s.name), h("div.small.muted", s.note)), h("td.nowrap", `${s.transport} • ${s.formats}`),
        h("td.nowrap", s.in ? btn("Import files", { small: true, icon: "download", onClick: () => dialogs.importFile(s.key) }) : null,
          s.out ? btn("Export for it", { small: true, icon: "upload", onClick: () => dialogs.exportRefs([]) }) : null,
          s.handoff ? btn("Open for a source…", { small: true, icon: "external", onClick: () => handoff() }) : null)))))),
      h("p.small", "API access and availability depend on each provider. File exchange works without an account and never shows as a live connection."));

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
      h("div", TRADITIONAL.map(([label, fmt]) => h("button.proj-row", { type: "button", style: { gridTemplateColumns: "28px 1fr", padding: "10px 12px" }, on: { click: () => dialogs.importFile("desktop-manager") } }, icon("doc"), h("span", label)))),
      btn("Import exported library", { kind: "primary", icon: "download", onClick: () => dialogs.importFile("desktop-manager") }),
      h("p.small.muted", "Start with a dry run: the migration scan shows what an export file carries and what it cannot (annotations inside another program's database, smart groups, files without the companion folder). Files and annotations need their own migration checks.")),
      h("div.card.pad", h("h3", "Network"), h("p.small.muted", "Every request is listed with what it sent. Nothing runs in the background."), h("div.btn-row", btn("Network activity", { small: true, onClick: () => dialogs.network() }), btn("Network settings", { small: true, onClick: () => go("settings", { tab: "network" }) }))),
      h("div.card.pad", h("h3", "Other Prism research tools"), h("p.small", "Export source-linked notes and evidence as a versioned exchange file from Notes. Direct sending requires a compatible receiving version and is planned for later."), btn("Go to Notes", { small: true, onClick: () => go("notes") })));
  },
});

function serviceLabel(k) {
  return ({ file: "File", "mapping-service": "Literature-mapping service", "ai-search": "AI research search", "reading-assistant": "AI reading assistant", "desktop-manager": "Reference manager export", lookup: "Identifier lookup", paste: "Pasted text", pdf: "PDF files" })[k] || k;
}

async function handoff() {
  const { pickSource } = await import("./notes.js");
  const id = await pickSource();
  if (!id) return;
  const d = await call("refs.get", { id });
  const doi = d.ref.fields.doi;
  if (!doi) return toast("This source has no DOI to hand off.", "info");
  let pattern = "";
  try { pattern = localStorage.getItem("handoff:citation-context") || ""; } catch (_) {}
  const p = await import("../dom.js").then((m) => m.promptDialog("Open in browser", "The service's web address for a DOI. Write {doi} where the DOI goes.", pattern || "https://", { help: "Copy the address of any report page from the service and replace its DOI with {doi}. It is remembered on this computer.", ok: "Open" }));
  if (!p || !p.includes("{doi}")) { if (p) toast("Include {doi} in the address.", "error"); return; }
  try { localStorage.setItem("handoff:citation-context", p); } catch (_) {}
  call("system.open_url", { url: p.replace("{doi}", encodeURIComponent(doi)) });
}
