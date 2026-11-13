// Citation styles in the interface: honest assurance labels, a grouped style picker, the
// citation-language list and the style catalog (search, preview, import, remove).

import { call } from "./api.js";
import { h, clear, icon, btn, toast, modal, confirmDialog, select, field, showError, runsHtml } from "./dom.js";
import { S, refresh, manuscript, loadInfo } from "./app.js";

export function assurance(a) {
  return ({
    verified: ["Verified profile", "success", "Every approved test case passes and has been reviewed."],
    limited: ["Limited testing", "attention", "Test cases pass but await review by a qualified person."],
    untested: ["Untested", "attention", "Bundled style with no test cases run in this program. Check every citation."],
    custom_unreviewed: ["Imported, not reviewed", "attention", "Imported style file. It has not been tested or reviewed here."],
  })[a] || ["Unsupported", "danger", "This style cannot be used."];
}

export function assuranceBadge(a) {
  const [t, k] = assurance(a);
  return h("span.badge." + k, t);
}

// Citation languages offered for styles that use CSL locales. APA 7 uses its own composer,
// which writes US English.
export const LOCALES = [
  ["en-US", "English (US)"], ["en-GB", "English (UK)"], ["de-DE", "German (Germany)"], ["de-AT", "German (Austria)"], ["de-CH", "German (Switzerland)"],
  ["fr-FR", "French (France)"], ["fr-CA", "French (Canada)"], ["es-ES", "Spanish (Spain)"], ["es-MX", "Spanish (Mexico)"], ["it-IT", "Italian"],
  ["pt-BR", "Portuguese (Brazil)"], ["pt-PT", "Portuguese (Portugal)"], ["nl-NL", "Dutch"], ["sv-SE", "Swedish"], ["da-DK", "Danish"], ["nb-NO", "Norwegian (Bokmål)"],
  ["fi-FI", "Finnish"], ["pl-PL", "Polish"], ["cs-CZ", "Czech"], ["hu-HU", "Hungarian"], ["ro-RO", "Romanian"], ["ru-RU", "Russian"], ["uk-UA", "Ukrainian"],
  ["tr-TR", "Turkish"], ["el-GR", "Greek"], ["he-IL", "Hebrew"], ["ar", "Arabic"], ["fa-IR", "Persian"], ["zh-CN", "Chinese (simplified)"], ["zh-TW", "Chinese (traditional)"],
  ["ja-JP", "Japanese"], ["ko-KR", "Korean"], ["id-ID", "Indonesian"], ["vi-VN", "Vietnamese"], ["th-TH", "Thai"],
];

export const localeName = (c) => (LOCALES.find((l) => l[0] === c) || [c, c])[1];

const GROUPS = [["builtin", "Tested"], ["imported", "Imported"], ["bundled", "Bundled CSL styles (untested)"]];

/** A style <select> grouped by where the style comes from. */
export function styleSelect(value, attrs = {}, { extra } = {}) {
  const s = h("select", attrs);
  for (const [src, label] of GROUPS) {
    const list = S.styles.filter((x) => (x.source || "builtin") === src);
    if (!list.length) continue;
    const g = h("optgroup", { label });
    for (const st of list) {
      const o = h("option", { value: st.id }, st.title);
      if (st.id === value) o.selected = true;
      g.append(o);
    }
    s.append(g);
  }
  if (extra) s.append(h("option", { value: extra[0] }, extra[1]));
  if (value && ![...s.options].some((o) => o.value === value)) {
    s.prepend(h("option", { value, selected: true }, "Style not installed"));
  }
  return s;
}

/** The style catalog: search every style, preview it with the manuscript's sources, choose
 *  style and citation language, import or remove CSL files. */
