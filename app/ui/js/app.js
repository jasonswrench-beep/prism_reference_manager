// Application frame: state, navigation, context bar, menus, command palette, shortcuts,
// status bar. Views live in ./views and render into <main>.

import { call, on } from "./api.js";
import { h, clear, icon, iconBtn, toast, modal, announce, showError, select } from "./dom.js";
import * as dialogs from "./dialogs.js";

export const S = {
  info: null,
  settings: {},
  projects: [],
  project: null, // ProjectSummary
  manuscriptId: "",
  view: "projects",
  params: {},
  types: [],
  styles: [],
  noteKinds: [],
  undo: null,
  redo: null,
  saveState: "Saved locally",
  jobs: [],
};

const views = {};
export function registerView(name, mod) { views[name] = mod; }

const NAV = [
  ["projects", "Projects", "folder"],
  ["library", "Library", "library"],
  ["reader", "Reader", "reader"],
  ["notes", "Notes", "notes"],
  ["cite", "Cite", "quote"],
  ["connections", "Connections", "connect"],
];

let root, mainEl, ctxBar, statusEl, navEl, bodyEl;
const listeners = new Set();
export function onRefresh(fn) { listeners.add(fn); return () => listeners.delete(fn); }

export function applyAppearance() {
  const s = S.settings || {};
  const theme = s.theme === "system" || !s.theme ? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : s.theme;
  document.documentElement.dataset.theme = theme === "high-contrast" ? "contrast" : theme;
  document.documentElement.dataset.density = s.density || "comfortable";
  document.documentElement.dataset.motion = s.reduce_motion ? "reduce" : "";
  document.documentElement.style.setProperty("--fs", (s.font_size || 14) + "px");
}

export async function loadInfo() {
  const info = await call("app.info");
  S.info = info;
  S.settings = info.settings;
  S.types = info.types;
  S.styles = info.styles;
  S.noteKinds = info.note_kinds;
  if (info.library) {
    S.undo = info.library.undo;
    S.redo = info.library.redo;
  }
  applyAppearance();
  return info;
}

export async function loadProjects() {
  if (!S.info?.library) { S.projects = []; S.project = null; return; }
  S.projects = await call("projects.list", { include_archived: true });
  const want = S.project?.project.id || S.settings.active_project;
  const active = S.projects.filter((p) => !p.project.archived);
  S.project = S.projects.find((p) => p.project.id === want) || active[0] || null;
  if (S.project) {
    const ms = S.project.manuscripts;
    if (!ms.find((m) => m.id === S.manuscriptId)) S.manuscriptId = S.project.active_manuscript?.id || ms[0]?.id || "";
  } else S.manuscriptId = "";
}

export async function setProject(id) {
  // Switching saves the previous workspace state (selection, view) before loading the new one.
  saveWorkspace();
  S.project = S.projects.find((p) => p.project.id === id) || null;
  S.manuscriptId = S.project?.active_manuscript?.id || S.project?.manuscripts[0]?.id || "";
  await call("projects.set_active", { id });
  S.settings.active_project = id;
  restoreWorkspace();
  announce(`Project ${S.project?.project.title || ""} is active`);
  render();
}

function workspaceKey() { return "ws:" + (S.project?.project.id || ""); }
function saveWorkspace() {
  try { localStorage.setItem(workspaceKey(), JSON.stringify({ view: S.view, params: S.params })); } catch (_) {}
}
function restoreWorkspace() {
  try {
    const w = JSON.parse(localStorage.getItem(workspaceKey()) || "null");
    if (w && w.view && w.view !== "projects") { S.view = w.view; S.params = w.params || {}; }
  } catch (_) {}
}

export async function setManuscript(id) {
  S.manuscriptId = id;
  if (S.project) await call("manuscripts.set_active", { project_id: S.project.project.id, id });
  render();
}

export function manuscript() {
  return S.project?.manuscripts.find((m) => m.id === S.manuscriptId) || null;
}

