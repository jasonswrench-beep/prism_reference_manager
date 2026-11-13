// Quick-add bar, special-character palette with keyboard entry, the first-run tour, and the
// shortcut editor. Shortcuts are data: every command has a default key, and Settings can change
// it (settings.shortcuts maps command ids to keys; an empty string turns a shortcut off).

import { call } from "./api.js";
import { h, clear, icon, btn, toast, modal, field, showError } from "./dom.js";
import { S, go, refresh, saveSettings, commands } from "./app.js";

// ---- shortcuts -------------------------------------------------------------------------------

const KEYNAME = { " ": "Space", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right", Escape: "Esc" };

/** The key combination of a keyboard event, written like "Ctrl+Shift+F". */
export function comboOf(e) {
  let k = e.key;
  if (["Control", "Shift", "Alt", "Meta"].includes(k)) return "";
  k = KEYNAME[k] || (k.length === 1 ? k.toUpperCase() : k);
  // Shift changes the character of punctuation and digits; name the physical key instead.
  if (e.shiftKey && /^Digit\d$/.test(e.code)) k = e.code.slice(5);
  if (e.shiftKey && e.code === "Slash") k = "/";
  return [e.ctrlKey || e.metaKey ? "Ctrl" : "", e.altKey ? "Alt" : "", e.shiftKey ? "Shift" : "", k].filter(Boolean).join("+");
}

/** A combination may be global only if it cannot be ordinary typing. */
export function usable(combo) {
  return /^(Ctrl|Alt)\+/.test(combo) || /^(Shift\+)?F\d{1,2}$/.test(combo);
}

export function keyFor(c) {
  const o = S.settings?.shortcuts || {};
  return Object.prototype.hasOwnProperty.call(o, c.id) ? o[c.id] : c.defaultKeys || "";
}

/** Combination -> command, honouring the person's changes. */
export function bindings() {
  const m = new Map();
  for (const c of commands) {
    const k = keyFor(c);
    if (k && !m.has(k)) m.set(k, c);
  }
  return m;
}

/** Keys that work only inside a view, listed with the shortcuts but not changeable here. */
export const FIXED = [
  ["Ctrl+F", "Search in the current view"], ["F6 / Shift+F6", "Move between navigation, context bar and content"],
  ["Alt+X (in a text field)", "Turn the code before the cursor into a character, for example 00E9 into é"],
  ["Reader: V H U S C T D A E", "Select, Highlight, Underline, Strikeout, Comment, Text box, Draw, Area, Ellipse"],
  ["Reader: 1 to 6", "Choose a mark colour and its meaning"],
  ["Reader: PageUp / PageDown", "Previous / next page"], ["Reader: Ctrl+= / Ctrl+-", "Zoom in / out"],
  ["Lists: arrows, Home, End", "Move through rows; Space selects; Shift+arrows extends"],
];

/** Change shortcuts: press a new combination for any command. */
export function editShortcuts() {
  const draft = { ...(S.settings.shortcuts || {}) };
  const body = h("div");
  const eff = (c) => (Object.prototype.hasOwnProperty.call(draft, c.id) ? draft[c.id] : c.defaultKeys || "");
  const draw = (msg) => {
    const owner = new Map();
    for (const c of commands) if (eff(c)) owner.set(eff(c), (owner.get(eff(c)) || []).concat(c));
    clear(body,
      h("p.small.muted", "Choose Change, then press the new combination. It must include Ctrl or Alt (or be a function key), so ordinary typing is never taken. Changes apply when you save."),
      msg ? h("div.notice.warn", icon("warn"), msg) : null,
      h("table.table", h("tbody", commands.filter((c) => c.label).map((c) => {
        const k = eff(c);
        const clash = k && owner.get(k).length > 1;
        return h("tr", h("td", c.label), h("td.small.muted", c.group || ""),
          h("td", k ? h("span.kbd-key", k) : h("span.muted.small", "None"), clash ? h("span.badge.attention", { title: "Also used by " + owner.get(k).filter((x) => x !== c).map((x) => x.label).join(", ") }, " Also used") : null),
          h("td", btn("Change", { small: true, onClick: (ev) => capture(c, ev.currentTarget) }), k ? btn("Remove", { small: true, kind: "ghost", onClick: () => { draft[c.id] = ""; draw(); } }) : null,
            Object.prototype.hasOwnProperty.call(draft, c.id) ? btn("Default", { small: true, kind: "ghost", onClick: () => { delete draft[c.id]; draw(); } }) : null));
      }))));
  };
  const capture = (c, button) => {
    button.textContent = "Press keys…";
    const onKey = (e) => {
      e.preventDefault(); e.stopPropagation();
      if (e.key === "Escape") { document.removeEventListener("keydown", onKey, true); draw(); return; }
      const k = comboOf(e);
      if (!k) return;
      document.removeEventListener("keydown", onKey, true);
      if (!usable(k)) { draw(`${k} cannot be a shortcut: it would get in the way of typing. Include Ctrl or Alt.`); return; }
      const other = commands.find((x) => x !== c && eff(x) === k);
      if (other) draft[other.id] = "";
      draft[c.id] = k;
      draw(other ? `${k} was used by “${other.label}”, which now has no shortcut.` : "");
    };
    document.addEventListener("keydown", onKey, true);
  };
  draw();
  return modal("Change keyboard shortcuts", body, { size: "wide", buttons: [
    { label: "Restore all defaults", run: () => { for (const k of Object.keys(draft)) delete draft[k]; draw(); return false; } },
    { label: "Cancel" },
    { label: "Save", kind: "primary", run: async () => { await saveSettings({ shortcuts: draft }); toast("Shortcuts saved", "success"); } }] }).done;
}

/** The printable list of every shortcut. */
export function shortcutList() {
  const rows = commands.filter((c) => keyFor(c)).map((c) => [keyFor(c), c.label]).concat(FIXED);
  const table = h("table.table.shortcut-sheet", h("tbody", rows.map(([k, d]) => h("tr", h("td", h("span.kbd-key", k)), h("td", d)))));
  modal("Keyboard shortcuts", h("div", table, h("p.small.muted", "Every action is also in the command palette (Ctrl+K) and the menus.")), { buttons: [
    { label: "Change shortcuts…", run: () => { setTimeout(() => editShortcuts(), 0); } },
    { label: "Print", run: () => { printSheet(rows); return false; } },
    { label: "Close", kind: "primary" }] });
}

function printSheet(rows) {
  const sheet = h("div.print-sheet", h("h1", "Prism Reference Manager — keyboard shortcuts"), h("table", h("tbody", rows.map(([k, d]) => h("tr", h("td", k), h("td", d))))));
  document.body.append(sheet);
  document.body.classList.add("printing-sheet");
  const done = () => { document.body.classList.remove("printing-sheet"); sheet.remove(); window.removeEventListener("afterprint", done); };
  window.addEventListener("afterprint", done);
  window.print();
  setTimeout(done, 60000);
}

// ---- quick-add bar ---------------------------------------------------------------------------

/** Paste or type an identifier, a web address or a reference, and add it in one step. */
export function quickAdd() {
  if (!S.info?.library) { toast("Open or create a library first.", "info"); return; }
  if (document.querySelector(".quick-add")) { document.querySelector(".quick-add input").focus(); return; }
  const prev = document.activeElement;
  const input = h("input", { type: "text", placeholder: "Paste a DOI, PMID, arXiv ID, ISBN, web address or a reference, then press Enter", "aria-label": "Quick add", "aria-describedby": "qa-status" });
  const status = h("div#qa-status.quick-status", { role: "status", "aria-live": "polite" });
  const box = h("div.palette.quick-add", { role: "dialog", "aria-label": "Quick add" }, input, status);
  let pending = null;
  const close = () => { box.remove(); prev?.focus?.(); };
  const project = () => S.project?.project.id || "";
  const show = (...kids) => clear(status, ...kids);
  async function look() {
    const text = input.value.trim();
    if (!text) return;
    pending = null;
    show(h("span.muted", "Looking up…"));
    try {
      let ids = await call("lookup.identifiers", { text });
      if (!ids.length && /^https?:\/\//i.test(text)) ids = [["url", text]];
      if (!ids.length) {
        // Not an identifier: treat it as a reference to parse and review.
        pending = { kind: "text", text };
        show(h("div", "Not an identifier. Press Enter to read it as a reference; you review it before anything is added."));
        return;
      }
      const [kind, id] = ids[0];
      const r = await call("lookup.resolve", { kind, id, project_id: project() });
      if (!r.ok || !r.candidate) { show(h("span.status-review", icon("warn", 14), ` ${r.message || "Nothing was found."}`)); return; }
      const c = r.candidate;
      pending = { kind: "ref", ref: c, conflicts: r.conflicts };
      show(h("div", h("strong", c.fields.title || "(no title)"), h("div.small.muted", [(c.creators || []).map((x) => x.name.family || x.name.literal).slice(0, 3).join(", "), c.issued?.year, c.fields.container_title].filter(Boolean).join(" • ")),
        r.conflicts.length ? h("div.small.status-review", icon("warn", 13), " Resolvers disagree; you will review it first.") : null,
        h("div.small.muted", `Press Enter to add${S.project ? ` to ${S.project.project.title}` : " to the library"}, Esc to cancel.`)));
    } catch (e) { show(h("span.status-review", icon("warn", 14), " " + e.message)); }
  }
  async function add() {
    if (!pending) return look();
    const dialogs = await import("./dialogs.js");
    try {
      if (pending.kind === "text") {
        const b = await call("import.stage", { name: "Quick add", text: pending.text, format: "paste", service: "paste", project_id: project() });
        close(); dialogs.importReview(b); return;
      }
      const b = await call("lookup.stage", { refs: [pending.ref], name: "Quick add", service: "lookup", project_id: project() });
      const rec = b.records[0];
      if (rec && rec.status === "new" && !pending.conflicts.length) {
        await call("import.commit", { id: b.id });
        await refresh();
        const id = (await call("import.get", { id: b.id })).records[0]?.committed_id;
        close();
        toast(`Added: ${pending.ref.fields.title || "source"}`, "success", { action: id ? { label: "Show it", run: () => go("library", { select: id }) } : null });
      } else {
        close();
        if (rec?.status === "duplicate") toast("This looks like a source already in the library. Review it before adding.", "info");
        dialogs.importReview(b);
      }
    } catch (e) { show(h("span.status-review", icon("warn", 14), " " + e.message)); }
  }
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); add(); }
    else if (e.key === "Escape") { e.preventDefault(); close(); }
  });
  input.addEventListener("input", () => { pending = null; clear(status); });
  input.addEventListener("paste", () => setTimeout(look, 0));
  box.addEventListener("focusout", () => setTimeout(() => document.querySelector(".quick-add") && !box.contains(document.activeElement) && !document.querySelector(".modal") && close(), 200));
  document.body.append(box);
  input.focus();
}

