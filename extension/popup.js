import { extractPage } from "./extract.js";
import { candidates } from "./csl.js";
import { settings, startPairing, pairingState, call, DEFAULT_PORT } from "./api.js";

const root = document.getElementById("root");

function el(tag, attrs = {}, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === null || v === undefined || v === false) continue;
    if (k === "on") for (const [ev, fn] of Object.entries(v)) e.addEventListener(ev, fn);
    else if (k === "class") e.className = v;
    else if (k in e && typeof v !== "string") e[k] = v;
    else e.setAttribute(k, v === true ? "" : v);
  }
  for (const c of kids.flat()) if (c !== null && c !== undefined && c !== false) e.append(c instanceof Node ? c : document.createTextNode(String(c)));
  return e;
}

function show(...kids) {
  root.replaceChildren(...kids.flat().filter(Boolean));
  root.querySelector("button.primary, input, select")?.focus();
}

function message(text, kind = "info") {
  return el("p", { class: "msg " + kind, role: kind === "error" ? "alert" : "status" }, text);
}

async function connectScreen(error) {
  const { port } = await settings();
  const portIn = el("input", { type: "number", id: "port", value: String(port), min: "1024", max: "65535" });
  show(
    el("h1", {}, "Connect to Prism Reference Manager"),
    error ? message(error, "error") : null,
    el("ol", {},
      el("li", {}, "Open Prism Reference Manager on this computer."),
      el("li", {}, "Turn on Settings › Local connections › Allow local connections."),
      el("li", {}, "Choose Connect below, then choose Allow in the program.")),
    el("details", {}, el("summary", {}, "Port"), el("label", { for: "port" }, "Port shown in the program's settings"), portIn),
    el("button", { class: "primary", on: { click: async () => {
      await chrome.storage.local.set({ port: +portIn.value || DEFAULT_PORT });
      try { await startPairing(); waitScreen(); } catch (e) { connectScreen(e.message); }
    } } }, "Connect"),
    el("p", { class: "fine" }, "Only programs on this computer are contacted. Nothing is sent anywhere else."),
  );
}

async function waitScreen() {
  show(el("h1", {}, "Waiting for approval"), message("Switch to Prism Reference Manager and choose Allow."), el("button", { on: { click: () => connectScreen() } }, "Cancel"));
  for (let i = 0; i < 120; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    let state;
    try { state = await pairingState(); } catch (e) { return connectScreen(e.message); }
    if (state === "approved") return main();
    if (state === "denied") return connectScreen("The connection was not allowed in the program.");
    if (state === "none") return connectScreen();
  }
  connectScreen("No answer from the program yet. Try again.");
}

