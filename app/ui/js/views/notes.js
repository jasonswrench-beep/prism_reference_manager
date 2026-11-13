// Notes and evidence: manuscript outline, typed knowledge items, editor with source links.

import { call } from "../api.js";
import { h, clear, icon, iconBtn, btn, toast, modal, confirmDialog, promptDialog, select, field, showError, fmtDate } from "../dom.js";
import { S, registerView, go, manuscript } from "../app.js";
import * as dialogs from "../dialogs.js";

const KIND_LABEL = { quotation: "Direct quotation", paraphrase: "Paraphrase", summary: "Summary", comment: "My interpretation", question: "Research question", task: "Task", evidence: "Evidence statement", thought: "Thought", note: "Note" };
const KIND_ICON = { quotation: "quote", question: "question", task: "check", evidence: "checkCircle", summary: "list" };
const N = { section: "", filterKind: "", sort: "newest", selected: "", search: "", libraryWide: false };

registerView("notes", {
  async render(el, params) {
    if (!S.project) {
      el.append(h("h1.view-title", { tabindex: "-1" }, "Notes and evidence"), h("div.empty", h("h3", "Choose a project"), h("p", "Notes belong to a project by default so they stay private to that work."), btn("Go to projects", { kind: "primary", onClick: () => go("projects") })));
      return;
    }
    const pid = S.project.project.id;
    const [notes, outline] = await Promise.all([call("notes.list", { project_id: pid, include_library: true }), call("outline.get", { project_id: pid })]);
    const refIds = [...new Set(notes.flatMap((n) => n.ref_ids))];
    const labels = refIds.length ? await sourceLabels(refIds, notes) : {};
    if (params.note) N.selected = params.note;
    const searchBox = h("input", { type: "search", value: N.search, placeholder: "Search notes and evidence", "aria-label": "Search notes", "data-search": "1" });
    searchBox.addEventListener("input", () => { N.search = searchBox.value; drawItems(); });
    el.append(h("div.row", { style: { marginBottom: "12px" } }, h("h1.view-title.grow", { tabindex: "-1" }, "Notes and evidence"), h("div", { style: { width: "420px" } }, searchBox),
      btn("New note", { kind: "primary", icon: "plus", onClick: () => newNote() })));
    const outlinePane = h("div.pane", { style: { display: "flex", flexDirection: "column" } });
    const itemsPane = h("div.pane", { style: { display: "flex", flexDirection: "column" } });
    const editorPane = h("div.pane", { style: { display: "flex", flexDirection: "column" } });
    el.append(h("div.panes", { style: { gridTemplateColumns: "300px minmax(0, 1fr) minmax(0, 1.1fr)", height: "calc(100vh - 230px)" } }, outlinePane, itemsPane, editorPane));
    el.append(h("div.btn-row", { style: { marginTop: "12px" } },
      btn("Export outline", { icon: "download", onClick: () => exportNotes(true) }),
      btn("Export notes", { icon: "download", onClick: () => exportNotes(false) }),
      btn("Exchange file for Prism research tools", { icon: "upload", onClick: async () => { const r = await call("family.export", { project_id: pid }); if (r && !r.cancelled) toast("Saved " + r.path + ". This is a file export; nothing is synchronized.", "success"); } }),
      h("span.small.muted", "Direct sending to other Prism tools is planned for a later version; the exchange file works now.")));

    // ---- outline ----------------------------------------------------------------------------
    const flatSections = [];
    const walk = (nodes, depth) => nodes.forEach((n) => { flatSections.push({ ...n.section, depth }); walk(n.children, depth + 1); });
    walk(outline, 0);
    function drawOutline() {
      clear(outlinePane);
      outlinePane.append(h("div.card-head", h("h3", "Manuscript outline"), h("div.row", iconBtn("plus", "Add section", () => addSection("")))));
      const tree = h("ul.tree", { role: "tree", "aria-label": "Manuscript outline" });
      tree.append(h("li", { role: "treeitem", "aria-selected": String(N.section === ""), tabindex: N.section === "" ? "0" : "-1" }, h("div.ti", { on: { click: () => { N.section = ""; drawOutline(); drawItems(); } } }, icon("notes", 17), "All notes", h("span.count", notes.length))));
      const render = (nodes, depth) => nodes.map((n) => {
        const s = n.section;
        const li = h("li", { role: "treeitem", "aria-selected": String(N.section === s.id), tabindex: N.section === s.id ? "0" : "-1", "aria-level": String(depth + 1), "data-sid": s.id, draggable: "true" },
          h("div.ti", { on: { click: () => { N.section = s.id; drawOutline(); drawItems(); } } }, icon("doc", 17), h("span.ellipsis", s.title), h("span.count", s.note_ids.length),
            iconBtn("dots", "Section actions", (e) => { e.stopPropagation(); sectionMenu(s); })));
        if (n.children.length) li.append(h("ul", { role: "group" }, render(n.children, depth + 1)));
        li.addEventListener("keydown", async (e) => {
          if (e.target !== li) return;
          // Keyboard equivalents of drag: Alt+Up/Down reorder, Alt+Right indent, Alt+Left outdent.
          const siblings = flatSections.filter((x) => x.parent_id === s.parent_id).sort((a, b) => a.position - b.position);
          const i = siblings.findIndex((x) => x.id === s.id);
          if (e.altKey && e.key === "ArrowUp" && i > 0) { e.preventDefault(); await call("outline.move_section", { id: s.id, parent_id: s.parent_id, position: i - 1 }); go("notes"); }
          else if (e.altKey && e.key === "ArrowDown" && i < siblings.length - 1) { e.preventDefault(); await call("outline.move_section", { id: s.id, parent_id: s.parent_id, position: i + 1 }); go("notes"); }
          else if (e.altKey && e.key === "ArrowRight" && i > 0) { e.preventDefault(); await call("outline.move_section", { id: s.id, parent_id: siblings[i - 1].id, position: 999 }); go("notes"); }
          else if (e.altKey && e.key === "ArrowLeft" && s.parent_id) { e.preventDefault(); const parent = flatSections.find((x) => x.id === s.parent_id); await call("outline.move_section", { id: s.id, parent_id: parent.parent_id, position: parent.position + 1 }); go("notes"); }
          else if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); const all = [...tree.querySelectorAll('[role="treeitem"]')]; const j = all.indexOf(li); all[j + (e.key === "ArrowDown" ? 1 : -1)]?.focus(); }
          else if (e.key === "Enter") { N.section = s.id; drawOutline(); drawItems(); }
        });
        li.addEventListener("dragstart", (e) => { e.stopPropagation(); e.dataTransfer.setData("text/section", s.id); });
        li.addEventListener("dragover", (e) => e.preventDefault());
        li.addEventListener("drop", async (e) => {
          e.preventDefault(); e.stopPropagation();
          const noteId = e.dataTransfer.getData("text/note");
          const fromSec = e.dataTransfer.getData("text/from");
          const secId = e.dataTransfer.getData("text/section");
          if (noteId) { await call("outline.file_note", { section_id: s.id, note_id: noteId, from_section: fromSec || "" }); go("notes"); }
          else if (secId && secId !== s.id) { await call("outline.move_section", { id: secId, parent_id: s.parent_id, position: s.position }); go("notes"); }
        });
        return li;
      });
      tree.append(...render(outline, 0));
      tree.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") { const all = [...tree.querySelectorAll('[role="treeitem"]')]; const j = all.indexOf(document.activeElement); if (j >= 0) { all[j + (e.key === "ArrowDown" ? 1 : -1)]?.focus(); e.preventDefault(); } } });
      outlinePane.append(h("div", { style: { overflow: "auto", flex: "1" } }, tree),
        h("p.small.muted", { style: { padding: "0 12px 10px" } }, "Drag notes onto sections, or select a note and use “File in section”. Keyboard: Alt+arrows move a section."));
    }
    async function addSection(parent) {
      const t = await promptDialog("New section", "Section title");
      if (!t) return;
      await call("outline.save_section", { section: { project_id: pid, manuscript_id: manuscript()?.id || "", parent_id: parent, title: t } });
      go("notes");
    }
    async function sectionMenu(s) {
      const a = await modal(s.title, h("p.muted", `${s.note_ids.length} notes filed here. One note can be filed under several sections.`), { size: "narrow", buttons: [
        { label: "Rename", value: "rename" }, { label: "Add sub-section", value: "sub" }, { label: "Delete section", value: "delete", kind: "danger" }] }).done;
      if (a === "rename") { const t = await promptDialog("Rename section", "Title", s.title); if (t) await call("outline.save_section", { section: { ...s, title: t } }); }
      if (a === "sub") await addSection(s.id);
      if (a === "delete" && await confirmDialog("Delete section", "Delete this section? Its notes stay in the project; sub-sections move up a level.", { ok: "Delete", kind: "danger" })) await call("outline.delete_section", { id: s.id });
      if (a) go("notes");
    }

    // ---- items ------------------------------------------------------------------------------
    function visible() {
      let list = notes.filter((n) => N.libraryWide || n.project_id === pid || !n.project_id);
      if (N.section) { const sec = flatSections.find((s) => s.id === N.section); list = sec ? sec.note_ids.map((id) => notes.find((n) => n.id === id)).filter(Boolean) : []; }
      if (N.filterKind) list = list.filter((n) => n.kind === N.filterKind || (N.filterKind === "machine" && ["ai", "agent"].includes(n.origin)));
      if (N.search) { const q = N.search.toLowerCase(); list = list.filter((n) => (n.title + " " + n.body + " " + n.exact_text).toLowerCase().includes(q)); }
      if (N.sort === "oldest") list = [...list].reverse();
      return list;
    }
    function drawItems() {
      clear(itemsPane);
      const list = visible();
      const kindSel = select([["", "All types"], ...Object.entries(KIND_LABEL), ["machine", "Machine-created"]], N.filterKind, { "aria-label": "Filter by type", style: { width: "150px" }, on: { change: (e) => { N.filterKind = e.target.value; drawItems(); } } });
      const sortSel = select(N.section ? [["newest", "Outline order"]] : [["newest", "Newest"], ["oldest", "Oldest"]], N.sort, { "aria-label": "Sort", style: { width: "130px" }, on: { change: (e) => { N.sort = e.target.value; drawItems(); } } });
      itemsPane.append(h("div.card-head", h("h3", `Evidence items (${list.length})`), h("div.row", kindSel, sortSel)));
      const box = h("div", { style: { overflow: "auto", padding: "10px", flex: "1" }, role: "listbox", "aria-label": "Notes" });
      for (const n of list) {
        const src = n.ref_ids.map((id) => labels[id]).filter(Boolean).join("; ");
        const card = h("div.note-card.kind-" + n.kind + (["ai", "agent"].includes(n.origin) ? ".origin-ai" : ""), { role: "option", tabindex: "0", "aria-selected": String(N.selected === n.id), draggable: "true", on: { click: () => { N.selected = n.id; drawItems(); drawEditor(); }, keydown: (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); N.selected = n.id; drawItems(); drawEditor(); } } } },
          h("header", icon(KIND_ICON[n.kind] || "notes", 18), KIND_LABEL[n.kind] || n.kind, ["ai", "agent"].includes(n.origin) ? h("span.badge.neutral", `${n.origin === "ai" ? "AI" : "Agent"}-created, not yet accepted`) : null, h("span.grow"), n.project_id ? null : h("span.small.muted", "Library-wide")),
          n.exact_text ? h("div.body", { style: { fontSize: "15px" } }, "“" + n.exact_text + "”", n.exact_text_edited ? h("div.small.status-review", "Edited after extraction") : null) : null,
          n.body ? h("div.body", n.body.slice(0, 400)) : null,
          (src || n.anchor_state === "unresolved" || n.tags?.length) ? h("footer", src ? [icon("doc", 15), src, n.locator ? ` • ${n.locator_label === "paragraph" ? "para." : "p."} ${n.locator}` : ""] : null,
            n.anchor_state === "unresolved" && ["quotation", "paraphrase", "evidence"].includes(n.kind) ? h("span.status-review", icon("warn", 13), " source or locator unresolved") : null,
            ...(n.tags || []).map((t) => h("span.chip", icon("tag", 12), t))) : null);
        card.addEventListener("dragstart", (e) => { e.dataTransfer.setData("text/note", n.id); e.dataTransfer.setData("text/from", N.section || ""); });
        box.append(card);
      }
      if (!list.length) box.append(h("div.empty", N.section ? "No notes filed in this section yet. Drag a note here, or use File in section." : "No notes yet. Create one, or add marks in the Reader and choose Add to outline."));
      itemsPane.append(box);
    }

    // ---- editor -------------------------------------------------------------------------------
    async function drawEditor() {
      clear(editorPane);
      const n = notes.find((x) => x.id === N.selected);
      if (!n) { editorPane.append(h("div.empty", "Select a note to edit it.")); return; }
      const title = h("input", { value: n.title, placeholder: "Title (optional)", "aria-label": "Note title" });
      const kind = select(Object.entries(KIND_LABEL), n.kind, { "aria-label": "Note type" });
      const scope = select([[pid, "This project: " + S.project.project.title], ["", "Library-wide (visible in every project)"]], n.project_id, { "aria-label": "Note scope" });
      const exact = h("textarea", { rows: 4, value: n.exact_text, "aria-label": "Exact source text" });
      const body = h("textarea", { rows: 8, value: n.body, "aria-label": "Note text", placeholder: "Your own words. Markdown: **bold**, *italic*, - lists, [link](url), @citekey" });
      const fmt = (pre, post = pre) => { const s = body.selectionStart, e2 = body.selectionEnd; body.setRangeText(pre + body.value.slice(s, e2) + post, s, e2, "end"); body.focus(); };
      const locLabel = select([["page", "Page"], ["paragraph", "Paragraph"], ["chapter", "Chapter"], ["section", "Section"], ["figure", "Figure"], ["table", "Table"], ["timestamp", "Time stamp"]], n.locator_label || "page", { "aria-label": "Locator type" });
      const loc = h("input", { value: n.locator, "aria-label": "Locator", style: { width: "110px" } });
      const noLoc = h("input", { type: "checkbox", id: "noloc", checked: n.anchor_state === "no-locator" });
      const srcBox = h("div");
      const drawSources = () => {
        clear(srcBox, ...n.ref_ids.map((id) => h("span.chip", { style: { margin: "0 6px 6px 0" } }, icon("doc", 14), labels[id] || id, h("button", { type: "button", "aria-label": "Remove source link", on: { click: () => { n.ref_ids = n.ref_ids.filter((x) => x !== id); drawSources(); } } }, icon("x", 14)))));
      };
      drawSources();
      const save = async () => {
        try {
          const saved = await call("notes.save", { note: { ...n, title: title.value, kind: kind.value, project_id: scope.value, exact_text: kind.value === "quotation" ? exact.value : n.exact_text && kind.value !== "quotation" ? "" : exact.value, body: body.value, locator_label: locLabel.value, locator: loc.value.trim(), anchor_state: noLoc.checked ? "no-locator" : "" } });
          Object.assign(n, saved);
          toast("Note saved", "success");
          drawItems();
          drawEditor();
        } catch (e) { showError(e, editorPane); }
      };
      editorPane.append(
        h("div.card-head", h("h3", n.title || KIND_LABEL[n.kind] || "Note"), iconBtn("trash", "Delete note", async () => { await call("notes.trash", { id: n.id }); N.selected = ""; go("notes"); })),
        h("div", { style: { padding: "12px", overflow: "auto", flex: "1" } },
          ["ai", "agent"].includes(n.origin) ? h("div.notice.warn", icon("beaker"), h("div", `Created by ${n.origin === "ai" ? "an AI provider" : "a connected tool"} (${n.origin_detail || "unknown"}). It stays separate from your own items until you accept it.`, h("div.btn-row", btn("Accept as my note", { small: true, onClick: async () => { await call("notes.accept", { id: n.id }); go("notes"); } })))) : null,
          h("div.grid2", field("Type", kind), field("Scope", scope)),
          field("Title", title),
          kind.value === "quotation" || n.exact_text ? field("Exact source text", exact, n.exact_text_edited ? "This quotation was edited after it was captured." : "Kept exactly as captured. Your interpretation goes below, never inside quotation marks.") : null,
          h("div.field", h("div.label", "Note"), h("div.toolbar", { style: { marginBottom: "4px" } }, iconBtn("text", "Bold", () => fmt("**")), iconBtn("edit", "Italic", () => fmt("*")), iconBtn("list", "List item", () => fmt("\n- ", "")), iconBtn("link", "Link", () => fmt("[", "](https://)"))), body),
          h("div.field", h("div.label", "Source links"), srcBox, h("div.btn-row", btn("Link a source…", { small: true, icon: "plus", onClick: async () => { const id = await pickSource(); if (id && !n.ref_ids.includes(id)) { n.ref_ids.push(id); Object.assign(labels, await sourceLabels([id], [])); drawSources(); } } }))),
          h("div.row", field("Locator", locLabel), h("div.field", h("label", { for: "loc-v" }, "Value"), Object.assign(loc, { id: "loc-v" })), h("label.row.small", { for: "noloc", style: { marginTop: "18px" } }, noLoc, "No page or locator exists")),
          h("div.btn-row", { style: { marginTop: "10px" } },
            btn("Save", { kind: "primary", icon: "save", onClick: save }),
            n.annotation_id ? btn("Open passage", { icon: "external", onClick: () => go("reader", { ref: n.ref_ids[0] }) }) : null,
            n.ref_ids.length ? btn(n.exact_text ? "Copy quote with citation" : "Copy citation", { icon: "quote", onClick: async () => {
              try {
                if (n.exact_text) { const r = await call("cite.quote_note", { note_id: n.id, manuscript_id: manuscript()?.id || "" }); toast("Copied: " + r.output.text.slice(0, 140), "success"); }
                else { const r = await call("cite.copy", { request: { manuscript_id: manuscript()?.id || "", items: n.ref_ids.map((id, i) => ({ ref_id: id, locator_label: i === 0 ? n.locator_label : "", locator: i === 0 ? n.locator : "" })), mode: "parenthetical" } }); toast("Copied: " + r.output.text, "success"); }
              } catch (e) { showError(e); }
            } }) : null,
            flatSections.length ? btn("File in section…", { icon: "list", onClick: async () => {
              const sel = select(flatSections.map((s) => [s.id, "  ".repeat(s.depth) + s.title]), flatSections[0].id);
              const r = await modal("File in section", field("Section", sel), { size: "narrow", buttons: [{ label: "Cancel" }, { label: "File", kind: "primary", run: () => call("outline.file_note", { section_id: sel.value, note_id: n.id }) }] }).done;
              if (r !== null) go("notes");
            } }) : null,
            N.section ? btn("Remove from this section", { onClick: async () => { await call("outline.unfile_note", { section_id: N.section, note_id: n.id }); go("notes"); } }) : null),
          h("p.small.muted", `Created ${fmtDate(n.created)} • updated ${fmtDate(n.modified)} • origin: ${n.origin || "user"}`)));
    }

    async function newNote(forRef) {
      const n = await call("notes.save", { note: { kind: forRef ? "comment" : "note", project_id: pid, title: "", body: "", ref_ids: forRef ? [forRef] : [] } });
      notes.unshift(n);
      if (forRef) Object.assign(labels, await sourceLabels([forRef], []));
      N.selected = n.id;
      if (N.section) await call("outline.file_note", { section_id: N.section, note_id: n.id });
      drawItems();
      drawEditor();
      setTimeout(() => editorPane.querySelector("textarea")?.focus(), 50);
    }
    async function exportNotes(outlineOnly) {
      const fmt = select([["docx", "DOCX document (.docx)"], ["md", "Markdown"], ["html", "HTML"], ["rtf", "Rich Text (.rtf)"], ["json", "Structured JSON"]], "docx");
      const r = await modal(outlineOnly ? "Export outline" : "Export notes", h("div", field("Format", fmt), h("p.small.muted", "Quotations carry their citations in the active manuscript's style, followed by a reference list.")),
        { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Export", kind: "primary", run: () => call("notes.export", { project_id: pid, manuscript_id: manuscript()?.id || "", outline_only: outlineOnly, format: fmt.value }) }] }).done;
      if (r && !r.cancelled) toast("Saved " + r.path, "success");
    }

    drawOutline();
    drawItems();
    drawEditor();
    if (params.newFor) { const f = params.newFor; params.newFor = null; newNote(f); }
  },
});

async function sourceLabels(ids) {
  if (!ids.length) return {};
  try { return await call("refs.labels", { ids }); } catch (_) { return {}; }
}

export async function pickSource() {
  const input = h("input", { type: "search", placeholder: "Search by author, title, year…", "aria-label": "Search sources" });
  const list = h("div", { style: { maxHeight: "50vh", overflow: "auto" } });
  let chosen = null;
  let m;
  const run = async () => {
    const q = input.value.trim();
    const r = await call("refs.list", { query: { search: q, project_id: q ? "" : S.project?.project.id || "", limit: 40 } });
    clear(list, ...r.rows.map((row) => h("button.btn.ghost", { type: "button", style: { display: "block", width: "100%", textAlign: "left", whiteSpace: "normal", fontWeight: 400 }, on: { click: () => { chosen = row.id; m.close(row.id); } } }, h("strong", row.authors || "—"), ` (${row.year || "n.d."}) `, row.title)));
    if (!r.rows.length) list.append(h("p.muted", "No matching sources."));
  };
  let t;
  input.addEventListener("input", () => { clearTimeout(t); t = setTimeout(run, 200); });
  m = modal("Choose a source", h("div", input, list), { size: "narrow" });
  run();
  await m.done;
  return chosen;
}