export function go(view, params = {}) {
  if (!S.info?.library && view !== "settings" && view !== "help") view = "welcome";
  S.view = view;
  S.params = params;
  saveWorkspace();
  render();
  setTimeout(() => mainEl?.querySelector("h1, [data-autofocus]")?.focus?.(), 30);
}

export async function refresh() {
  await loadInfo();
  await loadProjects();
  for (const fn of listeners) try { fn(); } catch (_) {}
  render();
}

// ---- rendering -------------------------------------------------------------------------------

function renderNav() {
  clear(navEl);
  for (const [key, label, ic] of NAV) {
    navEl.append(h("button.nav-item", { type: "button", "aria-current": S.view === key ? "page" : null, on: { click: () => go(key) }, title: `${label} (Alt+${NAV.findIndex((n) => n[0] === key) + 1})` }, icon(ic, 22), h("span", label)));
  }
  navEl.append(h("div.spacer"), h("hr"),
    h("button.nav-item", { type: "button", "aria-current": S.view === "settings" ? "page" : null, on: { click: () => go("settings") } }, icon("settings", 22), h("span", "Settings")),
    h("button.nav-item", { type: "button", "aria-current": S.view === "help" ? "page" : null, on: { click: () => go("help") } }, icon("help", 22), h("span", "Help")));
}

function renderContext() {
  clear(ctxBar);
  const showCtx = S.info?.library && !["projects", "welcome", "settings", "help"].includes(S.view);
  ctxBar.hidden = !showCtx;
  if (!showCtx) return;
  const active = S.projects.filter((p) => !p.project.archived);
  const projSel = select([["", active.length ? "Choose a project" : "No projects yet"], ...active.map((p) => [p.project.id, p.project.title])], S.project?.project.id || "", { id: "ctx-project", "aria-label": "Active project", on: { change: (e) => e.target.value && setProject(e.target.value) } });
  const ms = S.project?.manuscripts || [];
  const msSel = select([...ms.map((m) => [m.id, m.title]), ["__new", "New manuscript…"]], S.manuscriptId, { id: "ctx-manuscript", "aria-label": "Active manuscript", on: { change: async (e) => {
    if (e.target.value === "__new") { e.target.value = S.manuscriptId; await dialogs.newManuscript(); } else setManuscript(e.target.value);
  } } });
  const m = manuscript();
  const styleSel = select(S.styles.map((s) => [s.id, s.title]), m?.style || "apa-7", { id: "ctx-style", "aria-label": "Citation style", disabled: !m, on: { change: async (e) => {
    try { await call("manuscripts.update", { manuscript: { ...m, style: e.target.value } }); await refresh(); } catch (err) { showError(err); }
  } } });
  ctxBar.append(
    h("div.ctx", icon("folder", 20), h("label", { for: "ctx-project" }, "Project:"), projSel),
    h("div.sep"),
    h("div.ctx", icon("doc", 20), h("label", { for: "ctx-manuscript" }, "Manuscript:"), ms.length ? msSel : h("button.btn.small", { type: "button", on: { click: () => dialogs.newManuscript() } }, "Add manuscript")),
    h("div.sep"),
    h("div.ctx", icon("quote", 20), h("label.sr-only", { for: "ctx-style" }, "Citation style"), styleSel),
    h("div.grow"));
}

