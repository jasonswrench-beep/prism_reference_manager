// Library: collection tree, source list and inspector.

import { call } from "../api.js";
import { h, clear, icon, iconBtn, btn, toast, modal, confirmDialog, promptDialog, select, field, showError, fmtDate, runsHtml, tabs, announce } from "../dom.js";
import { S, registerView, go, refresh, manuscript } from "../app.js";
import * as dialogs from "../dialogs.js";
import { sourceEditor, fieldLabel } from "../editor.js";

const st = { scope: "project", view: "all", collection: "", label: "", search: "", sort: "authors", desc: false, selected: new Set(), focus: null, inspectorTab: "details", bottomTab: "abstract", layout: null, offset: 0, filters: [], showFilters: false };

// Columns the table can show. `sort` names the list sort key; `w` is the default width in pixels
// (the title column takes the remaining space).
const COLS = {
  authors: { label: "Author", sort: "authors", w: 190 },
  year: { label: "Year", sort: "year", w: 64 },
  title: { label: "Title", sort: "title", w: 0 },
  container: { label: "Published in", sort: "container", w: 190 },
  type: { label: "Type", sort: "type", w: 130 },
  citekey: { label: "Citation key", sort: "citekey", w: 150 },
  rating: { label: "Rating", sort: "rating", w: 84 },
  labels: { label: "Labels", w: 150 },
  created: { label: "Added", sort: "created", w: 104 },
  modified: { label: "Modified", sort: "modified", w: 104 },
  reading: { label: "Read", w: 52 },
  pdf: { label: "PDF", w: 52 },
};
const DEFAULT_COLS = ["authors", "year", "title", "reading", "pdf"];
const FILTER_FIELDS = [["type", "Type"], ["year", "Year"], ["label", "Label"], ["pdf", "PDF"], ["reading", "Reading"]];

// Library layout preferences live in settings.layout.library; named views in settings.layouts.library.
const prefs = () => S.settings.layout?.library || {};
let prefTimer = null;
function savePrefs(patch, now = false) {
  const lib = { ...prefs(), ...patch };
  S.settings.layout = { ...(S.settings.layout || {}), library: lib };
  clearTimeout(prefTimer);
  const go2 = () => call("settings.set", { settings: { layout: S.settings.layout } }).then((x) => { S.settings = x; }).catch(() => {});
  if (now) go2(); else prefTimer = setTimeout(go2, 400);
}
const savedViews = () => S.settings.layouts?.library || [];
async function saveViews(list) {
  S.settings = await call("settings.set", { settings: { layouts: { ...(S.settings.layouts || {}), library: list } } });
}
const colList = () => (prefs().columns?.length ? prefs().columns : DEFAULT_COLS.map((k) => ({ key: k }))).filter((c) => COLS[c.key]);
const colWidth = (c) => c.width || COLS[c.key].w;

/** A small anchored panel that closes on Escape or a click outside. */
function popover(anchor, content) {
  document.querySelector(".popover")?.remove();
  const r = anchor.getBoundingClientRect();
  const pop = h("div.popover", { role: "dialog", style: { position: "fixed", top: `${Math.round(r.bottom + 4)}px`, left: `${Math.round(Math.min(r.left, window.innerWidth - 300))}px`, zIndex: 50 } }, content);
  const close = () => { pop.remove(); document.removeEventListener("mousedown", outside, true); anchor.focus(); };
  const outside = (e) => { if (!pop.contains(e.target) && e.target !== anchor) close(); };
  pop.addEventListener("keydown", (e) => { if (e.key === "Escape") { e.preventDefault(); close(); } });
  document.addEventListener("mousedown", outside, true);
  document.body.append(pop);
  pop.querySelector("button, input")?.focus();
  return close;
}

/** A draggable, keyboard-adjustable divider. `get`/`set` work in pixels. */
function splitter(label, orientation, get, set, min, max, done) {
  const el = h("div.splitter" + (orientation === "horizontal" ? ".h" : ""), { role: "separator", tabindex: "0", "aria-label": label, "aria-orientation": orientation === "horizontal" ? "horizontal" : "vertical", "aria-valuemin": String(min), "aria-valuemax": String(max) });
  const sync = () => el.setAttribute("aria-valuenow", String(Math.round(get())));
  sync();
  const apply = (v) => { set(Math.max(min, Math.min(max, v))); sync(); };
  el.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    const start = orientation === "horizontal" ? e.clientY : e.clientX;
    const base = get();
    const sign = el.dataset.invert ? -1 : 1;
    const move = (ev) => apply(base + sign * ((orientation === "horizontal" ? ev.clientY : ev.clientX) - start));
    const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); done(); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  });
  el.addEventListener("keydown", (e) => {
    const sign = el.dataset.invert ? -1 : 1;
    const step = e.shiftKey ? 64 : 16;
    const back = orientation === "horizontal" ? "ArrowUp" : "ArrowLeft", fwd = orientation === "horizontal" ? "ArrowDown" : "ArrowRight";
    if (e.key === back) { apply(get() - sign * step); done(); e.preventDefault(); }
    if (e.key === fwd) { apply(get() + sign * step); done(); e.preventDefault(); }
  });
  return el;
}

// A click-to-build search: rows of field, condition and value, combined with all or any.
const BUILD_FIELDS = [["", "Anywhere", "text"], ["title", "Title", "text"], ["author", "Author", "text"], ["journal", "Published in", "text"], ["abstract", "Abstract", "text"],
  ["note", "Notes", "text"], ["annotation", "Annotations", "text"], ["fulltext", "PDF text", "text"], ["tag", "Tag or keyword", "text"], ["doi", "DOI", "text"],
  ["year", "Year", "number"], ["date", "Date", "date"], ["added", "Added", "date"], ["modified", "Modified", "date"], ["rating", "Rating", "number"],
  ["type", "Type", "type"], ["label", "Label", "text"], ["collection", "Collection", "text"], ["has", "Has", "has"]];