export async function styleCatalog() {
  const m = manuscript();
  let chosen = m?.style || "apa-7";
  const q = h("input", { type: "search", placeholder: "Search styles by name, for example Chicago, IEEE, Vancouver", "aria-label": "Search styles" });
  const list = h("div", { role: "listbox", "aria-label": "Styles", style: { maxHeight: "380px", overflow: "auto", border: "1px solid var(--border)", borderRadius: "8px" } });
  const lang = select(LOCALES, m?.locale || "en-US", { "aria-label": "Citation language" });
  const info = h("div");
  const preview = h("div", { "aria-live": "polite" });
  const ids = m ? (await call("manuscripts.sources", { id: m.id }).catch(() => [])).filter((x) => x.state !== "excluded").map((x) => x.ref_id).slice(0, 3) : [];

  const drawList = () => {
    clear(list);
    const term = q.value.trim().toLowerCase();
    const rows = S.styles.filter((s) => !term || s.title.toLowerCase().includes(term) || (s.csl_id || "").toLowerCase().includes(term));
    if (!rows.length) list.append(h("div.empty", "No styles match. You can import a CSL file for any style not listed."));
    for (const s of rows) {
      const row = h("button.proj-row", { type: "button", role: "option", "aria-selected": String(s.id === chosen), style: { gridTemplateColumns: "1fr auto", padding: "8px 12px", width: "100%", background: s.id === chosen ? "var(--primary-soft)" : "" },
        on: { click: () => { chosen = s.id; drawList(); drawInfo(); drawPreview(); } } },
        h("span", h("div", { style: { fontWeight: 600 } }, s.title), h("div.small.muted", [s.source === "builtin" ? "Built-in composer" : s.source === "imported" ? "Imported CSL file" : "Bundled CSL", s.class === "note" ? "notes" : "in-text"].join(" • "))),
        assuranceBadge(s.assurance));
      list.append(row);
    }
  };
  const drawInfo = () => {
    clear(info);
    const s = S.styles.find((x) => x.id === chosen);
    if (!s) return;
    const [, , why] = assurance(s.assurance);
    info.append(h("div.small", h("strong", "Assurance: "), assuranceBadge(s.assurance), " ", why),
      h("div.small", h("strong", "Licence: "), s.license || "Not stated"),
      s.parent ? h("div.small", h("strong", "Based on: "), s.parent) : null,
      s.id === "apa-7" ? h("div.small.muted", "APA 7 is written in US English by the tested composer; the citation language applies to CSL styles.") : null,
      s.source === "imported" ? btn("Remove this imported style", { small: true, kind: "ghost", onClick: async () => {
        if (!(await confirmDialog("Remove style?", `Remove “${s.title}” from this library? Manuscripts that use it must switch first.`, { ok: "Remove" }))) return;
        try { await call("styles.remove", { id: s.id }); await loadInfo(); chosen = m?.style || "apa-7"; drawList(); drawInfo(); drawPreview(); toast("Style removed", "success"); } catch (e) { showError(e); }
      } }) : null);
  };
  const drawPreview = async () => {
    clear(preview);
    if (!ids.length) { preview.append(h("p.small.muted", "Add sources to the active manuscript to preview them in each style.")); return; }
    preview.append(h("p.small.muted", "Rendering…"));
    try {
      const base = { manuscript_id: m.id, style: chosen };
      const c = await call("cite.render", { request: { ...base, mode: "parenthetical", items: [{ ref_id: ids[0] }] } });
      const b = await call("cite.render", { request: { ...base, mode: "bibliography", set: "selected", ref_ids: ids } });
      clear(preview);
      preview.append(h("div.small.muted", "Citation"), h("div.output-box", { html: runsHtml(c.runs) }),
        h("div.small.muted", { style: { marginTop: "8px" } }, "Reference list"), h("div.output-box.bib", { html: b.paragraphs.map((x) => `<p style="padding-left:${x.hanging_indent}in;text-indent:-${x.hanging_indent}in">${runsHtml(x.runs)}</p>`).join("") }),
        h("p.small.muted", "Preview uses the manuscript's citation language, not the one selected above, until you apply."));
    } catch (e) { clear(preview); preview.append(h("div.notice.error", e.message)); }
  };
  q.addEventListener("input", drawList);
  const importBtn = btn("Import CSL style file…", { icon: "download", onClick: async () => {
    try {
      const r = await call("styles.import");
      if (!r || r.cancelled) return;
      await loadInfo(); chosen = r.id; drawList(); drawInfo(); drawPreview();
      toast(`Imported “${r.title}”. It is marked Imported, not reviewed.`, "success");
    } catch (e) { showError(e); }
  } });
  drawList(); drawInfo(); drawPreview();
  const r = await modal("Citation style and language", h("div", { style: { display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)", gap: "16px" } },
    h("div", q, h("div", { style: { height: "8px" } }), list, h("div.btn-row", { style: { marginTop: "8px" } }, importBtn),
      h("p.small.muted", "Bundled styles come from the CSL style repository and are shown as Untested until test cases for them are run and reviewed.")),
    h("div", field("Citation language", lang, "Terms such as “and”, “edited by” and month names. Independent of the interface language."), info, h("hr"), preview)),
    { size: "wide", buttons: [{ label: "Cancel" }, m ? { label: "Use for this manuscript", kind: "primary", run: () => ({ style: chosen, locale: lang.value }) } : null].filter(Boolean) }).done;
  if (r && m) {
    try { await call("manuscripts.update", { manuscript: { ...m, style: r.style, locale: r.locale } }); await refresh(); toast("Style updated. Copies made with the old style are listed under Changed since copying.", "success"); }
    catch (e) { showError(e); }
  }
}