function renderStatus() {
  clear(statusEl);
  const lib = S.info?.library;
  statusEl.append(
    h("span", { role: "status", "aria-live": "polite", style: { display: "inline-flex", gap: "6px", alignItems: "center" } }, icon(lib?.read_only ? "warn" : "checkCircle", 16), lib ? (lib.read_only ? "Read-only (newer library version)" : S.saveState) : "No library open"),
    lib ? h("span", "|") : null,
    lib ? h("span", { style: { display: "inline-flex", gap: "6px", alignItems: "center" } }, icon("layers", 16), `${lib.counts.references} sources${S.project ? " • " + S.project.project.title : ""}`) : null,
    lib?.sync_folder_warning ? h("span.status-review", { title: "Two computers must never open this library at the same time." }, icon("warn", 16), " Library is in a synced folder") : null,
    h("span.grow"),
    S.jobs.length ? h("button", { type: "button", on: { click: () => dialogs.jobs() }, title: "Background jobs" }, icon("clock", 15), ` ${S.jobs.length} job${S.jobs.length === 1 ? "" : "s"}`) : null,
    S.undo ? h("button", { type: "button", on: { click: doUndo }, title: "Undo (Ctrl+Z)" }, icon("undo", 15), " Undo: " + S.undo) : null,
    h("span.muted", "Interface concept build • local data"),
  );
}

export async function render() {
  if (!root) return;
  if (!S.info?.library && !["settings", "help"].includes(S.view)) S.view = "welcome";
  bodyEl.classList.toggle("nav-collapsed", !!S.settings.layout?.navCollapsed);
  renderNav();
  renderContext();
  renderStatus();
  const v = views[S.view];
  const container = h("div.view", { id: "view-" + S.view });
  clear(mainEl, container);
  if (v) {
    try { await v.render(container, S.params); } catch (e) { showError(e, container); console.error(e); }
  }
}

// ---- undo, shortcuts, palette ------------------------------------------------------------

export async function doUndo() {
  try { const l = await call("undo"); if (l) toast("Undone: " + l, "info"); await refresh(); } catch (e) { showError(e); }
}
export async function doRedo() {
  try { const l = await call("redo"); if (l) toast("Redone: " + l, "info"); await refresh(); } catch (e) { showError(e); }
}

export const commands = [];
export function command(id, label, run, keys = "", group = "") { commands.push({ id, label, run, keys, group }); }

function registerCommands() {
  NAV.forEach(([k, l], i) => command("go." + k, "Go to " + l, () => go(k), "Alt+" + (i + 1), "View"));
  command("go.settings", "Open Settings", () => go("settings"), "", "View");
  command("go.help", "Open Help", () => go("help"), "F1", "Help");
  command("undo", "Undo", doUndo, "Ctrl+Z", "Edit");
  command("redo", "Redo", doRedo, "Ctrl+Y", "Edit");
  command("lib.new", "New library…", () => dialogs.newLibrary(), "", "File");
  command("lib.open", "Open library…", () => dialogs.openLibrary(), "", "File");
  command("project.new", "New project…", () => dialogs.newProject(), "Ctrl+Shift+N", "File");
  command("source.add", "Add source manually…", () => dialogs.addSource(), "Ctrl+N", "File");
  command("source.lookup", "Add source by identifier (DOI, PMID, arXiv, ISBN, URL)…", () => dialogs.lookup(), "Ctrl+I", "File");
  command("source.paste", "Paste a bibliography to review…", () => dialogs.pasteBibliography(), "", "File");
  command("import.file", "Import a bibliographic file…", () => dialogs.importFile(), "Ctrl+O", "File");
  command("import.pdfs", "Import PDF files…", () => dialogs.importPdfs(), "", "File");
  command("export.refs", "Export references…", () => dialogs.exportRefs(), "", "File");
  command("export.package", "Export complete library package…", () => dialogs.exportPackage(), "", "File");
  command("backup.now", "Back up now…", () => dialogs.backupNow(), "", "File");
  command("backup.verify", "Verify a backup…", () => dialogs.verifyBackup(), "", "File");
  command("backup.restore", "Restore a backup…", () => dialogs.restoreBackup(), "", "File");
  command("lib.close", "Close library", async () => { await call("library.close"); await refresh(); go("welcome"); }, "", "File");
  command("edit.find", "Search the library", () => go("library", { focusSearch: true }), "Ctrl+Shift+F", "Edit");
  command("edit.replace", "Find and replace…", () => dialogs.findReplace(), "Ctrl+H", "Edit");
  command("cite.compose", "Compose a citation", () => go("cite", { tab: "workspace" }), "Ctrl+Shift+C", "Cite");
  command("cite.readiness", "Manuscript readiness and changed copies", () => go("cite", { tab: "readiness" }), "", "Cite");
  command("cite.checks", "Style checks", () => go("cite", { tab: "checks" }), "", "Cite");
  command("tools.duplicates", "Find duplicates…", () => dialogs.duplicates(), "", "Tools");
  command("tools.health", "Library health report…", () => dialogs.health(), "", "Tools");
  command("tools.terms", "Term lists (journals, authors, keywords)…", () => dialogs.termLists(), "", "Tools");
  command("tools.files", "Check attached files…", () => dialogs.checkFiles(), "", "Tools");
  command("tools.trash", "Open trash", () => go("library", { view: "trash" }), "", "Tools");
  command("tools.history", "Change history…", () => dialogs.history(), "", "Tools");
  command("tools.network", "Network activity…", () => dialogs.network(), "", "Tools");
  command("tools.jobs", "Background jobs…", () => dialogs.jobs(), "", "Tools");
  command("tools.api", "Local connections…", () => go("settings", { tab: "api" }), "", "Tools");
  command("view.theme", "Switch light / dark theme", async () => {
    const t = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    await saveSettings({ theme: t });
  }, "", "View");
  command("view.nav", "Collapse or expand navigation", async () => {
    await saveSettings({ layout: { ...(S.settings.layout || {}), navCollapsed: !S.settings.layout?.navCollapsed } });
  }, "Ctrl+B", "View");
  command("help.shortcuts", "Keyboard shortcuts", () => dialogs.shortcuts(), "Ctrl+/", "Help");
  command("help.about", "About Prism Reference Manager", () => dialogs.about(), "", "Help");
  command("help.diagnostics", "Diagnostic bundle…", () => dialogs.diagnostics(), "", "Help");
}