// ---- special characters ----------------------------------------------------------------------

const CHARS = [
  ["Accented letters", "á à â ä ã å ā ă ą æ ç ć č ď đ é è ê ë ē ė ę ě ğ í ì î ï ī į ı ł ľ ń ň ñ ó ò ô ö õ ø ō ő œ ř ś š ş ș ß ť ţ ț ú ù û ü ū ů ű ų ý ÿ ź ż ž þ ð Á À Â Ä Ã Å Æ Ç Č É È Ê Ë Í Î Ï Ł Ñ Ó Ô Ö Ø Œ Š Ú Ü Ý Ž"],
  ["Punctuation", "– — ‘ ’ “ ” „ « » ‹ › … · • ‐ ‑ § ¶ † ‡ ′ ″ ¡ ¿"],
  ["Symbols", "© ® ™ ° ± × ÷ ≈ ≠ ≤ ≥ ∞ √ ∑ ∏ ∫ ∂ ∆ ∇ ∈ ∉ ⊂ ∩ ∪ → ← ↔ ⇒ ⇔ ‰ µ ¼ ½ ¾ ² ³ ¹ € £ ¥ ¢"],
  ["Greek", "α β γ δ ε ζ η θ ι κ λ μ ν ξ ο π ρ σ ς τ υ φ χ ψ ω Α Β Γ Δ Ε Ζ Η Θ Ι Κ Λ Μ Ν Ξ Ο Π Ρ Σ Τ Υ Φ Χ Ψ Ω"],
  ["Cyrillic", "а б в г д е ё ж з и й к л м н о п р с т у ф х ц ч ш щ ъ ы ь э ю я А Б В Г Д Е Ж З И К Л М Н О П Р С Т У Ф Х Ц Ч Ш Щ Э Ю Я"],
];

