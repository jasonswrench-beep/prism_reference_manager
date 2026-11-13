// Settings: appearance, storage and backup, network, citation keys, local connections, privacy.

import { call, on } from "../api.js";
import { h, clear, icon, btn, toast, select, field, showError, fmtDate, tabs, confirmDialog, modal } from "../dom.js";
import { S, registerView, go, saveSettings, refresh, autoIndex, loadInfo } from "../app.js";
import * as dialogs from "../dialogs.js";
import { fieldLabel } from "../editor.js";

// Updates: built in but not set up until releases are published. Checks happen only when asked,
// or once a day if the person turns that on; installing always needs a click.
async function updatesCard(card) {
  const u = await call("update.state");
  const out = h("div", { "aria-live": "polite" });
  const last = u.last_check ? h("p.small.muted", "Last checked " + fmtDate(u.last_check) + ".") : null;
  if (!u.configured) {
    return card("Updates", h("p", h("span.badge.neutral", "Not set up"), " This version cannot check for updates by itself. New versions are published on the project's release page; download and run the new installer, and your settings and libraries are kept."),
      h("p.small.muted", "When automatic updates are set up in a later version, they stay off until you turn them on, and nothing is downloaded without your go-ahead."));
  }
  const install = async (version) => {
    const ok = await confirmDialog("Install version " + version + "?", "The update is downloaded, its signature is checked, and the installer runs. The program closes while it installs; your library is saved and closed first.", { ok: "Download and install" });
    if (!ok) return;
    clear(out); out.append(h("p", "Downloading…"));
    const off = await on("update-progress", (p) => { clear(out); out.append(h("p", p.total ? `Downloading… ${Math.round((p.received / p.total) * 100)}%` : `Downloading… ${Math.round(p.received / 1024)} KB`)); });
    try { await call("update.install"); } catch (e) { clear(out); out.append(h("div.notice.error", e.message)); } finally { if (typeof off === "function") off(); }
  };
  const check = async () => {
    clear(out); out.append(h("p", "Checking…"));
    try {
      const r = await call("update.check");
      clear(out);
      out.append(r.available
        ? h("div", h("p", h("strong", `Version ${r.version} is available.`), ` You have ${u.version}.`), r.notes ? h("pre.small", { style: { whiteSpace: "pre-wrap" } }, r.notes) : null, btn("Download and install…", { kind: "primary", onClick: () => install(r.version) }))
        : h("p", `You have the latest version (${u.version}).`));
    } catch (e) { clear(out); out.append(h("div.notice.error", e.message)); }
  };
  const auto = h("input", { type: "checkbox", checked: u.automatic, disabled: u.locked, on: { change: (e) => saveSettings({ update_check: e.target.checked }) } });
  return card("Updates",
    h("p", `Update address: ${u.host}. A check sends this program's version and Windows architecture, and appears in the network log.`),
    h("label.row", auto, h("span", "Check for updates once a day")), u.locked ? h("p.small.muted", "Set by your administrator.") : null,
    h("p.small.muted", "Automatic checks only tell you a new version exists. Nothing is downloaded or installed without your go-ahead."),
    h("div.btn-row", btn("Check now", { onClick: check })), last, out);
}

