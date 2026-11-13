// Shared dialogs: libraries, projects, imports and review, lookups, exports, backups,
// duplicates, bulk edits, health, history, network, diagnostics and local connections.

import { styleSelect, LOCALES } from "./styles.js";
import { call, on } from "./api.js";
import { h, clear, icon, iconBtn, btn, modal, toast, confirmDialog, promptDialog, select, field, showError, fmtDate, runsHtml } from "./dom.js";
import { S, refresh, go, manuscript, loadProjects, render, setProject } from "./app.js";
import { sourceEditor, fieldLabel } from "./editor.js";

const pid = () => S.project?.project.id || "";

// ---- libraries ------------------------------------------------------------------------------

export async function newLibrary() {
  const name = h("input", { value: "My Library" });
  const loc = h("input", { value: S.info?.default_library || "" });
  const r = await modal("Create a library", h("div",
    h("p.muted", "A library is a folder on this computer that holds your sources, notes, files and backups. No account is needed and nothing leaves this computer unless you turn on an online lookup."),
    field("Library name", name),
    field("Folder", h("div.row", loc, btn("Choose…", { onClick: async () => { const p = await call("dialog.pick_folder", { title: "Choose where to create the library" }); if (p) loc.value = p + "\\" + (name.value || "My Library"); } })),
      "Avoid shared sync folders: two computers must never open the same library at once. Use backups to move between computers."),
  ), { buttons: [{ label: "Cancel" }, { label: "Create library", kind: "primary", run: async () => {
    if (!loc.value.trim()) throw new Error("Choose a folder.");
    return call("library.create", { path: loc.value.trim(), name: name.value.trim() });
  } }] }).done;
  if (r) { await refresh(); go("projects"); toast("Library ready. Backups run automatically once a day; change that in Settings.", "success"); }
}

export async function openLibrary() {
  const r = await call("library.choose");
  if (r?.cancelled) return;
  if (r?.empty_folder) {
    if (await confirmDialog("No library in this folder", `The folder ${r.empty_folder} has no library. Create a new library there?`, { ok: "Create library" })) {
      await call("library.create", { path: r.empty_folder, name: "" });
    } else return;
  }
  await refresh();
  go("projects");
}

// ---- projects and manuscripts ---------------------------------------------------------------

