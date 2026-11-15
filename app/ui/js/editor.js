// Source editor: type, structured creators, partial dates, type-specific fields, confirmed
// absence, citation key and provenance. Used for manual entry, editing and staged records.

import { call } from "./api.js";
import { h, clear, icon, iconBtn, btn, modal, select, field, confirmDialog, toast } from "./dom.js";
import { S } from "./app.js";

const ROLES = [["author", "Author"], ["editor", "Editor"], ["translator", "Translator"], ["director", "Director"], ["producer", "Producer"], ["host", "Host"], ["guest", "Guest"], ["inventor", "Inventor"], ["interviewer", "Interviewer"], ["recipient", "Recipient"], ["contributor", "Contributor"], ["series-editor", "Series editor"], ["composer", "Composer"], ["performer", "Performer"], ["chair", "Chair"], ["reviewed-author", "Reviewed author"]];
const MONTHS = ["", "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const FIELD_LABELS = {
  title: "Title", short_title: "Short title", container_title: "Published in", container_title_short: "Journal abbreviation", volume: "Volume", issue: "Issue", page: "Pages",
  article_number: "Article number", doi: "DOI", url: "URL", publisher: "Publisher", publisher_place: "Place", edition: "Edition",
  isbn: "ISBN", issn: "ISSN", pmid: "PubMed ID", pmcid: "PMC ID", arxiv: "arXiv ID", abstract: "Abstract", language: "Language",
  note: "Extra", keywords: "Keywords", genre: "Genre or degree", medium: "Medium or format", number: "Number", version: "Version",
  collection_title: "Series", event: "Event", event_place: "Event place", event_date: "Event date", archive: "Archive or repository",
  archive_location: "Location in archive", section: "Section", jurisdiction: "Jurisdiction", authority: "Court or authority",
  number_of_volumes: "Number of volumes", reviewed_title: "Reviewed work", original_publisher: "Original publisher", status: "Status",
  dimensions: "Dimensions", scale: "Scale",
  type: "Type", creators: "Authors and other people", issued: "Date",
  collections: "Collections (groups or folders)", rating: "Rating", source_id: "Record ID in the other program",
};
const HELP = {
  page: "A page range such as 115–132. An article number belongs in Article number, not here.",
  article_number: "For journals that number articles (for example e1042) instead of pages.",
  doi: "Without the https://doi.org/ prefix. A missing DOI does not make a reference incomplete.",
  title: "Enter the title as it should appear in reference lists (sentence case for APA).",
  container_title_short: "The standard short form, for example J. Appl. Psychol. Used only by styles that abbreviate journal names; APA uses the full name.",
};
export const fieldLabel = (f) => FIELD_LABELS[f] || f;

// Fields whose values repeat across sources get suggestions from values already used.
const SUGGEST = ["container_title", "publisher", "publisher_place", "language", "genre", "edition", "event", "event_place", "medium", "archive", "collection_title"];
let suggestN = 0;
function suggestFor(input, field) {
  const id = "sug" + ++suggestN;
  const list = h("datalist", { id });
  input.setAttribute("list", id);
  input.after?.(list);
  let t;
  const load = () => { clearTimeout(t); t = setTimeout(async () => {
    try { const vals = await call("fields.values", { field, prefix: input.value.trim() }); clear(list, ...vals.filter((v) => v !== input.value).map((v) => h("option", { value: v }))); } catch (_) {}
  }, 150); };
  input.addEventListener("focus", load);
  input.addEventListener("input", load);
  // The datalist must live in the document; attach it once the input is placed.
  setTimeout(() => { if (!list.isConnected) input.parentElement?.append(list); }, 0);
}

function typeOptions() {
  const groups = {};
  for (const t of S.types) (groups[t.group] = groups[t.group] || []).push(t);
  const sel = h("select");
  // The person's own types first; each is a variant of a built-in type ("sub:<id>").
  if (S.customTypes?.length) {
    const og = h("optgroup", { label: "Your types" });
    for (const t of S.customTypes) og.append(h("option", { value: "sub:" + t.id }, t.label));
    sel.append(og);
  }
  for (const [g, ts] of Object.entries(groups)) {
    const og = h("optgroup", { label: g });
    for (const t of ts) og.append(h("option", { value: t.id }, t.label));
    sel.append(og);
  }
  return sel;
}

