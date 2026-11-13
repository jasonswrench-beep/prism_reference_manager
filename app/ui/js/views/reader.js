// Reader: PDF tabs, page rendering, durable marks, annotation panel, search, outline,
// annotated-copy export, external annotation import and file replacement with re-anchoring.

import * as pdfjsLib from "../../vendor/pdfjs/pdf.min.js";
import { call, readAttachment, readPicked } from "../api.js";
import { h, clear, icon, iconBtn, btn, toast, modal, confirmDialog, promptDialog, select, field, showError, tabs, announce, runsHtml } from "../dom.js";
import { S, registerView, go, refresh, manuscript } from "../app.js";

pdfjsLib.GlobalWorkerOptions.workerSrc = "vendor/pdfjs/pdf.worker.min.js";

const TOOLS = [
  ["select", "Select", "cursor", "v"], ["highlight", "Highlight", "highlighter", "h"], ["underline", "Underline", "underline", "u"], ["strikeout", "Strikeout", "strike", "s"],
  ["note", "Comment", "comment", "c"], ["freetext", "Text box", "text", "t"], ["ink", "Draw", "pen", "d"], ["region", "Area", "area", "a"], ["ellipse", "Ellipse", "ellipse", "e"],
];
const MEANINGS = [["quotation", "Direct quotation"], ["important", "Important"], ["question", "Question"], ["disagree", "Disagree"], ["method", "Method"], ["evidence", "Evidence"], ["definition", "Definition"]];
const COLORS = [["#ffd400", "Yellow"], ["#5fe3e3", "Cyan"], ["#a0e870", "Green"], ["#ffaad4", "Pink"], ["#c3a6ff", "Purple"], ["#ff8c5a", "Orange"]];
// What each colour means by default; the person can change it (Settings layouts.reader_scheme).
const DEFAULT_SCHEME = { "#ffd400": "quotation", "#a0e870": "evidence", "#5fe3e3": "method", "#ffaad4": "question", "#c3a6ff": "definition", "#ff8c5a": "disagree" };
const scheme = () => ({ ...DEFAULT_SCHEME, ...(S.settings.layouts?.reader_scheme || {}) });
const meaningLabel = (m) => MEANINGS.find((x) => x[0] === m)?.[1] || "";
async function editScheme() {
  const cur = scheme();
  const sels = COLORS.map(([c, n], i) => [c, n, i, select([["", "No meaning"], ...MEANINGS], cur[c] || "", { "aria-label": `Meaning of ${n}` })]);
  const ok = await modal("Colour meanings", h("div",
    h("p.small.muted", "New marks in a colour get its meaning. Keys 1 to 6 choose a colour while reading."),
    h("table.table", h("tbody", sels.map(([c, n, i, sel]) => h("tr", h("td", h("span.swatch", { style: { background: c, display: "inline-block", width: "16px", height: "16px", borderRadius: "3px" } })), h("td", `${n} (${i + 1})`), h("td", sel)))))),
    { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Save", kind: "primary", value: true }] }).done;
  if (!ok) return false;
  const next = Object.fromEntries(sels.map(([c, , , sel]) => [c, sel.value]));
  S.settings = await call("settings.set", { settings: { layouts: { ...(S.settings.layouts || {}), reader_scheme: next } } });
  return true;
}

const R = {
  docs: [], // {ref, att, title}
  active: 0,
  pdf: null,
  pdfKey: "",
  scale: 0, // 0 = fit width
  mode: "continuous",
  tool: "select",
  color: "#ffd400",
  anns: [],
  selectedAnn: null,
  page: 1,
  panelTab: "annotations",
  leftTab: "thumbs",
  pageViews: [],
  filter: "",
  pageText: [],
  searchHits: [],
  distraction: false,
};

function relRects(clientRects, pageEl, viewport) {
  // Convert client rectangles to page-relative rectangles of the unrotated page.
  const box = pageEl.getBoundingClientRect();
  const view = viewport.viewBox; // [x0,y0,x1,y1] in PDF units
  const W = view[2] - view[0], H = view[3] - view[1];
  const out = [];
  for (const r of clientRects) {
    if (r.width < 1 || r.height < 1) continue;
    const [x1, y1] = viewport.convertToPdfPoint(r.left - box.left, r.top - box.top);
    const [x2, y2] = viewport.convertToPdfPoint(r.right - box.left, r.bottom - box.top);
    const minX = Math.min(x1, x2), maxX = Math.max(x1, x2), minY = Math.min(y1, y2), maxY = Math.max(y1, y2);
    out.push({ x: (minX - view[0]) / W, y: (view[3] - maxY) / H, w: (maxX - minX) / W, h: (maxY - minY) / H });
  }
  // Merge rectangles on the same line.
  out.sort((a, b) => a.y - b.y || a.x - b.x);
  const merged = [];
  for (const r of out) {
    const last = merged[merged.length - 1];
    if (last && Math.abs(last.y - r.y) < 0.004 && Math.abs(last.h - r.h) < 0.006 && r.x <= last.x + last.w + 0.01) {
      const right = Math.max(last.x + last.w, r.x + r.w);
      last.x = Math.min(last.x, r.x); last.w = right - last.x;
    } else merged.push({ ...r });
  }
  return merged;
}

function toViewport(rect, viewport) {
  const view = viewport.viewBox;
  const W = view[2] - view[0], H = view[3] - view[1];
  const px1 = view[0] + rect.x * W, px2 = view[0] + (rect.x + rect.w) * W;
  const py1 = view[3] - rect.y * H, py2 = view[3] - (rect.y + rect.h) * H;
  const [a, b, c, d] = viewport.convertToViewportRectangle([px1, py2, px2, py1]);
  return { left: Math.min(a, c), top: Math.min(b, d), width: Math.abs(c - a), height: Math.abs(d - b) };
}

function pointRel(e, pageEl, viewport) {
  const box = pageEl.getBoundingClientRect();
  const [x, y] = viewport.convertToPdfPoint(e.clientX - box.left, e.clientY - box.top);
  const view = viewport.viewBox;
  return [(x - view[0]) / (view[2] - view[0]), (view[3] - y) / (view[3] - view[1])];
}

async function loadDoc(d) {
  const key = d.att.id;
  if (R.pdfKey === key && R.pdf) return R.pdf;
  if (R.pdf) { try { await R.pdf.destroy(); } catch (_) {} }
  R.pdf = null;
  const data = await readAttachment(d.att.id);
  const task = pdfjsLib.getDocument({ data, cMapUrl: "vendor/pdfjs/cmaps/", cMapPacked: true, standardFontDataUrl: "vendor/pdfjs/standard_fonts/", isEvalSupported: false, enableXfa: false, disableAutoFetch: true });
  task.onPassword = async (updatePassword, reason) => {
    const pw = await promptDialog(reason === 2 ? "Incorrect password" : "Password-protected PDF", "Enter the document password. It is used only to open the file and is not stored.", "", { ok: "Open" });
    if (pw === null) task.destroy(); else updatePassword(pw);
  };
  R.pdf = await task.promise;
  R.pdfKey = key;
  return R.pdf;
}

async function extractText(pdf, onProgress) {
  const pages = [];
  for (let i = 1; i <= pdf.numPages; i++) {
    const p = await pdf.getPage(i);
    const tc = await p.getTextContent();
    let s = "";
    for (const it of tc.items) { s += it.str; if (it.hasEOL) s += "\n"; else if (it.str && !it.str.endsWith(" ")) s += " "; }
    pages.push(s.replace(/[ \t]+\n/g, "\n"));
    onProgress && onProgress(i, pdf.numPages);
  }
  return pages;
}

/** Render one page to a PNG (base64) for recognition: about 200 dots per inch, longest side at
 *  most 3000 pixels, on white. */
async function pageImage(page) {
  const base = page.getViewport({ scale: 1 });
  const scale = Math.min(200 / 72, 3000 / Math.max(base.width, base.height));
  const vp = page.getViewport({ scale });
  const c = document.createElement("canvas");
  c.width = Math.ceil(vp.width); c.height = Math.ceil(vp.height);
  const g = c.getContext("2d");
  g.fillStyle = "#fff"; g.fillRect(0, 0, c.width, c.height);
  await page.render({ canvasContext: g, viewport: vp }).promise;
  const url = c.toDataURL("image/png");
  c.width = c.height = 0;
  return url.slice(url.indexOf(",") + 1);
}

/** Recognize the text of a scanned PDF on this computer and store it for search and quotation.
 *  The file is not changed. Returns the updated attachment, or null when stopped. */
export async function recognizePdf(att, { lang = "", onProgress, stop, wait } = {}) {
  const data = await readAttachment(att.id);
  const pdf = await pdfjsLib.getDocument({ data, isEvalSupported: false, enableXfa: false, cMapUrl: "vendor/pdfjs/cmaps/", cMapPacked: true, standardFontDataUrl: "vendor/pdfjs/standard_fonts/" }).promise;
  const pages = [];
  let used = lang;
  try {
    for (let i = 1; i <= pdf.numPages; i++) {
      if (stop && stop()) return null;
      if (wait) await wait();
      onProgress && onProgress(i - 1, pdf.numPages);
      const page = await pdf.getPage(i);
      const r = await call("ocr.page", { image: await pageImage(page), lang });
      used = used || r.lang;
      pages.push({ text: r.text, words: r.words });
      page.cleanup();
    }
  } finally { await pdf.destroy(); }
  onProgress && onProgress(pdf.numPages, pdf.numPages);
  return await call("files.set_ocr_text", { id: att.id, pages, lang: used });
}

/** Ask for a recognition language, then recognize one or more PDFs with progress and Stop. */
export async function recognizeDialog(atts, onDone) {
  const langs = await call("ocr.languages").catch(() => []);
  if (!langs.length) { toast("Text recognition is not available: no recognition language is installed in Windows (Settings › Time and language › Language).", "warn"); return; }
  const sel = select(langs.map((l) => [l.tag, l.name]), S.settings.ocr_lang && langs.some((l) => l.tag === S.settings.ocr_lang) ? S.settings.ocr_lang : langs[0].tag, { "aria-label": "Language of the text" });
  const ok = await modal(atts.length === 1 ? "Recognize text" : `Recognize text in ${atts.length} scanned PDFs`, h("div",
      h("p", "Text is recognized on this computer by Windows; nothing is sent anywhere. The PDF itself is not changed: the recognized text is stored in the library for search and quotations."),
      field("Language of the text", sel, "Languages come from Windows. To add one, install it with its optical character recognition feature in Windows Settings."),
      h("p.small.muted", "Recognized text can contain errors. Check quotations against the page before citing them.")),
    { buttons: [{ label: "Recognize", kind: "primary", value: true }, { label: "Cancel", value: false }] }).done;
  if (!ok) return;
  call("settings.set", { settings: { ocr_lang: sel.value } }).then(() => (S.settings.ocr_lang = sel.value)).catch(() => {});
  await runRecognition(atts, sel.value, onDone);
}

/** Recognize files as a background job (pause, resume, cancel in the job panel). */
async function runRecognition(atts, lang, onDone) {
  const { startJob } = await import("../jobs.js");
  let done = 0, low = 0;
  const left = [];
  const t = toast("Recognizing text…", "info", { sticky: true, action: { label: "Stop", run: () => job.cancel() } });
  const line = t.querySelector(".grow div") || t;
  const job = startJob({
    label: atts.length === 1 ? `Recognizing text: ${atts[0].original_name}` : `Recognizing text in ${atts.length} PDFs`, source: "Text recognition",
    retry: () => runRecognition(left.length ? left : atts, lang, onDone),
    run: async (job) => {
      job.progress(0, atts.length);
      for (const a of atts) {
        await job.step();
        try {
          const r = await recognizePdf(a, { lang, wait: () => job.step(), onProgress: (i, n) => (line.textContent = `Recognizing text: ${atts.length > 1 ? `file ${done + 1} of ${atts.length}, ` : ""}page ${Math.min(i + 1, n)} of ${n}${job.state === "paused" ? " (paused)" : ""}`) });
          if (!r) break;
          done++;
          if ((r.ocr_quality ?? 1) < 0.75) low++;
          job.progress(done, atts.length);
        } catch (e) {
          if (e?.cancelled) throw e;
          left.push(a); job.fail(`${a.original_name}: ${e.message || e}`);
        }
      }
    },
  });
  try { await job.promise; } catch (e) { showError(e); }
  for (const a of atts.slice(done + left.length)) left.push(a);
  t.remove();
  toast(job.state === "cancelled" ? `Stopped. ${done} files recognized; the rest were left as they were.` : `Text recognized in ${done} file${done === 1 ? "" : "s"}.${low ? ` ${low} may be of poor quality; check them.` : ""}${job.failed ? ` ${job.failed} could not be read (see Background jobs).` : ""}`, low || job.failed ? "warn" : "success");
  onDone && onDone(done);
}

export function qualityText(att) {
  if (att.text_source !== "ocr") return "";
  const q = Math.round((att.ocr_quality ?? 0) * 100);
  return `Text recognized on this computer (${att.ocr_lang || "language not recorded"}); about ${q}% of words look clean.${q < 75 ? " The scan may be poor: check quotations against the page." : ""}`;
}

/** Index every attached PDF that is not yet in the full-text index (runs in the reader context). */
export async function indexAll(onProgress, { pace = 0, stop, wait, list: given } = {}) {
  const list = given || (await call("files.unindexed"));
  let done = 0;
  for (const a of list) {
    if (stop && stop()) break;
    if (wait) await wait();
    if (pace) await new Promise((r) => setTimeout(r, pace));
    try {
      const data = await readAttachment(a.id);
      const pdf = await pdfjsLib.getDocument({ data, isEvalSupported: false, enableXfa: false, cMapUrl: "vendor/pdfjs/cmaps/", cMapPacked: true }).promise;
      const pages = await extractText(pdf);
      await call("files.set_page_text", { id: a.id, pages });
      await pdf.destroy();
    } catch (_) {}
    done++;
    onProgress && onProgress(done, list.length);
  }
  return done;
}

registerView("reader", {
  async render(el, params) {
    if (params.ref) {
      try {
        const d = await call("refs.get", { id: params.ref, project_id: S.project?.project.id || "" });
        const pdfs = d.attachments.filter((a) => a.mime === "application/pdf" && !a.superseded);
        const att = d.attachments.find((a) => a.id === params.att) || pdfs[0];
        if (!att) {
          el.append(h("div.empty", h("h3", "No PDF is attached to this source"), h("p", d.ref.fields.title), h("div.btn-row", { style: { justifyContent: "center" } },
            btn("Attach a PDF…", { kind: "primary", onClick: async () => { const r = await call("files.attach_pick", { ref_id: d.ref.id, mode: "managed" }); if (!r?.cancelled) go("reader", { ref: d.ref.id }); } }),
            d.ref.fields.url ? btn("Open the source page", { onClick: () => call("system.open_url", { url: d.ref.fields.url }) }) : null,
            d.ref.fields.doi ? btn("Open the DOI page", { onClick: () => call("system.open_url", { url: "https://doi.org/" + d.ref.fields.doi }) }) : null)));
          return;
        }
        let idx = R.docs.findIndex((x) => x.att.id === att.id);
        if (idx < 0) { R.docs.push({ ref: d.ref, att, title: att.original_name }); idx = R.docs.length - 1; }
        R.docs[idx].att = att;
        if (R.active !== idx || R.pdfKey !== att.id) R.page = 1;
        R.active = idx;
        params.ref = null;
      } catch (e) { showError(e, el); return; }
    }
    if (!R.docs.length) {
      el.append(h("h1.view-title", { tabindex: "-1" }, "Reader"), h("div.empty", h("h3", "No document open"), h("p", "Open a PDF from the Library or the reading queue. Marks are stored in the library and never change the original file."), btn("Go to Library", { kind: "primary", onClick: () => go("library") })));
      return;
    }
    const d = R.docs[R.active];
    const project = S.project?.project.id || "";
    if (params.page) { R.page = params.page; params.page = null; }
    const docTabs = h("div.doc-tabs", { role: "tablist", "aria-label": "Open documents" }, R.docs.map((x, i) => h("div.doc-tab", { role: "tab", "aria-selected": String(i === R.active), tabindex: "0", on: { click: () => { R.active = i; go("reader"); }, keydown: (e) => e.key === "Enter" && (R.active = i, go("reader")) } },
      icon("pdf", 16), h("span.ellipsis", x.title), iconBtn("x", "Close " + x.title, (e) => { e.stopPropagation(); R.docs.splice(i, 1); R.active = Math.max(0, Math.min(R.active, R.docs.length - 1)); R.pdfKey = ""; go("reader"); }))));
    el.append(docTabs);
    if (d.att.text_source === "ocr") {
      el.append(h("div.notice" + ((d.att.ocr_quality ?? 1) < 0.75 ? ".warn" : ""), { style: { marginBottom: "8px" } }, icon("info"), h("span.grow", qualityText(d.att)),
        btn("Recognize again…", { small: true, onClick: () => recognizeDialog([d.att], async () => { const r = await call("refs.get", { id: d.ref.id }); d.att = r.attachments.find((x) => x.id === d.att.id) || d.att; go("reader"); }) })));
    } else if (d.att.state === "image-only" || R.noText === d.att.id) {
      el.append(h("div.notice.warn", { style: { marginBottom: "8px" } }, icon("warn"), h("span.grow", "This looks like a scanned PDF without a text layer, so its words cannot be selected, quoted or searched. Recognize the text on this computer to make them available. You can still use Area, Draw and Comment marks."),
        btn("Recognize text…", { small: true, kind: "primary", onClick: () => recognizeDialog([d.att], async () => { const r = await call("refs.get", { id: d.ref.id }); d.att = r.attachments.find((x) => x.id === d.att.id) || d.att; R.panelTab = "text"; go("reader"); }) })));
    }
    if (d.att.state && d.att.state !== "ok" && d.att.state !== "large" && d.att.state !== "image-only") {
      const msg = { protected: "This PDF is password-protected or encrypted. You can read it if pdf.js can open it; marks are kept in the library layer and an annotated copy may not be possible.", malformed: "The PDF structure is damaged. It may still render; the file is kept unchanged.", "image-only": "This looks like a scanned PDF without a text layer. Text selection, quotation capture and search need OCR, which is planned as an optional local step. You can still use Area, Draw and Comment marks.", placeholder: "The file is a cloud placeholder. Make it available offline in your sync program, then reopen it.", missing: "The file is missing. Relink it from Tools › Check attached files." }[d.att.state];
      if (msg) el.append(h("div.notice.warn", { style: { marginBottom: "8px" } }, icon("warn"), msg));
    }
    // Toolbar
    const toolBtns = TOOLS.map(([k, label, ic, key]) => h("button.tool", { type: "button", "aria-pressed": String(R.tool === k), title: `${label} (${key.toUpperCase()})`, on: { click: () => setTool(k) } }, icon(ic, 17), label));
    const colorSel = select(COLORS.map(([c, n], i) => [c, `${i + 1} ${n}${meaningLabel(scheme()[c]) ? " — " + meaningLabel(scheme()[c]) : ""}`]), R.color, { "aria-label": "Mark colour and meaning", style: { width: "190px" }, on: { change: (e) => (R.color = e.target.value) } });
    const schemeBtn = iconBtn("settings", "Colour meanings", async () => { if (await editScheme()) go("reader"); });
    const search = h("input", { type: "search", placeholder: "Search in PDF", "aria-label": "Search in PDF", "data-search": "1", style: { width: "180px" } });
    const zoomSel = select([["0", "Fit width"], ["0.75", "75%"], ["1", "100%"], ["1.25", "125%"], ["1.5", "150%"], ["2", "200%"]], String(R.scale), { "aria-label": "Zoom", style: { width: "110px" }, on: { change: (e) => { R.scale = +e.target.value; drawPages(); } } });
    const modeSel = select([["continuous", "Continuous"], ["single", "Single page"], ["two", "Two pages"]], R.mode, { "aria-label": "Page layout", style: { width: "130px" }, on: { change: (e) => { R.mode = e.target.value; drawPages(); } } });
    const themeSel = select([["light", "Light page"], ["dark", "Dark page"], ["sepia", "Sepia page"]], S.settings.reader_theme || "light", { "aria-label": "Reading theme", style: { width: "120px" }, on: { change: async (e) => { await call("settings.set", { settings: { reader_theme: e.target.value } }); S.settings.reader_theme = e.target.value; docEl.className = "doc reader-theme-" + e.target.value; } } });
    const toolbar = h("div.toolbar", { role: "toolbar", "aria-label": "Reader tools" }, ...toolBtns, colorSel, schemeBtn, h("span.sep"), search,
      iconBtn("chevronUp", "Previous match", () => nextHit(-1)), iconBtn("chevronDown", "Next match", () => nextHit(1)), h("span.sep"),
      iconBtn("minus", "Zoom out (Ctrl+-)", () => zoom(-1)), zoomSel, iconBtn("plus", "Zoom in (Ctrl+=)", () => zoom(1)), modeSel, themeSel,
      iconBtn("eye", "Distraction-free reading", () => { R.distraction = !R.distraction; layout.classList.toggle("no-thumbs", R.distraction); side.hidden = R.distraction; }, R.distraction));
    el.append(toolbar);
    const thumbs = h("div.thumbs", { role: "navigation", "aria-label": "Pages" });
    const docEl = h("div.doc.reader-theme-" + (S.settings.reader_theme || "light"), { tabindex: "0", "aria-label": "Document pages" });
    const pagesEl = h("div.pages" + (R.mode === "two" ? ".two-page" : ""));
    docEl.append(pagesEl);
    const side = h("div.pane", { style: { padding: 0, display: "flex", flexDirection: "column" } });
    side.hidden = R.distraction;
    const layout = h("div.reader" + (R.distraction ? ".no-thumbs" : ""), thumbs, docEl, side);
    el.append(layout);

    let pdf;
    try { pdf = await loadDoc(d); }
    catch (e) {
      clear(pagesEl, h("div.empty", h("h3", "This PDF could not be opened"), h("p", e.message), h("div.btn-row", { style: { justifyContent: "center" } }, btn("Open in default app", { onClick: () => call("files.open_external", { id: d.att.id }) }), btn("Check files", { onClick: () => import("../dialogs.js").then((m) => m.checkFiles()) }))));
      return;
    }
    R.anns = await call("ann.list", { attachment_id: d.att.id, project_id: project });
    // Index text in the background the first time a document is opened.
    if (!d.att.indexed) {
      extractText(pdf).then(async (pages) => { R.pageText = pages; await call("files.set_page_text", { id: d.att.id, pages }); d.att.indexed = true; noTextCheck(pages); }).catch(() => {});
    } else {
      call("files.page_text", { id: d.att.id }).then((p) => { R.pageText = p; noTextCheck(p); }).catch(() => {});
    }
    // A PDF whose pages carry almost no text is a scan: offer recognition once.
    function noTextCheck(pages) {
      if (d.att.text_source === "ocr" || d.att.state === "image-only" || R.noText === d.att.id || !pages.length) return;
      const chars = pages.reduce((n, x) => n + x.trim().length, 0);
      if (chars < 25 * pages.length) { R.noText = d.att.id; go("reader"); }
    }

    function setTool(k) {
      R.tool = k;
      toolBtns.forEach((b, i) => b.setAttribute("aria-pressed", String(TOOLS[i][0] === k)));
      pagesEl.querySelectorAll(".drawLayer").forEach((l) => (l.hidden = !["ink", "region", "ellipse", "note", "freetext"].includes(k)));
      announce(TOOLS.find((t) => t[0] === k)[1] + " tool");
    }
    function zoom(dir) {
      const cur = R.scale || currentFitScale || 1;
      R.scale = Math.max(0.4, Math.min(4, +(cur + dir * 0.25).toFixed(2)));
      zoomSel.value = String(R.scale);
      drawPages();
    }
    let currentFitScale = 1;
    let observer;

    async function drawPages() {
      pagesEl.className = "pages" + (R.mode === "two" ? " two-page" : "");
      clear(pagesEl);
      R.pageViews = [];
      observer?.disconnect();
      const first = await pdf.getPage(1);
      const vp1 = first.getViewport({ scale: 1 });
      const avail = docEl.clientWidth - 48;
      currentFitScale = Math.max(0.4, (R.mode === "two" ? avail / 2 - 10 : avail) / vp1.width);
      const scale = R.scale || currentFitScale;
      const showPages = R.mode === "single" ? [R.page] : Array.from({ length: pdf.numPages }, (_, i) => i + 1);
      observer = new IntersectionObserver((entries) => {
        for (const e of entries) if (e.isIntersecting) renderPage(+e.target.dataset.page);
        const vis = entries.filter((e) => e.isIntersecting).map((e) => +e.target.dataset.page);
        if (vis.length && R.mode !== "single") { R.page = Math.min(...vis); markThumb(); savePosition(); }
      }, { root: docEl, rootMargin: "600px 0px" });
      for (const n of showPages) {
        const p = n === 1 ? first : await pdf.getPage(n);
        const vp = p.getViewport({ scale });
        const wrap = h("div.page-wrap", { "data-page": n, role: "region", "aria-label": `Page ${n} of ${pdf.numPages}${d.att.page_labels?.[n - 1] && d.att.page_labels[n - 1] !== String(n) ? ", labelled " + d.att.page_labels[n - 1] : ""}`, style: { width: vp.width + "px", height: vp.height + "px" } },
          h("div.page-label", d.att.page_labels?.[n - 1] && d.att.page_labels[n - 1] !== String(n) ? `PDF page ${n} • label ${d.att.page_labels[n - 1]}` : `Page ${n}`));
        pagesEl.append(wrap);
        R.pageViews[n] = { page: p, viewport: vp, wrap, rendered: false };
        observer.observe(wrap);
      }
      if (R.mode === "single") { renderPage(R.page); }
      else setTimeout(() => R.pageViews[R.page]?.wrap.scrollIntoView({ block: "start" }), 30);
    }

    async function renderPage(n) {
      const pv = R.pageViews[n];
      if (!pv || pv.rendered) return;
      pv.rendered = true;
      const { page, viewport, wrap } = pv;
      const ratio = window.devicePixelRatio || 1;
      const canvas = h("canvas", { width: Math.floor(viewport.width * ratio), height: Math.floor(viewport.height * ratio), style: { width: viewport.width + "px", height: viewport.height + "px" }, "aria-hidden": "true" });
      wrap.prepend(canvas);
      const ctx = canvas.getContext("2d");
      await page.render({ canvasContext: ctx, viewport, transform: ratio !== 1 ? [ratio, 0, 0, ratio, 0, 0] : null, annotationMode: pdfjsLib.AnnotationMode.DISABLE }).promise.catch(() => {});
      const tl = h("div.textLayer");
      wrap.append(tl);
      wrap.style.setProperty("--scale-factor", viewport.scale);
      try {
        const textLayer = new pdfjsLib.TextLayer({ textContentSource: page.streamTextContent(), container: tl, viewport });
        await textLayer.render();
      } catch (_) {}
      const annLayer = h("div.annLayer");
      const drawLayer = h("div.drawLayer", { hidden: !["ink", "region", "ellipse", "note", "freetext"].includes(R.tool) });
      wrap.append(annLayer, drawLayer);
      paintAnnotations(n);
      tl.addEventListener("mouseup", () => setTimeout(() => maybeMark(n), 10));
      drawLayer.addEventListener("pointerdown", (e) => startDraw(e, n));
    }

    function paintAnnotations(n) {
      const pv = R.pageViews[n];
      if (!pv || !pv.rendered) return;
      const layer = pv.wrap.querySelector(".annLayer");
      clear(layer);
      for (const a of R.anns.filter((x) => x.page_index === n - 1)) {
        if (a.kind === "ink") {
          const ns = "http://www.w3.org/2000/svg";
          const svg = document.createElementNS(ns, "svg");
          svg.classList.add("ink");
          for (const stroke of a.ink) {
            const pts = stroke.map(([x, y]) => { const r = toViewport({ x, y, w: 0, h: 0 }, pv.viewport); return `${r.left},${r.top}`; }).join(" ");
            const pl = document.createElementNS(ns, "polyline");
            pl.setAttribute("points", pts); pl.setAttribute("fill", "none"); pl.setAttribute("stroke", a.color); pl.setAttribute("stroke-width", "2"); pl.setAttribute("stroke-linecap", "round");
            svg.append(pl);
          }
          layer.append(svg);
          const bb = boundsOfInk(a.ink);
          const r = toViewport(bb, pv.viewport);
          layer.append(h("div.ann" + (R.selectedAnn === a.id ? ".selected" : ""), { style: { left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px" }, title: a.comment || "Drawing", on: { click: () => selectAnn(a.id) } }));
          continue;
        }
        a.rects.forEach((rect, i) => {
          const r = toViewport(rect, pv.viewport);
          const el2 = h("div.ann." + a.kind + (R.selectedAnn === a.id ? ".selected" : ""), {
            style: { left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px", background: a.kind === "highlight" ? a.color : a.kind === "note" ? a.color : null, color: a.color, borderColor: a.color },
            title: (a.comment || a.text || a.kind).slice(0, 200), role: "button", tabindex: i === 0 ? "-1" : null, "aria-label": `${a.kind} mark: ${(a.text || a.comment || "").slice(0, 80)}`, on: { click: (e) => { e.stopPropagation(); selectAnn(a.id); } } });
          if (a.kind === "note") el2.append(icon("comment", 14));
          if (a.kind === "freetext") el2.textContent = a.text;
          layer.append(el2);
        });
      }
    }
    const boundsOfInk = (ink) => {
      const pts = ink.flat();
      const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
      return { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) };
    };

    async function maybeMark(n) {
      if (!["highlight", "underline", "strikeout"].includes(R.tool)) return;
      const sel = window.getSelection();
      if (!sel || sel.isCollapsed) return;
      const pv = R.pageViews[n];
      const range = sel.getRangeAt(0);
      if (!pv.wrap.contains(range.commonAncestorContainer)) { toast("Select text on one page at a time.", "info"); return; }
      const rects = relRects([...range.getClientRects()], pv.wrap, pv.viewport);
      const text = sel.toString().replace(/\s+/g, " ").trim();
      if (!rects.length || !text) return;
      const pageText = (R.pageText[n - 1] || "").replace(/\s+/g, " ");
      const at = pageText.indexOf(text);
      const prefix = at > 0 ? pageText.slice(Math.max(0, at - 40), at) : "";
      const suffix = at >= 0 ? pageText.slice(at + text.length, at + text.length + 40) : "";
      sel.removeAllRanges();
      await saveNew({ kind: R.tool, rects, text, prefix, suffix, page_index: n - 1, meaning: scheme()[R.color] || (R.tool === "highlight" ? "quotation" : "") });
    }

    function startDraw(e, n) {
      const pv = R.pageViews[n];
      const layer = e.currentTarget;
      if (R.tool === "note" || R.tool === "freetext") {
        const [x, y] = pointRel(e, pv.wrap, pv.viewport);
        (async () => {
          const text = await promptDialog(R.tool === "note" ? "Comment" : "Text box", R.tool === "note" ? "Comment" : "Text", "", { multiline: true, ok: "Add" });
          if (text === null) return;
          await saveNew({ kind: R.tool, rects: [{ x, y, w: R.tool === "note" ? 0.03 : 0.25, h: R.tool === "note" ? 0.025 : 0.05 }], comment: R.tool === "note" ? text : "", text: R.tool === "freetext" ? text : "", page_index: n - 1 });
        })();
        return;
      }
      layer.setPointerCapture(e.pointerId);
      const start = pointRel(e, pv.wrap, pv.viewport);
      const stroke = [start];
      const ghost = h("div.ann." + (R.tool === "ink" ? "rect" : R.tool), { style: { borderColor: R.color, color: R.color } });
      pv.wrap.querySelector(".annLayer").append(ghost);
      const move = (ev) => {
        const p = pointRel(ev, pv.wrap, pv.viewport);
        if (R.tool === "ink") {
          stroke.push(p);
          const r = toViewport({ x: p[0], y: p[1], w: 0, h: 0 }, pv.viewport);
          pv.wrap.querySelector(".annLayer").append(h("div", { style: { position: "absolute", left: r.left - 1 + "px", top: r.top - 1 + "px", width: "3px", height: "3px", background: R.color, borderRadius: "50%" } }));
        } else {
          const rect = { x: Math.min(start[0], p[0]), y: Math.min(start[1], p[1]), w: Math.abs(p[0] - start[0]), h: Math.abs(p[1] - start[1]) };
          const r = toViewport(rect, pv.viewport);
          Object.assign(ghost.style, { left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px" });
          ghost.dataset.rect = JSON.stringify(rect);
        }
      };
      const up = async () => {
        layer.removeEventListener("pointermove", move);
        layer.removeEventListener("pointerup", up);
        if (R.tool === "ink") {
          if (stroke.length > 2) await saveNew({ kind: "ink", ink: [stroke], rects: [], page_index: n - 1 });
          else paintAnnotations(n);
        } else if (ghost.dataset.rect) {
          const rect = JSON.parse(ghost.dataset.rect);
          if (rect.w > 0.005 && rect.h > 0.005) {
            let text = R.tool === "region" ? textInRect(n, rect) : "", origin = "";
            if (R.tool === "region" && d.att.text_source === "ocr") { text = await call("files.ocr_text_in", { id: d.att.id, page: n - 1, rects: [rect] }).catch(() => ""); origin = text ? "ocr" : ""; }
            await saveNew({ kind: R.tool === "region" ? "region" : "ellipse", rects: [rect], page_index: n - 1, text, text_origin: origin });
          }
          else paintAnnotations(n);
        }
      };
      layer.addEventListener("pointermove", move);
      layer.addEventListener("pointerup", up);
    }

    function textInRect(n, rect) {
      // Text under a region, from the rendered text layer.
      const pv = R.pageViews[n];
      const box = pv.wrap.getBoundingClientRect();
      const r = toViewport(rect, pv.viewport);
      const out = [];
      for (const span of pv.wrap.querySelectorAll(".textLayer span")) {
        const b = span.getBoundingClientRect();
        const cx = b.left - box.left + b.width / 2, cy = b.top - box.top + b.height / 2;
        if (cx >= r.left && cx <= r.left + r.width && cy >= r.top && cy <= r.top + r.height) out.push(span.textContent);
      }
      return out.join(" ").replace(/\s+/g, " ").trim();
    }

    async function saveNew(a) {
      const pv = R.pageViews[a.page_index + 1];
      const view = pv.viewport.viewBox;
      try {
        const saved = await call("ann.save", { annotation: { attachment_id: d.att.id, project_id: project, color: R.color, page_width: view[2] - view[0], page_height: view[3] - view[1], rotation: 0, author: "", ...a } });
        R.anns.push(saved);
        R.selectedAnn = saved.id;
        paintAnnotations(a.page_index + 1);
        drawPanel();
        announce(`${a.kind} added on page ${a.page_index + 1}`);
      } catch (e) { showError(e); }
    }

    function selectAnn(id) {
      R.selectedAnn = id;
      const a = R.anns.find((x) => x.id === id);
      if (a) {
        for (let n = 1; n < R.pageViews.length; n++) paintAnnotations(n);
        R.pageViews[a.page_index + 1]?.wrap.scrollIntoView({ block: "center" });
      }
      drawPanel();
    }

    // ---- thumbnails and outline ---------------------------------------------------------------
    const markThumb = () => thumbs.querySelectorAll("button[data-p]").forEach((b) => b.setAttribute("aria-current", String(+b.dataset.p === R.page)));
    async function drawThumbs() {
      clear(thumbs);
      const lt = tabs([["thumbs", "Pages"], ["outline", "Outline"]], R.leftTab, (k) => { R.leftTab = k; drawThumbs(); }, { label: "Navigation" });
      lt.style.width = "100%";
      thumbs.append(lt);
      if (R.leftTab === "outline") {
        const ol = await pdf.getOutline().catch(() => null);
        if (!ol?.length) { thumbs.append(h("p.small.muted", "This PDF has no outline.")); return; }
        const list = h("ul.tree", { style: { width: "100%" } });
        const add = (items, depth) => { for (const it of items) { list.append(h("li", h("button.btn.ghost.small", { type: "button", style: { paddingLeft: 6 + depth * 10 + "px", whiteSpace: "normal", textAlign: "left" }, on: { click: async () => { try { const dest = typeof it.dest === "string" ? await pdf.getDestination(it.dest) : it.dest; const idx = await pdf.getPageIndex(dest[0]); gotoPage(idx + 1); } catch (_) {} } } }, it.title))); if (it.items?.length) add(it.items, depth + 1); } };
        add(ol, 0);
        thumbs.append(list);
        return;
      }
      for (let n = 1; n <= pdf.numPages; n++) {
        const b = h("button", { type: "button", "data-p": n, "aria-current": String(n === R.page), "aria-label": `Go to page ${n}`, on: { click: () => gotoPage(n) } }, h("canvas", { width: 80, height: 104 }), String(d.att.page_labels?.[n - 1] || n));
        thumbs.append(b);
      }
      const io = new IntersectionObserver(async (entries) => {
        for (const e of entries) if (e.isIntersecting && !e.target.dataset.done) {
          e.target.dataset.done = "1";
          const n = +e.target.dataset.p;
          const p = await pdf.getPage(n);
          const vp = p.getViewport({ scale: 80 / p.getViewport({ scale: 1 }).width });
          const c = e.target.querySelector("canvas");
          c.width = vp.width; c.height = vp.height;
          p.render({ canvasContext: c.getContext("2d"), viewport: vp }).promise.catch(() => {});
        }
      }, { root: thumbs });
      thumbs.querySelectorAll("button[data-p]").forEach((b) => io.observe(b));
    }
    function gotoPage(n) {
      R.page = Math.max(1, Math.min(pdf.numPages, n));
      if (R.mode === "single") drawPages();
      else R.pageViews[R.page]?.wrap.scrollIntoView({ block: "start" });
      markThumb();
      announce(`Page ${R.page} of ${pdf.numPages}`);
    }
    let posT;
    function savePosition() {
      clearTimeout(posT);
      if (!project) return;
      posT = setTimeout(() => call("membership.position", { project_id: project, ref_id: d.ref.id, position: { attachment_id: d.att.id, page_index: R.page - 1, scroll: 0, zoom: R.scale, time: new Date().toISOString() } }).catch(() => {}), 1200);
    }

    // ---- search -------------------------------------------------------------------------------
    let hitIndex = -1;
    async function runSearch() {
      const q = search.value.trim().toLowerCase();
      R.searchHits = [];
      hitIndex = -1;
      if (!q) return;
      if (!R.pageText.length) R.pageText = await extractText(pdf);
      R.pageText.forEach((t, i) => { const s = t.toLowerCase(); let k = s.indexOf(q); while (k >= 0) { R.searchHits.push(i + 1); k = s.indexOf(q, k + q.length); } });
      announce(R.searchHits.length ? `${R.searchHits.length} matches` : "No matches");
      if (!R.searchHits.length) toast(d.att.state === "image-only" ? "No text layer: this scanned PDF cannot be searched without OCR." : "No matches in this document.", "info");
      nextHit(1);
    }
    function nextHit(dir) {
      if (!R.searchHits.length) return;
      hitIndex = (hitIndex + dir + R.searchHits.length) % R.searchHits.length;
      gotoPage(R.searchHits[hitIndex]);
      setTimeout(() => {
        const q = search.value.trim().toLowerCase();
        const pv = R.pageViews[R.searchHits[hitIndex]];
        pv?.wrap.querySelectorAll(".textLayer span").forEach((s) => { s.style.background = s.textContent.toLowerCase().includes(q) ? "rgba(255,150,0,.4)" : ""; });
      }, 400);
    }
    search.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); if (R.searchHits.length && e.shiftKey) nextHit(-1); else runSearch(); } });

    // ---- annotation panel ---------------------------------------------------------------------
    function drawPanel() {
      clear(side);
      const ocr = d.att.text_source === "ocr";
      if (R.panelTab === "text" && !ocr) R.panelTab = "annotations";
      const t = tabs([["annotations", `Annotations (${R.anns.length})`], ["notes", "Notes"], ...(ocr ? [["text", "Recognized text"]] : [])], R.panelTab, (k) => { R.panelTab = k; drawPanel(); }, { label: "Annotation panel" });
      const body = h("div", { style: { padding: "12px", overflow: "auto", flex: "1" } });
      side.append(t, body);
      if (R.panelTab === "notes") return drawNotesTab(body);
      if (R.panelTab === "text") return drawTextTab(body).catch((e) => showError(e, body));
      body.append(h("h2", { style: { fontSize: "18px", margin: "0 0 6px" } }, "Project notes"),
        S.project ? h("span.chip", icon("folder", 14), S.project.project.title) : h("span.chip", "Library-wide (no project chosen)"));
      const filt = select([["", "All marks"], ...TOOLS.slice(1).map(([k, l]) => [k, l]), ["review", "Needs review"]], R.filter, { "aria-label": "Filter marks by kind", on: { change: (e) => { R.filter = e.target.value; drawPanel(); } } });
      const colF = select([["", "Any colour"], ...COLORS.map(([c, n]) => [c, n])], R.filterColor || "", { "aria-label": "Filter marks by colour", on: { change: (e) => { R.filterColor = e.target.value; drawPanel(); } } });
      const from = h("input", { type: "number", min: "1", value: R.filterFrom || "", placeholder: "from", "aria-label": "From PDF page", style: { width: "70px" }, on: { change: (e) => { R.filterFrom = e.target.value; drawPanel(); } } });
      const to = h("input", { type: "number", min: "1", value: R.filterTo || "", placeholder: "to", "aria-label": "To PDF page", style: { width: "70px" }, on: { change: (e) => { R.filterTo = e.target.value; drawPanel(); } } });
      const authors = [...new Set(R.anns.map((a) => a.author).filter(Boolean))];
      const autF = authors.length ? select([["", "Anyone"], ["__me", "My marks"], ...authors.map((x) => [x, x])], R.filterAuthor || "", { "aria-label": "Filter marks by author", on: { change: (e) => { R.filterAuthor = e.target.value; drawPanel(); } } }) : null;
      body.append(h("div.row.small", { style: { flexWrap: "wrap", gap: "6px", margin: "8px 0" } }, filt, colF, h("span", "Pages"), from, to, autF));
      const sel = R.anns.find((a) => a.id === R.selectedAnn);
      if (sel) body.append(annCard(sel, true));
      const list = R.anns.filter((a) => a.id !== R.selectedAnn && (!R.filter || a.kind === R.filter || (R.filter === "review" && (a.anchor_state !== "matched" || !a.locator_confirmed)))
        && (!R.filterColor || a.color === R.filterColor) && (!R.filterFrom || a.page_index + 1 >= +R.filterFrom) && (!R.filterTo || a.page_index + 1 <= +R.filterTo)
        && (!R.filterAuthor || (R.filterAuthor === "__me" ? !a.author : a.author === R.filterAuthor)));
      for (const a of list) body.append(annCard(a, false));
      if (!R.anns.length) body.append(h("p.muted", "Choose a tool and select text, or press a tool key (H for highlight) and select. Marks are saved in the library, not in the PDF."));
      body.append(h("div.btn-row", { style: { marginTop: "12px" } },
        btn("Export annotated PDF", { kind: "primary", icon: "upload", onClick: () => exportPdf(false) }),
        btn("Export notes", { icon: "upload", onClick: exportList })),
        h("div.btn-row", { style: { marginTop: "6px" } }, btn("Flattened copy", { small: true, onClick: () => exportPdf(true) }), btn("Import marks from another reader…", { small: true, onClick: importExternal }), btn("Replace file version…", { small: true, onClick: replaceVersion })));
    }

    async function drawTextTab(body) {
      const n = R.page || 1;
      const pages = await call("files.page_text", { id: d.att.id }).catch(() => []);
      const txt = pages[n - 1] || "";
      body.append(h("h2", { style: { fontSize: "18px", margin: "0 0 6px" } }, `Recognized text, page ${n}`),
        h("p.small.muted", qualityText(d.att)),
        h("div.row.small", btn("Previous page", { small: true, disabled: n <= 1, onClick: () => { R.page = n - 1; go("reader"); } }), btn("Next page", { small: true, disabled: n >= pdf.numPages, onClick: () => { R.page = n + 1; go("reader"); } })),
        txt ? h("pre.mono.small", { tabindex: "0", "aria-label": `Recognized text of page ${n}`, style: { whiteSpace: "pre-wrap", userSelect: "text", border: "1px solid var(--border)", padding: "8px", borderRadius: "6px" } }, txt) : h("p.muted", "No text was recognized on this page."),
        h("p.small.muted", "Compare with the page on the left. To quote a passage, draw an Area mark around it: its text is taken from the recognized words and can be corrected."),
        h("div.btn-row", btn("Remove recognized text", { small: true, kind: "danger", onClick: async () => { if (await confirmDialog("Remove recognized text", "Remove the recognized text of this file from search? Marks keep their text. You can recognize it again later.", { ok: "Remove" })) { await call("files.clear_ocr", { id: d.att.id }); const r = await call("refs.get", { id: d.ref.id }); d.att = r.attachments.find((x) => x.id === d.att.id) || d.att; R.panelTab = "annotations"; go("reader"); } } })));
    }

    function annCard(a, full) {
      const pageCount = pdf.numPages;
      const card = h("div.ann-card" + (full ? ".selected" : ""));
      const meaning = MEANINGS.find((m) => m[0] === a.meaning)?.[1] || TOOLS.find((t) => t[0] === a.kind)?.[1] || a.kind;
      card.append(h("header", { style: { background: full ? "var(--attention-soft)" : "var(--surface-2)" } }, h("span.swatch", { style: { background: a.color } }), meaning, h("span.grow"),
        a.anchor_state && a.anchor_state !== "matched" ? h("span.badge.attention", a.anchor_state === "uncertain" ? "Check position" : "Not found in new version") : null,
        full ? null : btn("Open", { small: true, kind: "ghost", onClick: () => selectAnn(a.id) })));
      if (a.text) card.append(h("div.body", { style: { fontSize: "15px" } }, a.kind === "freetext" ? a.text : "“" + a.text + "”"));
      const origin = a.text_edited ? h("span.badge.attention", { title: "The text was changed after it was extracted; the original extraction is kept." }, "Edited text") : a.text_origin === "ocr" && a.text ? h("span.badge.neutral", { title: "Recognized from the page image; check it against the page." }, "Recognized text") : null;
      if (origin) card.append(h("div.body.small", origin));
      if (!full) { if (a.comment) card.append(h("div.body.small.muted", a.comment)); return card; }
      if (a.text && a.kind !== "freetext") {
        const qt = h("textarea", { rows: 3, value: a.text, "aria-label": "Quotation text" });
        qt.addEventListener("change", () => update({ text: qt.value }));
        card.append(h("div.body", field("Quotation text", qt, a.text_origin === "ocr" ? "Recognized from the page image. Correct it against the page; the change is recorded and the original kept." : "Exactly as extracted. A change is recorded and the original kept."),
          a.text_edited && a.extracted_text ? h("details.small", h("summary", "Original extraction"), h("p.mono", a.extracted_text), btn("Restore original", { small: true, onClick: () => update({ text: a.extracted_text }) })) : null));
      }
      const meaningSel = select([["", "No type"], ...MEANINGS], a.meaning || "", { "aria-label": "Type of mark", on: { change: (e) => update({ meaning: e.target.value }) } });
      const locInput = h("input", { value: a.locator || "", style: { width: "90px" }, "aria-label": "Citation page" });
      const conf = a.locator_confirmed ? h("span.status-ok.small", icon("check", 13), " Confirmed") : h("span.status-review.small", icon("warn", 13), " Not confirmed");
      card.append(h("div.meta",
        icon("doc", 18), h("span", `PDF page ${a.page_index + 1} of ${pageCount}${a.page_label && a.page_label !== String(a.page_index + 1) ? ` (label ${a.page_label})` : ""}`),
        icon("quote", 18), h("span.row", "Citation page", locInput, btn("Confirm", { small: true, onClick: () => update({ locator: locInput.value.trim(), locator_confirmed: !!locInput.value.trim() }) }), conf),
        h("span"), h("span.small", h("a", { href: "#", on: { click: (e) => { e.preventDefault(); pageMapping(a); } } }, "Set the page mapping for this file"))));
      const comment = h("textarea", { rows: 3, value: a.comment || "", "aria-label": "My interpretation", placeholder: "My interpretation (kept separate from the quotation)" });
      comment.addEventListener("change", () => update({ comment: comment.value }));
      const tagsIn = h("input", { value: (a.tags || []).join("; "), placeholder: "Labels, separated by semicolons", "aria-label": "Labels" });
      tagsIn.addEventListener("change", () => update({ tags: tagsIn.value.split(";").map((s) => s.trim()).filter(Boolean) }));
      const others = R.anns.filter((x) => x.id !== a.id);
      const linked = (a.links || []).map((id) => others.find((x) => x.id === id)).filter(Boolean);
      const short = (x) => `p. ${x.page_index + 1}: ${(x.text || x.comment || meaningLabel(x.meaning) || x.kind).slice(0, 50)}`;
      const linkSel = others.length ? select([["", "Link to another mark…"], ...others.filter((x) => !(a.links || []).includes(x.id)).map((x) => [x.id, short(x)])], "", { "aria-label": "Link to another mark", on: { change: (e) => e.target.value && update({ links: [...(a.links || []), e.target.value] }) } }) : null;
      const linksBox = h("div", linked.map((x) => h("div.row.small", h("a", { href: "#", on: { click: (e) => { e.preventDefault(); selectAnn(x.id); } } }, short(x)), iconBtn("x", "Remove link", () => update({ links: (a.links || []).filter((id) => id !== x.id) })))), linkSel);
      card.append(h("div.body", field("Type", meaningSel), field("My interpretation", comment), field("Labels", tagsIn), others.length ? field("Linked marks", linksBox) : null,
        h("div.btn-row",
          a.text ? btn("Copy with narrative citation", { small: true, disabled: !a.locator_confirmed, title: "Authors and year before the quotation, page after it", onClick: () => copyQuote(a, "narrative_quote") }) : null,
          a.text ? btn("Copy quote with citation", { kind: "primary", icon: "copy", disabled: !a.locator_confirmed, title: a.locator_confirmed ? "" : "Confirm the citation page first", onClick: () => copyQuote(a) }) : null,
          btn("Add to outline", { icon: "list", onClick: () => addToOutline(a) }),
          iconBtn("trash", "Delete mark", async () => { await call("ann.delete", { id: a.id }); R.anns = R.anns.filter((x) => x.id !== a.id); R.selectedAnn = null; paintAnnotations(a.page_index + 1); drawPanel(); toast("Mark deleted", "info", { action: { label: "Undo", run: async () => { await call("undo"); R.anns = await call("ann.list", { attachment_id: d.att.id, project_id: project }); paintAnnotations(a.page_index + 1); drawPanel(); } } }); })),
        !a.locator_confirmed && a.text ? h("p.small.muted", "PDF page, printed page and paragraph are different values. Confirm the page as printed in the source before citing a quotation.") : null,
        a.anchor_state === "uncertain" || a.anchor_state === "unmatched" ? h("div.btn-row", btn("Keep at this position", { small: true, onClick: async () => { await call("ann.resolve_anchor", { id: a.id, keep: true }); await refreshAnns(); } }), btn("Remove from this version", { small: true, kind: "danger", onClick: async () => { await call("ann.resolve_anchor", { id: a.id, keep: false }); await refreshAnns(); } })) : null));
      async function update(patch) {
        try {
          const saved = await call("ann.save", { annotation: { ...a, ...patch } });
          Object.assign(a, saved);
          paintAnnotations(a.page_index + 1);
          drawPanel();
        } catch (e) { showError(e); }
      }
      return card;
    }
    async function refreshAnns() { R.anns = await call("ann.list", { attachment_id: d.att.id, project_id: project }); for (let n = 1; n < R.pageViews.length; n++) paintAnnotations(n); drawPanel(); }

    async function pageMapping(a) {
      const printed = await promptDialog("Page mapping", `What page number is printed on PDF page ${a.page_index + 1}?`, a.locator || "", { help: "For example, if PDF page 7 shows page 121, enter 121. New marks then suggest citation pages from this mapping; each still needs your confirmation.", ok: "Save mapping" });
      if (!printed || isNaN(+printed)) return;
      await call("files.set_offset", { id: d.att.id, offset: +printed - (a.page_index + 1) });
      d.att.citation_page_offset = +printed - (a.page_index + 1);
      toast("Page mapping saved for this file version.", "success");
    }

    async function copyQuote(a, mode = "quote_with_citation") {
      const m = manuscript();
      try {
        const r = await call("cite.copy", { request: { manuscript_id: m?.id || "", items: [{ ref_id: d.ref.id, locator_label: "page", locator: a.locator }], mode, quote: a.text } });
        toast("Copied: " + r.output.text.slice(0, 140), "success");
        if (r.output.block_quote) toast("This quotation has 40 or more words: format it as a block quotation in your document.", "info");
      } catch (e) { showError(e); }
    }

    async function addToOutline(a) {
      const note = await call("notes.from_annotation", { annotation_id: a.id, kind: a.text ? "quotation" : "comment" });
      const outline = project ? await call("outline.get", { project_id: project }) : [];
      const flat = [];
      const walk = (ns, depth) => ns.forEach((n) => { flat.push([n.section.id, "  ".repeat(depth) + n.section.title]); walk(n.children, depth + 1); });
      walk(outline, 0);
      if (!flat.length) { toast("Saved as a note. Create outline sections in Notes to organize it.", "success", { action: { label: "Open Notes", run: () => go("notes", { note: note.id }) } }); return; }
      const sel = select(flat, flat[0][0]);
      const r = await modal("Add to outline", field("Section", sel), { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Add", kind: "primary", run: () => call("outline.file_note", { section_id: sel.value, note_id: note.id }) }] }).done;
      if (r !== null) toast("Added to the outline", "success");
    }

    async function exportPdf(flatten) {
      if (flatten && !(await confirmDialog("Flattened copy", "A flattened copy draws the marks into the pages for display only; they can no longer be edited in other readers. The original PDF is not changed.", { ok: "Export flattened copy" }))) return;
      try {
        const r = await call("ann.export_pdf", { attachment_id: d.att.id, project_id: project, flatten, name: d.att.original_name });
        if (r && !r.cancelled) toast(`${r.count} marks written to ${r.path}${flatten ? "" : " (with a sidecar file for project scope and links)"}. The original file is unchanged.`, "success");
      } catch (e) { showError(e); }
    }
    async function exportList() {
      const fmt = select([["docx", "DOCX document (.docx)"], ["md", "Markdown"], ["html", "HTML"], ["txt", "Plain text"], ["json", "Structured JSON (with page and citation for each)"]], "docx");
      const r = await modal("Export notes", field("Format", fmt), { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Export", kind: "primary", run: () => call("ann.export_list", { attachment_id: d.att.id, project_id: project, manuscript_id: manuscript()?.id || "", format: fmt.value }) }] }).done;
      if (r && !r.cancelled) toast("Saved " + r.path, "success");
    }

    async function importExternal() {
      let res;
      try { res = await call("ann.preview_external", { attachment_id: d.att.id }); } catch (e) { return showError(e); }
      if (!res || res.cancelled) return;
      const pv = res.preview;
      // Recover highlighted text from this document's text layer where possible.
      if (!R.pageText.length) R.pageText = await extractText(pdf).catch(() => []);
      const items = [];
      for (const ex of pv.additions) {
        let text = "";
        if (["highlight", "underline", "strikeout"].includes(ex.kind) && pv.version_matches) {
          const n = ex.page_index + 1;
          await renderPage(n);
          text = ex.rects.map((r) => textInRect(n, r)).join(" ").trim();
        }
        items.push({ external: ex, text });
      }
      const ok = await modal("Import marks from another reader", h("div",
        !pv.version_matches ? h("div.notice.warn", icon("warn"), "That PDF is not the same file version as this attachment, so positions may not match. Review the marks after importing.") : null,
        h("ul", h("li", `${pv.additions.length} new marks`), h("li", `${pv.conflicts.length} marks changed in the other reader (conflicts)`), h("li", `${pv.unchanged} already here (not duplicated)`)),
        pv.unsupported.length ? h("p.small.muted", "Not supported and left out: " + pv.unsupported.join(", ")) : null,
        h("table.table", h("tbody", items.slice(0, 50).map((it) => h("tr", h("td", it.external.kind), h("td", `p. ${it.external.page_index + 1}`), h("td.small", (it.text || it.external.comment || it.external.text).slice(0, 120)))))),
        pv.conflicts.length ? h("p.small", "Conflicts update the comment as a new revision of the existing mark; your project notes are never deleted.") : null),
        { size: "wide", buttons: [{ label: "Cancel" }, ...(pv.conflicts.length ? [{ label: "Import and accept changes", run: () => call("ann.import_external", { attachment_id: d.att.id, project_id: project, items: [...items, ...pv.conflicts.map(([ex]) => ({ external: ex, text: "" }))], accept_conflicts: true }) }] : []), { label: "Import new marks", kind: "primary", run: () => call("ann.import_external", { attachment_id: d.att.id, project_id: project, items, accept_conflicts: false }) }] }).done;
      if (ok !== null && ok !== undefined) { toast(`${ok} marks imported`, "success"); await refreshAnns(); }
    }

    async function replaceVersion() {
      const r = await call("files.replace_pick", { id: d.att.id });
      if (!r || r.cancelled) return;
      toast("Reading the new version…", "info");
      try {
        const bytes = await readPicked(r.path);
        const npdf = await pdfjsLib.getDocument({ data: bytes, isEvalSupported: false, enableXfa: false }).promise;
        const pages = await extractText(npdf);
        await npdf.destroy();
        const [att, rep] = await call("files.replace", { id: d.att.id, path: r.path, pages });
        await modal("New version attached", h("div", h("p", "The previous version is kept. Marks were copied to the new version and matched by their text:"),
          h("ul", h("li", `${rep.matched} matched`), h("li", `${rep.uncertain.length} uncertain (moved or ambiguous) — review each`), h("li", `${rep.unmatched.length} not found in the new version`)),
          h("p.small.muted", "Uncertain quotations are never moved silently; they are marked “Check position” in the annotation panel.")), { buttons: [{ label: "Review marks", kind: "primary" }] }).done;
        R.docs[R.active].att = att;
        R.pdfKey = "";
        go("reader");
      } catch (e) { showError(e); }
    }

    async function drawNotesTab(body) {
      const notes = await call("notes.for_ref", { ref_id: d.ref.id, project_id: project });
      body.append(h("div.btn-row", btn("New note", { small: true, icon: "plus", onClick: () => go("notes", { newFor: d.ref.id }) })));
      for (const n of notes) body.append(h("div.note-card.kind-" + n.kind, h("header", n.kind), n.exact_text ? h("div.body", "“" + n.exact_text + "”") : null, n.body ? h("div.body", n.body) : null));
      if (!notes.length) body.append(h("p.muted", "No notes for this source yet."));
    }

    // Keyboard: tools and navigation.
    docEl.addEventListener("keydown", (e) => {
      if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
      const t = TOOLS.find((x) => x[3] === e.key.toLowerCase());
      if (t && !e.ctrlKey && !e.altKey) { setTool(t[0]); e.preventDefault(); return; }
      if (/^[1-6]$/.test(e.key) && !e.ctrlKey && !e.altKey) { R.color = COLORS[+e.key - 1][0]; colorSel.value = R.color; announce(`${COLORS[+e.key - 1][1]}${meaningLabel(scheme()[R.color]) ? ", " + meaningLabel(scheme()[R.color]) : ""}`); e.preventDefault(); return; }
      if (e.key === "PageDown" || (e.key === "ArrowRight" && R.mode !== "continuous")) { gotoPage(R.page + (R.mode === "two" ? 2 : 1)); e.preventDefault(); }
      if (e.key === "PageUp" || (e.key === "ArrowLeft" && R.mode !== "continuous")) { gotoPage(R.page - (R.mode === "two" ? 2 : 1)); e.preventDefault(); }
      if (e.key === "Home" && e.ctrlKey) gotoPage(1);
      if (e.key === "End" && e.ctrlKey) gotoPage(pdf.numPages);
      if (e.ctrlKey && (e.key === "=" || e.key === "+")) { zoom(1); e.preventDefault(); }
      if (e.ctrlKey && e.key === "-") { zoom(-1); e.preventDefault(); }
      if (e.key === "Enter") { const n = [...pagesEl.querySelectorAll(".page-wrap")].find((w) => w.contains(window.getSelection()?.anchorNode))?.dataset.page; if (n) maybeMark(+n); }
    });

    await drawPages();
    drawThumbs();
    drawPanel();
    clearInterval(R.timeTimer);
    let lastInput = Date.now(), pending = 0;
    for (const ev of ["mousemove", "keydown", "wheel", "scroll"]) docEl.addEventListener(ev, () => (lastInput = Date.now()), { passive: true });
    if (S.project) {
      const refId = d.ref.id, projectId = S.project.project.id;
      R.timeTimer = setInterval(() => {
        if (!document.body.contains(docEl)) { clearInterval(R.timeTimer); if (pending) call("membership.add_time", { project_id: projectId, ref_id: refId, secs: pending }).catch(() => {}); return; }
        if (document.visibilityState === "visible" && document.hasFocus() && Date.now() - lastInput < 5 * 60 * 1000) pending += 15;
        if (pending >= 60) { const n = pending; pending = 0; call("membership.add_time", { project_id: projectId, ref_id: refId, secs: n }).catch(() => {}); }
      }, 15000);
    }
    if (params.replace) { params.replace = false; replaceVersion(); }
  },
});