export async function newProject() {
  if (!S.info?.library) return toast("Open or create a library first.", "error");
  const title = h("input", { placeholder: "For example: AI literacy in communication education" });
  const ms = h("input", { value: "Untitled manuscript" });
  const style = styleSelect("apa-7");
  const r = await modal("New project", h("div",
    field("Project title", title),
    field("First manuscript", ms, "A manuscript is the article, chapter or other output whose citations this project produces. You can add more later."),
    field("Default citation style", style),
  ), { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Create project", kind: "primary", run: () => call("projects.create", { title: title.value, style: style.value, manuscript: ms.value }) }] }).done;
  if (r) { await loadProjects(); await setProject(r.id); toast("Project created", "success"); }
}

export async function newManuscript() {
  if (!S.project) return toast("Choose a project first.", "error");
  const title = h("input", { placeholder: "Article, chapter or dissertation title" });
  const style = styleSelect(S.project.project.default_style || "apa-7");
  const locale = select(LOCALES, "en-US");
  const r = await modal("New manuscript", h("div", field("Title", title), field("Citation style", style), field("Citation language", locale, "Independent of the interface language.")),
    { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Create", kind: "primary", run: () => call("manuscripts.create", { project_id: pid(), title: title.value, style: style.value, locale: locale.value }) }] }).done;
  if (r) { await call("manuscripts.set_active", { project_id: pid(), id: r.id }); S.manuscriptId = r.id; await refresh(); }
}

export async function copyProject(p) {
  const title = h("input", { value: p.title + " (copy)" });
  const box = (id, label, checked, help) => { const c = h("input", { type: "checkbox", id, checked }); return [c, h("div", h("label.row", { for: id }, c, label), help ? h("div.help", { style: { marginLeft: "26px" } }, help) : null)]; };
  const [mem, memEl] = box("cp-mem", "Source membership", true, "Sources stay one shared record; the copy gets its own inclusion and reading decisions.");
  const [notes, notesEl] = box("cp-notes", "Project notes", false, "Copied notes become new notes owned by the new project.");
  const [mss, mssEl] = box("cp-ms", "Manuscripts and their reference sets", false, "The copy history stays with the original manuscripts.");
  const [outl, outlEl] = box("cp-out", "Outline", false);
  const r = await modal("Copy project", h("div", field("New title", title), memEl, notesEl, mssEl, outlEl), { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Copy project", kind: "primary", run: () => call("projects.copy", { id: p.id, options: { title: title.value, membership: mem.checked, notes: notes.checked, manuscripts: mss.checked, outline: outl.checked } }) }] }).done;
  if (r) { await refresh(); toast("Project copied", "success"); }
}

export async function removeFromProject(ids) {
  if (!S.project || !ids.length) return;
  const previews = await Promise.all(ids.slice(0, 20).map((id) => call("projects.removal_preview", { id: pid(), ref_id: id })));
  const lines = previews.map((p) => h("li", h("strong", p.title || "Untitled"), h("div.small.muted",
    [p.project_notes ? `${p.project_notes} project notes stay in the project without this source link` : "no project notes",
     p.project_annotations ? `${p.project_annotations} project annotations remain on the file` : null,
     p.manuscripts_using.length ? `removed from: ${p.manuscripts_using.join(", ")}` : null,
     p.copies ? `${p.copies} copied citations keep their history` : null,
     p.other_projects.length ? `still in: ${p.other_projects.join(", ")}` : "not in any other project"].filter(Boolean).join(" • "))));
  const ok = await confirmDialog("Remove from project", h("div", h("p", `Remove ${ids.length} source${ids.length === 1 ? "" : "s"} from “${S.project.project.title}”? The source records, files and other projects are not affected. You can undo this.`), h("ul", lines)), { ok: "Remove from project", kind: "danger" });
  if (!ok) return;
  await call("projects.remove_sources", { id: pid(), ids });
  await refresh();
}

// ---- adding sources -------------------------------------------------------------------------

export async function addSource() {
  if (!S.info?.library) return;
  const r = await sourceEditor(null, { mode: "new", projectId: pid() });
  if (r) { await refresh(); go("library", { select: r.id }); }
}

export async function lookup(initial = "") {
  const ta = h("textarea", { rows: 4, placeholder: "One per line: 10.1037/abc123, PMID 31234567, PMC1234567, arXiv:2401.01234, ISBN 978-0-00-000000-0, or a web page address", value: initial });
  const out = h("div");
  let found = [];
  const run = async () => {
    clear(out, h("div.muted", "Looking up…"));
    const text = ta.value;
    let ids = await call("lookup.identifiers", { text });
    for (const line of text.split(/\n/)) { const l = line.trim(); if (/^https?:\/\//i.test(l) && !/doi\.org\//i.test(l)) ids.push(["url", l]); }
    if (!ids.length) { clear(out, h("div.notice.warn", icon("warn"), "No identifiers were recognised. For a source without an online record, use Add source manually.")); return false; }
    found = [];
    clear(out);
    for (const [kind, id] of ids) {
      const row = h("div.card.pad", { style: { marginBottom: "8px" } }, h("div.muted", `${kind.toUpperCase()} ${id}: looking up…`));
      out.append(row);
      try {
        const r = await call("lookup.resolve", { kind, id, project_id: pid() });
        clear(row);
        if (r.ok && r.candidate) {
          found.push(r.candidate);
          row.append(h("div", h("strong", r.candidate.fields.title || "(no title)")),
            h("div.small.muted", [(r.candidate.creators || []).map((c) => c.name.family || c.name.literal).join(", "), r.candidate.issued?.year, r.candidate.fields.container_title].filter(Boolean).join(" • ")),
            ...r.corroboration.map((c) => h("div.small.status-ok", icon("check", 14), " " + c)),
            ...r.conflicts.map((c) => h("div.small.status-review", icon("warn", 14), " " + c)),
            ...(r.candidate.notices || []).map((n) => h("div.small.status-review", icon("warn", 14), ` Notice: ${n.kind} (${n.source})`)));
        } else row.append(h("div.status-review", icon("warn", 15), ` ${kind.toUpperCase()} ${id}: ${r.message}`));
      } catch (e) { clear(row, h("div.status-review", icon("warn", 15), " " + e.message)); }
    }
    return false;
  };
  const res = await modal("Add by identifier", h("div",
    h("p.muted", "Lookups send only the identifier to an open metadata service (listed in Settings › Network). Results are reviewed before anything is added; a valid identifier confirms which work it is, not that the work supports a claim."),
    field("Identifiers", ta), out), { size: "wide", buttons: [{ label: "Close" }, { label: "Look up", run }, { label: "Review import", kind: "primary", run: async () => {
      if (!found.length) await run();
      if (!found.length) return false;
      return call("lookup.stage", { refs: found, name: "Identifier lookup", service: "lookup", project_id: pid() });
    } }] }).done;
  if (res && res.id) importReview(res);
}

export async function pasteBibliography() {
  const ta = h("textarea", { rows: 12, placeholder: "Paste a reference list. Each entry becomes a candidate with its original text kept; nothing is added until you review it." });
  const r = await modal("Paste a bibliography", h("div", field("References", ta), h("p.small.muted", "Parsing pasted text is approximate. Entries with an identifier are matched more reliably; low-confidence entries are marked for review.")),
    { size: "wide", buttons: [{ label: "Cancel" }, { label: "Review", kind: "primary", run: () => call("import.stage", { name: "Pasted bibliography", text: ta.value, format: "paste", service: "paste", project_id: pid() }) }] }).done;
  if (r) importReview(r);
}

// Where an imported file came from. Service and program names here are interoperability labels
// (which export a file came from), never comparisons.
export const SERVICES = [
  ["file", "A file from my computer"],
  ["mapping-service", "ResearchRabbit export (BibTeX, RIS or CSV)"],
  ["ai-search", "Consensus export (RIS or CSV)"],
  ["reading-assistant", "SciSpace export (BibTeX or RIS)"],
  ["zotero", "Zotero export (RIS, BibTeX or CSL JSON)"],
  ["endnote", "EndNote export (XML or RIS)"],
  ["mendeley", "Mendeley export (RIS or BibTeX)"],
  ["desktop-manager", "Another reference manager's export (RIS, BibTeX or XML)"],
];

export async function importFile(serviceDefault = "file") {
  if (!S.info?.library) return;
  const f = await call("import.pick");
  if (!f || f.cancelled) return;
  const service = select(SERVICES, serviceDefault);
  const dest = select([["", "Library only (no project)"], ...S.projects.filter((p) => !p.project.archived).map((p) => [p.project.id, p.project.title])], pid());
  const fmt = select([["ris", "RIS"], ["bibtex", "BibTeX / biblatex"], ["csljson", "CSL JSON"], ["xml", "XML records (EndNote XML)"], ["csv", "CSV / spreadsheet"], ["paste", "Plain text references"]], f.format);
  const mapBox = h("div");
  const mapping = {};
  const targets = ["", "ignore", "type", "authors", "editors", "year", "date", "title", "container_title", "volume", "issue", "page", "article_number", "doi", "url", "publisher", "abstract", "keywords", "isbn", "issn", "edition", "language", "note", "pmid", "collections", "rating", "source_id"];
  const drawMap = () => {
    clear(mapBox);
    if (fmt.value !== "csv" || !f.csv_headers.length) return;
    mapBox.append(h("h3", "Column mapping"), h("p.small.muted", "Columns left as “Keep as extra field” are preserved in each record's overflow area."),
      h("table.table", h("thead", h("tr", h("th", "Column"), h("th", "Becomes"))), h("tbody", f.csv_headers.map(([col, sug]) => {
        mapping[col] = sug;
        const s = select(targets.map((t) => [t, t === "" ? "Keep as extra field" : t === "ignore" ? "Ignore" : fieldLabel(t) === t ? t : fieldLabel(t)]), sug, { "aria-label": "Mapping for " + col, on: { change: (e) => (mapping[col] = e.target.value) } });
        return h("tr", h("td", col), h("td", s));
      }))));
  };
  fmt.addEventListener("change", drawMap);
  drawMap();
  const scanBtn = btn("Dry run (migration scan)", { onClick: async () => {
    const r = await call("import.scan", { name: f.name, text: f.text });
    modal("Migration scan", h("div",
      h("p", `${r.records} records in ${r.format.toUpperCase()} format.`),
      h("h3", "Types"), h("ul", Object.entries(r.types).map(([k, v]) => h("li", `${k}: ${v}`))),
      h("p", `${r.tags} tags • ${r.records_with_file_links} records with file links • ${r.records_with_notes} with notes`),
      Object.keys(r.unmapped_fields).length ? h("div", h("h3", "Fields kept in overflow (no direct mapping)"), h("ul", Object.entries(r.unmapped_fields).map(([k, v]) => h("li.mono", `${k} (${v})`)))) : null,
      h("h3", "Not carried by an export file"), h("ul", r.cannot_carry.map((c) => h("li", c))),
      h("div.notice", icon("info"), r.note)), { buttons: [{ label: "Close" }] });
  } });
  const r = await modal("Import " + f.name, h("div",
    h("p.muted", `${(f.size / 1024).toFixed(1)} KB. Detected format: ${f.format.toUpperCase()}. Records are staged for review; nothing changes until you commit.`),
    h("div.grid2", field("Where it came from", service, "File exchange only: no account is connected."), field("Destination project", dest)),
    field("Format", fmt), mapBox, h("div.btn-row", scanBtn)),
    { size: "wide", buttons: [{ label: "Cancel" }, { label: "Review records", kind: "primary", run: () => withImportProgress(() => call("import.stage", { name: f.name, text: f.text, format: fmt.value, service: service.value, project_id: dest.value, mapping: fmt.value === "csv" ? mapping : null })) }] }).done;
  if (r) importReview(r);
}

// Progress for reading files and looking up identifiers, shown while an import runs.
async function withImportProgress(run) {
  const bar = h("div.small", "Starting…");
  const stop = h("button.btn.small", { type: "button", on: { click: () => { stop.disabled = true; bar.textContent = "Stopping…"; call("import.stop"); } } }, "Stop");
  const back = h("div.toasts", { role: "status" }, h("div.toast", h("div.row", bar, h("span.grow"), stop)));
  document.body.append(back);
  const off = await on("import-progress", (p) => { bar.textContent = p.total ? `${p.phase}: ${Math.min(p.done + 1, p.total)} of ${p.total}` : p.phase; });
  try { return await run(); } finally { if (typeof off === "function") off(); back.remove(); }
}

export async function importPdfs() {
  if (!S.info?.library) return;
  try {
    const r = await withImportProgress(() => call("import.pick_pdfs", { project_id: pid() }));
    if (!r || r.cancelled) return;
    importReview(r);
  } catch (e) { showError(e); }
}

/** Import every PDF in a folder and its subfolders, optionally turning subfolders into collections. */
export async function importFolder() {
  if (!S.info?.library) return;
  const mirror = h("input", { type: "checkbox", id: "if-mirror", checked: true });
  const go2 = await modal("Import a folder of PDFs", h("div",
    h("p", "Every PDF in the folder you choose and its subfolders is read, identifiers (DOI, arXiv) are looked up when network use allows, and the results wait for your review."),
    h("label.row", { for: "if-mirror" }, mirror, "Turn subfolders into collections"),
    h("p.small.muted", "Files that are already in the library are recognized and marked to skip. Originals are not moved or changed; imported files are copied into the library.")),
    { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Choose folder…", kind: "primary", value: "go" }] }).done;
  if (go2 !== "go") return;
  try {
    const r = await withImportProgress(() => call("import.folder", { project_id: pid(), mirror: mirror.checked }));
    if (!r || r.cancelled) return;
    importReview(r);
  } catch (e) { showError(e); }
}

/** Look for a free, legal copy of each source and attach it; explain the options when none is found. */
export async function findFreeCopies(ids) {
  const list = Array.isArray(ids) ? ids : [ids];
  const bar = h("div.small", "Looking for free copies…");
  const back = h("div.toasts", { role: "status", style: { pointerEvents: "none" } }, h("div.toast", bar));
  document.body.append(back);
  const off = await on("oa-progress", (p) => { bar.textContent = `Looking for free copies: ${Math.min(p.done + 1, p.total)} of ${p.total}`; });
  let results;
  try { results = list.length === 1 ? [await call("files.find_oa", { ref_id: list[0], project_id: pid() })] : await call("files.find_oa_many", { ids: list, project_id: pid() }); }
  catch (e) { showError(e); return; }
  finally { if (typeof off === "function") off(); back.remove(); }
  const versionName = (v) => ({ publishedVersion: "published version", acceptedVersion: "accepted manuscript", submittedVersion: "submitted manuscript (preprint)" })[v] || (v ? v : "version not stated");
  const attached = results.filter((r) => r.status === "attached").length;
  const rows = results.map((r) => h("div.card.pad", { style: { marginBottom: "8px" } },
    h("div.row", icon(r.status === "attached" ? "checkCircle" : r.error || r.status === "error" || r.status === "download_failed" ? "warn" : "info", 18), h("strong.grow.ellipsis", r.title || "Source")),
    h("div.small", r.error || r.message),
    r.status === "attached" ? h("div.small.muted", `Source: ${r.host} • ${versionName(r.version)} • licence: ${r.license || "not stated"}`) : null,
    r.status === "attached" && r.version && r.version !== "publishedVersion" ? h("div.small.status-review", icon("info", 13), " This is not the published version. Cite the published version, and check page numbers against it.") : null,
    r.status !== "attached" && r.status !== "has_pdf" ? h("div.btn-row", { style: { marginTop: "6px" } },
      r.publisher_url ? btn("Open publisher page", { small: true, icon: "external", onClick: () => call("system.open_url", { url: r.publisher_url }) }) : null,
      r.landing_url ? btn("Open free copy page", { small: true, icon: "external", onClick: () => call("system.open_url", { url: r.landing_url }) }) : null,
      btn("Attach a file…", { small: true, onClick: async () => { const res = await call("files.attach_pick", { ref_id: r.ref_id, mode: "managed" }); if (!res?.cancelled) toast("Attached", "success"); } })) : null));
  await modal(list.length === 1 ? "Free copy" : `Free copies: ${attached} of ${list.length} attached`, h("div",
    h("p.small.muted", "Free copies come from open-access repositories and publishers listed in an open scholarly index. Only legal open-access copies are fetched; a licence and version are recorded with each file."),
    rows), { size: "narrow", buttons: [{ label: "Close" }] }).done;
  await refresh();
}

const STATUS_LABEL = { update: ["Changed since last import", "info"], unchanged: ["Unchanged since last import", "neutral"], new: ["New source", "success"], duplicate: ["Possible duplicate", "attention"], conflict: ["Identifier conflict", "danger"], unresolved: ["Needs metadata review", "attention"], version: ["Another version of a library work", "info"], invalid: ["Invalid", "danger"] };
const DECISIONS = [["update", "Update the library record"], ["accept", "Add"], ["skip", "Skip"], ["merge", "Merge into match"], ["keep_both", "Keep both"], ["link_version", "Add and link as version"]];

export async function importReview(batch) {
  let b = batch;
  const body = h("div");
  const draw = () => {
    const counts = {};
    for (const r of b.records) counts[r.status] = (counts[r.status] || 0) + 1;
    const willAdd = b.records.filter((r) => ["accept", "keep_both", "link_version"].includes(r.decision)).length;
    clear(body,
      h("div.card", { style: { marginBottom: "12px" } },
        h("div.row", { style: { padding: "10px 14px" } }, icon("doc", 20), h("strong", b.file_name), h("span.badge.info", `${b.records.length} records found`), h("span.muted.small", `${b.adapter.toUpperCase()} adapter ${b.adapter_version} • from ${b.service}`)),
        h("table.table", h("tbody",
          h("tr", h("td", icon("plus")), h("td", h("strong", counts.new || 0), " new sources")),
          counts.update || counts.unchanged ? h("tr", h("td", icon("refresh")), h("td", h("strong", counts.update || 0), " changed since they were last imported from the same program, ", h("strong", counts.unchanged || 0), " unchanged (skipped)")) : null,
          h("tr", h("td", icon("copy")), h("td", h("strong", (counts.duplicate || 0) + (counts.version || 0)), " possible duplicates or versions")),
          h("tr", h("td", icon("warn")), h("td", h("strong", (counts.unresolved || 0) + (counts.conflict || 0)), " need metadata review")),
          h("tr", h("td", icon("doc")), h("td", h("strong", b.files_included), b.files_included === 1 ? " file link included" : " file links included", b.files_included === 0 ? h("span.muted", " (no files in this export; that is different from missing files)") : null)),
        ))),
      groupsBox(),
      Object.keys(b.unmapped_fields || {}).length ? h("details", h("summary", `${Object.keys(b.unmapped_fields).length} fields without a direct mapping are kept in each record's overflow area`), h("ul", Object.entries(b.unmapped_fields).map(([k, v]) => h("li.mono", `${k} (${v})`)))) : null,
      h("div.btn-row", { style: { margin: "8px 0" } }, h("span.muted", "Set all:"),
        btn("Add all new", { small: true, onClick: async () => { b = await call("import.set_all", { id: b.id, decision: "accept", status: "new" }) || b; draw(); } }),
        btn("Skip all duplicates", { small: true, onClick: async () => { b = await call("import.set_all", { id: b.id, decision: "skip", status: "duplicate" }) || b; draw(); } }),
        btn("Merge all duplicates", { small: true, onClick: async () => { b = await call("import.set_all", { id: b.id, decision: "merge", status: "duplicate" }) || b; draw(); } })),
      h("div", { style: { maxHeight: "48vh", overflow: "auto" } }, h("table.table", h("thead", h("tr", h("th", "Record"), h("th", "Status"), h("th", "Decision"), h("th", ""))),
        h("tbody", b.records.map((r) => {
          const [label, kind] = STATUS_LABEL[r.status] || [r.status, "neutral"];
          const dec = select(DECISIONS.filter(([d]) => (d !== "update" || r.status === "update") && (r.match_ref_id || !["merge", "keep_both", "link_version"].includes(d))), r.decision, { "aria-label": "Decision for " + (r.record.fields.title || "record"), on: { change: async (e) => { b = await call("import.set", { id: b.id, index: r.index, decision: e.target.value }); draw(); } } });
          return h("tr",
            h("td", h("div", h("strong", r.record.fields.title || "(no title)")),
              h("div.small.muted", [(r.record.creators || []).slice(0, 3).map((c) => c.name.family || c.name.literal).join(", "), r.record.issued?.year, r.record.fields.container_title].filter(Boolean).join(" • ")),
              r.match_reason ? h("div.small", "Match: " + r.match_reason) : null,
              ...(r.issues || []).map((i) => h("div.small.status-review", icon("warn", 13), " " + i)),
              h("details.support", h("summary", "Original text"), h("pre", r.raw.slice(0, 2000)))),
            h("td", h("span.badge." + kind, label)),
            h("td", dec),
            h("td", btn("Edit", { small: true, onClick: async () => { const rec = await sourceEditor(r.record, { mode: "staged" }); if (rec) { b = await call("import.set", { id: b.id, index: r.index, record: rec }); draw(); } } })));
        })))),
      h("p.small.muted", `${willAdd} will be added; merges fill missing fields and record conflicting values for review without overwriting your edits. The whole import can be undone from Connections › Import history.`));
  };
  function groupsBox() {
    const hasLabels = b.adapter === "xml" && b.records.some((r) => r.record.overflow?.["xml:label"]);
    if (!b.groups_found?.length && !hasLabels) return null;
    const save = async (patch) => { b = await call("import.options", { id: b.id, make_collections: b.make_collections, label_as_groups: b.label_as_groups, ...patch }); draw(); };
    const mk = h("input", { type: "checkbox", id: "imp-groups", checked: !!b.make_collections, on: { change: (e) => save({ make_collections: e.target.checked }) } });
    const lab = h("input", { type: "checkbox", id: "imp-labels", checked: !!b.label_as_groups, on: { change: (e) => save({ label_as_groups: e.target.checked, make_collections: e.target.checked || b.make_collections }) } });
    return h("div.card.pad", { style: { marginBottom: "12px" } },
      b.groups_found?.length ? h("div", h("label.row", { for: "imp-groups" }, mk, `Create collections for ${b.groups_found.length} group${b.groups_found.length === 1 ? "" : "s"} from the other program`),
        h("div.small.muted", { style: { marginLeft: "26px" } }, b.groups_found.slice(0, 12).join(" • "), b.groups_found.length > 12 ? ` and ${b.groups_found.length - 12} more` : "")) : null,
      hasLabels ? h("label.row.small", { for: "imp-labels" }, lab, "Also use the Label field as group names (some programs' users file records this way)") : null,
      h("div.small.muted", "Nested groups become nested collections. Smart groups and saved searches cannot be carried; recreate them as smart collections."));
  }
  draw();
  const res = await modal("Import review", body, { size: "wide", buttons: [
    { label: "Cancel import", run: async () => { await call("import.cancel", { id: b.id }); toast("Import cancelled. No changes were committed.", "info"); return true; } },
    { label: "Import selected", kind: "primary", icon: "download", run: () => withImportProgress(() => call("import.commit", { id: b.id })).catch((e) => { showError(e); return false; }) },
  ] }).done;
  if (res && res.report) {
    const rp = res.report;
    toast(`Imported: ${rp.added + rp.linked} added, ${rp.merged} merged${rp.updated ? `, ${rp.updated} updated` : ""}, ${rp.skipped} skipped${rp.collections ? `, ${rp.collections} collections created` : ""}${res.attached ? `, ${res.attached} PDFs attached` : ""}.`, "success", { action: { label: "Undo import", run: () => undoImport(rp.batch_id) } });
    await refresh();
  }
}

export async function undoImport(batchId) {
  const p = await call("import.undo_preview", { id: batchId });
  let include = false;
  const cb = h("input", { type: "checkbox", id: "undo-dep", on: { change: (e) => (include = e.target.checked) } });
  const ok = await confirmDialog("Undo import", h("div",
    h("p", `${p.removable.length} imported sources have no later work and will move to the trash.`),
    p.dependent.length ? h("div", h("p", `${p.dependent.length} imported sources have later work and are kept unless you include them:`), h("ul", p.dependent.map(([, why]) => h("li", why))), h("label.row", { for: "undo-dep" }, cb, "Also move these to the trash")) : null), { ok: "Undo import" });
  if (!ok) return;
  const n = await call("import.undo", { id: batchId, include_dependent: include });
  toast(`${n} sources moved to the trash. Restore them from the trash if needed.`, "success");
  await refresh();
}

// ---- exports and backups --------------------------------------------------------------------

export async function exportRefs(ids = []) {
  const fmt = select([["ris", "RIS"], ["bibtex", "BibTeX"], ["biblatex", "biblatex"], ["csljson", "CSL JSON"], ["csv", "CSV spreadsheet"], ["xml", "Prism XML records"]], "ris");
  const scope = select([...(ids.length ? [["selected", `Selected sources (${ids.length})`]] : []), ...(S.project ? [["project", "Active project's sources"]] : []), ["all", "Whole library"]], ids.length ? "selected" : "all");
  const excl = h("input", { placeholder: "abstract, note" });
  const r = await modal("Export references", h("div", field("Format", fmt), field("Sources", scope), field("Leave out fields (BibTeX)", excl, "Comma separated."),
    h("div.notice", icon("info"), "An export file is not a complete backup. Use Export complete library package or Back up now to keep everything.")),
    { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Export", kind: "primary", run: async () => {
      let list = ids;
      if (scope.value === "project") list = (await call("refs.list", { query: { project_id: pid(), limit: 1000000 } })).rows.map((x) => x.id);
      if (scope.value === "all") list = [];
      return call("export.refs", { format: fmt.value, ids: list, exclude_fields: excl.value.split(",").map((s) => s.trim()).filter(Boolean) });
    } }] }).done;
  if (r && !r.cancelled) toast(`Exported ${r.count} references to ${r.path}`, "success");
}

export async function exportPackage() {
  const cb = h("input", { type: "checkbox", id: "pk-files", checked: true });
  const ok = await modal("Export complete library package", h("div", h("p", "Writes every source, project, manuscript context, note, annotation, copy record, snapshot and history needed to rebuild the library, as open JSON and Markdown with a checksum manifest."), h("label.row", { for: "pk-files" }, cb, "Include attached files")),
    { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Export", kind: "primary", run: () => call("export.package", { include_files: cb.checked }) }] }).done;
  if (ok && !ok.cancelled) toast("Package saved: " + ok.path, "success");
}

export async function backupNow() {
  const cb = h("input", { type: "checkbox", id: "bk-files", checked: !!S.settings.backup_include_files });
  const r = await modal("Back up now", h("div", h("p", "A backup is a verified snapshot of the library. It can be restored into an empty folder on this or another computer."), h("label.row", { for: "bk-files" }, cb, "Include attached files")),
    { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Save backup", kind: "primary", run: () => call("backup.now", { include_files: cb.checked }) }] }).done;
  if (r && !r.cancelled) verifyReport(r.verify, "Backup saved: " + r.path);
}

function verifyReport(v, title) {
  modal(title || "Backup verification", h("div",
    h("div.notice" + (v.ok ? "" : ".error"), icon(v.ok ? "checkCircle" : "warn"), v.ok ? "Verified: the backup is complete and readable." : "This backup did not pass every check."),
    h("ul.check-list", v.checks.map(([name, ok, detail]) => h("li", icon(ok ? "check" : "warn", 18, ok ? "Passed" : "Failed"), h("div", h("strong", name), h("div.small.muted", detail))))),
    h("p.small.muted", `Created ${fmtDate(v.manifest.created)} • schema ${v.manifest.schema} • journal watermark ${v.manifest.journal_watermark}`)), { buttons: [{ label: "Close" }] });
}

export async function verifyBackup() {
  try { const v = await call("backup.verify"); if (v && !v.cancelled) verifyReport(v); } catch (e) { showError(e); }
}

export async function restoreBackup() {
  await modal("Restore a backup", h("div", h("p", "First choose the backup, then an empty folder. The backup is verified before anything is written; your current library is never overwritten. When it finishes you can open the restored library.")),
    { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Choose backup…", kind: "primary", run: async () => {
      const r = await call("backup.restore");
      if (!r || r.cancelled) return false;
      if (await confirmDialog("Restore complete", `The library was restored to ${r.path}. Open it now?`, { ok: "Open restored library" })) {
        await call("library.open", { path: r.path });
        await refresh();
        go("projects");
      }
      return true;
    } }] }).done;
}

// ---- editing tools ---------------------------------------------------------------------------

export async function findReplace(ids) {
  const fieldSel = select([["*", "All text fields"], ...["title", "container_title", "publisher", "abstract", "note", "url", "volume", "issue", "page", "edition"].map((f) => [f, fieldLabel(f)])], "title");
  const find = h("input"), rep = h("input");
  const rx = h("input", { type: "checkbox", id: "fr-rx" }), cs = h("input", { type: "checkbox", id: "fr-cs" });
  const scope = select([...(ids?.length ? [["sel", `Selected sources (${ids.length})`]] : []), ["all", "Whole library"]], ids?.length ? "sel" : "all");
  const prev = h("div");
  const args = (apply) => ({ ids: scope.value === "sel" ? ids : null, field: fieldSel.value, find: find.value, replace: rep.value, regex: rx.checked, case_sensitive: cs.checked, apply });
  const preview = async () => {
    const ch = await call("refs.find_replace", args(false));
    clear(prev, h("p", h("strong", `${ch.length} changes`), " would be made."), h("div", { style: { maxHeight: "300px", overflow: "auto" } }, h("table.table", h("thead", h("tr", h("th", "Source"), h("th", "Field"), h("th", "Before"), h("th", "After"))), h("tbody", ch.slice(0, 300).map((c) => h("tr", h("td", c.title), h("td", fieldLabel(c.field)), h("td", c.before), h("td", c.after)))))));
    return false;
  };
  const r = await modal("Find and replace", h("div", h("div.grid2", field("In", fieldSel), field("Sources", scope)), h("div.grid2", field("Find", find), field("Replace with", rep)),
    h("div.row", h("label.row", { for: "fr-rx" }, rx, "Regular expression"), h("label.row", { for: "fr-cs" }, cs, "Match case")), prev),
    { size: "wide", buttons: [{ label: "Cancel" }, { label: "Preview", run: preview }, { label: "Apply all", kind: "primary", run: async () => { const ch = await call("refs.find_replace", args(true)); return ch.length; } }] }).done;
  if (r !== null && r !== undefined) { toast(`${r} values changed. Undo is available.`, "success"); await refresh(); }
}

export async function bulkEdit(ids) {
  if (!ids?.length) return toast("Select sources first.", "error");
  const op = select([["set", "Set to"], ["append", "Append"], ["prefix", "Prefix"], ["clear", "Clear"], ["copy", "Copy to another field"], ["move", "Move to another field"]], "set");
  const fields = ["title", "container_title", "publisher", "publisher_place", "volume", "issue", "page", "edition", "language", "genre", "note", "url", "tags", "type"];
  const fieldSel = select(fields.map((f) => [f, fieldLabel(f)]), "language");
  const target = select(fields.map((f) => [f, fieldLabel(f)]), "note");
  const value = h("input");
  const prev = h("div");
  const mk = () => ({ op: op.value, field: fieldSel.value, value: value.value, target: target.value });
  const r = await modal(`Bulk edit ${ids.length} sources`, h("div", h("div.grid2", field("Change", op), field("Field", fieldSel)), field("Value", value), field("Target field (copy or move)", target), prev),
    { size: "wide", buttons: [{ label: "Cancel" }, { label: "Preview", run: async () => {
      const ch = await call("refs.bulk_edit", { ids, op: mk(), apply: false });
      clear(prev, h("p", h("strong", `${ch.length} of ${ids.length}`), " sources would change."), h("div", { style: { maxHeight: "260px", overflow: "auto" } }, h("table.table", h("tbody", ch.slice(0, 200).map((c) => h("tr", h("td", c.title), h("td", c.before), h("td", "→"), h("td", c.after)))))));
      return false;
    } }, { label: "Apply", kind: "primary", run: async () => (await call("refs.bulk_edit", { ids, op: mk(), apply: true })).length }] }).done;
  if (r !== null && r !== undefined) { toast(`${r} sources changed in one step. Undo restores them all.`, "success"); await refresh(); }
}

// ---- duplicates --------------------------------------------------------------------------

export async function duplicates() {
  const body = h("div", h("p.muted", "Finding possible duplicates…"));
  const sens = h("input", { type: "range", min: 0.7, max: 0.98, step: 0.01, value: S.settings.duplicate_sensitivity || 0.88, "aria-label": "Sensitivity" });
  const load = async () => {
    const list = await call("refs.duplicates", { threshold: +sens.value });
    const ids = [...new Set(list.flatMap((c) => [c.a, c.b]))];
    const labels = ids.length ? (await call("search.run", { query: "", limit: 1 }), {}) : {};
    clear(body,
      h("div.row", h("label", { for: "dsens" }, "Sensitivity"), Object.assign(sens, { id: "dsens" }), h("span.small.muted", "Lower finds more, with more false matches"), btn("Search again", { small: true, onClick: load })),
      list.length ? h("p", `${list.length} possible duplicate pairs.`) : h("div.empty", h("h3", "No likely duplicates"), h("p", "Decisions you made earlier (not duplicates, keep both) are remembered.")),
      h("div", list.slice(0, 200).map((c) => h("div.card.pad", { style: { marginBottom: "8px" } },
        h("div.row", h("span.badge." + (c.kind === "version" ? "info" : "attention"), c.kind === "version" ? "Versions of one work" : `${Math.round(c.score * 100)}% similar`), h("span.small.muted", c.reasons.join(", ")), h("span.grow"),
          btn("Compare", { small: true, kind: "primary", onClick: () => compareRecords(c.a, c.b, load) }),
          btn("Not duplicates", { small: true, onClick: async () => { await call("refs.decide_duplicate", { a: c.a, b: c.b, decision: "not_duplicate" }); load(); } })),
        h("div.small", { "data-pair": c.a + "|" + c.b }, "…")))));
    for (const el of body.querySelectorAll("[data-pair]")) {
      const [a, b] = el.dataset.pair.split("|");
      Promise.all([call("refs.get", { id: a }), call("refs.get", { id: b })]).then(([x, y]) => clear(el, h("div", "A: " + (x.ref.fields.title || "") + ` (${x.ref.issued?.year || "n.d."})`), h("div", "B: " + (y.ref.fields.title || "") + ` (${y.ref.issued?.year || "n.d."})`))).catch(() => {});
    }
    void labels;
  };
  modal("Duplicate review", body, { size: "wide", buttons: [{ label: "Close" }] });
  load();
}

export async function compareRecords(a, b, after) {
  const [x, y] = await Promise.all([call("refs.get", { id: a }), call("refs.get", { id: b })]);
  const A = x.ref, B = y.ref;
  const keys = [...new Set(["type", "creators", "issued", ...Object.keys(A.fields), ...Object.keys(B.fields)])];
  const val = (r, k) => k === "type" ? r.type : k === "creators" ? (r.creators || []).map((c) => c.name.literal || `${c.name.family}, ${c.name.given || ""}`).join("; ") : k === "issued" ? JSON.stringify(r.issued || "") : r.fields[k] || "";
  const choices = {};
  let target = A.id;
  const tsel = select([[A.id, "Keep A as the canonical record"], [B.id, "Keep B as the canonical record"]], A.id, { on: { change: (e) => (target = e.target.value) } });
  const rows = keys.filter((k) => val(A, k) || val(B, k)).map((k) => {
    const same = val(A, k) === val(B, k);
    const name = "ch-" + k;
    const ra = h("input", { type: "radio", name, checked: true, on: { change: () => (choices[k] = A.id) }, "aria-label": "Use A for " + fieldLabel(k) });
    const rb = h("input", { type: "radio", name, on: { change: () => (choices[k] = B.id) }, "aria-label": "Use B for " + fieldLabel(k) });
    return h("tr", h("td", h("strong", fieldLabel(k))), h("td", same ? null : ra, " ", val(A, k)), h("td", same ? null : rb, " ", val(B, k)), h("td", same ? h("span.small.muted", "same") : h("span.small.status-review", "differs")));
  });
  const impact = (r, im) => `${im.projects.length} projects, ${im.manuscripts.length} manuscripts, ${im.notes} notes, ${im.annotations} annotations, ${im.copies} copied citations, ${r.attachments?.length || 0} files`;
  const r = await modal("Compare records", h("div",
    h("table.table", h("thead", h("tr", h("th", "Field"), h("th", "A"), h("th", "B"), h("th", ""))), h("tbody", rows)),
    h("p.small", "A: " + impact(x, x.impact)), h("p.small", "B: " + impact(y, y.impact)),
    field("Merge into", tsel),
    h("div.notice", icon("info"), "Merging moves memberships, notes, files, annotations and copy history to the kept record. The retired record's id keeps working as an alias, and Undo restores both.")),
    { size: "wide", buttons: [
      { label: "Cancel" },
      { label: "Keep both", run: async () => { await call("refs.decide_duplicate", { a: A.id, b: B.id, decision: "keep_both" }); return "kept"; } },
      { label: "Link as versions", run: async () => { await call("refs.link_versions", { ids: [A.id, B.id], preferred: B.type === "preprint" ? A.id : B.id }); return "linked"; } },
      { label: "Merge", kind: "primary", icon: "merge", run: async () => {
        const ch = {};
        for (const [k, v] of Object.entries(choices)) ch[k] = target === A.id ? v : (v === A.id ? A.id : B.id);
        await call("refs.merge", { target, others: [target === A.id ? B.id : A.id], choices: ch });
        return "merged";
      } },
    ] }).done;
  if (r) { toast(r === "merged" ? "Records merged. Undo is available." : r === "linked" ? "Linked as versions of one work." : "Both records kept; they will not be suggested again.", "success"); await refresh(); after && after(); }
}

// ---- reports -----------------------------------------------------------------------------

// ---- clean-up: proposals reviewed before anything changes ----------------------------------------

async function withCleanupProgress(run) {
  const bar = h("div.small", "Working…");
  const back = h("div.toasts", { role: "status", style: { pointerEvents: "none" } }, h("div.toast", bar));
  document.body.append(back);
  const off = await on("cleanup-progress", (p) => { bar.textContent = p.total ? `${p.phase}: ${Math.min(p.done + 1, p.total)} of ${p.total}` : p.phase; });
  try { return await run(); } finally { if (typeof off === "function") off(); back.remove(); }
}

const CLEANUP = {
  doi: { title: "Find missing DOIs", label: "Add found DOIs", none: "No DOI was found for the selected sources that do not have one. Only very close title and year matches are offered." },
  abstract: { title: "Fetch missing abstracts", label: "Add fetched abstracts", none: "No abstracts were found for the selected sources with a DOI and no abstract." },
  title_case: { title: "Titles to sentence case", label: "Change titles to sentence case", none: "The selected titles are already in sentence case." },
};

/** Propose a batch fix for the selected sources and let the person review every change. */
export async function cleanup(kind, ids) {
  const c = CLEANUP[kind];
  let ps;
  try { ps = await withCleanupProgress(() => call("cleanup.propose", { kind, ids, project_id: pid() })); }
  catch (e) { return showError(e); }
  if (!ps.length) return toast(c.none, "info");
  const rows = ps.map((p) => {
    const cb = h("input", { type: "checkbox", checked: true, "aria-label": "Accept change for " + p.title });
    const val = p.field === "abstract" ? h("textarea", { rows: 4, value: p.after, "aria-label": "New " + p.field }) : h("input", { value: p.after, "aria-label": "New " + p.field });
    return { p, cb, val, el: h("tr", h("td", cb), h("td", h("strong", p.title || "Source"), p.before ? h("div.small.muted", "Now: " + p.before) : null, p.note ? h("div.small.status-review", icon("info", 13), " " + p.note) : null), h("td", { style: { minWidth: "40%" } }, val)) };
  });
  const ok = await modal(`${c.title}: review ${ps.length} change${ps.length === 1 ? "" : "s"}`, h("div",
    h("p.small.muted", "Nothing has changed yet. Untick anything that is wrong, edit a value if needed, then apply. Changes are recorded in each source's history and can be undone."),
    h("div", { style: { maxHeight: "55vh", overflow: "auto" } }, h("table.table", h("thead", h("tr", h("th", ""), h("th", "Source"), h("th", "New value"))), h("tbody", rows.map((r) => r.el))))),
    { size: "wide", buttons: [{ label: "Cancel" }, { label: c.label, kind: "primary", run: async () => {
      const chosen = rows.filter((r) => r.cb.checked).map((r) => ({ ...r.p, after: r.val.value }));
      if (!chosen.length) return true;
      const n = await call("cleanup.apply", { proposals: chosen, label: c.title });
      toast(`${n} source${n === 1 ? "" : "s"} updated.`, "success", { action: { label: "Undo", run: async () => { await call("undo"); await refresh(); } } });
      return true;
    } }] }).done;
  if (ok) await refresh();
}

/** Check the selected sources' web addresses and offer to update access dates. */
export async function checkLinks(ids) {
  let r;
  try { r = await withCleanupProgress(() => call("links.check", { ids, project_id: pid() })); }
  catch (e) { return showError(e); }
  if (!r.checked.length) return toast("None of the selected sources has a web address.", "info");
  const working = r.checked.filter((x) => x.status.state === "working").map((x) => x.ref_id);
  const badge = { working: ["Works", "success"], moved: ["Moved", "attention"], broken: ["Broken", "danger"], unreachable: ["No answer", "attention"] };
  await modal("Web address check", h("div",
    h("p", `${r.checked.length} checked: ${working.length} working.` + (r.without_url ? ` ${r.without_url} selected sources have no web address.` : "")),
    h("div", { style: { maxHeight: "50vh", overflow: "auto" } }, h("table.table", h("tbody", r.checked.map((x) => {
      const [t, k] = badge[x.status.state] || [x.status.state, "neutral"];
      return h("tr", h("td", h("strong", x.title), h("div.small.mono", x.status.url)), h("td", h("span.badge." + k, t)), h("td.small", x.status.message),
        h("td", x.status.state !== "working" ? btn("Open", { small: true, onClick: () => call("system.open_url", { url: x.status.final_url || x.status.url }) }) : null));
    })))),
    h("p.small.muted", "A retrieval date matters only for content designed to change. Update access dates only for pages you have checked.")),
    { size: "wide", buttons: [{ label: "Close" }, working.length ? { label: `Set today's access date on ${working.length} working`, kind: "primary", run: async () => { const n = await call("links.set_accessed", { ids: working }); toast(`${n} access dates updated.`, "success"); return true; } } : null].filter(Boolean) }).done;
  await refresh();
}

export async function health() {
  const body = h("div", h("p.muted", "Checking the library…"));
  modal("Library health report", body, { size: "wide", buttons: [{ label: "Close" }] });
  try {
    const r = await call("health.run");
    clear(body,
      h("p", `${r.checked} sources checked ${fmtDate(r.generated)}. Nothing is changed automatically.`),
      h("div.btn-row", h("span.badge." + (r.items.length ? "attention" : "success"), `${r.items.length} items to review`), h("span.badge.neutral", `${r.duplicates} possible duplicate pairs`), h("span.badge.neutral", `${r.missing_files} file problems`), h("span.badge.neutral", `${r.unindexed} PDFs awaiting text indexing`)),
      h("div", { style: { maxHeight: "55vh", overflow: "auto", marginTop: "10px" } }, h("table.table", h("thead", h("tr", h("th", "Check"), h("th", "Source"), h("th", "Detail"), h("th", "Action"))),
        h("tbody", r.items.map((i) => h("tr", h("td", i.check), h("td", i.title), h("td.small", i.detail), h("td", btn(i.action, { small: true, onClick: () => { document.querySelector(".modal-back")?.remove(); go("library", { select: i.ref_id }); } }))))))));
  } catch (e) { showError(e, body); }
}

export async function checkFiles() {
  const body = h("div", h("p.muted", "Checking files…"));
  modal("Attached files", body, { size: "wide", buttons: [{ label: "Close" }] });
  const draw = async (rehash) => {
    const list = await call("files.check", { rehash });
    const bad = list.filter((f) => f.state !== "ok");
    const oldP = h("input", { placeholder: "D:\\Old\\Papers" }), newP = h("input", { placeholder: "E:\\Papers" });
    clear(body,
      h("p", `${list.length} files, ${bad.length} need attention.`), btn("Check contents (slower)", { small: true, onClick: () => draw(true) }),
      bad.length ? h("table.table", h("tbody", bad.map((f) => h("tr", h("td.small.mono", f.path), h("td", h("span.badge.attention", f.state)), h("td.small", f.message), h("td", btn("Relink…", { small: true, onClick: async () => { try { const r = await call("files.relink_pick", { id: f.attachment_id }); if (!r?.cancelled) { toast("Relinked", "success"); draw(false); } } catch (e) { showError(e); } } })))))) : h("div.notice", icon("checkCircle"), "All files were found."),
      h("h3", "Moved a whole folder?"), h("div.grid2", field("Old folder", oldP), field("New folder", newP)),
      btn("Relink all", { kind: "primary", onClick: async () => { const [n, missing] = await call("files.relink_folder", { old_prefix: oldP.value, new_prefix: newP.value }); toast(`${n} files relinked; ${missing} still missing.`, n ? "success" : "info"); draw(false); } }));
  };
  draw(false).catch((e) => showError(e, body));
}

export async function history() {
  const list = await call("history");
  modal("Change history", h("div", h("p.muted", "Every change is an undo step, kept across sessions. Undo the latest step with Ctrl+Z."),
    h("table.table", h("tbody", list.map((x) => h("tr", h("td", x.label), h("td.small.muted", fmtDate(x.time)), h("td", x.state === "undone" ? h("span.badge.neutral", "Undone") : null)))))), { buttons: [{ label: "Close" }] });
}

export async function network() {
  const log = await call("network.log");
  modal("Network activity", h("div",
    h("p.muted", "Every outbound request this program made. Nothing is sent in the background, and there is no telemetry. Turn kinds of requests on or off in Settings › Network."),
    log.length ? h("table.table", h("thead", h("tr", h("th", "Time"), h("th", "Kind"), h("th", "Service"), h("th", "Sent"), h("th", "Result"))), h("tbody", log.map((l) => h("tr", h("td.small", fmtDate(l.time)), h("td", l.kind), h("td", l.host), h("td.small.mono", l.sent), h("td", l.ok ? h("span.status-ok", "OK") : h("span.status-review", l.detail || "Failed")))))) : h("div.empty", "No network requests have been made.")), { size: "wide", buttons: [{ label: "Close" }] });
}

export async function jobs() {
  const list = await call("jobs.list");
  modal("Background jobs", h("div", list.length ? h("ul.check-list", list.slice().reverse().map((j) => h("li", icon(j.ok ? "check" : "warn"), h("div", j.message)))) : h("div.empty", "No background jobs have run in this session.")),
    { buttons: [{ label: "Clear", run: async () => { await call("jobs.clear"); S.jobs = []; render(); } }, { label: "Close" }] });
}

export function shortcuts() {
  const rows = [
    ["Ctrl+K", "Command palette (every action)"], ["Ctrl+F", "Search in the current view"], ["Ctrl+Shift+F", "Search the library"], ["Ctrl+Shift+C", "Compose a citation"],
    ["Ctrl+N", "Add a source manually"], ["Ctrl+I", "Add by identifier"], ["Ctrl+O", "Import a file"], ["Ctrl+Shift+N", "New project"], ["Ctrl+H", "Find and replace"],
    ["Ctrl+Z / Ctrl+Y", "Undo / Redo"], ["Alt+1 … Alt+6", "Projects, Library, Reader, Notes, Cite, Connections"], ["F6 / Shift+F6", "Move between navigation, context bar and content"],
    ["Ctrl+B", "Collapse navigation"], ["F1", "Help"],
    ["Reader: V H U S C T D A E", "Select, Highlight, Underline, Strikeout, Comment, Text box, Draw, Area, Ellipse"],
    ["Reader: PageUp / PageDown", "Previous / next page"], ["Reader: Ctrl+= / Ctrl+-", "Zoom in / out"], ["Reader: Enter on a selection", "Create a mark with the active tool"],
    ["Lists: arrows, Home, End", "Move through rows; Space selects; Shift+arrows extends"],
  ];
  modal("Keyboard shortcuts", h("div", h("table.table", h("tbody", rows.map(([k, d]) => h("tr", h("td", h("span.kbd-key", k)), h("td", d))))), h("p.small.muted", "Print this list from the Help view.")), { buttons: [{ label: "Close" }] });
}

export function about() {
  modal("About Prism Reference Manager", h("div",
    h("img", { src: "logo.svg", alt: "Prism Reference Manager", style: { width: "100%", borderRadius: "8px" } }),
    h("p", `Version ${S.info?.version}. A local-first reference manager in the Prism research family.`),
    h("p", "Licensed under the Apache License 2.0. Third-party components and their licenses are listed in THIRD-PARTY.md in the install folder."),
    h("p.small.muted", "No account, subscription or telemetry. Your library stays in the folder you chose.")), { buttons: [{ label: "Close" }] });
}

export async function diagnostics() {
  let expanded = false;
  const pre = h("pre", { style: { maxHeight: "50vh", overflow: "auto", background: "var(--surface-2)", padding: "10px", whiteSpace: "pre-wrap" } });
  const draw = async () => (pre.textContent = JSON.stringify(await call("diagnostics.preview", { expanded }), null, 2));
  const cb = h("input", { type: "checkbox", id: "dg-exp", on: { change: (e) => { expanded = e.target.checked; draw(); } } });
  await draw();
  modal("Diagnostic bundle", h("div", h("p", "This is exactly what the bundle contains. It excludes library content, titles, notes and tokens. Nothing is sent: you choose where to save it."), h("label.row", { for: "dg-exp" }, cb, "Include full file paths"), pre),
    { size: "wide", buttons: [{ label: "Close" }, { label: "Save bundle…", kind: "primary", run: async () => { const r = await call("diagnostics.save", { expanded }); if (r && !r.cancelled) toast("Saved " + r.path, "success"); } }] });
}

export function pairRequest(p) {
  modal("Allow a local connection?", h("div",
    h("p", h("strong", p.name), " on this computer is asking to connect to your library."),
    h("p", "It would be allowed to:"), h("ul", p.scopes.map((s) => h("li", { read: "Read sources and search", format: "Format citations", write: "Add and edit sources and notes (marked as machine-created)", import: "Stage imports and look up identifiers", files: "Read and annotate attached files", export: "Export data" }[s] || s))),
    h("p.small.muted", "You can revoke it at any time in Settings › Local connections. Only programs on this computer can connect.")),
    { size: "narrow", buttons: [{ label: "Deny", run: () => call("api.deny", { id: p.id }) }, { label: "Allow", kind: "primary", run: () => call("api.approve", { id: p.id }) }] });
}

export async function fieldHistory(refId, field) {
  const list = await call("refs.field_history", { id: refId, field });
  modal(`History of ${fieldLabel(field)}`, h("div", list.length ? h("table.table", h("tbody", list.map((x) => h("tr", h("td.small", x.time ? fmtDate(x.time) : "Original"), h("td.small.muted", x.actor), h("td", typeof x.value === "string" ? x.value : JSON.stringify(x.value)),
    h("td", btn("Restore", { small: true, onClick: async () => { await call("refs.restore_field", { id: refId, field, value: x.value }); toast("Restored", "success"); document.querySelector(".modal-back")?.remove(); await refresh(); } })))))) : h("p.muted", "No changes recorded.")), { buttons: [{ label: "Close" }] });
}

export function outputPreview(out) {
  return h("div.output-box" + (out.paragraphs?.length ? ".bib" : ""), { html: out.paragraphs?.length ? out.paragraphs.map((p) => `<p style="padding-left:.5in;text-indent:-.5in">${runsHtml(p.runs)}</p>`).join("") : runsHtml(out.runs) });
}

export { manuscript };

// ---- term lists and refreshing records -----------------------------------------------------

export async function termLists() {
  let kind = "journal";
  const body = h("div");
  const draw = async () => {
    const [variants, terms] = await Promise.all([call("terms.variants", { kind }), call("terms.list", { kind })]);
    clear(body,
      select([["journal", "Journals"], ["publisher", "Publishers"], ["creator", "Authors and editors"], ["keyword", "Keywords"]], kind, { "aria-label": "Term list", on: { change: (e) => { kind = e.target.value; draw(); } } }),
      h("h3", `Possible variants (${variants.length} groups)`),
      variants.length ? h("div", variants.slice(0, 100).map((g) => {
        const pref = select(g.map((t) => [t.value, `${t.value} (${t.count})`]), g.sort((a, b) => b.count - a.count)[0].value, { "aria-label": "Preferred form" });
        return h("div.card.pad", { style: { marginBottom: "8px" } }, h("div.small.muted", g.map((t) => `“${t.value}” ×${t.count}`).join(" • ")),
          h("div.row", { style: { marginTop: "6px" } }, h("span", "Use:"), pref, btn("Merge variants", { small: true, kind: "primary", onClick: async () => {
            const n = await call("terms.merge", { kind, from: g.map((t) => t.value).filter((v) => v !== pref.value), to: pref.value });
            toast(`${n} sources updated in one undo step.`, "success");
            draw();
          } })));
      })) : h("p.muted", "No likely variants found."),
      h("details", h("summary", `All ${terms.length} values`), h("table.table", h("tbody", terms.slice(0, 1000).map((t) => h("tr", h("td", t.value), h("td.num", t.count)))))));
  };
  modal("Term lists", body, { size: "wide", buttons: [{ label: "Close" }] });
  draw().catch((e) => showError(e, body));
}

export async function refreshFromSource(refId) {
  const d = await call("refs.get", { id: refId });
  const r = d.ref;
  const kind = r.fields.doi ? "doi" : r.fields.pmid ? "pmid" : r.fields.arxiv ? "arxiv" : r.fields.isbn ? "isbn" : null;
  if (!kind) return toast("This source has no DOI, PubMed ID, arXiv ID or ISBN to refresh from.", "info");
  const res = await call("lookup.resolve", { kind, id: r.fields[kind] });
  if (!res.ok) return toast(res.message, "error");
  const c = res.candidate;
  const val = (x, k) => k === "creators" ? (x.creators || []).map((n) => n.name.literal || `${n.name.family}, ${n.name.given || ""}`).join("; ") : k === "issued" ? JSON.stringify(x.issued || null) : x.fields[k] || "";
  const keys = [...new Set(["creators", "issued", ...Object.keys(c.fields)])].filter((k) => val(c, k) && val(c, k) !== val(r, k));
  if (!keys.length) return toast("The online record matches this source.", "success");
  const boxes = {};
  const rows = keys.map((k) => {
    const userEdited = r.provenance?.[k]?.user_edited;
    const cb = h("input", { type: "checkbox", checked: !userEdited && !val(r, k), "aria-label": "Use new value for " + fieldLabel(k) });
    boxes[k] = cb;
    return h("tr", h("td", cb), h("td", h("strong", fieldLabel(k)), userEdited ? h("div.small.status-review", "You edited this field") : null), h("td.small", val(r, k) || h("em.muted", "empty")), h("td.small", val(c, k)));
  });
  const ok = await modal("Refresh from the online record", h("div",
    h("p", `Values from ${kind.toUpperCase()} ${r.fields[kind]}. Only the fields you tick are changed; the rest stay as they are. Each change keeps its earlier value in history.`),
    ...res.conflicts.map((x) => h("div.small.status-review", icon("warn", 13), " " + x)),
    h("table.table", h("thead", h("tr", h("th", "Use"), h("th", "Field"), h("th", "Current"), h("th", "Online record"))), h("tbody", rows))),
    { size: "wide", buttons: [{ label: "Cancel" }, { label: "Apply ticked fields", kind: "primary", run: async () => {
      const nr = structuredClone(r);
      for (const k of keys) if (boxes[k].checked) {
        if (k === "creators") nr.creators = c.creators;
        else if (k === "issued") nr.issued = c.issued;
        else nr.fields[k] = c.fields[k];
      }
      nr.notices = [...(r.notices || []), ...(c.notices || []).filter((n) => !(r.notices || []).some((o) => o.url === n.url))];
      return call("refs.update", { ref: nr, source: "lookup:" + kind });
    } }] }).done;
  if (ok) { toast("Source refreshed", "success"); await refresh(); }
}