function dateEditor(label, d, onChange) {
  d = d || {};
  const y = h("input", { type: "number", value: d.year ?? "", placeholder: "Year", "aria-label": label + " year", style: { width: "90px" } });
  const m = select(MONTHS.map((n, i) => [String(i || ""), n || "Month"]), String(d.month || ""), { "aria-label": label + " month", style: { width: "130px" } });
  const day = h("input", { type: "number", value: d.day ?? "", placeholder: "Day", min: 1, max: 31, "aria-label": label + " day", style: { width: "70px" } });
  const season = select([["", "Season"], ["1", "Spring"], ["2", "Summer"], ["3", "Fall"], ["4", "Winter"]], String(d.season || ""), { "aria-label": label + " season", style: { width: "110px" } });
  const status = select([["", "Published"], ["in press", "In press"], ["forthcoming", "Forthcoming"]], d.status || "", { "aria-label": label + " status", style: { width: "130px" } });
  const approx = h("input", { type: "checkbox", checked: !!d.approximate, id: "ap" + Math.random().toString(36).slice(2) });
  const endM = select(MONTHS.map((n, i) => [String(i || ""), n || "End month"]), String(d.end?.month || ""), { "aria-label": label + " end month", style: { width: "130px" } });
  const endD = h("input", { type: "number", value: d.end?.day ?? "", placeholder: "End day", "aria-label": label + " end day", style: { width: "90px" } });
  const endY = h("input", { type: "number", value: d.end?.year ?? "", placeholder: "End year", "aria-label": label + " end year", style: { width: "100px" } });
  const read = () => {
    const v = {};
    if (y.value) v.year = +y.value;
    if (m.value) v.month = +m.value;
    if (day.value) v.day = +day.value;
    if (season.value) v.season = +season.value;
    if (status.value) v.status = status.value;
    if (approx.checked) v.approximate = true;
    if (endY.value || endM.value || endD.value) v.end = { ...(endY.value ? { year: +endY.value } : {}), ...(endM.value ? { month: +endM.value } : {}), ...(endD.value ? { day: +endD.value } : {}) };
    return Object.keys(v).length ? v : null;
  };
  [y, m, day, season, status, approx, endM, endD, endY].forEach((x) => x.addEventListener("change", () => onChange(read())));
  return h("fieldset", { style: { border: 0, padding: 0, margin: "0 0 10px" } }, h("legend.label", { style: { fontWeight: 600, fontSize: "13px", color: "var(--text-2)" } }, label),
    h("div.row", { style: { flexWrap: "wrap" } }, y, m, day, season, status, h("label.row.small", { for: approx.id }, approx, "Approximate")),
    h("details", h("summary.small.muted", "Date range"), h("div.row", { style: { marginTop: "6px" } }, endY, endM, endD)));
}