function searchBuilder(apply) {
  const rows = [];
  const box = h("div");
  const quote = (v) => (/[\s:()]/.test(v) ? `"${v.replace(/"/g, "")}"` : v);
  const addRow = () => {
    const fieldSel = select(BUILD_FIELDS.map(([k, l]) => [k, l]), "", { "aria-label": "Field" });
    const opSel = h("select", { "aria-label": "Condition" });
    const v1 = h("input", { "aria-label": "Value", placeholder: "words" });
    const v2 = h("input", { "aria-label": "Second value", placeholder: "to", hidden: true, style: { width: "110px" } });
    const valueSel = h("select", { "aria-label": "Value", hidden: true });
    const sync = () => {
      const kind = BUILD_FIELDS.find((f) => f[0] === fieldSel.value)[2];
      const ops = kind === "text" ? [["contains", "contains"], ["phrase", "contains the phrase"], ["not", "does not contain"]]
        : kind === "number" || kind === "date" ? [["is", "is"], ["between", "is between"], [">=", "is on or after"], ["<=", "is on or before"], ["not", "is not"]]
        : [["is", "is"], ["not", "is not"]];
      const keep = opSel.value;
      clear(opSel, ...ops.map(([k, l]) => h("option", { value: k }, l)));
      if (ops.some((o) => o[0] === keep)) opSel.value = keep;
      const choice = kind === "type" || kind === "has";
      valueSel.hidden = !choice; v1.hidden = choice;
      if (kind === "type") clear(valueSel, ...S.types.map((t) => h("option", { value: t.id }, t.label)));
      if (kind === "has") clear(valueSel, ...[["pdf", "a PDF"], ["doi", "a DOI"], ["notes", "notes"], ["conflict", "a metadata conflict"], ["notice", "a retraction or correction notice"]].map(([k, l]) => h("option", { value: k }, l)));
      v1.placeholder = kind === "date" ? "2021-03 or 2021-03-15" : kind === "number" ? (fieldSel.value === "rating" ? "1 to 5" : "2020") : "words";
      v2.hidden = opSel.value !== "between";
    };
    fieldSel.addEventListener("change", sync); opSel.addEventListener("change", sync);
    const row = { fieldSel, opSel, v1, v2, valueSel };
    const el = h("div.row", { style: { gap: "6px", marginBottom: "6px", flexWrap: "wrap" } }, fieldSel, opSel, v1, v2, valueSel, iconBtn("trash", "Remove condition", () => { rows.splice(rows.indexOf(row), 1); el.remove(); }));
    row.el = el; rows.push(row); box.append(el); sync(); v1.focus();
  };
  const toQuery = (r) => {
    const f = r.fieldSel.value, op = r.opSel.value, kind = BUILD_FIELDS.find((x) => x[0] === f)[2];
    const val = kind === "type" || kind === "has" ? r.valueSel.value : r.v1.value.trim();
    if (!val) return "";
    const pre = f ? f + ":" : "";
    if (kind === "number" || kind === "date") {
      const v = op === "between" ? `${val}..${r.v2.value.trim()}` : op === ">=" ? `>=${val}` : op === "<=" ? `<=${val}` : val;
      return (op === "not" ? "-" : "") + pre + v;
    }
    if (op === "phrase") return pre + `"${val.replace(/"/g, "")}"`;
    if (op === "not") return "-" + pre + quote(val);
    if (!f) return val;
    return val.split(/\s+/).map((w) => pre + quote(w)).join(" ");
  };
  const match = select([["all", "all of these"], ["any", "any of these"]], "all", { "aria-label": "Combine conditions" });
  addRow();
  modal("Build a search", h("div", h("div.row.small", { style: { marginBottom: "8px" } }, "Find sources that match ", match), box,
    btn("Add condition", { small: true, icon: "plus", onClick: addRow }),
    h("p.small.muted", "The search is written into the search box, where you can see and edit it. Spelling mistakes in single words are corrected when nothing matches.")),
    { buttons: [{ label: "Cancel" }, { label: "Search", kind: "primary", run: () => {
      const parts = rows.map(toQuery).filter(Boolean);
      if (!parts.length) return;
      apply(match.value === "any" ? parts.map((p) => (p.includes(" ") && !p.startsWith('"') ? `(${p})` : p)).join(" OR ") : parts.join(" "));
      return true;
    } }] });
}

function anchorReadingIcon(r) {
  return r.reading === "read" ? icon("check", 18, "Read") : r.reading === "reading" ? icon("reader", 18, "Reading") : icon("reader", 18, "Not read");
}

