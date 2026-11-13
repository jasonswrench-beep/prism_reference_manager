// Entry point: register views, then start the application frame.
import "./views/projects.js";
import "./views/library.js";
import "./views/reader.js";
import "./views/notes.js";
import "./views/cite.js";
import "./views/connections.js";
import "./views/settings.js";
import "./views/help.js";
import { start } from "./app.js";

start().finally(() => document.getElementById("app").removeAttribute("aria-busy"));
window.addEventListener("error", (e) => console.error(e.error || e.message));