function charName(ch) {
  return "U+" + ch.codePointAt(0).toString(16).toUpperCase().padStart(4, "0");
}

let lastField = null;
const isTextField = (t) => ((t instanceof HTMLInputElement && ["text", "search", "url", ""].includes(t.type)) || t instanceof HTMLTextAreaElement) && !t.closest(".chars-dialog");
document.addEventListener("focusin", (e) => { if (isTextField(e.target)) lastField = e.target; });

function insertAt(el, text) {
  if (!el || !el.isConnected) { navigator.clipboard?.writeText(text).catch(() => {}); toast(`${text} copied; paste it where you need it.`, "info"); return; }
  const s = el.selectionStart ?? el.value.length, e2 = el.selectionEnd ?? s;
  el.value = el.value.slice(0, s) + text + el.value.slice(e2);
  el.selectionStart = el.selectionEnd = s + text.length;
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.focus();
}

/** A palette of accents, punctuation, symbols and other scripts, inserted where the cursor was. */
export function specialChars() {
  // The field being typed in; else the last one used (the palette may be opened from a menu).
  const target = isTextField(document.activeElement) ? document.activeElement : lastField?.isConnected ? lastField : null;
  const q = h("input", { type: "search", placeholder: "Type a character code such as 00E9, or a character to find", "aria-label": "Find a character" });
  const grid = h("div.chars-grid");
  const draw = () => {
    const v = q.value.trim();
    const code = /^(?:u\+)?([0-9a-f]{4,6})$/i.exec(v);
    clear(grid,
      code ? h("div", btn(`Insert ${String.fromCodePoint(parseInt(code[1], 16))} (U+${code[1].toUpperCase()})`, { kind: "primary", onClick: () => pick(String.fromCodePoint(parseInt(code[1], 16))) })) : null,
      ...CHARS.map(([name, list]) => {
        const chars = list.split(" ").filter((c) => !v || code || c === v || c.toLowerCase() === v.toLowerCase() || c.normalize("NFD").replace(/\p{M}/gu, "") === v);
        return chars.length ? h("div", h("h3.small", name), h("div.chars-row", chars.map((c) => h("button.char", { type: "button", title: charName(c), "aria-label": `${c} ${charName(c)}`, on: { click: () => pick(c) } }, c)))) : null;
      }));
  };
  let m;
  const pick = (c) => { m.close(c); setTimeout(() => insertAt(target, c), 0); };
  q.addEventListener("input", draw);
  draw();
  m = modal("Special characters", h("div.chars-dialog", q, grid, h("p.small.muted", "In any text field you can also type a four-digit character code and press Alt+X, for example 00E9 then Alt+X gives é (for longer codes type U+ first, as in U+1F600).")), { buttons: [{ label: "Close" }] });
  setTimeout(() => q.focus(), 0);
}

