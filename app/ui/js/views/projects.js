// Welcome (no library open) and the projects dashboard.

import { call } from "../api.js";
import { h, icon, iconBtn, btn, fmtDate, toast, confirmDialog, showError, promptDialog } from "../dom.js";
import { S, registerView, go, setProject, refresh, setManuscript } from "../app.js";
import * as dialogs from "../dialogs.js";

registerView("welcome", {
  async render(el) {
    const recent = (S.settings.recent_libraries || []).filter(Boolean);
    el.append(h("div.welcome",
      h("img.logo", { src: "logo.svg", alt: "Prism Reference Manager" }),
      h("h1.view-title", { tabindex: "-1" }, "Welcome"),
      h("p.view-sub", "Collect sources, read and mark documents, organize evidence by project, and produce citations you can check before they leave the program."),
      h("div.options",
        h("div.card.opt", icon("plus", 26), h("h3", "Start fresh"), h("p.muted", "Create a new library in a folder on this computer. No account is needed."), btn("Create library", { kind: "primary", onClick: () => dialogs.newLibrary() })),
        h("div.card.opt", icon("download", 26), h("h3", "Bring in an existing library"), h("p.muted", "Create a library, then import an export file (RIS, BibTeX, CSL JSON, XML or CSV) with a dry run and review."), btn("Create and import", { onClick: async () => { await dialogs.newLibrary(); if (S.info?.library) dialogs.importFile("desktop-manager"); } })),
        h("div.card.opt", icon("folder", 26), h("h3", "Open a library"), h("p.muted", "Open a library folder, or restore a verified backup into a new folder."), h("div.btn-row", btn("Open…", { onClick: () => dialogs.openLibrary() }), btn("Restore backup…", { onClick: () => dialogs.restoreBackup() })))),
      recent.length ? h("div.card.pad", { style: { marginTop: "16px" } }, h("h3", "Recent libraries"), h("ul", recent.map((p) => h("li", h("button.btn.ghost", { type: "button", on: { click: async () => { try { await call("library.open", { path: p }); await refresh(); go("projects"); } catch (e) { showError(e); } } } }, p))))) : null,
      h("div.card.pad", { style: { marginTop: "16px" } }, h("h3", "Three things to know"),
        h("ol",
          h("li", h("strong", "Capture: "), "add sources by identifier, file, PDF or by hand. Everything automated waits in a review step before it joins the library."),
          h("li", h("strong", "Copy citations: "), "choose a project and manuscript first. The manuscript's sources decide year suffixes like 2024a, and the program tells you when copied text has changed since."),
          h("li", h("strong", "Back up: "), "a verified backup is made once a day by default. Restore it into any empty folder."))),
    ));
  },
});

// Tasks for the active project: open first, then done; quick add; source links open the source.
async function tasksCard(active) {
  const tasks = await call("tasks.list", { project_id: active.project.id }).catch(() => []);
  const box = h("div.card");
  const input = h("input", { placeholder: "Add a task and press Enter", "aria-label": "New task" });
  input.addEventListener("keydown", async (e) => { if (e.key === "Enter" && input.value.trim()) { await call("tasks.save", { task: { title: input.value.trim(), project_id: active.project.id } }); go("projects"); } });
  const row = (t) => {
    const cb = h("input", { type: "checkbox", checked: t.status === "done", "aria-label": `Done: ${t.title}`, on: { change: async (e) => { await call("tasks.save", { task: { ...t, status: e.target.checked ? "done" : "open" } }); go("projects"); } } });
    return h("div.row", { style: { padding: "6px 12px", borderBottom: "1px solid var(--divider)", opacity: t.status === "done" ? 0.6 : 1 } }, cb,
      h("div.grow", h("div", { style: { textDecoration: t.status === "done" ? "line-through" : "none" } }, t.title), t.due ? h("div.small.muted", "Due " + t.due) : null,
        t.ref_id ? h("a.small", { href: "#", on: { click: (e) => { e.preventDefault(); go("library", { select: t.ref_id }); } } }, "Open source") : null),
      iconBtn("trash", `Delete task ${t.title}`, async () => { await call("tasks.delete", { id: t.id }); go("projects"); }));
  };
  const open = tasks.filter((t) => t.status !== "done");
  box.append(h("div", { style: { padding: "8px 12px" } }, input), ...open.map(row), ...tasks.filter((t) => t.status === "done").slice(0, 5).map(row));
  if (!tasks.length) box.append(h("p.small.muted", { style: { padding: "0 12px 10px" } }, "No tasks yet. Add one here, or select a source in the Library and choose Add task."));
  return h("div", h("h2.section-title", `Tasks (${open.length} open)`), box);
}

