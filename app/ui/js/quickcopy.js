// The quick-copy launcher window (spec WRT-019): search a source, choose the citation form, press
// Enter. The citation goes to the clipboard through the program's own clipboard write; nothing is
// typed into another program. The window then hides so the previous program has focus again.

import { call, on } from "./api.js";
import { h, clear, icon } from "./dom.js";

const MODES = [["parenthetical", "Parenthetical", "(Author, 2020)"], ["narrative", "Narrative", "Author (2020)"], ["reference", "Reference entry", "Full reference"]];
const root = document.getElementById("qc");
let mode = "parenthetical";
try { mode = localStorage.getItem("qc-mode") || mode; } catch (_) {}
let results = [], sel = 0, manuscript = "", t = null;

const input = h("input", { type: "search", placeholder: "Find a source: author, title, year, key…", "aria-label": "Find a source", role: "combobox", "aria-controls": "qc-list", "aria-expanded": "true", "aria-autocomplete": "list" });
const locator = h("input", { placeholder: "Page (optional)", "aria-label": "Page or other locator", style: { width: "130px" } });
const list = h("ul#qc-list.qc-list", { role: "listbox", "aria-label": "Sources" });
const status = h("div.small.qc-status", { role: "status", "aria-live": "polite" });
const modeBox = h("div.seg", { role: "radiogroup", "aria-label": "Citation form" });

function drawModes() {
  clear(modeBox, ...MODES.map(([k, label, eg]) => h("button", { type: "button", role: "radio", "aria-checked": String(mode === k), title: eg, on: { click: () => { mode = k; try { localStorage.setItem("qc-mode", k); } catch (_) {} drawModes(); input.focus(); } } }, label)));
}

function drawList() {
  clear(list, ...results.map((r, i) => h("li", { role: "option", id: "qc" + i, "aria-selected": String(i === sel), on: { mousedown: (e) => { e.preventDefault(); sel = i; copy(); } } }, r.label)));
  input.setAttribute("aria-activedescendant", results.length ? "qc" + sel : "");
}

async function search() {
  const q = input.value.trim();
  if (q.length < 2) { results = []; drawList(); return; }
  try {
    const r = await call("search.run", { query: q, limit: 12 });
    results = r.results.ref_ids.slice(0, 12).map((id) => ({ id, label: r.labels[id] || id }));
    sel = 0; drawList();
    status.textContent = results.length ? "" : "No source found.";
  } catch (e) { status.textContent = e.message; }
}

async function copy() {
  const r = results[sel];
  if (!r) return;
  try {
    const out = await call("cite.copy", { request: { manuscript_id: manuscript, mode, ref_ids: mode === "reference" ? [r.id] : [], items: mode === "reference" ? [] : [{ ref_id: r.id, locator_label: locator.value.trim() ? "page" : "", locator: locator.value.trim() }] } });
    status.textContent = "Copied: " + out.output.text.slice(0, 160);
    setTimeout(() => call("quickcopy.hide"), 700);
  } catch (e) { status.textContent = e.message; }
}

input.addEventListener("input", () => { clearTimeout(t); t = setTimeout(search, 150); });
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { e.preventDefault(); call("quickcopy.hide"); }
  else if (e.key === "ArrowDown") { e.preventDefault(); sel = Math.min(results.length - 1, sel + 1); drawList(); }
  else if (e.key === "ArrowUp") { e.preventDefault(); sel = Math.max(0, sel - 1); drawList(); }
  else if (e.key === "Enter") { e.preventDefault(); copy(); }
});

async function open() {
  try {
    const info = await call("app.info");
    const theme = info.settings?.theme;
    if (theme === "light" || theme === "dark" || theme === "high-contrast") document.documentElement.dataset.theme = theme;
    if (!info.library) { clear(root, h("div.qc", h("p", "Open a library in Prism Reference Manager first."))); return; }
    // Cite for the active project's manuscript, so its style and year letters apply.
    const projects = await call("projects.list").catch(() => []);
    const p = projects.find((x) => x.project.id === info.settings.active_project);
    manuscript = p?.project.active_manuscript || "";
    clear(root, h("div.qc", h("div.row", icon("copy", 18), h("strong", "Quick copy"), h("span.grow"), h("span.small.muted", p ? `${p.project.title}` : "No project: standalone citation")),
      input, h("div.row", { style: { gap: "8px", flexWrap: "wrap" } }, modeBox, locator), list, status,
      h("p.small.muted", "Enter copies the citation; then paste it where you are writing. Esc closes.")));
    drawModes();
    input.value = ""; results = []; drawList(); status.textContent = "";
    input.focus();
  } finally { root.removeAttribute("aria-busy"); }
}

on("quickcopy-open", open);
open();