registerView("settings", {
  async render(el, params) {
    const tab = params.tab || "appearance";
    const locked = S.info?.policy?.locked || {};
    const lockNote = (k) => (k in locked ? h("span.badge.neutral", { title: "Set by your administrator" }, icon("shield", 13), "Managed") : null);
    el.append(h("h1.view-title", { tabindex: "-1" }, "Settings"), h("p.view-sub", "Saved for you on this computer" + (S.info?.portable ? " (portable mode: next to the program)" : "") + "."),
      tabs([["appearance", "Appearance"], ["library", "Library and backup"], ["fields", "Fields and types"], ["network", "Network and privacy"], ["citation", "Citation keys"], ["api", "Local connections"], ["about", "About"]], tab, (k) => go("settings", { tab: k }), { label: "Settings sections" }));
    const body = h("div", { style: { marginTop: "14px", maxWidth: "900px" } });
    el.append(body);
    const s = S.settings;
    const card = (title, ...kids) => h("div.card.pad", { style: { marginBottom: "12px" } }, h("h2", { style: { fontSize: "18px" } }, title), ...kids);

    if (tab === "appearance") {
      body.append(card("Theme and text",
        h("div.grid2",
          field("Theme", select([["system", "Match Windows"], ["light", "Light"], ["dark", "Dark"], ["high-contrast", "High contrast"]], s.theme, { on: { change: (e) => saveSettings({ theme: e.target.value }) } })),
          field("Density", select([["comfortable", "Comfortable"], ["compact", "Compact"]], s.density, { on: { change: (e) => saveSettings({ density: e.target.value }) } })),
          field("Text size", select([12, 13, 14, 15, 16, 18, 20, 22, 24, 28].map((n) => [String(n), n + " px" + (n === 14 ? " (default)" : "")]), String(s.font_size), { on: { change: (e) => saveSettings({ font_size: +e.target.value }) } }), "Windows text scaling also applies."),
          field("Motion", select([["no", "Use Windows setting"], ["yes", "Reduce motion"]], s.reduce_motion ? "yes" : "no", { on: { change: (e) => saveSettings({ reduce_motion: e.target.value === "yes" }) } }))),
        h("p.small.muted", "Colour is never the only cue: states also use text and icons. Windows contrast themes are respected.")),
        card("Keyboard", h("p", "Every action is in the command palette (Ctrl+K) and the menus. "), btn("Show keyboard shortcuts", { onClick: () => dialogs.shortcuts() })));
    }
    if (tab === "library") {
      const info = S.info?.library;
      if (info) {
        const rules = S.settings.save_rules || {};
        const rule = (k, label, help) => {
          const cb = h("input", { type: "checkbox", id: "rule-" + k, checked: !!rules[k], on: { change: (e) => saveSettings({ save_rules: { ...(S.settings.save_rules || {}), [k]: e.target.checked } }) } });
          return h("div", { style: { marginBottom: "6px" } }, h("label.row", { for: "rule-" + k }, cb, label), help ? h("div.small.muted", { style: { marginLeft: "26px" } }, help) : null);
        };
        body.append(card("Tidying when sources are saved",
          h("p.small.muted", "Applied every time a source is added, edited or imported. Changes show in the source's history."),
          rule("tidy_spaces", "Remove extra spaces"),
          rule("standard_identifiers", "Write DOIs, ISBNs and ISSNs in one standard form"),
          rule("title_final_period", "Remove a final period from titles", "Citation styles add their own punctuation."),
          rule("capital_titles", "Change titles typed entirely in capitals to sentence case", "Words in Protected terms keep their capitals; check names afterwards."),
          rule("capital_names", "Change author names typed entirely in capitals (SMITH, JOHN)")));
        const hd = select([["0", "Off"], ["7", "Every week"], ["30", "Every month"]], String(S.settings.health_days ?? 7), { "aria-label": "Automatic health check", on: { change: (e) => saveSettings({ health_days: +e.target.value }) } });
        body.append(card("Library health check",
          field("Run a quick check automatically", hd, "Looks for missing information, duplicate citation keys and file problems, and tells you if anything needs attention. It never changes anything. The duplicate search runs only when you open the full report."),
          h("div.btn-row", btn("Open the full report", { small: true, onClick: () => dialogs.health() })),
          S.settings.health_last ? h("p.small.muted", "Last automatic check: " + fmtDate(S.settings.health_last)) : null));
        const ai = h("input", { type: "checkbox", id: "auto-index", checked: S.settings.auto_index !== false, on: { change: async (e) => { await saveSettings({ auto_index: e.target.checked }); if (e.target.checked) autoIndex(); } } });
        body.append(card("Full-text search",
          h("label.row", { for: "auto-index" }, ai, "Read the text of PDFs in the background so full-text search finds them"),
          h("p.small.muted", "One PDF at a time, with a pause between files. A file can be left out of search from its Files tab."),
          h("p.small", "Scanned PDFs have no text to read. Their text can be recognized on this computer by Windows (nothing is sent anywhere, and the files are not changed)."),
          h("div.btn-row", btn("Recognize text in scanned PDFs…", { small: true, onClick: async () => { const list = await call("files.scanned"); if (!list.length) { toast("No scanned PDFs without recognized text were found. PDFs are checked when their text is first read.", "info"); return; } const { recognizeDialog } = await import("./reader.js"); await recognizeDialog(list); } }))));
        const watched = await call("watch.list").catch(() => []);
        const mirror = h("input", { type: "checkbox", id: "w-mirror" });
        body.append(card("Watched folders",
          h("p.small.muted", "New PDFs that appear in a watched folder (for example your downloads folder) are read about once a minute and wait for your review under Connections; nothing is added without it. Files already in the library are ignored."),
          watched.length ? h("table.table", h("tbody", watched.map((w) => h("tr", h("td.mono.small", w.path), h("td.small", w.project_id ? (S.projects.find((p) => p.project.id === w.project_id)?.project.title || "Project") : "Library only"), h("td.small", w.mirror ? "Subfolders become collections" : ""),
            h("td", btn("Stop watching", { small: true, onClick: async () => { await call("watch.remove", { path: w.path }); go("settings", { tab: "library" }); } }))))) ) : h("p.small", "No folders are watched."),
          h("label.row.small", { for: "w-mirror" }, mirror, "Turn subfolders into collections"),
          h("div.btn-row", btn("Watch a folder…", { small: true, kind: "primary", onClick: async () => { try { const r = await call("watch.add", { project_id: S.project?.project.id || "", mirror: mirror.checked }); if (r && !r.cancelled) { toast(`Watching ${r.path}`, "success"); go("settings", { tab: "library" }); } } catch (e) { showError(e); } } }),
            watched.length ? btn("Check now", { small: true, onClick: async () => { const r = await call("watch.scan"); toast(r.length ? `${r.reduce((n, x) => n + x.count, 0)} new PDFs staged for review` : "No new PDFs found", r.length ? "success" : "info"); } }) : null)));
        const fb = await call("files.base").catch(() => null);
        const fnPat = h("input", { value: S.settings.file_name_pattern || "", placeholder: "Keep the original name", "aria-label": "File name pattern", style: { width: "260px" } });
        if (fb) {
          const reload = () => go("settings", { tab: "library" });
          const setBase = async (thisComputer) => {
            const p = await call("dialog.pick_folder", { title: thisComputer ? "Choose where the linked-files folder is on this computer" : "Choose the folder your linked files live in" });
            if (!p) return;
            try { const n = await call("files.set_base", { folder: p, this_computer: thisComputer }); toast(n ? `${n} links now relative to ${p}` : `Linked files are now found in ${p}`, "success"); reload(); } catch (e) { showError(e); }
          };
          const convert = async (to) => {
            let folder = "";
            if (to === "linked") {
              folder = await call("dialog.pick_folder", { title: "Choose a folder to put the files in" });
              if (!folder) return;
            } else if (!(await confirmDialog("Copy files into the library", `Copy ${fb.linked} linked files into the library folder? The originals stay where they are. This uses disk space in the library folder.`, { ok: "Copy files" }))) return;
            const t = toast(to === "managed" ? "Copying files into the library…" : "Copying files to the folder…", "info", { sticky: true });
            try {
              const r = await call("files.convert", { ids: [], to, folder });
              t.remove();
              toast(`${r.converted} files converted${r.failed.length ? `; ${r.failed.length} could not be (missing or changed)` : ""}. Undo is available.`, r.failed.length ? "warn" : "success");
              reload();
            } catch (e) { t.remove(); showError(e); }
          };
          body.append(card("File storage",
            h("p.small.muted", "Managed files are copied into the library folder and travel with it. Linked files stay where you keep them. Linked files inside a base folder are stored relative to it, so the library still finds them when that folder is at a different place on another computer or drive."),
            h("p.small", `${fb.managed} managed files • ${fb.linked} linked files, ${fb.relative} of them relative to the base folder`),
            field("Base folder for linked files", h("div.row", h("input", { value: fb.library, placeholder: "None (links store full paths)", readOnly: true }), btn("Choose…", { onClick: () => setBase(false) }), fb.library ? btn("Clear", { onClick: async () => { await call("files.set_base", { folder: "" }); reload(); } }) : null)),
            field("On this computer the base folder is at", h("div.row", h("input", { value: fb.this_computer, placeholder: fb.library ? "Same as above" : "Same as above (none set)", readOnly: true }), btn("Choose…", { onClick: () => setBase(true) }), fb.this_computer ? btn("Reset", { onClick: async () => { await call("files.set_base", { folder: "", this_computer: true }); reload(); } }) : null), "Use this when the shared folder sits at a different path here (for example another drive letter)."),
            h("div.btn-row",
              fb.linked ? btn("Copy all linked files into the library…", { small: true, onClick: () => convert("managed") }) : null,
              fb.managed ? btn("Copy managed files out to a folder and link them…", { small: true, onClick: () => convert("linked") }) : null,
              fb.effective && fb.linked > fb.relative ? btn("Make links relative", { small: true, onClick: async () => { const n = await call("files.make_relative"); toast(`${n} links made relative`, n ? "success" : "info"); reload(); } }) : null),
            h("p.small.muted", "Converting copies files; nothing is deleted. A file that is missing or changed outside the program is left as it is and reported."),
            field("Name files copied into the library", h("div.row", fnPat, btn("Save", { small: true, onClick: () => saveSettings({ file_name_pattern: fnPat.value.trim() }) })), "Tokens: [auth] [Auth] [year] [shorttitle] [title3] [citekey]. Empty keeps the downloaded name. The original name is always kept."),
            h("div.btn-row", btn("Storage report…", { small: true, onClick: () => dialogs.storageReport() }), btn("Check attached files…", { small: true, onClick: () => dialogs.checkFiles() }))));
        }
      }
      const bl = info ? await call("backup.list").catch(() => null) : null;
      body.append(card("This library", info ? h("div",
        h("p", h("strong", info.name), h("br"), h("span.mono", info.root)),
        info.sync_folder_warning ? h("div.notice.warn", icon("warn"), "This library is inside a synchronized folder. Never open it on two computers at the same time; the program refuses a second writer, but sync conflicts can still damage files. Prefer a local folder and move between computers with backups.") : null,
        h("p.small", `${info.counts.references} sources • ${info.counts.projects} projects • ${info.counts.notes} notes • ${info.counts.annotations} annotations • ${info.counts.attachments} files • schema ${info.schema}`),
        h("div.btn-row", btn("Rename library…", { onClick: async () => { const { promptDialog } = await import("../dom.js"); const n = await promptDialog("Rename library", "Name", info.name, { ok: "Rename" }); if (n && n.trim()) { try { await call("library.rename", { name: n.trim() }); await loadInfo(); go("settings", { tab: "library" }); } catch (e) { showError(e); } } } }), btn("Show in folder", { onClick: () => call("library.reveal") }), btn("Open another library…", { onClick: () => dialogs.openLibrary() }), btn("New library…", { onClick: () => dialogs.newLibrary() }), btn("Library health", { onClick: () => dialogs.health() }), btn("Index all PDFs for search", { onClick: async () => { const { indexAll } = await import("./reader.js"); const t = toast("Indexing PDF text…", "info", { sticky: true }); const n = await indexAll((d, all) => (t.querySelector(".grow div").textContent = `Indexing PDF text… ${d} of ${all}`)); t.remove(); toast(`${n} PDFs indexed`, "success"); } }))) : h("p.muted", "No library is open.")),
        card("Automatic backup",
          h("label.row", { for: "ab" }, h("input", { type: "checkbox", id: "ab", checked: s.auto_backup, disabled: "auto_backup" in locked, on: { change: (e) => saveSettings({ auto_backup: e.target.checked }) } }), "Back up automatically", lockNote("auto_backup")),
          h("div.grid2",
            field("How often", select([["6", "Every 6 hours"], ["12", "Every 12 hours"], ["24", "Daily"], ["168", "Weekly"]], String(s.backup_interval_hours), { on: { change: (e) => saveSettings({ backup_interval_hours: +e.target.value }) } })),
            field("Keep", select([["3", "3 backups"], ["5", "5 backups"], ["10", "10 backups"], ["30", "30 backups"]], String(s.backup_keep), { on: { change: (e) => saveSettings({ backup_keep: +e.target.value }) } }))),
          field("Backup folder", h("div.row", h("input", { value: s.backup_dir, placeholder: "Inside the library (backups folder)", readOnly: true }), btn("Choose…", { onClick: async () => { const p = await call("dialog.pick_folder", { title: "Choose a backup folder (ideally on another drive)" }); if (p) saveSettings({ backup_dir: p }); } }), s.backup_dir ? btn("Reset", { onClick: () => saveSettings({ backup_dir: "" }) }) : null), "A folder on another drive protects against disk failure."),
          h("label.row", { for: "bf" }, h("input", { type: "checkbox", id: "bf", checked: s.backup_include_files, on: { change: (e) => saveSettings({ backup_include_files: e.target.checked }) } }), "Include attached files (larger backups)"),
          h("p.small.muted", s.last_backup ? `Last backup ${fmtDate(s.last_backup)}. Each backup is verified after it is written.` : "No backup has been made yet."),
          h("div.btn-row", btn("Back up now…", { kind: "primary", onClick: () => dialogs.backupNow() }), btn("Verify a backup…", { onClick: () => dialogs.verifyBackup() }), btn("Restore a backup…", { onClick: () => dialogs.restoreBackup() }), btn("Export complete package…", { onClick: () => dialogs.exportPackage() })),
          bl?.backups?.length ? h("details", h("summary", `${bl.backups.length} backups in ${bl.dir}`), h("ul.small", bl.backups.slice(0, 20).map((b) => h("li", `${b.name} (${(b.size / 1048576).toFixed(1)} MB)`)))) : null),
        card("Trash", field("Delete items from the trash after", select([["7", "7 days"], ["30", "30 days"], ["90", "90 days"], ["365", "One year"]], String(s.trash_retention_days), { on: { change: (e) => saveSettings({ trash_retention_days: +e.target.value }) } })),
          btn("Empty trash now", { kind: "danger", onClick: async () => { if (await confirmDialog("Empty trash", "Permanently delete everything in the trash, with its files and annotations? This cannot be undone.", { ok: "Empty trash", kind: "danger" })) { const n = await call("refs.empty_trash", { confirmed: true }); toast(`${n} items deleted permanently`, "success"); await refresh(); } } })),
        card("Duplicate detection", field("Sensitivity", select([["0.80", "More matches"], ["0.88", "Balanced"], ["0.94", "Fewer, surer matches"]], String(s.duplicate_sensitivity?.toFixed?.(2) || "0.88"), { on: { change: (e) => saveSettings({ duplicate_sensitivity: +e.target.value }) } }))));
    }
    if (tab === "fields") {
      if (!S.info?.library) { body.append(card("Fields and types", h("p", "Open a library first."))); return; }
      const [fields, types] = await Promise.all([call("custom.fields"), call("custom.types")]);
      const KIND = { text: "Text", number: "Number", date: "Date", list: "List of values", boolean: "Yes or no", link: "Web link" };
      const reload = async () => { await loadInfo(); go("settings", { tab: "fields" }); };
      const typeName = (id) => S.customTypes.find((t) => t.id === id)?.label || S.types.find((t) => t.id === id)?.label || id;
      const editField = async (d = {}) => {
        const name = h("input", { value: d.name || "" });
        const kind = select(Object.entries(KIND), d.kind || "text");
        const opts = h("textarea", { rows: 4, value: (d.options || []).join("\n"), placeholder: "One value per line" });
        const optsField = field("Allowed values", opts, "For a list: the values people can choose from.");
        const def = h("input", { value: d.default || "" });
        const req = h("input", { type: "checkbox", id: "cf-req", checked: !!d.required });
        const help = h("input", { value: d.help || "" });
        const scope = new Set(d.types || []);
        const typeBoxes = h("div", { style: { maxHeight: "160px", overflow: "auto", columns: 2 } }, [...S.customTypes.map((t) => [t.id, t.label + " (yours)"]), ...S.types.map((t) => [t.id, t.label])].map(([id, l]) => {
          const cb = h("input", { type: "checkbox", checked: scope.has(id), on: { change: (e) => (e.target.checked ? scope.add(id) : scope.delete(id)) } });
          return h("label.row.small", cb, l);
        }));
        const sync = () => { optsField.hidden = kind.value !== "list"; };
        kind.addEventListener("change", sync); sync();
        const r = await modal(d.id ? "Edit field" : "New field", h("div", field("Name", name), field("Kind of value", kind, d.id ? "Changing the kind of a field with values can make existing values invalid." : null), optsField,
          field("Default value", def, "Filled in for new sources."), h("label.row", { for: "cf-req" }, req, "Required: a source cannot be saved without it"), field("Help text", help),
          h("details", h("summary", `Applies to ${scope.size ? "some types" : "every type"}`), h("p.small.muted", "Leave all unticked to use the field for every type."), typeBoxes)),
          { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Save", kind: "primary", run: () => call("custom.save_field", { field: { ...d, name: name.value, kind: kind.value, options: opts.value.split("\n"), default: def.value, required: req.checked, help: help.value, types: [...scope] } }) }] }).done;
        if (r) { toast(`Field “${r.name}” saved`, "success"); reload(); }
      };
      const editType = async (t = {}) => {
        const label = h("input", { value: t.label || "" });
        const base = select(S.types.map((b) => [b.id, b.label]), t.base || "report");
        const help = h("input", { value: t.help || "" });
        const ALL = ["container_title", "volume", "issue", "page", "article_number", "publisher", "publisher_place", "edition", "isbn", "issn", "number", "genre", "medium", "version", "event", "event_place", "event_date", "archive", "archive_location", "collection_title", "section", "jurisdiction", "authority", "original_publisher", "status", "dimensions", "scale"];
        const picked = new Set(t.fields?.length ? t.fields : (S.types.find((b) => b.id === base.value)?.fields || []));
        const boxes = h("div", { style: { columns: 2 } });
        const drawBoxes = () => clear(boxes, ...ALL.map((f) => h("label.row.small", h("input", { type: "checkbox", checked: picked.has(f), on: { change: (e) => (e.target.checked ? picked.add(f) : picked.delete(f)) } }), fieldLabel(f))));
        base.addEventListener("change", () => { if (!t.id) { picked.clear(); for (const f of S.types.find((b) => b.id === base.value)?.fields || []) picked.add(f); drawBoxes(); } });
        drawBoxes();
        const r = await modal(t.id ? "Edit type" : "New type", h("div", field("Name", label, "For example Policy brief, Lab protocol, Podcast transcript."),
          field("Cited as", base, "Citation styles and export formats treat it as this built-in type, so output stays correct."), field("Help text", help),
          h("h3", "Fields shown"), h("p.small.muted", "Title, authors, date, DOI, URL, abstract, language, keywords and notes are always available."), boxes),
          { size: "narrow", buttons: [{ label: "Cancel" }, { label: "Save", kind: "primary", run: () => call("custom.save_type", { type: { ...t, label: label.value, base: base.value, help: help.value, fields: ALL.filter((f) => picked.has(f)) } }) }] }).done;
        if (r) { toast(`Type “${r.label}” saved`, "success"); reload(); }
      };
      body.append(card("Your fields",
        h("p.small.muted", "Fields for information the built-in fields do not cover, such as study design or sample size. Values can be searched, exported to CSV and CSL JSON, and shown in the source details."),
        fields.length ? h("table.table", h("thead", h("tr", h("th", "Name"), h("th", "Kind"), h("th", "Applies to"), h("th", ""))), h("tbody", fields.map((d) => h("tr", h("td", h("strong", d.name), d.required ? h("span.small.muted", " required") : null), h("td.small", KIND[d.kind] + (d.kind === "list" ? `: ${d.options.join(", ")}` : "")), h("td.small", d.types?.length ? d.types.map(typeName).join(", ") : "Every type"),
          h("td.nowrap", btn("Edit", { small: true, onClick: () => editField(d) }), btn("Delete", { small: true, kind: "ghost", onClick: async () => {
            try { await call("custom.delete_field", { id: d.id }); toast("Field deleted", "success"); reload(); }
            catch (e) { if (await confirmDialog("Delete field and its values?", e.message + " Undo can bring them back.", { ok: "Delete field and values", kind: "danger" })) { const n = await call("custom.delete_field", { id: d.id, remove_values: true }); toast(`Field deleted; values removed from ${n} sources.`, "success"); reload(); } }
          } })))))) : h("p.small", "No fields yet."),
        btn("New field…", { small: true, kind: "primary", icon: "plus", onClick: () => editField() })),
        card("Your types",
          h("p.small.muted", "A type of your own is a named variant of a built-in type with its own fields. Citation styles format it as the built-in type it is based on."),
          types.length ? h("table.table", h("tbody", types.map((t) => h("tr", h("td", h("strong", t.label)), h("td.small", "Cited as " + typeName(t.base)), h("td.nowrap", btn("Edit", { small: true, onClick: () => editType(t) }), btn("Delete", { small: true, kind: "ghost", onClick: async () => {
            if (!(await confirmDialog("Delete type?", `Sources of type “${t.label}” keep their data and become “${typeName(t.base)}”.`, { ok: "Delete type" }))) return;
            const n = await call("custom.delete_type", { id: t.id }); toast(`Type deleted${n ? `; ${n} sources are now ${typeName(t.base)}` : ""}.`, "success"); reload();
          } })))))) : h("p.small", "No types yet."),
          btn("New type…", { small: true, kind: "primary", icon: "plus", onClick: () => editType() })),
        card("Share fields and types",
          h("p.small.muted", "Save your fields and types as a file that another library or colleague can import. Matching fields and types are updated, new ones added; nothing is deleted."),
          h("div.btn-row", btn("Export…", { small: true, onClick: async () => { const r = await call("custom.export"); if (r && !r.cancelled) toast("Saved to " + r.path, "success"); } }),
            btn("Import…", { small: true, onClick: async () => { try { const r = await call("custom.import"); if (r && !r.cancelled) { toast(`${r.fields_added} fields and ${r.types_added} types added, ${r.fields_updated + r.types_updated} updated.${r.problems.length ? " Problems: " + r.problems.join("; ") : ""}`, r.problems.length ? "info" : "success"); reload(); } } catch (e) { showError(e); } } }))));
    }
    if (tab === "network") {
      const inv = S.info.network_inventory;
      body.append(card("Network use",
        h("label.row", { for: "net" }, h("input", { type: "checkbox", id: "net", checked: s.network_enabled, disabled: "network_enabled" in locked, on: { change: (e) => saveSettings({ network_enabled: e.target.checked }) } }), h("strong", "Allow optional online lookups"), lockNote("network_enabled")),
        h("p.small.muted", "With this off, nothing leaves this computer. The library, reader, notes, citations, exports and backups all work offline."),
        h("table.table", h("thead", h("tr", h("th", "Allowed"), h("th", "Kind"), h("th", "Service"), h("th", "What it sends"))), h("tbody", inv.map((i) => h("tr",
          h("td", h("input", { type: "checkbox", "aria-label": "Allow " + i.kind, checked: !s.network_disabled_kinds.includes(i.kind), disabled: !s.network_enabled, on: { change: (e) => saveSettings({ network_disabled_kinds: e.target.checked ? s.network_disabled_kinds.filter((k) => k !== i.kind) : [...s.network_disabled_kinds, i.kind] }) } })),
          h("td", i.kind), h("td.mono", i.host), h("td.small", i.purpose))))),
        h("div.btn-row", { style: { marginTop: "8px" } }, btn("Network activity log", { onClick: () => dialogs.network() }))),
        card("Privacy", h("ul",
          h("li", "No telemetry and no account. Nothing is sent in the background."),
          h("li", "PDFs and notes are never sent anywhere by this program. Each project can also forbid all outside requests:"),
          S.project ? h("li", h("label.row", { for: "pol" }, h("input", { type: "checkbox", id: "pol", checked: !!S.project.project.policy_no_external, on: { change: async (e) => { await call("projects.update", { project: { ...S.project.project, policy_no_external: e.target.checked } }); await refresh(); } } }), `Block outside requests for “${S.project.project.title}”`)) : null,
          h("li", "Diagnostic bundles are created only when you ask, and you can read them before saving.")),
          btn("Diagnostic bundle…", { onClick: () => dialogs.diagnostics() })));
    }
    if (tab === "citation") {
      const pat = h("input", { value: s.citekey_pattern, "aria-label": "Citation key pattern" });
      const terms = h("textarea", { rows: 3, value: (s.protected_terms || []).join("\n"), "aria-label": "Protected terms" });
      const bibExcl = h("input", { placeholder: "abstract, file, note", "aria-label": "Fields to leave out of kept BibTeX files" });
      const excl = () => bibExcl.value.split(",").map((x) => x.trim()).filter(Boolean);
      const keepFmt = select([["bibtex", "BibTeX (.bib)"], ["biblatex", "biblatex (.bib)"], ["ris", "RIS"], ["csljson", "CSL JSON"], ["csv", "CSV spreadsheet"], ["xlsx", "Excel workbook"]], "bibtex", { "aria-label": "Format of the kept file" });
      const keepScope = select([["project", "Active project"], ["library", "Whole library"]], S.project ? "project" : "library", { "aria-label": "Sources in the kept file" });
      body.append(card("Citation keys", field("Pattern", pat, "Tokens: [auth] [Auth] [authors2] [year] [shorttitle] [title3] [type]. Keys stay unique; locked keys never change."),
        btn("Save", { kind: "primary", onClick: () => saveSettings({ citekey_pattern: pat.value }) })),
        card("Files kept up to date", h("p.small.muted", "Keep an export file current automatically, for LaTeX or Markdown writing, a shared spreadsheet or another program. It is rewritten within seconds of any change, and only when its content changes."),
          (s.bib_exports || []).length ? h("table.table", h("tbody", s.bib_exports.map((b) => h("tr", h("td.mono.small", b.path), h("td", b.project_id ? (S.projects.find((p) => p.project.id === b.project_id)?.project.title || "Project") : "Whole library"),
            h("td", { bibtex: "BibTeX", biblatex: "biblatex", ris: "RIS", csljson: "CSL JSON", csv: "CSV", xlsx: "Excel" }[b.format || (b.biblatex ? "biblatex" : "bibtex")]), h("td.small", (b.exclude_fields || []).length ? "Leaves out " + b.exclude_fields.join(", ") : "All fields"),
            h("td", btn("Stop", { small: true, onClick: async () => { await call("bib.remove_export", { path: b.path }); await refresh(); go("settings", { tab: "citation" }); } })))))) : null,
          h("div.grid2", field("Format", keepFmt), field("Sources", keepScope)),
          field("Leave out fields", bibExcl, "Comma separated, for BibTeX and biblatex. Applies to the file you add next; to change a kept file, stop it and add it again."),
          h("div.btn-row", btn("Keep a file up to date…", { kind: "primary", onClick: async () => {
            if (keepScope.value === "project" && !S.project) return toast("Choose a project first, or keep a file for the whole library.", "info");
            try { const r = await call("bib.add_export", { project_id: keepScope.value === "project" ? S.project.project.id : "", format: keepFmt.value, exclude_fields: excl() }); if (r && !r.cancelled) { toast(`${r.count} sources written to ${r.path}`, "success"); await refresh(); go("settings", { tab: "citation" }); } } catch (e) { showError(e); }
          } }))),
        card("Protected terms", field("Words whose capitals must never change", terms, "One per line, for example species names, chemical formulas and acronyms."), btn("Save", { kind: "primary", onClick: () => saveSettings({ protected_terms: terms.value.split(/\n/).map((x) => x.trim()).filter(Boolean) }) })),
        card("Clipboard", field("Font for exported files", select([["", "Receiving document's font"], ["Times New Roman", "Times New Roman"], ["Calibri", "Calibri"], ["Arial", "Arial"]], s.default_paste_font, { on: { change: (e) => saveSettings({ default_paste_font: e.target.value }) } })),
          h("p.small.muted", "Copies put plain text, HTML and rich text on the clipboard together, so the receiving program can keep italics and indents. A program's “paste as plain text” always removes formatting; use Export for files.")));
    }
    if (tab === "api") {
      const st = await call("api.state");
      body.append(card("Local connections",
        h("p", "Other programs on this computer (for example a browser capture helper or a writing tool) can use your library through a local connection. It is off until you turn it on, accepts only connections from this computer, and each program must be approved by you."),
        h("label.row", { for: "api" }, h("input", { type: "checkbox", id: "api", checked: st.enabled, disabled: "api_enabled" in locked, on: { change: async (e) => { await saveSettings({ api_enabled: e.target.checked }); go("settings", { tab: "api" }); } } }), "Allow local connections", lockNote("api_enabled")),
        field("Port", h("input", { type: "number", value: st.port, style: { width: "120px" }, on: { change: (e) => saveSettings({ api_port: +e.target.value }) } }), "127.0.0.1 only."),
        h("div.card.pad", { style: { margin: "12px 0", background: "var(--surface-2)" } },
          h("h3", { style: { marginTop: 0 } }, "Browser capture"),
          h("p", "The capture extension saves the paper on the page you are reading, with its details and PDF, into this program for review. It works in Chromium-based browsers."),
          h("ol",
            h("li", "Turn on “Allow local connections” above."),
            h("li", "In your browser, open the extensions page, switch on developer mode, choose “Load unpacked”, and pick the extension folder."),
            h("li", "Click the extension's button and choose Connect. Approve it here when asked.")),
          h("div.btn-row", btn("Open the extension folder", { onClick: async () => { try { await call("capture.extension_folder", { open: "1" }); } catch (e) { showError(e); } } }))),
        h("h3", "Approved programs"), st.clients.length ? h("table.table", h("thead", h("tr", h("th", "Program"), h("th", "Permissions"), h("th", "Last used"), h("th", ""))), h("tbody", st.clients.map((c) => h("tr", h("td", c.name), h("td.small", c.scopes.join(", ")), h("td.small", c.last_used ? fmtDate(c.last_used) : "Never"), h("td", btn("Revoke", { small: true, kind: "danger", onClick: async () => { await call("api.revoke", { id: c.id }); toast("Revoked. The program can no longer connect.", "success"); go("settings", { tab: "api" }); } })))))) : h("p.muted", "None."),
        h("h3", "Recent activity"), st.log.length ? h("table.table", h("tbody", st.log.slice(0, 40).map((l) => h("tr", h("td.small", fmtDate(l.time)), h("td", l.client), h("td.mono", l.method), h("td", l.allowed ? "Allowed" : h("span.status-review", "Refused")))))) : h("p.muted", "No activity."),
        h("p.small.muted", "Content from your library (titles, PDFs, notes) is passed to connected programs as data only. Items created by a connected program are marked as machine-created and can be filtered and removed.")));
    }
    if (tab === "about") {
      body.append(card("Prism Reference Manager", h("img", { src: "logo.svg", alt: "", style: { maxWidth: "460px", width: "100%", borderRadius: "8px" } }),
        h("p", `Version ${S.info.version}. Licensed under the Apache License 2.0; third-party components are listed in THIRD-PARTY.md in the install folder.`),
        h("p", "Settings are stored in your Windows profile. Libraries live only in the folders you choose. Uninstalling removes the program and its settings, never your libraries."),
        S.info.policy?.source ? h("p.small", "Administrator policy: " + S.info.policy.source) : null));
      body.append(await updatesCard(card));
    }
  },
});