function creatorRow(c, onChange, onRemove, onMove) {
  c.name = c.name || {};
  const kind = select([["person", "Person"], ["organization", "Organization"], ["mononym", "Single name"]], c.name.kind || "person", { "aria-label": "Name kind", style: { width: "130px" } });
  const role = select(ROLES, c.role || "author", { "aria-label": "Role", style: { width: "140px" } });
  const family = h("input", { value: c.name.family || "", placeholder: "Family name", "aria-label": "Family name" });
  const given = h("input", { value: c.name.given || "", placeholder: "Given names", "aria-label": "Given names" });
  const particle = h("input", { value: c.name.particle || "", placeholder: "Particle", "aria-label": "Particle such as van or de", style: { width: "90px" } });
  const suffix = h("input", { value: c.name.suffix || "", placeholder: "Suffix", "aria-label": "Suffix such as Jr.", style: { width: "70px" } });
  const literal = h("input", { value: c.name.literal || "", placeholder: "Full name", "aria-label": "Organization or single name" });
  const person = h("div.row.grow", particle, family, given, suffix);
  const lit = h("div.row.grow", literal);
  const sync = () => {
    const k = kind.value;
    person.hidden = k !== "person";
    lit.hidden = k === "person";
    c.role = role.value;
    // Keep fields this row does not edit (alternative-script variants and any future parts).
    const { kind: _k, family: _f, given: _g, particle: _p, suffix: _s, literal: _l, ...kept } = c.name;
    c.name = k === "person" ? { ...kept, kind: k, family: family.value.trim(), given: given.value.trim(), particle: particle.value.trim(), suffix: suffix.value.trim() } : { ...kept, kind: k, literal: literal.value.trim() };
    onChange();
  };
  [kind, role, family, given, particle, suffix, literal].forEach((x) => x.addEventListener("input", sync));
  kind.addEventListener("change", sync);
  person.hidden = (c.name.kind || "person") !== "person";
  lit.hidden = !person.hidden;
  return h("div.row", { style: { marginBottom: "6px" }, role: "group", "aria-label": "Creator" }, role, kind, person, lit,
    iconBtn("chevronUp", "Move up", () => onMove(-1)), iconBtn("chevronDown", "Move down", () => onMove(1)), iconBtn("x", "Remove creator", onRemove));
}