registerView("library", {
  async render(el, params) {
    if (params.view) { st.view = params.view; st.scope = params.view === "trash" ? "library" : st.scope; }
    if (params.select) { st.selected = new Set([params.select]); st.focus = params.select; }
    if (params.search !== undefined) { st.search = params.search; st.collection = ""; st.label = ""; st.view = "all"; st.scope = "library"; st.offset = 0; if (!st.sortChosen) { st.sortBefore = st.sort === "relevance" ? st.sortBefore : st.sort; st.sort = "relevance"; } }
    if (params.label) { st.label = params.label; st.collection = ""; st.search = ""; st.view = "all"; st.scope = "library"; st.offset = 0; }
    if (params.collection) { st.collection = params.collection; st.label = ""; st.search = ""; st.view = "all"; st.offset = 0; }
    if (!S.project) st.scope = "library";
    const tree = h("div.pane", { style: { padding: "6px" } });
    const listPane = h("div", { style: { display: "flex", flexDirection: "column", gap: "10px", minWidth: 0, minHeight: 0 } });
    const inspector = h("div.pane", { style: { padding: "0" } });
    const pane = { tree: prefs().panes?.tree || 230, inspector: prefs().panes?.inspector || 380, bottom: prefs().panes?.bottom || 230 };
    const grid = h("div.panes.resizable");
    const layoutGrid = () => { grid.style.gridTemplateColumns = `${pane.tree}px 6px minmax(0, 1fr) 6px ${pane.inspector}px`; };
    const savePanes = () => savePrefs({ panes: { ...pane } });
    const treeSplit = splitter("Resize collections pane", "vertical", () => pane.tree, (v) => { pane.tree = v; layoutGrid(); }, 160, 480, savePanes);
    const inspSplit = splitter("Resize details pane", "vertical", () => pane.inspector, (v) => { pane.inspector = v; layoutGrid(); }, 260, 720, savePanes);
    inspSplit.dataset.invert = "1";
    layoutGrid();
    grid.append(tree, treeSplit, listPane, inspSplit, inspector);
    el.append(grid);
    const [collections, labels, saved] = await Promise.all([call("collections.list"), call("labels.list"), call("searches.list")]);

    // ---- tree --------------------------------------------------------------------------------
    const treeItem = (label, ic, key, count, children) => {
      const sel = (key.type === "view" && st.collection === "" && st.label === "" && st.view === key.value && st.scope === key.scope) || (key.type === "collection" && st.collection === key.value) || (key.type === "label" && st.label === key.value);
      const li = h("li", { role: "treeitem", "aria-selected": String(sel), tabindex: sel ? "0" : "-1" },
        h("div.ti", { on: { click: () => pick(key) } }, icon(ic, 18), h("span.ellipsis", label), count !== undefined ? h("span.count", count) : null,
          key.type === "collection" ? iconBtn("dots", "Collection actions", (e) => { e.stopPropagation(); collectionMenu(key.c); }) : null));
      li.addEventListener("keydown", (e) => {
        const items = [...tree.querySelectorAll('[role="treeitem"]')];
        const i = items.indexOf(li);
        if (e.key === "ArrowDown") { items[i + 1]?.focus(); e.preventDefault(); }
        if (e.key === "ArrowUp") { items[i - 1]?.focus(); e.preventDefault(); }
        if (e.key === "Enter" || e.key === " ") { pick(key); e.preventDefault(); }
      });
      if (children?.length) li.append(h("ul", { role: "group" }, children));
      return li;
    };
    const pick = (key) => {
      st.offset = 0;
      st.collection = ""; st.label = "";
      if (key.type === "view") { st.view = key.value; st.scope = key.scope; }
      if (key.type === "collection") { st.collection = key.value; st.view = "all"; }
      if (key.type === "label") { st.label = key.value; st.view = "all"; }
      drawTree(); loadList();
    };
    const buildCollections = (parent) => collections.filter((c) => (c.parent_id || "") === parent && (!c.project_id || c.project_id === S.project?.project.id)).sort((a, b) => a.name.localeCompare(b.name))
      .map((c) => treeItem(c.name, c.query ? "search" : "folder", { type: "collection", value: c.id, c }, c.query ? undefined : c.ref_ids.length, buildCollections(c.id)));
    const counts = { project: S.project?.sources, all: S.info.library.counts.references, trash: S.info.library.counts.trash };
    const drawTree = () => {
      clear(tree);
      const ul = h("ul.tree", { role: "tree", "aria-label": "Collections" });
      if (S.project) {
        ul.append(h("li", h("div.ti", { style: { fontWeight: 700 } }, icon("folder", 18), "Project library")),
          treeItem("All sources", "doc", { type: "view", value: "all", scope: "project" }, counts.project),
          treeItem("To read", "reader", { type: "view", value: "to_read", scope: "project" }, S.project.unread),
          treeItem("Needs metadata", "warn", { type: "view", value: "needs_metadata", scope: "project" }, S.project.metadata_review));
      }
      ul.append(h("li", h("div.ti", { style: { fontWeight: 700, marginTop: "8px" } }, icon("library", 18), "Whole library")),
        treeItem("All sources", "doc", { type: "view", value: "all", scope: "library" }, counts.all),
        treeItem("Recently added", "clock", { type: "view", value: "recent_added", scope: "library" }),
        treeItem("Recently modified", "edit", { type: "view", value: "recent_modified", scope: "library" }),
        treeItem("Needs metadata", "warn", { type: "view", value: "needs_metadata", scope: "library" }),
        treeItem("Machine-created", "beaker", { type: "view", value: "machine", scope: "library" }),
        treeItem("Trash", "trash", { type: "view", value: "trash", scope: "library" }, counts.trash));
      ul.append(h("li", h("div.ti", { style: { fontWeight: 700, marginTop: "8px" } }, icon("folder", 18), "Collections", h("span.grow"),
        iconBtn("plus", "New collection", async () => { const n = await promptDialog("New collection", "Name"); if (n) { await call("collections.save", { collection: { name: n, project_id: "" } }); go("library"); } }))));
      ul.append(...buildCollections(""));
      if (saved.length) ul.append(h("li", h("div.ti", { style: { fontWeight: 700, marginTop: "8px" } }, icon("search", 18), "Saved searches")), ...saved.map((s) => h("li", { role: "treeitem", tabindex: "-1" }, h("div.ti", { on: { click: () => { st.search = s.query; searchBox.value = s.query; loadList(); } } }, icon("search", 16), s.name))));
      if (labels.length) ul.append(h("li", h("div.ti", { style: { fontWeight: 700, marginTop: "8px" } }, icon("tag", 18), "Labels")), ...labels.map((l) => treeItem(l.name, "tag", { type: "label", value: l.id })));
      tree.append(ul);
    };
    const collectionMenu = async (c) => {
      const act = await modal(c.name, h("div", h("p.muted", c.query ? "Smart collection: " + c.query : `${c.ref_ids.length} sources`)), { size: "narrow", buttons: [
        { label: "Rename", value: "rename" }, { label: "New sub-collection", value: "sub" }, ...(c.query ? [] : [{ label: "Add selected sources", value: "add" }]), { label: "Delete", value: "delete", kind: "danger" }] }).done;
      if (act === "rename") { const n = await promptDialog("Rename collection", "Name", c.name); if (n) await call("collections.save", { collection: { ...c, name: n } }); }
      if (act === "sub") { const n = await promptDialog("New sub-collection", "Name"); if (n) await call("collections.save", { collection: { name: n, parent_id: c.id } }); }
      if (act === "add") await call("collections.collect", { id: c.id, ids: [...st.selected], add: true });
      if (act === "delete" && await confirmDialog("Delete collection", `Delete “${c.name}” and its sub-collections? Sources are not deleted.`, { ok: "Delete", kind: "danger" })) await call("collections.delete", { id: c.id });
      if (act) go("library");
    };

    // ---- list ----------------------------------------------------------------------------------
    const searchBox = h("input", { type: "search", value: st.search, placeholder: "Search sources, notes, and full text   (try: author:chen year:2020..2024 -survey)", "aria-label": "Search", "data-search": "1" });
    const searchHelp = iconBtn("help", "Search syntax", () => modal("Search syntax", h("div",
      h("table.table", h("tbody", [["reflection teaching", "All words (each word may be the start of a longer word)"], ['"reflective practice"', "Exact phrase"], ['"faculty support"~5', "Words within 5 words of each other"], ["-survey  or  NOT survey", "Exclude"], ["a OR b, (a OR b) c", "Either; grouping"], ["title: author: journal: doi: tag: abstract: note: annotation: fulltext:", "Search one field"], ["year:2020..2024  year:>=2021  rating:>=3", "Ranges"], ["type:book  label:Theory  collection:Methods  has:pdf  has:conflict", "Filters"], ["re:/gener(ative|al) AI/", "Regular expression over titles and abstracts (limited)"]].map(([q, d]) => h("tr", h("td.mono", q), h("td", d))))),
      h("p.small.muted", "Accents and capitals do not matter: “muller” finds “Müller”. Results always show the original spelling.")), { buttons: [{ label: "Close" }] }));
    let t;
    searchBox.addEventListener("input", () => { clearTimeout(t); t = setTimeout(() => {
      const was = st.search.trim();
      st.search = searchBox.value; st.offset = 0;
      // Searching sorts by relevance until the person picks another order; clearing restores it.
      if (!was && st.search.trim() && !st.sortChosen) { st.sortBefore = st.sort; st.sort = "relevance"; }
      if (was && !st.search.trim() && st.sort === "relevance") { st.sort = st.sortBefore || "authors"; st.sortChosen = false; }
      loadList();
    }, 220); });
    const builderBtn = iconBtn("filter", "Build a search", () => searchBuilder((q) => { searchBox.value = q; searchBox.dispatchEvent(new Event("input")); searchBox.focus(); }));
    const addMenuBtn = btn("Add source", { kind: "primary", icon: "plus", onClick: async () => {
      const v = await modal("Add source", h("div.btn-row", { style: { flexDirection: "column", alignItems: "stretch" } },
        btn("By identifier (DOI, PMID, arXiv, ISBN, web page)", { icon: "search", onClick: () => { document.querySelector(".modal-back").remove(); dialogs.lookup(); } }),
        btn("Enter manually", { icon: "edit", onClick: () => { document.querySelector(".modal-back").remove(); dialogs.addSource(); } }),
        btn("Paste a bibliography", { icon: "copy", onClick: () => { document.querySelector(".modal-back").remove(); dialogs.pasteBibliography(); } }),
        btn("Import PDF files", { icon: "pdf", onClick: () => { document.querySelector(".modal-back").remove(); dialogs.importPdfs(); } }),
        btn("Import a folder of PDFs", { icon: "folder", onClick: () => { document.querySelector(".modal-back").remove(); dialogs.importFolder(); } })), { size: "narrow" }).done;
      void v;
    } });
    const toolbar = h("div.row", h("div.grow", { style: { position: "relative" } }, searchBox), searchHelp, builderBtn, addMenuBtn, btn("Import", { icon: "download", onClick: () => dialogs.importFile() }));
    if (!st.layout) st.layout = prefs().view || "table";
    const viewSwitch = h("div.seg", { role: "radiogroup", "aria-label": "List layout" }, [["table", "Table"], ["list", "List"], ["cards", "Cards"]].map(([k, l]) =>
      h("button", { type: "button", role: "radio", "aria-checked": String(st.layout === k), on: { click: () => { st.layout = k; savePrefs({ view: k }); for (const b of viewSwitch.children) b.setAttribute("aria-checked", String(b.textContent === l)); drawList(); } } }, l)));
    const filtersBtn = btn("Filters", { small: true, icon: "filter", onClick: () => { st.showFilters = !st.showFilters; filtersBtn.setAttribute("aria-pressed", String(st.showFilters)); loadList(); } });
    filtersBtn.setAttribute("aria-pressed", String(st.showFilters));
    const viewsBtn = btn("Views", { small: true, onClick: () => viewsMenu(viewsBtn) });
    const colsBtn = btn("Columns…", { small: true, onClick: () => columnsDialog() });
    const toolbar2 = h("div.row.small", viewSwitch, colsBtn, filtersBtn, viewsBtn);
    const filterBar = h("div", { "aria-live": "polite" });
    const bulk = h("div.row.small", { style: { minHeight: "34px" } });
    const listBox = h("div.pane", { style: { flex: "1" } });
    const bottom = h("div.pane", { style: { height: pane.bottom + "px", flex: "none" } });
    const bottomSplit = splitter("Resize abstract and notes pane", "horizontal", () => pane.bottom, (v) => { pane.bottom = v; bottom.style.height = v + "px"; }, 90, 600, savePanes);
    bottomSplit.dataset.invert = "1";
    listPane.append(toolbar, toolbar2, filterBar, bulk, listBox, bottomSplit, bottom);

    // ---- columns, saved views, filters -----------------------------------------------------------
    const columnsDialog = async () => {
      let list = colList().map((c) => ({ ...c }));
      const box = h("div");
      const draw = () => {
        clear(box);
        const shown = new Set(list.map((c) => c.key));
        const order = [...list.map((c) => c.key), ...Object.keys(COLS).filter((k) => !shown.has(k))];
        order.forEach((k) => {
          const on = shown.has(k);
          const idx = list.findIndex((c) => c.key === k);
          const cb = h("input", { type: "checkbox", id: "col-" + k, checked: on, disabled: k === "title", on: { change: (e) => { if (e.target.checked) list.push({ key: k }); else list = list.filter((c) => c.key !== k); draw(); } } });
          box.append(h("div.row", { style: { padding: "4px 0" } }, cb, h("label.grow", { for: "col-" + k }, COLS[k].label),
            on ? iconBtn("chevronUp", `Move ${COLS[k].label} left`, () => { if (idx > 0) { [list[idx - 1], list[idx]] = [list[idx], list[idx - 1]]; draw(); box.querySelector(`[aria-label="Move ${COLS[k].label} left"]`)?.focus(); } }) : null,
            on ? iconBtn("chevronDown", `Move ${COLS[k].label} right`, () => { if (idx < list.length - 1) { [list[idx + 1], list[idx]] = [list[idx], list[idx + 1]]; draw(); box.querySelector(`[aria-label="Move ${COLS[k].label} right"]`)?.focus(); } }) : null));
        });
      };
      draw();
      const r = await modal("Columns", h("div", h("p.small.muted", "Choose and order the table columns. Drag a column edge in the table to change its width."), box), { size: "narrow", buttons: [
        { label: "Reset", value: "reset" }, { label: "Cancel" }, { label: "Apply", kind: "primary", value: "apply" }] }).done;
      if (r === "reset") savePrefs({ columns: null }, true);
      else if (r === "apply") savePrefs({ columns: list }, true);
      if (r) drawList();
    };
    const viewsMenu = (anchor) => {
      const list = savedViews();
      const current = () => ({ view: st.layout, columns: colList(), sort: st.sort, desc: st.desc, filters: st.filters });
      let close;
      const content = h("div", { role: "menu", "aria-label": "Saved views", style: { minWidth: "240px" } },
        list.length ? list.map((v, i) => h("div.row", h("button.menu-item.grow", { type: "button", role: "menuitem", on: { click: () => {
          st.layout = v.view || "table"; st.sort = v.sort || "authors"; st.desc = !!v.desc; st.filters = v.filters || []; st.showFilters = st.filters.length > 0; st.offset = 0;
          savePrefs({ view: st.layout, columns: v.columns || null }, true); close(); go("library");
        } } }, v.name), iconBtn("trash", `Delete view ${v.name}`, async () => { list.splice(i, 1); await saveViews(list); close(); toast("View deleted", "success"); }))) : h("p.small.muted", { style: { padding: "6px 10px" } }, "No saved views yet."),
        h("hr"),
        h("button.menu-item", { type: "button", role: "menuitem", on: { click: async () => {
          close();
          const n = await promptDialog("Save view", "Name", "", { help: "Saves the layout, columns, sort order and quick filters. Collections and searches are not part of a view." });
          if (!n) return;
          await saveViews([...savedViews().filter((v) => v.name !== n), { name: n, ...current() }]);
          toast(`View “${n}” saved`, "success");
        } } }, "Save current view…"));
      close = popover(anchor, content);
    };
    const drawFilters = () => {
      clear(filterBar);
      if (!st.showFilters && !st.filters.length) return;
      const facets = page?.facets || {};
      const chips = st.filters.map((f, i) => {
        const fl = FILTER_FIELDS.find((x) => x[0] === f.field)?.[1] || f.field;
        const lab = f.label || (facets[f.field] || []).find((x) => x.value === f.value)?.label || f.value;
        return h("span.chip", { style: f.negate ? { background: "var(--danger-soft)" } : null },
          h("button", { type: "button", title: f.negate ? "Excluding: click to include instead" : "Including: click to exclude instead", "aria-label": `${f.negate ? "Excluding" : "Including"} ${fl} ${lab}; switch`, on: { click: () => { f.negate = !f.negate; st.offset = 0; loadList(); } } }, f.negate ? "not " : "", `${fl}: ${lab}`),
          h("button", { type: "button", "aria-label": `Remove filter ${fl} ${lab}`, on: { click: () => { st.filters.splice(i, 1); st.offset = 0; loadList(); } } }, icon("x", 14)));
      });
      const adders = st.showFilters ? FILTER_FIELDS.filter(([k]) => k !== "reading" || st.scope === "project").map(([k, l]) => {
        const b = h("button.btn.small", { type: "button", "aria-haspopup": "menu" }, l, icon("chevronDown", 14));
        b.addEventListener("click", () => {
          const vals = (facets[k] || []).slice(0, 60);
          let close;
          close = popover(b, h("div", { role: "menu", "aria-label": `${l} values`, style: { maxHeight: "320px", overflow: "auto", minWidth: "220px" } },
            vals.length ? vals.map((v) => h("button.menu-item", { type: "button", role: "menuitem", style: { display: "flex", gap: "8px", width: "100%" }, on: { click: () => {
              st.filters = st.filters.filter((x) => !(x.field === k && x.value === v.value));
              st.filters.push({ field: k, value: v.value, label: v.label, negate: false }); st.offset = 0; close(); loadList();
            } } }, h("span.grow", { style: { textAlign: "left" } }, v.label), h("span.muted", String(v.count)))) : h("p.small.muted", { style: { padding: "6px 10px" } }, "No values in the current list.")));
        });
        return b;
      }) : [];
      filterBar.append(h("div.row.small", { style: { flexWrap: "wrap", gap: "6px", padding: "2px 0" } }, ...chips, ...adders,
        st.filters.length ? btn("Clear filters", { small: true, kind: "ghost", onClick: () => { st.filters = []; st.offset = 0; loadList(); } }) : null,
        st.showFilters ? h("span.small.muted", "Counts are for the list as currently filtered. Click a filter to switch between including and excluding.") : null));
    };
    let page = null;

    const loadList = async () => {
      try {
        page = await call("refs.list", { query: { project_id: st.scope === "project" && st.view !== "trash" ? S.project?.project.id || "" : "", collection_id: st.collection, view: st.view, search: st.search, sort: st.sort, desc: st.desc, label: st.label, offset: st.offset, limit: 400, filters: st.filters, facets: st.showFilters } });
        drawFilters();
        drawList();
      } catch (e) { clear(listBox); showError(e, listBox); }
    };
    const cols = [["authors", "Author"], ["year", "Year"], ["title", "Title"], ["reading", "Read"], ["pdf", "PDF"]];
    const drawBulk = () => {
      clear(bulk);
      const n = st.selected.size;
      if (st.view === "trash") {
        bulk.append(h("span", `${page?.total || 0} in trash • items are deleted after ${S.settings.trash_retention_days} days`), h("span.grow"),
          btn("Restore", { small: true, disabled: !n, onClick: async () => { await call("refs.restore", { ids: [...st.selected] }); st.selected.clear(); await refresh(); } }),
          btn("Delete permanently", { small: true, kind: "danger", disabled: !n, onClick: async () => {
            if (await confirmDialog("Delete permanently", `Permanently delete ${n} source${n === 1 ? "" : "s"}, their files and annotations? This cannot be undone.`, { ok: "Delete permanently", kind: "danger" })) { await call("refs.purge", { ids: [...st.selected], confirmed: true }); st.selected.clear(); await refresh(); }
          } }));
        return;
      }
      bulk.append(h("span", page ? `${page.total} source${page.total === 1 ? "" : "s"}${n ? ` • ${n} selected` : ""}` : ""));
      if (st.search && st.sort === "relevance") bulk.append(h("span.small.muted", "• most relevant first ", h("button.btn.ghost.small", { type: "button", on: { click: () => { st.sort = "authors"; st.sortChosen = true; loadList(); } } }, "Sort by author")));
      for (const c of page?.corrections || []) bulk.append(h("span.small.status-review", icon("info", 13), ` No exact match for “${c.word}”; also searched ${c.used.map((u) => `“${u}”`).join(", ")}.`));
      if (page?.pending_index && st.search) bulk.append(h("span.small.status-review", { title: "Full text of some PDFs is not indexed yet; open them in the Reader to index." }, icon("clock", 14), ` ${page.pending_index} PDFs not yet indexed`));
      bulk.append(h("span.grow"));
      if (n) {
        bulk.append(
          S.project && st.scope !== "project" ? btn("Add to project", { small: true, onClick: async () => { const k = await call("projects.add_sources", { id: S.project.project.id, ids: [...st.selected] }); toast(`${k} added to ${S.project.project.title}`, "success"); await refresh(); } }) : null,
          S.project && st.scope === "project" ? btn("Remove from project", { small: true, onClick: () => dialogs.removeFromProject([...st.selected]) }) : null,
          btn("Cite", { small: true, icon: "quote", onClick: () => go("cite", { tab: "workspace", refs: [...st.selected] }) }),
          btn("Label", { small: true, icon: "tag", onClick: () => labelDialog([...st.selected], labels) }),
          btn("Bulk edit", { small: true, onClick: () => dialogs.bulkEdit([...st.selected]) }),
          btn("Export", { small: true, onClick: () => dialogs.exportRefs([...st.selected]) }),
          btn("Clean up", { small: true, icon: "chevronDown", onClick: (e) => {
            const ids = [...st.selected];
            let close;
            const item = (label, run) => h("button.menu-item", { type: "button", role: "menuitem", style: { display: "block", width: "100%" }, on: { click: () => { close(); run(); } } }, label);
            close = popover(e.currentTarget, h("div", { role: "menu", "aria-label": "Clean up", style: { minWidth: "240px" } },
              item("Find missing DOIs", () => dialogs.cleanup("doi", ids)),
              item("Fetch missing abstracts", () => dialogs.cleanup("abstract", ids)),
              item("Titles to sentence case", () => dialogs.cleanup("title_case", ids)),
              item("Check web addresses", () => dialogs.checkLinks(ids))));
          } }),
          btn("Find free full texts", { small: true, title: "Look for legal open-access PDFs for the selected sources that have a DOI and no PDF", onClick: () => dialogs.findFreeCopies([...st.selected]) }),
          n === 2 ? btn("Compare", { small: true, onClick: () => dialogs.compareRecords(...st.selected) }) : null,
          btn("Trash", { small: true, kind: "danger", icon: "trash", onClick: async () => { const k = await call("refs.trash", { ids: [...st.selected] }); toast(`${k} moved to trash`, "success", { action: { label: "Undo", run: async () => { await call("undo"); await refresh(); } } }); st.selected.clear(); await refresh(); } }));
      } else if (st.search) {
        bulk.append(btn("Save as smart collection", { small: true, onClick: async () => { const n2 = await promptDialog("Save search", "Name", st.search); if (n2) { await call("collections.save", { collection: { name: n2, query: st.search } }); go("library"); } } }));
      }
    };
    const drawList = () => {
      drawBulk();
      clear(listBox);
      if (!page.rows.length) {
        const emptyMsg = st.search ? [h("h3", "No matching sources"), h("p", page.pending_index ? `${page.pending_index} PDFs have not been indexed yet, so their text is not searched.` : "Try fewer words, or remove a filter."), h("div.btn-row", { style: { justifyContent: "center" } }, btn("Clear search", { onClick: () => { st.search = ""; searchBox.value = ""; loadList(); } }), st.scope === "project" ? btn("Search the whole library", { onClick: () => { st.scope = "library"; drawTree(); loadList(); } }) : null)]
          : st.view === "trash" ? [h("h3", "The trash is empty")]
          : st.scope === "project" ? [h("h3", "No sources in this project yet"), h("p", "Add sources already in your library, or bring in new ones."), h("div.btn-row", { style: { justifyContent: "center" } }, btn("Add existing sources", { onClick: () => { st.scope = "library"; drawTree(); loadList(); } }), btn("Add source", { kind: "primary", onClick: () => dialogs.addSource() }), btn("Import", { onClick: () => dialogs.importFile() }))]
          : [h("h3", "Your library is empty"), h("div.btn-row", { style: { justifyContent: "center" } }, btn("Add source", { kind: "primary", onClick: () => dialogs.addSource() }), btn("Import", { onClick: () => dialogs.importFile() }))];
        listBox.append(h("div.empty", emptyMsg));
        drawInspector();
        return;
      }
      if (st.layout === "list" || st.layout === "cards") { drawItems(); return; }
      const shown = colList();
      const table = h("table.table.fixed", { role: "grid", "aria-label": "Sources", "aria-rowcount": String(page.total + 1), "aria-multiselectable": "true" });
      const colgroup = h("colgroup", shown.map((c) => h("col", { style: colWidth(c) ? { width: colWidth(c) + "px" } : null })));
      const head = h("tr", { role: "row" }, shown.map((c, ci) => {
        const k = c.key, def = COLS[k];
        const th = h("th", { role: "columnheader", "aria-sort": st.sort === def.sort ? (st.desc ? "descending" : "ascending") : null, style: { position: "relative", textAlign: k === "reading" || k === "pdf" ? "center" : null } },
          def.sort ? h("button", { type: "button", on: { click: () => { st.sortChosen = true; if (st.sort === def.sort) st.desc = !st.desc; else { st.sort = def.sort; st.desc = false; } loadList(); } } }, def.label, st.sort === def.sort ? icon(st.desc ? "chevronDown" : "chevronUp", 14) : null) : def.label);
        if (def.w) {
          const colEl = colgroup.children[ci];
          const grip = splitter(`Resize ${def.label} column`, "vertical", () => colWidth(c), (v) => { c.width = v; colEl.style.width = v + "px"; }, 44, 600, () => savePrefs({ columns: shown }));
          grip.classList.add("col-grip");
          th.append(grip);
        }
        return th;
      }));
      const body = h("tbody");
      page.rows.forEach((r, i) => {
        const tr = h("tr", { role: "row", tabindex: r.id === (st.focus || page.rows[0].id) ? "0" : "-1", "aria-selected": String(st.selected.has(r.id)), "aria-rowindex": String(st.offset + i + 2), "data-id": r.id },
          shown.map((c) => cell(c.key, r)));
        tr.addEventListener("click", (e) => selectRow(r.id, i, e));
        tr.addEventListener("dblclick", () => (r.has_pdf ? go("reader", { ref: r.id }) : editSelected()));
        tr.addEventListener("keydown", (e) => rowKey(e, i));
        body.append(tr);
      });
      table.prepend(colgroup);
      table.append(h("thead", head), body);
      listBox.append(table);
      pager();
      // Stable selection: keep the focused row visible after sorting and refreshes.
      const f = listBox.querySelector(`tr[data-id="${st.focus}"]`);
      if (f) f.scrollIntoView({ block: "nearest" });
      drawInspector();
    };
    const typeIcon = (r) => icon(r.rtype.startsWith("article") ? "doc" : r.rtype.includes("book") || r.rtype === "chapter" ? "book" : "doc", 16);
    const hitsFor = (r) => (page.search_hits || []).filter((x) => x.ref_id === r.id && x.field !== "title").slice(0, 2).map((x) => h("div.hit", h("b", { page: "PDF text", fulltext: "PDF text", annotation: "Annotation", comment: "Annotation comment", note: "Note", quotation: "Quotation", author: "Author", journal: "Published in", abstract: "Abstract", tag: "Tag", keywords: "Keyword", doi: "DOI", citekey: "Citation key", publisher: "Publisher" }[x.field] || x.field), x.page >= 0 && (x.field === "fulltext" || x.entity === "annotation") ? ` (PDF page ${x.page + 1})` : "", ": ", x.snippet,
      x.field === "fulltext" ? h("button.btn.ghost.small", { type: "button", on: { click: (e) => { e.stopPropagation(); go("reader", { ref: r.id, att: x.entity_id, page: x.page + 1 }); } } }, "Open at page") : null));
    const flags = (r) => [
      r.notices ? h("span.badge.danger.small", { title: "Retraction or correction notice" }, icon("warn", 13), "Notice") : null,
      r.issues ? h("span.small.status-review", " ", icon("warn", 13), " review") : null,
      r.origin === "ai" || r.origin === "agent" ? h("span.badge.neutral.small", "machine-created") : null];
    const pdfBtn = (r) => r.has_pdf ? h("button.icon-btn", { type: "button", "aria-label": "Open PDF", title: "Open in Reader", on: { click: (e) => { e.stopPropagation(); go("reader", { ref: r.id }); } } }, icon("pdf", 18)) : "";
    const stars = (n) => n ? h("span", { "aria-label": `${n} of 5` }, "★".repeat(n)) : "";
    const cell = (k, r) => {
      switch (k) {
        case "authors": return h("td", h("span.row", typeIcon(r), h("span.ellipsis", r.authors || "—")));
        case "year": return h("td", r.year || "n.d.");
        case "title": return h("td", h("div.ellipsis", { title: r.title }, r.title || h("em.muted", "No title")), ...hitsFor(r), ...flags(r));
        case "container": return h("td", h("div.ellipsis", { title: r.container }, r.container));
        case "type": return h("td", h("div.ellipsis", r.type_label));
        case "citekey": return h("td.mono.small", h("div.ellipsis", r.citekey));
        case "rating": return h("td", stars(r.rating));
        case "labels": return h("td", h("div.ellipsis", { title: r.labels.join(", ") }, r.labels.join(", ")));
        case "created": return h("td.small", fmtDate(r.created));
        case "modified": return h("td.small", fmtDate(r.modified));
        case "reading": return h("td", { style: { textAlign: "center" } }, st.scope === "project" ? anchorReadingIcon(r) : "");
        case "pdf": return h("td", { style: { textAlign: "center" } }, pdfBtn(r));
        default: return h("td");
      }
    };
    // List and card layouts: the same rows as options in a listbox.
    const drawItems = () => {
      const cards = st.layout === "cards";
      const box = h("div" + (cards ? ".card-grid" : ".ref-list"), { role: "listbox", "aria-label": "Sources", "aria-multiselectable": "true" });
      page.rows.forEach((r, i) => {
        const meta = [r.authors || "No author", r.year || "n.d.", r.container].filter(Boolean).join(" • ");
        const item = h("div" + (cards ? ".ref-card" : ".ref-item"), { role: "option", tabindex: r.id === (st.focus || page.rows[0].id) ? "0" : "-1", "aria-selected": String(st.selected.has(r.id)), "data-id": r.id },
          h("div.row", typeIcon(r), h("strong.grow" + (cards ? "" : ".ellipsis"), { title: r.title }, r.title || "No title"), st.scope === "project" ? anchorReadingIcon(r) : null, pdfBtn(r)),
          h("div.small.muted" + (cards ? "" : ".ellipsis"), meta),
          cards ? h("div.small", r.type_label, r.rating ? " • " : "", stars(r.rating)) : null,
          r.labels.length ? h("div.row.small", { style: { flexWrap: "wrap", gap: "4px", marginTop: "4px" } }, r.labels.map((l) => h("span.chip", l))) : null,
          ...hitsFor(r), ...flags(r));
        item.addEventListener("click", (e) => selectRow(r.id, i, e));
        item.addEventListener("dblclick", () => (r.has_pdf ? go("reader", { ref: r.id }) : editSelected()));
        item.addEventListener("keydown", (e) => rowKey(e, i));
        box.append(item);
      });
      listBox.append(box);
      pager();
      const f = listBox.querySelector(`[data-id="${st.focus}"]`);
      if (f) f.scrollIntoView({ block: "nearest" });
      drawInspector();
    };
    const pager = () => {
      if (page.total > st.offset + page.rows.length || st.offset > 0) {
        listBox.append(h("div.btn-row", { style: { padding: "8px" } },
          st.offset > 0 ? btn("Previous 400", { small: true, onClick: () => { st.offset = Math.max(0, st.offset - 400); loadList(); } }) : null,
          h("span.small.muted", `Showing ${st.offset + 1}–${st.offset + page.rows.length} of ${page.total}`),
          page.total > st.offset + page.rows.length ? btn("Next 400", { small: true, onClick: () => { st.offset += 400; loadList(); } }) : null));
      }
    };
    let anchor = null;
    const selectRow = (id, i, e) => {
      if (e && (e.ctrlKey || e.metaKey)) { st.selected.has(id) ? st.selected.delete(id) : st.selected.add(id); anchor = i; }
      else if (e && e.shiftKey && anchor !== null) { const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)]; st.selected = new Set(page.rows.slice(a, b + 1).map((r) => r.id)); }
      else { st.selected = new Set([id]); anchor = i; }
      st.focus = id;
      S.params.selected = id;
      for (const tr of listBox.querySelectorAll("[data-id]")) { tr.setAttribute("aria-selected", String(st.selected.has(tr.dataset.id))); tr.tabIndex = tr.dataset.id === id ? 0 : -1; }
      drawBulk();
      drawInspector();
    };
    const rowKey = (e, i) => {
      const rows = [...listBox.querySelectorAll("[data-id]")];
      let j = null;
      // In cards, left and right move by one card and up and down by a row of cards.
      const across = st.layout === "cards" ? Math.max(1, Math.round(listBox.clientWidth / 260)) : 1;
      if (e.key === "ArrowDown") j = Math.min(rows.length - 1, i + across);
      if (e.key === "ArrowUp") j = Math.max(0, i - across);
      if (st.layout === "cards" && e.key === "ArrowRight") j = Math.min(rows.length - 1, i + 1);
      if (st.layout === "cards" && e.key === "ArrowLeft") j = Math.max(0, i - 1);
      if (e.key === "Home") j = 0;
      if (e.key === "End") j = rows.length - 1;
      if (e.key === "PageDown") j = Math.min(rows.length - 1, i + 15);
      if (e.key === "PageUp") j = Math.max(0, i - 15);
      if (j !== null) { e.preventDefault(); rows[j].focus(); selectRow(page.rows[j].id, j, e.shiftKey ? { shiftKey: true } : null); return; }
      if (e.key === " ") { e.preventDefault(); selectRow(page.rows[i].id, i, { ctrlKey: true }); }
      if (e.key === "Enter") { e.preventDefault(); page.rows[i].has_pdf ? go("reader", { ref: page.rows[i].id }) : editSelected(); }
      if (e.key === "Delete") { e.preventDefault(); bulk.querySelector(".btn.danger")?.click(); }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") { e.preventDefault(); st.selected = new Set(page.rows.map((r) => r.id)); drawList(); }
    };
    const editSelected = async () => {
      if (!st.focus) return;
      const d = await call("refs.get", { id: st.focus, project_id: S.project?.project.id || "" });
      const r = await sourceEditor(d.ref, { mode: "edit" });
      if (r) await refresh();
    };

    // ---- inspector -----------------------------------------------------------------------------
    const drawInspector = async () => {
      const id = st.focus && st.selected.has(st.focus) ? st.focus : null;
      clear(inspector);
      clear(bottom);
      if (!id) {
        inspector.append(h("div.empty", h("p", st.selected.size > 1 ? `${st.selected.size} sources selected` : "Select a source to see its details.")));
        return;
      }
      let d;
      try { d = await call("refs.get", { id, project_id: S.project?.project.id || "" }); } catch (e) { showError(e, inspector); return; }
      if (st.focus !== id) return;
      const r = d.ref;
      const tabBar = tabs([["details", "Details"], ["files", `Files (${d.attachments.length})`], ["notes", `Notes (${d.notes.length})`], ["history", "History"]], st.inspectorTab, (k) => { st.inspectorTab = k; drawInspector(); }, { label: "Source details" });
      const body = h("div", { style: { padding: "12px 14px" } });
      inspector.append(tabBar, body);
      if (st.inspectorTab === "details") {
        const typeLabel = (r.subtype && S.customTypes?.find((t) => t.id === r.subtype)?.label) || S.types.find((t) => t.id === r.type)?.label || r.type;
        const rows = [];
        const people = (role) => r.creators.filter((c) => c.role === role).map((c) => c.name.literal || [c.name.particle, c.name.family].filter(Boolean).join(" ") + (c.name.given ? ", " + c.name.given : "") + (c.name.suffix ? ", " + c.name.suffix : "")).join("; ");
        const authors = people("author");
        if (authors) rows.push(["Author", authors]);
        else if (r.confirmed_absent?.includes("author")) rows.push(["Author", h("span.muted", "None (confirmed)")]);
        const eds = people("editor"); if (eds) rows.push(["Editor", eds]);
        rows.push(["Year", r.issued ? (r.issued.year || r.issued.status || "") : r.confirmed_absent?.includes("issued") ? h("span.muted", "No date (confirmed)") : h("span.status-review", "Not recorded")]);
        for (const f of ["container_title", "volume", "issue", "page", "article_number", "publisher", "edition", "isbn", "url"]) if (r.fields[f]) rows.push([fieldLabel(f), f === "container_title" ? h("i", r.fields[f]) : r.fields[f]]);
        rows.push(["DOI", r.fields.doi ? r.fields.doi : r.confirmed_absent?.includes("doi") ? h("span.muted", "This work has no DOI (confirmed)") : h("span", h("span.muted", "Not supplied"), h("div.small.muted", "A DOI is not recorded for this source. Add one if the work has one; a missing DOI does not by itself make the reference incomplete."))]);
        rows.push(["Citation key", h("span.mono", r.citekey)]);
        for (const def of S.customFields || []) {
          const v = r.custom?.[def.id];
          if (v === undefined || v === null || v === "") continue;
          rows.push([def.name, def.kind === "boolean" ? (v ? "Yes" : "No") : def.kind === "link" ? h("a", { href: "#", on: { click: (e) => { e.preventDefault(); call("system.open_url", { url: String(v) }); } } }, String(v)) : String(v)]);
        }
        const membership = d.membership;
        body.append(
          h("div.row.muted", icon("doc"), typeLabel, h("span.grow"), btn("Edit", { small: true, icon: "edit", onClick: editSelected })),
          h("h2", { style: { fontSize: "20px", margin: "6px 0 10px" } }, r.fields.title || h("em.muted", "No title")),
          ...(r.notices || []).map((n) => h("div.notice.error", icon("warn"), h("div", h("strong", `${n.kind} notice`), h("div.small", `${n.date} • ${n.source}`), n.url ? h("a", { href: "#", on: { click: (e) => { e.preventDefault(); call("system.open_url", { url: n.url }); } } }, "Read the notice") : null))),
          (r.conflicts || []).length ? h("div.notice.warn", icon("warn"), h("div", h("strong", "Conflicting values"), ...r.conflicts.map((c) => h("div.small", `${fieldLabel(c.field)}: “${c.value}” from ${c.source}`, " ", btn("Use this", { small: true, onClick: async () => { await call("refs.resolve_conflict", { id: r.id, field: c.field, value: c.value }); drawInspector(); } }), btn("Keep current", { small: true, onClick: async () => { await call("refs.resolve_conflict", { id: r.id, field: c.field, value: null }); drawInspector(); } }))))) : null,
          h("table", { style: { width: "100%", borderCollapse: "collapse" } }, h("tbody", rows.map(([k, v]) => h("tr", h("td.muted", { style: { padding: "5px 8px 5px 0", verticalAlign: "top", width: "34%" } }, k), h("td", { style: { padding: "5px 0" } }, v))))),
          d.versions.length ? h("div.card.pad", { style: { marginTop: "8px" } }, h("strong", "Other versions of this work"), ...d.versions.map((v) => h("div.small", `${v.version_label || v.type}: ${v.fields.title} (${v.issued?.year || "n.d."})${v.preferred_version ? " • cited version" : ""}`))) : null,
          membership ? h("div", { style: { marginTop: "12px" } }, h("h3", { style: { fontSize: "15px" } }, `In “${S.project.project.title}”`),
            h("div.grid2",
              field("Reading", select([["unread", "Not read"], ["reading", "Reading"], ["read", "Read"]], membership.reading || "unread", { on: { change: (e) => saveMembership({ ...membership, reading: e.target.value }) } })),
              field("Priority", select([["low", "Low"], ["normal", "Normal"], ["high", "High"]], membership.priority || "normal", { on: { change: (e) => saveMembership({ ...membership, priority: e.target.value }) } })),
              field("Inclusion", select([["candidate", "Candidate"], ["included", "Included"], ["excluded", "Excluded"], ["undecided", "Undecided"]], membership.inclusion || "candidate", { on: { change: (e) => saveMembership({ ...membership, inclusion: e.target.value }) } })),
              field("Rating", select([["0", "None"], ["1", "★"], ["2", "★★"], ["3", "★★★"], ["4", "★★★★"], ["5", "★★★★★"]], String(membership.rating || 0), { on: { change: (e) => saveMembership({ ...membership, rating: +e.target.value }) } }))),
            membership.inclusion === "excluded" ? field("Exclusion reason", h("input", { value: membership.exclusion_reason || "", on: { change: (e) => saveMembership({ ...membership, exclusion_reason: e.target.value }) } })) : null,
            h("div.small.muted", `Used in ${d.impact.projects.length} project${d.impact.projects.length === 1 ? "" : "s"}${d.impact.projects.length > 1 ? ": " + d.impact.projects.map((p) => p[1]).join(", ") : ""}`))
            : S.project ? h("div.btn-row", { style: { marginTop: "10px" } }, btn(`Add to “${S.project.project.title}”`, { small: true, onClick: async () => { await call("projects.add_sources", { id: S.project.project.id, ids: [r.id] }); await refresh(); } })) : null,
          await citationPreview(r));
      } else if (st.inspectorTab === "files") {
        body.append(h("div.btn-row", btn("Attach files…", { small: true, icon: "plus", onClick: async () => { const res = await call("files.attach_pick", { ref_id: r.id, mode: "managed" }); if (!res?.cancelled) { toast("Attached", "success"); drawInspector(); } } }),
          btn("Link files in place…", { small: true, onClick: async () => { const res = await call("files.attach_pick", { ref_id: r.id, mode: "linked" }); if (!res?.cancelled) drawInspector(); } }),
          !d.attachments.some((x) => x.mime === "application/pdf" && !x.superseded) && r.fields.doi ? btn("Find free full text", { small: true, icon: "search", onClick: async () => { await dialogs.findFreeCopies(r.id); drawInspector(); } }) : null),
          h("p.small.muted", "Attached files are copied into the library (managed). Linked files stay where they are. Originals are never modified; annotations live in the library and export into copies."));
        for (const a of d.attachments) {
          const state = { ok: null, protected: "Password-protected or encrypted", malformed: "The file structure could not be read", large: "Very large file", "image-only": "No text layer (scanned); needs OCR for text search", placeholder: "Cloud placeholder not downloaded", missing: "Missing" }[a.state];
          body.append(h("div.card.pad", { style: { marginBottom: "8px", opacity: a.superseded ? 0.7 : 1 } },
            h("div.row", icon(a.mime === "application/pdf" ? "pdf" : "doc"), h("strong.ellipsis", a.original_name), h("span.grow"), a.superseded ? h("span.badge.neutral", "Earlier version") : null),
            h("div.small.muted", `${a.role} • ${(a.size / 1048576).toFixed(1)} MB • ${a.mode}${a.page_count ? ` • ${a.page_count} pages` : ""} • ${a.indexed ? "text indexed" : a.index_excluded ? "excluded from search" : "not yet indexed"}`),
            state ? h("div.small.status-review", icon("warn", 13), " " + state) : null,
            a.mime === "application/pdf" ? h("label.row.small", { style: { marginTop: "4px" } }, h("input", { type: "checkbox", checked: !!a.index_excluded, on: { change: async (e) => { await call("files.exclude_index", { id: a.id, excluded: e.target.checked }); toast(e.target.checked ? "Left out of full-text search" : "Included in full-text search", "success"); drawInspector(); } } }), "Leave this file out of full-text search") : null,
            a.source_url ? h("div.small.muted", "Free copy from ", h("span.mono", a.source_url.split("/")[2] || a.source_url), ` • ${({ publishedVersion: "published version", acceptedVersion: "accepted manuscript", submittedVersion: "submitted manuscript" })[a.source_version] || "version not stated"} • licence: ${a.license || "not stated"}`) : null,
            h("div.btn-row", { style: { marginTop: "6px" } },
              a.mime === "application/pdf" ? btn("Read", { small: true, onClick: () => go("reader", { ref: r.id, att: a.id }) }) : null,
              btn("Open in default app", { small: true, onClick: () => call("files.open_external", { id: a.id }) }),
              btn("Show in folder", { small: true, onClick: () => call("files.reveal", { id: a.id }) }),
              btn("Replace with new version…", { small: true, onClick: () => go("reader", { ref: r.id, att: a.id, replace: true }) }),
              btn("Remove", { small: true, kind: "danger", onClick: async () => { if (await confirmDialog("Remove attachment", "Remove this file and its annotations from the source? Undo is available.", { ok: "Remove", kind: "danger" })) { await call("files.delete", { id: a.id }); drawInspector(); } } }))));
        }
        if (!d.attachments.length) body.append(h("div.empty.small", "No files attached."));
      } else if (st.inspectorTab === "notes") {
        body.append(h("div.btn-row", btn("New note", { small: true, icon: "plus", onClick: () => go("notes", { newFor: r.id }) })));
        for (const n of d.notes) body.append(h("div.note-card.kind-" + n.kind, h("header", n.kind === "quotation" ? icon("quote") : icon("notes"), n.kind, h("span.grow"), n.project_id ? h("span.small.muted", "Project") : h("span.small.muted", "Library-wide")),
          n.exact_text ? h("div.body", "“" + n.exact_text + "”") : null, n.body ? h("div.body", n.body) : null,
          n.locator ? h("footer", `${n.locator_label || "page"} ${n.locator}`) : n.anchor_state === "unresolved" ? h("footer.status-review", icon("warn", 13), " Locator not confirmed") : null));
        if (!d.notes.length) body.append(h("div.empty.small", "No notes for this source in this project."));
      } else {
        body.append(h("p.small.muted", "Every field keeps its earlier values. Choose a field to see who changed it and when, and restore an earlier value."));
        const fs = ["title", "creators", "issued", ...Object.keys(r.fields).filter((f) => f !== "title"), "type"];
        body.append(h("table.table", h("tbody", fs.map((f) => h("tr", h("td", fieldLabel(f) === f ? f : fieldLabel(f)), h("td.small.muted", r.provenance?.[f] ? `${r.provenance[f].source}${r.provenance[f].user_edited ? " (edited)" : ""} • ${fmtDate(r.provenance[f].time)}` : ""),
          h("td", btn("History", { small: true, onClick: () => dialogs.fieldHistory(r.id, f) })))))),
          h("div.btn-row", { style: { marginTop: "10px" } }, (r.fields.doi || r.fields.pmid || r.fields.arxiv || r.fields.isbn) ? btn("Refresh from online record…", { small: true, icon: "refresh", onClick: () => dialogs.refreshFromSource(r.id) }) : null, btn("Regenerate citation key", { small: true, onClick: async () => { try { await call("refs.regenerate_citekey", { id: r.id }); drawInspector(); } catch (e) { showError(e); } } }),
            btn("Copy citation key", { small: true, onClick: async () => { await call("refs.citekey_copy", { ids: [r.id], syntax: "pandoc" }); toast("Copied @" + r.citekey, "success"); } })));
      }
      // Bottom panel
      const bt = tabs([["abstract", "Abstract"], ["related", "Related"], ["project_notes", "Project notes"]], st.bottomTab, (k) => { st.bottomTab = k; drawInspector(); }, { label: "Source content" });
      const bb = h("div", { style: { padding: "10px 14px" } });
      bottom.append(bt, bb);
      if (st.bottomTab === "abstract") bb.append(r.fields.abstract ? h("p", { style: { lineHeight: 1.55, margin: 0 } }, r.fields.abstract) : h("p.muted", "No abstract recorded."));
      if (st.bottomTab === "related") bb.append(d.versions.length ? h("ul", d.versions.map((v) => h("li", `${v.version_label}: ${v.fields.title}`))) : h("p.muted", "No linked versions. Related sources can be linked as versions from duplicate review."), h("p.small.muted", "Used in: " + (d.impact.projects.map((p) => p[1]).join(", ") || "no projects")));
      if (st.bottomTab === "project_notes") bb.append(d.notes.length ? d.notes.slice(0, 6).map((n) => h("p.small", h("strong", n.kind + ": "), (n.exact_text || n.body).slice(0, 220))) : h("p.muted", "No project notes for this source."));
    };
    const saveMembership = async (m) => { await call("membership.update", { membership: m }); announce("Saved"); };

    const citationPreview = async (r) => {
      const m = manuscript();
      const box = h("div", { style: { marginTop: "14px", borderTop: "1px solid var(--divider)", paddingTop: "12px" } }, h("h3", { style: { fontSize: "17px", margin: "0 0 8px" } }, "Citation preview"));
      try {
        const out = await call("cite.render", { request: { manuscript_id: m?.id || "", items: [{ ref_id: r.id }], mode: "parenthetical" } });
        box.append(h("div.output-box", { html: runsHtml(out.runs) }),
          out.provisional ? h("div.small.muted", icon("info", 13), " Standalone preview; choose a manuscript for context-dependent output.") : h("div.small.muted", `${out.style_title} • ${m.title}`),
          h("div.btn-row", { style: { marginTop: "8px" } },
            btn("Copy citation", { kind: "primary", icon: "copy", onClick: () => copyOut({ manuscript_id: m?.id || "", items: [{ ref_id: r.id }], mode: "parenthetical" }) }),
            btn("Copy reference", { icon: "copy", onClick: () => copyOut({ manuscript_id: m?.id || "", items: [{ ref_id: r.id }], mode: "reference", ref_ids: [r.id] }) }),
            btn("More…", { kind: "ghost", onClick: () => go("cite", { tab: "workspace", refs: [r.id] }) })));
      } catch (e) { box.append(h("div.small.muted", e.message)); }
      return box;
    };

    drawTree();
    await loadList();
    if (params.focusSearch) setTimeout(() => searchBox.focus(), 50);
  },
});

