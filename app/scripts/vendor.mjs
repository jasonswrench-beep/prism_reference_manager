// Copies the PDF rendering library into ui/vendor so the program works offline.
import { cpSync, mkdirSync, copyFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, "..", "node_modules", "pdfjs-dist");
const dest = join(here, "..", "ui", "vendor", "pdfjs");
if (!existsSync(src)) throw new Error("Run npm install first.");
mkdirSync(dest, { recursive: true });
copyFileSync(join(src, "build", "pdf.min.mjs"), join(dest, "pdf.min.js"));
copyFileSync(join(src, "build", "pdf.worker.min.mjs"), join(dest, "pdf.worker.min.js"));
cpSync(join(src, "cmaps"), join(dest, "cmaps"), { recursive: true });
cpSync(join(src, "standard_fonts"), join(dest, "standard_fonts"), { recursive: true });
copyFileSync(join(src, "LICENSE"), join(dest, "LICENSE"));
copyFileSync(join(src, "web", "pdf_viewer.css"), join(dest, "pdf_viewer.css"));
console.log("PDF library copied to", dest);
