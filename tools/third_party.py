"""Generate THIRD-PARTY.md and sbom.json from the shipped dependency graph, and check licenses.

Run from the repository root:  python tools/third_party.py
Exits with status 1 if a dependency's license is not on the allow-list, so a build stops
instead of shipping an unreviewed license.
"""
import json, subprocess, sys, os, re, datetime

ALLOWED = {"MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016",
           "MPL-2.0", "CC0-1.0", "0BSD", "BSL-1.0", "Unlicense", "Apache-2.0 WITH LLVM-exception", "OFL-1.1", "CDLA-Permissive-2.0", "NCSA", "bzip2-1.0.6"}

def license_ok(expr):
    if not expr:
        return False
    # Accept an OR-expression if any branch is allowed; an AND-expression if all are.
    for alt in re.split(r"\s+OR\s+|/", expr.replace("(", "").replace(")", "")):
        parts = [p.strip() for p in re.split(r"\s+AND\s+", alt)]
        if all(p in ALLOWED for p in parts):
            return True
    return False

meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"], text=True, encoding="utf-8"))
pkgs = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
root = next(p["id"] for p in meta["packages"] if p["name"] == "prism-reference-manager")
seen, stack = set(), [root]
while stack:
    i = stack.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        kinds = [k["kind"] for k in d["dep_kinds"]]
        target_ok = any((k.get("target") or "") in ("", "cfg(windows)", "cfg(target_os = \"windows\")") or "windows" in (k.get("target") or "") for k in d["dep_kinds"])
        if any(k is None for k in kinds) and target_ok:
            stack.append(d["pkg"])

shipped = sorted((pkgs[i] for i in seen if not pkgs[i]["name"].startswith("prm-") and pkgs[i]["name"] != "prism-reference-manager"), key=lambda p: p["name"])
web = [
    {"name": "pdfjs-dist", "version": json.load(open("app/node_modules/pdfjs-dist/package.json"))["version"] if os.path.exists("app/node_modules/pdfjs-dist/package.json") else "4.10.38", "license": "Apache-2.0", "repository": "https://github.com/mozilla/pdf.js", "note": "PDF rendering in the reader"},
    {"name": "pdf.js character maps (cmaps)", "version": "bundled with pdfjs-dist", "license": "BSD-3-Clause", "repository": "https://github.com/adobe-type-tools/cmap-resources", "note": "CJK text rendering"},
    {"name": "pdf.js standard fonts (Foxit and Liberation)", "version": "bundled with pdfjs-dist", "license": "BSD-3-Clause AND OFL-1.1", "repository": "https://github.com/mozilla/pdf.js", "note": "Fallback fonts for PDFs without embedded fonts"},
]

bad = [p for p in shipped if not license_ok(p.get("license"))] + [w for w in web if not license_ok(w["license"])]

lines = ["# Third-party software", "", "Prism Reference Manager includes the following third-party components. Each is used under its own license; full license texts are available from each project's repository.", "",
         f"Generated {datetime.date.today().isoformat()} from the shipped dependency graph.", "", "## Rust components", "", "| Component | Version | License | Source |", "|---|---|---|---|"]
for p in shipped:
    lines.append(f"| {p['name']} | {p['version']} | {p.get('license') or 'see source'} | {p.get('repository') or ''} |")
lines += ["", "## Interface components", "", "| Component | Version | License | Source | Use |", "|---|---|---|---|---|"]
for w in web:
    lines.append(f"| {w['name']} | {w['version']} | {w['license']} | {w['repository']} | {w['note']} |")
lines += ["", "The Windows WebView2 runtime that displays the interface is part of Windows and is not distributed with this program.", ""]
open("THIRD-PARTY.md", "w", encoding="utf-8", newline="\n").write("\n".join(lines))

sbom = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
        "metadata": {"timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(), "component": {"type": "application", "name": "Prism Reference Manager", "version": next(p["version"] for p in meta["packages"] if p["name"] == "prism-reference-manager")}},
        "components": [{"type": "library", "name": p["name"], "version": p["version"], "licenses": [{"expression": p.get("license") or "NOASSERTION"}], "purl": f"pkg:cargo/{p['name']}@{p['version']}"} for p in shipped]
        + [{"type": "library", "name": w["name"], "version": w["version"], "licenses": [{"expression": w["license"]}]} for w in web]}
json.dump(sbom, open("sbom.json", "w", encoding="utf-8"), indent=1)
print(f"{len(shipped)} Rust components, {len(web)} interface components")
if bad:
    print("LICENSES NEEDING REVIEW:")
    for p in bad:
        print("  ", p["name"], p.get("version"), p.get("license"))
    sys.exit(1)
print("All licenses are on the allow-list.")