async function readTab(tab) {
  const looksPdf = /\.pdf($|[?#])/i.test(tab.url || "");
  try {
    const [r] = await chrome.scripting.executeScript({ target: { tabId: tab.id }, func: extractPage });
    const page = r && r.result;
    if (page) {
      page.isPdf = looksPdf || page.contentType === "application/pdf";
      return page;
    }
  } catch (_) {}
  if (looksPdf) return { url: tab.url, title: tab.title || "", isPdf: true, meta: {}, jsonld: [], coins: [], links: [], pdfLinks: [], dois: [] };
  return null;
}

function summary(it) {
  const c = it.csl || {};
  const a = (c.author || []).map((x) => x.family || x.literal).filter(Boolean);
  const who = a.length > 2 ? `${a[0]} et al.` : a.join(" & ");
  const year = c.issued && c.issued["date-parts"] ? c.issued["date-parts"][0][0] : "";
  const ids = Object.entries(it.identifiers || {}).map(([k, v]) => `${k.toUpperCase()} ${v}`).join(" • ");
  return [who, year, c["container-title"], ids].filter(Boolean).join(" • ");
}

async function main() {
  const { token } = await settings();
  if (!token) return connectScreen();
  show(message("Reading this page…"));
  let options;
  try { options = await call("capture.options"); } catch (e) { return e.kind === "unauthorized" || e.kind === "offline" ? connectScreen(e.message) : show(message(e.message, "error")); }
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const page = tab ? await readTab(tab) : null;
  if (!page) return show(el("h1", {}, "This page can't be read"), message("Browser pages and extension stores can't be captured. Open the article page or the PDF, then try again."));
  const { primary, others } = candidates(page);

  const items = [];
  const form = el("form", { on: { submit: (e) => { e.preventDefault(); save(); } } });
  if (primary) {
    const titleIn = el("textarea", { id: "title", rows: 3 }, primary.title || "");
    const useBox = el("input", { type: "checkbox", id: "use-primary", checked: true });
    const pdfBox = primary.pdf_url ? el("input", { type: "checkbox", id: "pdf-primary", checked: true }) : null;
    const status = el("p", { class: "fine", id: "dup" });
    items.push({ item: primary, use: useBox, pdf: pdfBox, title: titleIn });
    form.append(el("section", { class: "card" },
      el("label", { class: "row strong", for: "use-primary" }, useBox, page.isPdf ? "This PDF" : "This page's paper"),
      el("label", { for: "title", class: "sr" }, "Title"), titleIn,
      el("p", { class: "fine" }, summary(primary) || (primary.csl ? "" : "Details will be looked up from the identifier, or read from the PDF.")),
      status,
      pdfBox ? el("label", { class: "row", for: "pdf-primary" }, pdfBox, "Include the PDF") : el("p", { class: "fine" }, "No PDF link was found on this page.")));
    call("capture.check", { doi: primary.identifiers.doi || (primary.csl && primary.csl.DOI) || "", title: primary.title || "" })
      .then((r) => { if (r.found) { status.textContent = `Already in your library (matched by ${r.by}). Saving again goes to review, where it is marked as a possible duplicate.`; status.className = "msg warn"; } })
      .catch(() => {});
  }
  if (others.length) {
    const list = el("div", { class: "list", role: "group", "aria-label": "Other papers on this page" });
    others.slice(0, 200).forEach((o, i) => {
      const cb = el("input", { type: "checkbox", id: "o" + i, checked: !primary });
      items.push({ item: o, use: cb, pdf: null, title: null });
      list.append(el("label", { class: "row item", for: "o" + i }, cb, el("span", {}, el("span", { class: "t" }, o.title), el("span", { class: "fine" }, summary(o)))));
    });
    const all = el("input", { type: "checkbox", id: "all", checked: !primary, on: { change: (e) => list.querySelectorAll("input").forEach((x) => (x.checked = e.target.checked)) } });
    form.append(el("details", { class: "card", open: !primary },
      el("summary", {}, `${others.length} other paper${others.length === 1 ? "" : "s"} on this page`),
      el("label", { class: "row", for: "all" }, all, "Select all"), list));
  }
  const proj = el("select", { id: "project" }, el("option", { value: "" }, "Library only (no project)"), ...options.projects.map((p) => el("option", { value: p.id, selected: p.id === options.active_project }, p.title)));
  const coll = el("select", { id: "collection" });
  const fillColl = () => coll.replaceChildren(el("option", { value: "" }, "No collection"), ...options.collections.filter((c) => !c.project_id || c.project_id === proj.value).map((c) => el("option", { value: c.id }, c.name)));
  proj.addEventListener("change", fillColl);
  fillColl();
  const labels = el("input", { id: "labels", list: "label-list", placeholder: "Separate with commas" });
  const note = el("textarea", { id: "note", rows: 2, placeholder: "Why this source matters (optional)" });
  form.append(
    el("div", { class: "grid" }, el("label", { for: "project" }, "Project"), proj, el("label", { for: "collection" }, "Collection"), coll, el("label", { for: "labels" }, "Labels"), labels, el("label", { for: "note" }, "Note"), note),
    el("datalist", { id: "label-list" }, ...options.labels.map((l) => el("option", { value: l }))),
    el("button", { class: "primary", type: "submit" }, "Save for review"),
    el("p", { class: "fine" }, `Saves to “${options.library || "your library"}”. Nothing joins the library until you review it in the program.`));
  show(el("h1", {}, "Save to Prism Reference Manager"), form);

  function save() {
    const chosen = items.filter((x) => x.use.checked);
    if (!chosen.length) return form.prepend(message("Choose at least one item.", "error"));
    // Ask for access to the PDF sites now, while the click still counts as the person's action.
    const origins = [...new Set(chosen.filter((x) => x.pdf && x.pdf.checked && x.item.pdf_url).map((x) => new URL(x.item.pdf_url).origin + "/*"))];
    const perm = origins.length ? chrome.permissions.request({ origins }) : Promise.resolve(true);
    perm.then(async (granted) => {
      const payload = {
        items: chosen.map((x) => ({
          csl: x.item.csl ? { ...x.item.csl, title: x.title ? x.title.value.trim() : x.item.csl.title } : null,
          identifiers: x.item.identifiers || {},
          url: x.item.url,
          title: x.title ? x.title.value.trim() : x.item.title,
          pdf_url: granted && x.pdf && x.pdf.checked ? x.item.pdf_url : "",
          want_pdf: !!(granted && x.pdf && x.pdf.checked),
        })),
        project_id: proj.value,
        collection_id: coll.value,
        labels: labels.value.split(",").map((s) => s.trim()).filter(Boolean),
        note: note.value.trim(),
        page_url: page.url,
      };
      show(message(`Saving ${payload.items.length} item${payload.items.length === 1 ? "" : "s"}…`));
      const r = await chrome.runtime.sendMessage({ type: "save", payload });
      if (!r || !r.ok) return show(el("h1", {}, "Not saved"), message((r && r.error) || "Something went wrong.", "error"), el("button", { on: { click: main } }, "Try again"));
      show(el("h1", {}, "Saved for review"),
        message(`${r.result.count === 1 ? "1 item waits" : `${r.result.count} items wait`} in Prism Reference Manager's import review.`, "ok"),
        ...(r.result.notes || []).map((n) => message(n, "warn")),
        !granted ? message("PDFs were not included because access to the site was not allowed.", "warn") : null,
        el("button", { class: "primary", on: { click: () => window.close() } }, "Done"));
    });
  }
}

main();
