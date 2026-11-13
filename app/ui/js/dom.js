// Small DOM helpers, icons, accessible modals, toasts and confirmation dialogs.

export function h(tag, attrs, ...children) {
  const m = tag.match(/^([a-z0-9-]+)?((?:[.#][\w-]+)*)$/i);
  const el = document.createElement((m && m[1]) || "div");
  if (m && m[2]) {
    for (const part of m[2].match(/[.#][\w-]+/g) || []) {
      if (part[0] === ".") el.classList.add(part.slice(1));
      else el.id = part.slice(1);
    }
  }
  if (attrs !== undefined && attrs !== null && attrs !== false && (typeof attrs !== "object" || attrs instanceof Node || Array.isArray(attrs))) {
    children.unshift(attrs);
    attrs = null;
  }
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "on") for (const [ev, fn] of Object.entries(v)) el.addEventListener(ev, fn);
    else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
    else if (k === "html") el.innerHTML = v;
    else if (k === "class") el.className = v;
    // Text boxes take their text as a property: a "value" attribute is ignored by <textarea>.
    else if (k === "value" && (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement)) { el.value = String(v); el.defaultValue = String(v); }
    else if (k in el && typeof v !== "string" && k !== "list") el[k] = v;
    else el.setAttribute(k, v === true ? "" : v);
  }
  // Elements made here skip null, undefined and false children, like the constructor does.
  el.append = (...kids) => append(el, kids);
  append(el, children);
  return el;
}

const nativeAppend = Element.prototype.append;
function append(el, children) {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    if (Array.isArray(c)) append(el, c);
    else nativeAppend.call(el, c instanceof Node ? c : document.createTextNode(String(c)));
  }
}

export function clear(el, ...children) {
  el.replaceChildren();
  append(el, children);
  return el;
}

const P = {
  folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
  library: "M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zm0 0v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
  reader: "M3 5c3-1 6-1 9 1 3-2 6-2 9-1v14c-3-1-6-1-9 1-3-2-6-2-9-1zM12 6v14",
  notes: "M7 3h8l4 4v14H7zM15 3v4h4M10 11h6M10 15h6M5 6v15h11",
  quote: "M5 17c2.5-1 4-3 4-6H6V6h5v5c0 4-2 7-6 8zm9 0c2.5-1 4-3 4-6h-3V6h5v5c0 4-2 7-6 8z",
  connect: "M18 5a2.5 2.5 0 1 1 0 .1M6 12a2.5 2.5 0 1 1 0 .1M18 19a2.5 2.5 0 1 1 0 .1M8.2 11l7.6-4.4M8.2 13l7.6 4.4",
  settings: "M12 9a3 3 0 1 1 0 6 3 3 0 0 1 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  help: "M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18zM9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17h.01",
  plus: "M12 5v14M5 12h14",
  download: "M12 4v11M7 10l5 5 5-5M5 19h14",
  upload: "M12 20V9M7 14l5-5 5 5M5 5h14",
  copy: "M9 9h10v11H9zM5 15V4h10",
  check: "M5 12l5 5L20 7",
  checkCircle: "M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18zM8 12l3 3 5-6",
  warn: "M12 3l10 18H2zM12 10v5M12 18h.01",
  info: "M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18zM12 11v6M12 7.5h.01",
  x: "M6 6l12 12M18 6L6 18",
  chevronDown: "M6 9l6 6 6-6",
  chevronRight: "M9 6l6 6-6 6",
  chevronUp: "M6 15l6-6 6 6",
  search: "M11 18a7 7 0 1 1 0-14 7 7 0 0 1 0 14zM16 16l5 5",
  filter: "M3 5h18l-7 8v6l-4 2v-8z",
  doc: "M7 3h7l5 5v13H7zM14 3v5h5M10 13h6M10 17h6",
  pdf: "M7 3h7l5 5v13H7zM14 3v5h5M9.5 15.5h1.5a1.5 1.5 0 0 0 0-3H9.5v5",
  book: "M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2zM6 19h13v2H6",
  edit: "M4 20h4L19 9l-4-4L4 16zM14 6l4 4",
  trash: "M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3",
  undo: "M9 14L4 9l5-5M4 9h11a5 5 0 0 1 0 10h-3",
  redo: "M15 14l5-5-5-5M20 9H9a5 5 0 0 0 0 10h3",
  tag: "M3 12V4h8l10 10-8 8zM7.5 8.5h.01",
  layers: "M12 3l9 5-9 5-9-5zM3 13l9 5 9-5M3 17.5l9 5 9-5",
  play: "M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18zM10 8l6 4-6 4z",
  pen: "M4 20l4-1L19 8l-3-3L5 16zM14 7l3 3",
  highlighter: "M9 11l-5 5v4h4l5-5M13 7l4-4 4 4-4 4M9 11l4 4 4-4-4-4z",
  underline: "M7 4v7a5 5 0 0 0 10 0V4M5 20h14",
  strike: "M5 12h14M16 6a4 4 0 0 0-4-2c-3 0-4 2-4 3.5M8 18a4 4 0 0 0 4 2c3 0 4-2 4-3.5",
  comment: "M4 5h16v11H9l-5 4z",
  text: "M5 6V4h14v2M12 4v16M9 20h6",
  cursor: "M5 3l14 8-6 2-2 6z",
  area: "M4 7V4h3M17 4h3v3M20 17v3h-3M7 20H4v-3M10 4h4M10 20h4M4 10v4M20 10v4",
  ellipse: "M12 19c4.4 0 8-3.1 8-7s-3.6-7-8-7-8 3.1-8 7 3.6 7 8 7z",
  minus: "M5 12h14",
  external: "M14 4h6v6M20 4l-9 9M18 14v5H5V6h5",
  list: "M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  save: "M5 4h11l3 3v13H5zM8 4v5h8V4M8 20v-6h8v6",
  refresh: "M20 11a8 8 0 0 0-14.9-4M4 5v4h4M4 13a8 8 0 0 0 14.9 4M20 19v-4h-4",
  shield: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z",
  beaker: "M9 3h6M10 3v6L4 19a1.5 1.5 0 0 0 1.3 2h13.4A1.5 1.5 0 0 0 20 19l-6-10V3",
  globe: "M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18zM3 12h18M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9z",
  arrowRight: "M5 12h14M13 6l6 6-6 6",
  dots: "M5 12h.01M12 12h.01M19 12h.01",
  star: "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.5 2.9 1-6.1L3.2 9.5l6.1-.9z",
  clock: "M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18zM12 7v5l3 2",
  merge: "M6 4v5a5 5 0 0 0 5 5h7M15 11l3 3-3 3M6 20V4",
  question: "M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18zM9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17h.01",
  outline: "M4 6h16M8 12h12M12 18h8",
  grip: "M9 6h.01M9 12h.01M9 18h.01M15 6h.01M15 12h.01M15 18h.01",
  keyboard: "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M7 14h10",
  eye: "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 15a3 3 0 1 1 0-6 3 3 0 0 1 0 6z",
  archive: "M3 5h18v4H3zM5 9v10h14V9M10 13h4",
  columns: "M4 4h7v16H4zM13 4h7v16h-7z",
};

export function icon(name, size = 18, label) {
  const ns = "http://www.w3.org/2000/svg";
  const s = document.createElementNS(ns, "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.setAttribute("width", size);
  s.setAttribute("height", size);
  s.setAttribute("fill", "none");
  s.setAttribute("stroke", "currentColor");
  s.setAttribute("stroke-width", "1.8");
  s.setAttribute("stroke-linecap", "round");
  s.setAttribute("stroke-linejoin", "round");
  if (label) {
    s.setAttribute("role", "img");
    s.setAttribute("aria-label", label);
  } else s.setAttribute("aria-hidden", "true");
  const p = document.createElementNS(ns, "path");
  p.setAttribute("d", P[name] || P.doc);
  if (name === "quote" || name === "star") {
    p.setAttribute("fill", "currentColor");
    p.setAttribute("stroke", "none");
  }
  s.append(p);
  return s;
}

export function btn(label, opts = {}) {
  const b = h("button.btn" + (opts.kind ? "." + opts.kind : "") + (opts.small ? ".small" : ""), { type: "button", title: opts.title, "aria-label": opts.aria, disabled: opts.disabled, on: { click: opts.onClick } });
  if (opts.icon) b.append(icon(opts.icon, 17));
  if (label) b.append(h("span", label));
  return b;
}

export function iconBtn(name, label, onClick, pressed) {
  const b = h("button.icon-btn", { type: "button", title: label, "aria-label": label, on: { click: onClick } }, icon(name, 18));
  if (pressed !== undefined) b.setAttribute("aria-pressed", String(!!pressed));
  return b;
}

// ---- live region and toasts ----------------------------------------------------------------
let live;
export function announce(msg) {
  if (!live) {
    live = h("div.sr-only", { "aria-live": "polite", role: "status" });
    document.body.append(live);
  }
  live.textContent = "";
  setTimeout(() => (live.textContent = msg), 30);
}

let toastBox;
export function toast(msg, kind = "info", opts = {}) {
  if (!toastBox) {
    toastBox = h("div.toasts", { role: "region", "aria-label": "Notifications" });
    document.body.append(toastBox);
  }
  const t = h("div.toast." + kind, { role: kind === "error" ? "alert" : "status" },
    icon(kind === "error" ? "warn" : kind === "success" ? "checkCircle" : "info", 18),
    h("div.grow", h("div", msg), opts.detail ? h("details.support", h("summary", "Details for support"), h("pre", opts.detail)) : null,
      opts.action ? h("div.btn-row", { style: { marginTop: "6px" } }, btn(opts.action.label, { small: true, onClick: () => { opts.action.run(); t.remove(); } })) : null),
    iconBtn("x", "Dismiss", () => t.remove()));
  toastBox.append(t);
  if (kind !== "error" && !opts.sticky) setTimeout(() => t.remove(), opts.ms || 6000);
  return t;
}

// ---- modal dialogs ----------------------------------------------------------------------
const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function modal(title, content, { buttons = [], size = "", onClose, initial } = {}) {
  const prev = document.activeElement;
  const titleId = "m" + Math.random().toString(36).slice(2);
  const footer = h("footer");
  const dlg = h("div.modal" + (size ? "." + size : ""), { role: "dialog", "aria-modal": "true", "aria-labelledby": titleId },
    h("header", h("h2", { id: titleId }, title), h("span.grow"), iconBtn("x", "Close", () => close(null))),
    h("div.content", content), footer);
  const back = h("div.modal-back", dlg);
  let resolveFn;
  const done = new Promise((r) => (resolveFn = r));
  function close(v) {
    back.remove();
    document.removeEventListener("keydown", onKey, true);
    if (prev && prev.focus) prev.focus();
    if (onClose) onClose(v);
    resolveFn(v);
  }
  for (const b of buttons) {
    footer.append(btn(b.label, { kind: b.kind, icon: b.icon, onClick: async () => {
      if (b.run) {
        try {
          const v = await b.run();
          if (v === false) return;
          close(v === undefined ? b.value ?? true : v);
        } catch (e) {
          showError(e, dlg.querySelector(".content"));
        }
      } else close(b.value ?? null);
    } }));
  }
  if (!buttons.length) footer.remove();
  function onKey(e) {
    if (!document.body.contains(back)) return;
    if (e.key === "Escape") { e.stopPropagation(); close(null); }
    if (e.key === "Tab") {
      const f = [...dlg.querySelectorAll(FOCUSABLE)].filter((x) => x.offsetParent !== null);
      if (!f.length) return;
      if (e.shiftKey && document.activeElement === f[0]) { e.preventDefault(); f[f.length - 1].focus(); }
      else if (!e.shiftKey && document.activeElement === f[f.length - 1]) { e.preventDefault(); f[0].focus(); }
    }
  }
  document.addEventListener("keydown", onKey, true);
  document.body.append(back);
  setTimeout(() => {
    const target = (initial && dlg.querySelector(initial)) || dlg.querySelector(".content " + FOCUSABLE) || dlg.querySelector(FOCUSABLE);
    target && target.focus();
  }, 20);
  return { close, done, el: dlg };
}

export function confirmDialog(title, message, { ok = "Continue", kind = "primary", cancel = "Cancel", details } = {}) {
  return modal(title, h("div", typeof message === "string" ? h("p", message) : message, details || null), {
    size: "narrow",
    buttons: [{ label: cancel, value: false }, { label: ok, kind, value: true }],
  }).done;
}

export function promptDialog(title, label, value = "", { help, ok = "Save", multiline } = {}) {
  const inp = multiline ? h("textarea", { value }) : h("input", { type: "text", value });
  const id = "p" + Math.random().toString(36).slice(2);
  inp.id = id;
  return modal(title, h("div.field", h("label", { for: id }, label), inp, help ? h("div.help", help) : null), {
    size: "narrow",
    buttons: [{ label: "Cancel", value: null }, { label: ok, kind: "primary", run: () => inp.value }],
  }).done;
}

export function showError(e, where) {
  const msg = (e && e.message) || String(e);
  const detail = e && e.detail;
  if (where) {
    where.querySelectorAll(".notice.error").forEach((n) => n.remove());
    where.prepend(h("div.notice.error", { role: "alert" }, icon("warn"), h("div", h("div", msg), detail ? h("details.support", h("summary", "Details for support"), h("pre", detail)) : null)));
  } else toast(msg, "error", { detail });
}

export function fmtDate(iso) {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d)) return iso;
  const now = new Date();
  const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  if (d.toDateString() === now.toDateString()) return "Today, " + time;
  const y = new Date(now); y.setDate(now.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return "Yesterday, " + time;
  return d.toLocaleDateString([], { month: "short", day: "numeric", year: d.getFullYear() === now.getFullYear() ? undefined : "numeric" }) + ", " + time;
}

export function runsHtml(runs) {
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  return (runs || []).map((r) => {
    let t = esc(r.text).replace(/\n/g, "<br>");
    if (r.sup) t = `<sup>${t}</sup>`;
    if (r.italic) t = `<i>${t}</i>`;
    return t;
  }).join("");
}

export function select(options, value, attrs = {}) {
  const s = h("select", attrs);
  for (const o of options) {
    const [v, l] = Array.isArray(o) ? o : [o, o];
    const opt = h("option", { value: v }, l);
    if (v === value) opt.selected = true;
    s.append(opt);
  }
  return s;
}

export function field(label, control, help) {
  const id = control.id || "f" + Math.random().toString(36).slice(2);
  control.id = id;
  return h("div.field", h("label", { for: id }, label), control, help ? h("div.help", help) : null);
}

export function tabs(names, current, onPick, { seg = false, label = "Sections" } = {}) {
  const list = h("div." + (seg ? "seg" : "tabs"), { role: "tablist", "aria-label": label });
  names.forEach(([key, text], i) => {
    const t = h("button", { role: "tab", type: "button", "aria-selected": String(key === current), tabindex: key === current ? "0" : "-1", on: { click: () => onPick(key) } }, text);
    t.addEventListener("keydown", (e) => {
      if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        const n = names[(i + (e.key === "ArrowRight" ? 1 : names.length - 1)) % names.length][0];
        onPick(n);
        setTimeout(() => list.querySelector('[aria-selected="true"]')?.focus(), 0);
      }
    });
    list.append(t);
  });
  return list;
}