/** Open the editor. Returns the saved reference, or null. `mode`: new, edit, staged. */
export async function sourceEditor(ref, { mode = "edit", projectId = "", title } = {}) {
  const r = structuredClone(ref || { type: "article-journal", creators: [{ role: "author", name: { kind: "person" } }], fields: {}, confirmed_absent: [], tags: [] });
  r.fields = r.fields || {};
  r.creators = r.creators || [];
  r.confirmed_absent = r.confirmed_absent || [];
  const content = h("div");
  let impactNote = null;
  if (mode === "edit" && r.id) {
    try {
      const imp = await call("refs.impact", { id: r.id });
      if (imp.projects.length > 1 || imp.manuscripts.length) {
        impactNote = h("div.notice.warn", icon("info"), h("div", h("strong", "This source is shared. "),
          `Edits change it in ${imp.projects.length} project${imp.projects.length === 1 ? "" : "s"} (${imp.projects.map((p) => p[1]).join(", ")})` +
          (imp.manuscripts.length ? ` and ${imp.manuscripts.length} manuscript${imp.manuscripts.length === 1 ? "" : "s"}; copied citations that change will be flagged for review.` : ".")));
      }
    } catch (_) {}
  }
  const typeSel = typeOptions();
  typeSel.value = r.subtype && S.customTypes?.some((t) => t.id === r.subtype) ? "sub:" + r.subtype : r.type;
  // The chosen custom type (if any) and the built-in type that citation styles use.
  const chosen = () => {
    const v = typeSel.value;
    if (v.startsWith("sub:")) { const c = S.customTypes.find((t) => "sub:" + t.id === v); return { base: c.base, sub: c.id, fields: c.fields, help: c.help || `Your type, cited as ${S.types.find((b) => b.id === c.base)?.label || c.base}.` }; }
    const t = S.types.find((x) => x.id === v) || {};
    return { base: v, sub: "", fields: t.fields || [], help: t.help || "" };
  };
  const typeHelp = h("div.help");
  const creatorsBox = h("div");
  const fieldsBox = h("div");
  const drawCreators = () => {
    clear(creatorsBox);
    r.creators.forEach((c, i) => creatorsBox.append(creatorRow(c, () => {}, () => { r.creators.splice(i, 1); drawCreators(); }, (d) => {
      const j = i + d;
      if (j < 0 || j >= r.creators.length) return;
      [r.creators[i], r.creators[j]] = [r.creators[j], r.creators[i]];
      drawCreators();
    })));
    creatorsBox.append(h("div.btn-row", h("button.btn.small", { type: "button", on: { click: () => { r.creators.push({ role: "author", name: { kind: "person" } }); drawCreators(); creatorsBox.querySelectorAll("input")[creatorsBox.querySelectorAll("input").length - 4]?.focus(); } } }, icon("plus", 15), "Add person"),
      h("button.btn.small", { type: "button", on: { click: () => { r.creators.push({ role: "author", name: { kind: "organization" } }); drawCreators(); } } }, icon("plus", 15), "Add organization"),
      absentToggle("author", "No author (confirmed)")));
  };
  function absentToggle(f, label) {
    const id = "abs-" + f + Math.random().toString(36).slice(2, 6);
    const cb = h("input", { type: "checkbox", id, checked: r.confirmed_absent.includes(f), on: { change: (e) => {
      r.confirmed_absent = r.confirmed_absent.filter((x) => x !== f);
      if (e.target.checked) r.confirmed_absent.push(f);
    } } });
    return h("label.row.small.muted", { for: id, title: "Confirmed absence is different from unknown." }, cb, label);
  }
  const drawFields = () => {
    const t = chosen();
    typeHelp.textContent = t.help || "";
    const list = ["title", ...(t.fields || []), "short_title", "doi", "url", "abstract", "language", "keywords", "note"];
    const seen = new Set();
    clear(fieldsBox);
    const grid = h("div.grid2");
    for (const f of list) {
      if (seen.has(f)) continue;
      seen.add(f);
      const big = f === "abstract" || f === "note" || f === "title";
      const inp = big ? h("textarea", { value: r.fields[f] || "", rows: f === "title" ? 2 : 4 }) : h("input", { value: r.fields[f] || "" });
      if (SUGGEST.includes(f)) suggestFor(inp, f);
      inp.dataset.fieldName = f;
      inp.addEventListener("input", () => { if (inp.value.trim()) r.fields[f] = inp.value; else delete r.fields[f]; });
      const prov = r.provenance?.[f];
      const helpTxt = [HELP[f], prov ? `From ${sourceLabel(prov.source)}${prov.confidence != null ? `, ${Math.round(prov.confidence * 100)}% confidence` : ""}${needsCheck(prov) ? ": check this value" : prov.review === "reviewed" && prov.confidence != null ? ", checked" : ""}` : ""].filter(Boolean).join(" · ");
      const fl = field(fieldLabel(f), inp, helpTxt || null);
      if (needsCheck(prov)) fl.classList.add("needs-check");
      if (f === "doi" || f === "page") fl.append(absentToggle(f === "doi" ? "doi" : "page", f === "doi" ? "Has no DOI (confirmed)" : "Has no pages (confirmed)"));
      if (big) fl.style.gridColumn = "1 / -1";
      grid.append(fl);
      if (f === "container_title_short") abbrevHint(fl, inp);
    }
    // English translation of a non-English title (shown in brackets after the title).
    const tr = h("input", { value: r.translations?.title?.en || "", placeholder: "Only for titles not in English" });
    tr.addEventListener("input", () => {
      r.translations = r.translations || {};
      const v = tr.value.trim();
      if (v) r.translations.title = { ...(r.translations.title || {}), en: v };
      else if (r.translations.title) { delete r.translations.title.en; if (!Object.keys(r.translations.title).length) delete r.translations.title; }
    });
    const trField = field("English translation of the title", tr, "APA places it in square brackets after the original title.");
    trField.style.gridColumn = "1 / -1";
    grid.append(trField);
    // Retrieval date: only for content designed to change that is not archived.
    const cid = "chg-" + Math.random().toString(36).slice(2, 6);
    const chg = h("input", { type: "checkbox", id: cid, checked: ["yes", "true", "1"].includes(String(r.fields.changes_over_time || "").toLowerCase()), on: { change: (e) => { if (e.target.checked) r.fields.changes_over_time = "yes"; else delete r.fields.changes_over_time; } } });
    const chgRow = h("div.field", { style: { gridColumn: "1 / -1" } }, h("label.row", { for: cid }, chg, "Content is designed to change and is not archived"), h("div.help", "Only then does APA include a retrieval date (for example a page that is updated in place). Add the access date under More dates."));
    grid.append(chgRow);
    fieldsBox.append(grid);
    const own = (S.customFields || []).filter((d) => !d.types?.length || d.types.includes(t.base) || (t.sub && d.types.includes(t.sub)));
    if (own.length) {
      r.custom = r.custom || {};
      const cg = h("div.grid2");
      for (const d of own) {
        const cur = r.custom[d.id];
        const set = (v) => { if (v === "" || v === null || v === undefined) delete r.custom[d.id]; else r.custom[d.id] = v; };
        let ctl;
        if (d.kind === "list") ctl = select([["", ""], ...d.options.map((o) => [o, o])], cur ?? "", { on: { change: (e) => set(e.target.value) } });
        else if (d.kind === "boolean") ctl = select([["", ""], ["true", "Yes"], ["false", "No"]], cur === true ? "true" : cur === false ? "false" : "", { on: { change: (e) => set(e.target.value === "" ? "" : e.target.value === "true") } });
        else {
          ctl = h("input", { value: cur ?? "", placeholder: d.kind === "date" ? "2024, 2024-03 or 2024-03-15" : d.kind === "link" ? "https://" : d.kind === "number" ? "A number" : "" });
          ctl.addEventListener("input", () => set(ctl.value.trim()));
          if (d.kind === "text") suggestFor(ctl, "custom:" + d.id);
        }
        cg.append(field(d.name + (d.required ? " (required)" : ""), ctl, d.help || null));
      }
      fieldsBox.append(h("h3", { style: { margin: "14px 0 6px" } }, "Your fields"), cg);
    }
  };
  typeSel.addEventListener("change", () => {
    const t = chosen();
    // Fields that no longer fit move to a visible overflow section instead of disappearing.
    const keep = new Set(["title", "short_title", "doi", "url", "abstract", "language", "keywords", "note", ...(t.fields || [])]);
    for (const k of Object.keys(r.fields)) if (!keep.has(k)) { r.overflow = r.overflow || {}; r.overflow["moved:" + k] = r.fields[k]; delete r.fields[k]; }
    r.type = t.base;
    r.subtype = t.sub;
    drawFields();
    drawOverflow();
  });
  const issued = dateEditor("Date", r.issued, (v) => (r.issued = v));
  const orig = dateEditor("Original publication date", r.original_date, (v) => (r.original_date = v));
  const accessed = dateEditor("Accessed (for pages designed to change)", r.accessed, (v) => (r.accessed = v));
  const key = h("input", { value: r.citekey || "", "aria-label": "Citation key" });
  const lock = h("input", { type: "checkbox", id: "keylock", checked: !!r.citekey_locked });
  const tags = h("input", { value: (r.tags || []).join("; "), placeholder: "Separate with semicolons" });
  const overflowBox = h("div");
  const drawOverflow = () => {
    clear(overflowBox);
    const entries = Object.entries(r.overflow || {});
    if (!entries.length) return;
    overflowBox.append(h("details", { open: mode === "staged" }, h("summary", `Other imported fields (${entries.length}) — kept, not dropped`),
      h("table.table", h("tbody", entries.map(([k, v]) => h("tr", h("td.mono", k), h("td", String(v).slice(0, 300)), h("td", iconBtn("x", "Remove field " + k, () => { delete r.overflow[k]; drawOverflow(); }))))))));
  };
  const conflicts = (r.conflicts || []).length ? h("div.notice.warn", icon("warn"), h("div", h("strong", "Conflicting values from other sources:"),
    h("ul", r.conflicts.map((c) => h("li", `${fieldLabel(c.field)}: “${c.value}” (${c.source}) — current: “${r.fields[c.field] || ""}”`))), h("div.small", "Use Compare sources in the inspector to choose. Both values are kept in history."))) : null;
  drawCreators();
  drawFields();
  drawOverflow();
  content.append(
    impactNote,
    conflicts,
    h("div.grid2", field("Type", typeSel, null), h("div", typeHelp)),
    h("div.field", h("div.label", "Creators"), creatorsBox),
    issued,
    h("div.row", absentToggle("issued", "No date (confirmed; n.d. is used)")),
    fieldsBox,
    h("details", h("summary", "More dates"), orig, accessed),
    h("div.grid2", field("Citation key", key, "Generated automatically when empty."), h("div.field", h("label.row", { for: "keylock", style: { marginTop: "24px" } }, lock, "Lock the citation key"))),
    field("Tags", tags),
    overflowBox,
  );
  const res = await modal(title || (mode === "new" ? "Add source" : mode === "staged" ? "Review imported record" : "Edit source"), content, {
    size: "wide",
    buttons: [
      { label: "Cancel", value: null },
      { label: mode === "new" ? "Add source" : "Save", kind: "primary", run: async () => {
        r.citekey = key.value.trim();
        r.citekey_locked = lock.checked;
        r.tags = tags.value.split(";").map((s) => s.trim()).filter(Boolean);
        r.creators = r.creators.filter((c) => (c.name.family || c.name.literal || c.name.given || "").trim());
        if (!(r.fields.title || "").trim() && !r.confirmed_absent.includes("title")) {
          if (!(await confirmDialog("No title", "This source has no title. Save it anyway? It will be listed under Needs metadata.", { ok: "Save without title" }))) return false;
        }
        if (mode === "staged") return r;
        if (mode === "new") return await call("refs.add", { ref: r, project_id: projectId });
        return await call("refs.update", { ref: r });
      } },
    ],
  }).done;
  if (res && mode === "new") toast("Source added", "success");
  return res;
}