/** Alt+X in a text field: turn the hexadecimal code before the cursor into its character, or a
 *  character back into its code. */
export function altX(e) {
  const el = e.target;
  if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) return false;
  const pos = el.selectionStart;
  const before = el.value.slice(0, pos);
  // Four hex digits, or up to six after "U+" (letters a to f are also hex digits, so a longer
  // plain run would swallow the end of a word).
  const m = /U\+([0-9A-Fa-f]{4,6})$/i.exec(before) || /([0-9A-Fa-f]{4})$/.exec(before);
  if (m) {
    const cp = parseInt(m[1], 16);
    if (cp > 0x10ffff) return false;
    const ch = String.fromCodePoint(cp);
    el.value = before.slice(0, before.length - m[0].length) + ch + el.value.slice(pos);
    el.selectionStart = el.selectionEnd = pos - m[0].length + ch.length;
  } else if (before) {
    const ch = [...before].pop();
    const hex = ch.codePointAt(0).toString(16).toUpperCase().padStart(4, "0");
    el.value = before.slice(0, before.length - ch.length) + hex + el.value.slice(pos);
    el.selectionStart = el.selectionEnd = pos - ch.length + hex.length;
  } else return false;
  el.dispatchEvent(new Event("input", { bubbles: true }));
  return true;
}

// ---- first-run tour --------------------------------------------------------------------------

const TOUR = [
  ["Collect sources", "Add a source from its DOI, PubMed, arXiv or ISBN number, or a web address, with Quick add (Ctrl+Shift+A) or File › Add source by identifier. Import files from other programs, a folder of PDFs, or use the browser button. Everything is reviewed before it is added.", "Open Quick add", () => quickAdd()],
  ["Read and mark", "Open a PDF from the Library to highlight, comment and take quotations. Each colour carries a meaning you choose. Marks are kept in the library; the PDF itself is never changed.", "Go to the Library", () => go("library")],
  ["Copy a citation", "In Cite, choose sources and copy the in-text citation and the reference in your manuscript's style. Quotation pages must be confirmed first, so every citation points to the right page.", "Go to Cite", () => go("cite", { tab: "workspace" })],
  ["Backups", "The library is backed up automatically, daily by default, and each backup is checked after it is written. Choose a folder on another drive in Settings so a disk failure cannot take both.", "Open backup settings", () => go("settings", { tab: "library" })],
  ["Find anything", "Ctrl+K opens the command palette: every action and every source. Ctrl+/ lists the keyboard shortcuts, which you can change. Help (F1) explains each part.", "Open the command palette", null],
];

/** A short tour: collecting, reading, citing, backups. Shown once; always available from Help. */
export function tour(step = 0) {
  const [title, text, action, run] = TOUR[step];
  const last = step === TOUR.length - 1;
  const dots = h("div.tour-dots", { "aria-hidden": "true" }, TOUR.map((_, i) => h("span" + (i === step ? ".on" : ""))));
  const done = () => { if (!S.settings.tour_done) saveSettings({ tour_done: true }).catch(() => {}); };
  const m = modal(`Getting started (${step + 1} of ${TOUR.length}): ${title}`, h("div.tour", h("p", text), dots), { buttons: [
    { label: "Skip the tour", run: () => { done(); } },
    ...(step ? [{ label: "Back", run: () => { setTimeout(() => tour(step - 1), 0); } }] : []),
    { label: action, run: () => { done(); setTimeout(() => (run ? run() : document.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true }))), 0); } },
    { label: last ? "Finish" : "Next", kind: "primary", run: () => { if (last) done(); else setTimeout(() => tour(step + 1), 0); } }] });
  return m;
}
