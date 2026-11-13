// Saves selected items: fetches PDFs with the browser's own session (so licensed full text the
// person can open is captured as they see it), then sends everything to the program, where it
// waits for review. Keeps working if the popup closes.

import { call } from "./api.js";

function toBase64(buf) {
  const bytes = new Uint8Array(buf);
  let s = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) s += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  return btoa(s);
}

async function fetchPdf(url) {
  const res = await fetch(url, { credentials: "include", redirect: "follow" });
  if (!res.ok) throw new Error(`The site answered ${res.status}.`);
  const buf = await res.arrayBuffer();
  const head = new Uint8Array(buf.slice(0, 5));
  if (String.fromCharCode(...head) !== "%PDF-") throw new Error("The link did not return a PDF (the site may require signing in).");
  if (buf.byteLength > 200 * 1024 * 1024) throw new Error("The PDF is larger than 200 MB.");
  return buf;
}

async function badge(text, color) {
  await chrome.action.setBadgeBackgroundColor({ color: color || "#1554d1" });
  await chrome.action.setBadgeText({ text });
}

async function save(p) {
  await badge("…");
  const notes = [];
  for (const it of p.items) {
    if (it.want_pdf && it.pdf_url) {
      try {
        const buf = await fetchPdf(it.pdf_url);
        it.pdf_base64 = toBase64(buf);
        it.pdf_name = decodeURIComponent(it.pdf_url.split(/[?#]/)[0].split("/").pop() || "document.pdf");
      } catch (e) {
        notes.push(`PDF not saved for “${(it.title || "").slice(0, 60)}”: ${e.message}`);
      }
    }
    delete it.want_pdf;
  }
  try {
    const r = await call("capture.stage", p);
    await badge("✓", "#267a38");
    setTimeout(() => badge(""), 6000);
    return { ...r, notes };
  } catch (e) {
    await badge("!", "#b42318");
    setTimeout(() => badge(""), 8000);
    throw e;
  }
}

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg && msg.type === "save") {
    save(msg.payload).then((result) => reply({ ok: true, result }), (e) => reply({ ok: false, error: e.message }));
    return true;
  }
  return false;
});