export async function saveSettings(patch) {
  try {
    S.settings = await call("settings.set", { settings: patch });
    applyAppearance();
    render();
  } catch (e) { showError(e); }
}

function palette() {
  if (document.querySelector(".palette")) return;
  const prev = document.activeElement;
  const input = h("input", { type: "search", placeholder: "Type a command or search sources…", "aria-label": "Command", role: "combobox", "aria-expanded": "true", "aria-controls": "pal-list", "aria-autocomplete": "list" });
  const list = h("ul#pal-list", { role: "listbox" });
  const box = h("div.palette", { role: "dialog", "aria-label": "Command palette" }, input, list);
  let items = [];
  let sel = 0;
  let sourceHits = [];
  function draw() {
    const q = input.value.toLowerCase().trim();
    items = commands.filter((c) => !q || c.label.toLowerCase().includes(q) || c.group.toLowerCase().includes(q)).slice(0, 30);
    const extra = sourceHits.map((s) => ({ label: "Open source: " + s.label, run: () => go("library", { select: s.id }), keys: "" }));
    items = [...items, ...extra];
    clear(list, ...items.map((c, i) => h("li", { role: "option", id: "pal" + i, "aria-selected": String(i === sel), on: { mousedown: (e) => { e.preventDefault(); sel = i; run(); } } }, h("span", c.label), h("span.muted.small", c.keys || c.group || ""))));
    input.setAttribute("aria-activedescendant", "pal" + sel);
  }
  let t;
  input.addEventListener("input", () => {
    sel = 0; draw();
    clearTimeout(t);
    const q = input.value.trim();
    if (q.length >= 3 && S.info?.library) t = setTimeout(async () => {
      try {
        const r = await call("search.run", { query: q, limit: 8 });
        sourceHits = r.results.ref_ids.slice(0, 8).map((id) => ({ id, label: r.labels[id] || id }));
        draw();
      } catch (_) {}
    }, 200);
    else sourceHits = [];
  });
  function close() { box.remove(); prev?.focus?.(); }
  function run() { const c = items[sel]; close(); if (c) c.run(); }
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") { sel = Math.min(items.length - 1, sel + 1); draw(); e.preventDefault(); }
    else if (e.key === "ArrowUp") { sel = Math.max(0, sel - 1); draw(); e.preventDefault(); }
    else if (e.key === "Enter") { run(); e.preventDefault(); }
    else if (e.key === "Escape") close();
  });
  input.addEventListener("blur", () => setTimeout(() => document.querySelector(".palette") && !box.contains(document.activeElement) && close(), 150));
  document.body.append(box);
  draw();
  input.focus();
}