export async function copyOut(request, plain = false) {
  try {
    const r = await call("cite.copy", { request, plain });
    toast(`Copied: ${r.output.text.slice(0, 120)}`, "success");
    if (r.output.provisional) toast("Standalone preview: no manuscript is chosen, so year suffixes and name disambiguation may change once it is.", "info");
    return r;
  } catch (e) { showError(e); return null; }
}

async function labelDialog(ids, labels) {
  const name = h("input", { placeholder: "New label name" });
  const color = h("input", { type: "color", value: "#1554d1", "aria-label": "Colour" });
  const r = await modal("Label sources", h("div",
    labels.length ? h("div", h("p.muted", "Add or remove a label for the selected sources."), ...labels.map((l) => h("div.row", h("span.swatch", { style: { background: l.color } }), h("span.grow", l.name),
      btn("Add", { small: true, onClick: async () => { await call("refs.set_labels", { ids, label: l.id, on: true }); toast("Labelled", "success"); } }),
      btn("Remove", { small: true, onClick: async () => { await call("refs.set_labels", { ids, label: l.id, on: false }); } }),
      iconBtn("edit", "Rename or recolour", async () => { const n = await promptDialog("Rename label", "Name", l.name); if (n) await call("labels.save", { label: { ...l, name: n } }); }),
      iconBtn("trash", "Delete label", async () => { if (await confirmDialog("Delete label", `Delete the label “${l.name}”? Sources keep everything else.`, { ok: "Delete", kind: "danger" })) await call("labels.delete", { id: l.id }); })))) : null,
    h("h3", "New label"), h("div.row", name, color)),
    { buttons: [{ label: "Close" }, { label: "Create and add", kind: "primary", run: async () => { if (!name.value.trim()) return false; const l = await call("labels.save", { label: { name: name.value.trim(), color: color.value } }); await call("refs.set_labels", { ids, label: l.id, on: true }); return true; } }] }).done;
  if (r) go("library");
}