/** Values filled in automatically below this confidence ask to be checked (matches the core). */
export const REVIEW_BELOW = 0.7;
export function needsCheck(p) {
  return !!p && !p.user_edited && p.review !== "reviewed" && p.confidence != null && p.confidence < REVIEW_BELOW;
}
export function sourceLabel(src = "") {
  const [kind, detail = ""] = src.split(":");
  const named = { doi: "DOI record", pmid: "PubMed record", pmcid: "PubMed Central record", arxiv: "arXiv record", isbn: "book catalogue (ISBN)", url: "the web page", webpage: "the web page", crossref: "registration record", "crossref-search": "a title search", other: "an online lookup" };
  if (kind === "user") return detail === "bulk" ? "you (bulk edit)" : "you";
  if (kind === "lookup") return named[detail] || "an online lookup";
  if (kind === "import") return detail === "paste" ? "a pasted reference list" : `an imported file (${detail})`;
  if (kind === "pdf") return "the PDF file";
  if (kind === "capture") return "the browser capture";
  if (kind === "rule") return "a tidying rule";
  return src;
}
export function confidenceBadge(p) {
  if (!p || p.confidence == null) return null;
  const pct = Math.round(p.confidence * 100);
  return h("span.badge." + (needsCheck(p) ? "attention" : "neutral"), { title: "How sure the automatic source is about this value" + (p.review === "reviewed" ? "; you checked it" : "") }, `${pct}%${p.review === "reviewed" ? " ✓" : ""}`);
}

/** Under the journal abbreviation: offer the abbreviation the journal list knows for the journal
 *  name, when the field is empty; refreshed when the journal name changes. */
function abbrevHint(fl, inp) {
  const box = h("div.small", { "aria-live": "polite" });
  fl.append(box);
  const full = () => fl.closest(".grid2")?.querySelector("[data-field-name='container_title']")?.value || "";
  const refresh = async () => {
    clear(box);
    const name = full().trim();
    if (inp.value.trim() || !name) return;
    try {
      const a = await call("journals.abbrev", { full: name });
      if (a && !inp.value.trim()) box.append(btn(`Use “${a}”`, { small: true, kind: "ghost", title: "From the journal list", onClick: () => { inp.value = a; inp.dispatchEvent(new Event("input")); clear(box); } }));
    } catch (_) {}
  };
  setTimeout(() => {
    const j = fl.closest(".grid2")?.querySelector("[data-field-name='container_title']");
    j?.addEventListener("change", refresh);
    refresh();
  }, 0);
  inp.addEventListener("input", () => { if (inp.value.trim()) clear(box); });
}
