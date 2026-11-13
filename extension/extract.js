// Runs inside the page only when the person clicks the capture button. It reads what the page
// declares about itself and returns plain data; it changes nothing on the page.
// This function must stay self-contained: it is serialized and injected by the browser.

export function extractPage() {
  const out = {
    url: location.href,
    title: document.title || "",
    contentType: document.contentType || "",
    meta: {},
    jsonld: [],
    coins: [],
    links: [],
    pdfLinks: [],
    dois: [],
  };
  for (const m of document.querySelectorAll("meta[name], meta[property]")) {
    const k = (m.getAttribute("name") || m.getAttribute("property") || "").toLowerCase().trim();
    const v = m.getAttribute("content");
    if (!k || v == null || !v.trim()) continue;
    (out.meta[k] = out.meta[k] || []).push(v.trim());
  }
  for (const s of document.querySelectorAll('script[type="application/ld+json"]')) {
    try {
      const j = JSON.parse(s.textContent);
      const list = Array.isArray(j) ? j : j && j["@graph"] ? j["@graph"] : [j];
      for (const x of list) if (x && typeof x === "object") out.jsonld.push(x);
    } catch (_) {}
  }
  for (const s of document.querySelectorAll("span.Z3988[title]")) out.coins.push(s.getAttribute("title"));

  const linkTitle = (a) => {
    const t = (a.textContent || "").trim().replace(/\s+/g, " ");
    if (t.length > 15 && !/^(https?:|10\.\d)/i.test(t)) return t.slice(0, 300);
    const block = a.closest("li, article, tr, section, div");
    const hd = block && block.querySelector("h1, h2, h3, h4, [class*='title']");
    return (hd ? hd.textContent : t).trim().replace(/\s+/g, " ").slice(0, 300);
  };
  const seen = new Set();
  for (const a of document.querySelectorAll("a[href]")) {
    const href = a.href;
    let kind = null, id = null, m;
    if ((m = href.match(/doi\.org\/(10\.\d{4,9}\/[^?#\s]+)/i)) || (m = href.match(/\/doi\/(?:abs\/|full\/|pdf\/|epdf\/)?(10\.\d{4,9}\/[^?#\s]+)/i))) {
      kind = "doi";
      try { id = decodeURIComponent(m[1]); } catch (_) { id = m[1]; }
    } else if ((m = href.match(/arxiv\.org\/abs\/([\w.\-\/]+?)(v\d+)?\/?$/i))) {
      kind = "arxiv"; id = m[1];
    } else if ((m = href.match(/pubmed\.ncbi\.nlm\.nih\.gov\/(\d{5,9})\/?(?:[?#].*)?$/))) {
      kind = "pmid"; id = m[1];
    } else if ((m = href.match(/\/pmc\/articles\/(PMC\d+)/i))) {
      kind = "pmcid"; id = m[1].toUpperCase();
    }
    if (kind) {
      const key = kind + ":" + id.toLowerCase();
      if (!seen.has(key)) {
        seen.add(key);
        out.links.push({ kind, id, text: linkTitle(a), href });
      }
    }
    if ((/\.pdf($|[?#])/i.test(href) || a.type === "application/pdf") && !out.pdfLinks.includes(href)) out.pdfLinks.push(href);
  }
  const text = document.body ? document.body.innerText.slice(0, 20000) : "";
  for (const m of text.matchAll(/\b(10\.\d{4,9}\/[^\s"<>]+)/g)) {
    const d = m[1].replace(/[.,;)\]]+$/, "");
    if (!out.dois.includes(d)) out.dois.push(d);
    if (out.dois.length >= 20) break;
  }
  return out;
}
