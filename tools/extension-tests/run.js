// Runs the extension's page reader and record builder against this test page, then checks the
// results. Serve the repository root over http and open article.html or results.html.
import { extractPage } from "../../extension/extract.js";
import { candidates, fromCoins, nameOf, dateOf } from "../../extension/csl.js";

const checks = [];
const ok = (cond, label) => checks.push([!!cond, label]);
const page = extractPage();
const c = candidates(page);

if (location.pathname.endsWith("article.html")) {
  const p = c.primary.csl;
  ok(p.type === "article-journal", "journal article type");
  ok(p.title === "Teaching with generative AI", "title");
  ok(p.author.length === 2 && p.author[0].family === "Chen" && p.author[1].family === "Patel" && p.author[1].given === "Maya", "authors in both name orders");
  ok(JSON.stringify(p.issued) === JSON.stringify({ "date-parts": [[2024, 3, 5]] }), "full date");
  ok(p.page === "115-132" && p.volume === "12" && p.issue === "2", "volume, issue, pages");
  ok(p.DOI === "10.5555/jtp.2024.12.2.115", "DOI");
  ok(c.primary.pdf_url === "https://example.org/articles/jtp-2024-115.pdf", "PDF link from metadata");
  ok(c.others.length === 2 && c.others.every((o) => o.identifiers.doi), "reference-list DOIs offered separately");
} else {
  ok(c.primary === null, "results page has no single primary paper");
  const coins = c.others.find((o) => o.key.startsWith("coins"));
  ok(coins && coins.csl.author.length === 2 && coins.csl.DOI === "10.5555/tt.2020.5" && coins.csl.page === "5-9", "embedded citation span parsed");
  ok(c.others.some((o) => o.identifiers.arxiv === "2401.01234"), "arXiv link (version suffix removed)");
  ok(c.others.some((o) => o.identifiers.pmid === "31234567"), "PubMed link");
  ok(c.others.filter((o) => (o.identifiers.doi || "").toLowerCase() === "10.5555/tt.2020.5").length === 1, "same paper not listed twice");
  ok(c.others.length === 3, "three distinct papers");
}
ok(JSON.stringify(nameOf("van der Berg, Johanna")) === JSON.stringify({ family: "van der Berg", given: "Johanna" }), "inverted name with particle");
ok(dateOf("Spring 2019").issued === undefined && dateOf("2019-13-40")["date-parts"][0].length === 1, "invalid month ignored");
ok(fromCoins("rft.genre=article") === null, "span without a title ignored");

const failed = checks.filter(([p]) => !p);
document.getElementById("out").textContent = `${failed.length ? "FAIL" : "PASS"} ${checks.length - failed.length}/${checks.length}\n` + checks.map(([p, l]) => `${p ? "ok  " : "FAIL"} ${l}`).join("\n");
document.title = failed.length ? "FAIL" : "PASS";