function inText(e) {
  const t = e.target;
  return t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
}

function keys(e) {
  const ctrl = e.ctrlKey || e.metaKey;
  const k = e.key.toLowerCase();
  if (ctrl && k === "k") { e.preventDefault(); palette(); return; }
  if (ctrl && e.shiftKey && k === "f") { e.preventDefault(); go("library", { focusSearch: true }); return; }
  if (ctrl && e.shiftKey && k === "c") { e.preventDefault(); go("cite", { tab: "workspace", ...(S.params.selected ? { ref: S.params.selected } : {}) }); return; }
  if (ctrl && e.shiftKey && k === "n") { e.preventDefault(); dialogs.newProject(); return; }
  if (ctrl && !e.shiftKey && k === "n" && S.info?.library) { e.preventDefault(); dialogs.addSource(); return; }
  if (ctrl && k === "i" && S.info?.library) { e.preventDefault(); dialogs.lookup(); return; }
  if (ctrl && k === "o" && S.info?.library) { e.preventDefault(); dialogs.importFile(); return; }
  if (ctrl && k === "h" && S.info?.library) { e.preventDefault(); dialogs.findReplace(); return; }
  if (ctrl && k === "b") { e.preventDefault(); commands.find((c) => c.id === "view.nav").run(); return; }
  if (ctrl && k === "/") { e.preventDefault(); dialogs.shortcuts(); return; }
  if (ctrl && k === "f") {
    const s = mainEl.querySelector("[data-search]");
    if (s) { e.preventDefault(); s.focus(); s.select?.(); }
    return;
  }
  if (e.key === "F1") { e.preventDefault(); go("help"); return; }
  if (e.altKey && /^[1-6]$/.test(e.key)) { e.preventDefault(); go(NAV[+e.key - 1][0]); return; }
  if (e.key === "F6") {
    // Move focus between regions: navigation, context bar, main content.
    e.preventDefault();
    const regions = [navEl, ctxBar.hidden ? null : ctxBar, mainEl].filter(Boolean);
    const i = regions.findIndex((r) => r.contains(document.activeElement));
    const next = regions[(i + (e.shiftKey ? regions.length - 1 : 1)) % regions.length];
    next.querySelector("button, select, input, [tabindex='0']")?.focus();
    return;
  }
  if (!inText(e) && ctrl && k === "z" && !e.shiftKey) { e.preventDefault(); doUndo(); return; }
  if (!inText(e) && ctrl && (k === "y" || (k === "z" && e.shiftKey))) { e.preventDefault(); doRedo(); }
}

// ---- menu bar --------------------------------------------------------------------------------