registerView("projects", {
  async render(el) {
    const showArchived = !!S.params.archived;
    const [recent, queue] = await Promise.all([call("projects.recent", { limit: 8 }), call("projects.reading_queue", { limit: 6 })]);
    const list = S.projects.filter((p) => !!p.project.archived === showArchived);
    const head = h("div.row", { style: { alignItems: "flex-start" } },
      h("div.grow", h("h1.view-title", { tabindex: "-1" }, "Your research projects"), h("p.view-sub", "One library. Separate project workspaces.")),
      h("div.btn-row", btn("New project", { kind: "primary", icon: "plus", onClick: () => dialogs.newProject() }), btn("Import library", { icon: "download", onClick: () => dialogs.importFile("desktop-manager") })));
    const rows = h("div.card", { role: "list", "aria-label": showArchived ? "Archived projects" : "Projects" });
    if (!list.length) {
      rows.append(h("div.empty", showArchived ? h("p", "No archived projects.") : [h("h3", "No projects yet"), h("p", "A project keeps its own notes, reading decisions and manuscripts while sharing sources with the rest of the library."), h("div.btn-row", { style: { justifyContent: "center" } }, btn("New project", { kind: "primary", onClick: () => dialogs.newProject() }), btn("Import library", { onClick: () => dialogs.importFile() }))]));
    }
    for (const p of list) {
      const review = p.metadata_review ? `${p.metadata_review} metadata item${p.metadata_review === 1 ? "" : "s"} to review` : p.changed_copies ? `${p.changed_copies} copied citation${p.changed_copies === 1 ? "" : "s"} to review` : p.unread ? `${p.unread} to read` : "Nothing waiting";
      const row = h("div", { role: "listitem", style: { display: "flex" } },
        h("button.proj-row", { type: "button", on: { click: async () => { await setProject(p.project.id); go("library"); } }, "aria-label": `${p.project.title}, ${p.sources} sources, ${review}` },
          h("span", { style: { display: "flex", gap: "8px", alignItems: "center" } }, h("span.swatchbar", { style: { background: p.project.color || "var(--primary)" } }), icon("folder", 26)),
          h("span", h("div.t", p.project.title), h("div.muted", "Active manuscript: " + (p.active_manuscript?.title || "none"))),
          h("span.row", icon("doc"), `${p.sources} sources`),
          h("span.row", icon(p.metadata_review || p.changed_copies ? "warn" : "check"), review),
          icon("chevronRight")),
        h("div.row", { style: { padding: "0 10px", background: "var(--surface)", borderBottom: "1px solid var(--divider)" } },
          showArchived ? btn("Restore", { small: true, onClick: async () => { await call("projects.archive", { id: p.project.id, archived: false }); await refresh(); } }) : btn("Copy", { small: true, onClick: () => dialogs.copyProject(p.project) }),
          showArchived ? btn("Delete", { small: true, kind: "danger", onClick: async () => {
            if (await confirmDialog("Delete project", `Delete “${p.project.title}”, its notes, manuscripts and copy history? Sources stay in the library and in other projects. Undo can bring it back during this session.`, { ok: "Delete project", kind: "danger" })) { await call("projects.delete", { id: p.project.id, confirmed: true }); await refresh(); }
          } }) : btn("Archive", { small: true, onClick: async () => { if (!(await confirmDialog(`Archive “${p.project.title}”?`, "The project leaves the active list and the project menus. Nothing is deleted: Restore it from Archived projects at any time.", { ok: "Archive" }))) return; await call("projects.archive", { id: p.project.id, archived: true }); toast("Project archived. Find it under Archived projects.", "success"); await refresh(); } })));
      rows.append(row);
    }
    const recentTable = recent.length ? h("table.table", h("thead", h("tr", h("th", "Source title"), h("th", "Project"), h("th", "Activity"), h("th", "Updated"))),
      h("tbody", recent.map((r) => h("tr", { tabindex: "0", on: { dblclick: () => go("library", { select: r.entity === "refs" ? r.entity_id : undefined }), keydown: (e) => e.key === "Enter" && go("library") } },
        h("td", h("span.row", icon("doc"), r.title)), h("td", r.project), h("td", h("span.row", icon(r.activity.startsWith("Copied") ? "quote" : r.activity.startsWith("Read") ? "reader" : r.activity.startsWith("Added note") || r.activity.startsWith("Marked") ? "pen" : "plus"), r.activity)), h("td.nowrap", fmtDate(r.time)))))) : h("div.empty", "Work you do appears here.");
    const active = S.project;
    const right = h("div", { style: { display: "flex", flexDirection: "column", gap: "14px" } },
      h("div", h("h2.section-title", "Continue working"), active?.active_manuscript ? h("div.card.pad",
        h("div.row", icon("doc", 30), h("div", h("strong", { style: { fontSize: "17px" } }, active.active_manuscript.title), h("div.muted", (S.styles.find((s) => s.id === active.active_manuscript.style)?.title || active.active_manuscript.style) + " • " + active.project.title))),
        h("div.btn-row", { style: { marginTop: "10px" } }, btn("Open citation workspace", { kind: "primary", icon: "external", onClick: async () => { await setManuscript(active.active_manuscript.id); go("cite"); } }))) : h("div.card.pad.muted", "Create a project to begin.")),
      active ? h("div.row.small", { style: { gap: "6px" } },
        btn("Rename project…", { small: true, kind: "ghost", onClick: async () => { const t = await promptDialog("Rename project", "Name", active.project.title, { ok: "Rename" }); if (t && t.trim()) { try { await call("projects.update", { project: { ...active.project, title: t.trim() } }); await refresh(); go("projects"); } catch (e) { showError(e); } } } }),
        h("label.row.small", "Colour ", h("input", { type: "color", value: active.project.color || "#1554D1", "aria-label": "Project colour", on: { change: async (e) => { await call("projects.update", { project: { ...active.project, color: e.target.value } }); await refresh(); go("projects"); } } }))) : null,
      active ? await tasksCard(active) : null,
      h("div", h("h2.section-title", `Reading queue (${queue.length})`), h("div.card", queue.length ? queue.map((q) => h("button.proj-row", { type: "button", style: { gridTemplateColumns: "28px 1fr" }, on: { click: async () => { await setProject(q.project_id); go("reader", { ref: q.ref_id }); } } }, icon("doc"), h("span", h("div", { style: { fontWeight: 600 } }, q.title), h("div.small.muted", q.project + (q.priority === "high" ? " • high priority" : ""))))) : h("div.empty.small", "Mark sources as high priority or start reading to fill the queue."))));
    el.append(h("div", { style: { display: "grid", gridTemplateColumns: "minmax(0, 1fr) 340px", gap: "20px" } },
      h("div", head, rows,
        h("div.btn-row", { style: { margin: "8px 0 18px" } }, btn(showArchived ? "Show active projects" : `Archived projects (${S.projects.filter((p) => p.project.archived).length})`, { small: true, kind: "ghost", icon: "archive", onClick: () => go("projects", { archived: !showArchived }) })),
        h("h2.section-title", "Recent work"), h("div.card", { style: { overflow: "hidden" } }, recentTable)),
      right));
  },
});