function buildMenus(bar) {
  const groups = ["File", "Edit", "View", "Tools", "Help"];
  for (const g of groups) {
    const items = commands.filter((c) => c.group === g || (g === "View" && c.group === "Cite"));
    const rootEl = h("div.menu-root");
    const title = h("button.menu-title", { type: "button", "aria-haspopup": "menu", "aria-expanded": "false" }, g);
    let menu = null;
    function close() { if (menu) { menu.remove(); menu = null; title.setAttribute("aria-expanded", "false"); document.removeEventListener("mousedown", outside, true); } }
    function outside(e) { if (!rootEl.contains(e.target)) close(); }
    function open(focusFirst) {
      bar.querySelectorAll(".menu").forEach((m) => m.remove());
      bar.querySelectorAll(".menu-title").forEach((t) => t.setAttribute("aria-expanded", "false"));
      menu = h("div.menu", { role: "menu", "aria-label": g });
      for (const c of items) {
        menu.append(h("button", { role: "menuitem", type: "button", tabindex: "-1", on: { click: () => { close(); c.run(); } } }, h("span", c.label), h("span.kbd", c.keys)));
      }
      menu.addEventListener("keydown", (e) => {
        const its = [...menu.querySelectorAll('[role="menuitem"]')];
        const i = its.indexOf(document.activeElement);
        if (e.key === "ArrowDown") { its[(i + 1) % its.length].focus(); e.preventDefault(); }
        if (e.key === "ArrowUp") { its[(i - 1 + its.length) % its.length].focus(); e.preventDefault(); }
        if (e.key === "Escape") { close(); title.focus(); }
        if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
          const roots = [...bar.querySelectorAll(".menu-title")];
          const j = roots.indexOf(title);
          close();
          const n = roots[(j + (e.key === "ArrowRight" ? 1 : roots.length - 1)) % roots.length];
          n.click();
        }
      });
      rootEl.append(menu);
      title.setAttribute("aria-expanded", "true");
      document.addEventListener("mousedown", outside, true);
      if (focusFirst) menu.querySelector('[role="menuitem"]')?.focus();
    }
    title.addEventListener("click", () => (menu ? close() : open(true)));
    title.addEventListener("keydown", (e) => { if (e.key === "ArrowDown") { e.preventDefault(); open(true); } });
    rootEl.append(title);
    bar.append(rootEl);
  }
}

// ---- start ---------------------------------------------------------------------------------

export async function start() {
  registerCommands();
  root = document.getElementById("app");
  const menubar = h("nav.menubar", { "aria-label": "Menu bar" });
  buildMenus(menubar);
  ctxBar = h("div.contextbar", { role: "region", "aria-label": "Active project, manuscript and style" });
  navEl = h("nav.nav", { "aria-label": "Main navigation" });
  mainEl = h("main", { id: "main", tabindex: "-1" });
  bodyEl = h("div.shell-body", navEl, mainEl);
  statusEl = h("footer.statusbar", { "aria-label": "Status" });
  clear(root, h("a.skip", { href: "#main" }, "Skip to main content"), menubar, ctxBar, bodyEl, statusEl);
  document.addEventListener("keydown", keys);
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyAppearance);
  on("changed", (p) => { S.undo = p.undo; S.redo = p.redo; S.saveState = "Saved locally"; renderStatus(); });
  on("job", (j) => { S.jobs.push(j); toast(j.message, j.ok ? "success" : "error"); renderStatus(); });
  on("api-pair", (p) => dialogs.pairRequest(p));
  on("update-available", (u) => toast(`Version ${u.version} is available.`, "info", { sticky: true, action: { label: "Review", run: () => go("settings", { tab: "about" }) } }));
  on("capture", (c) => {
    const what = c.count === 1 ? `“${(c.title || "a source").slice(0, 80)}”` : `${c.count} sources`;
    toast(`Saved from your browser: ${what}. It waits for your review before joining the library.`, "info", { sticky: true, action: { label: "Review", run: async () => dialogs.importReview(await call("import.get", { id: c.batch_id })) } });
  });
  try {
    await loadInfo();
    if (!S.info.library && S.settings.last_library) {
      try { await call("library.open", { path: S.settings.last_library }); await loadInfo(); }
      catch (e) { toast("The last library could not be opened: " + e.message, "error", { detail: e.detail }); }
    }
    await loadProjects();
    S.view = S.info.library ? (S.settings.last_view && S.settings.last_view !== "welcome" ? S.settings.last_view : "projects") : "welcome";
    if (S.view !== "projects") restoreWorkspace();
  } catch (e) {
    showError(e);
  }
  await render();
  window.addEventListener("beforeunload", () => saveWorkspace());
}
