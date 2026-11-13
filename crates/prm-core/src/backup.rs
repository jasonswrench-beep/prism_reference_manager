//! Verified backups, restore with checks, and the complete open export package.
//!
//! A backup is a zip holding a consistent database snapshot (taken with VACUUM INTO), the
//! managed files when requested, and a manifest with checksums, entity counts and the event
//! journal watermark. Restore verifies everything into a new folder before it can replace the
//! live library. The export package holds canonical JSON for every entity, notes as Markdown,
//! files and a checksum manifest; importing it into an empty library reproduces the data.

use crate::error::{Error, Result};
use crate::library::Library;
use crate::store::{Store, TABLES};
use crate::util::{canonical_json, now, sha256_file, sha256_hex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub library_id: String,
    pub library_name: String,
    pub schema: i64,
    pub created: String,
    pub producer: String,
    pub journal_watermark: i64,
    pub counts: BTreeMap<String, i64>,
    /// path in archive -> sha256
    pub checksums: BTreeMap<String, String>,
    pub includes_files: bool,
    pub attachments_expected: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub ok: bool,
    pub manifest: Manifest,
    pub checks: Vec<(String, bool, String)>,
}

fn zopt() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true)
}

/// Create a backup zip at `dest`.
pub fn backup(lib: &Library, dest: &Path, include_files: bool) -> Result<Manifest> {
    let tmpdir = std::env::temp_dir().join(format!("prm-backup-{}", crate::util::new_id()));
    std::fs::create_dir_all(&tmpdir)?;
    let snap = tmpdir.join("library.db");
    lib.store.conn.execute("VACUUM INTO ?1", [snap.to_string_lossy()])?;
    let watermark: i64 = lib.store.conn.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?;
    let mut counts = BTreeMap::new();
    for t in TABLES {
        counts.insert(t.to_string(), lib.store.count(t)?);
    }
    let mut m = Manifest {
        format: "prism-reference-manager-backup".into(),
        version: 1,
        library_id: lib.store.meta("library_id").unwrap_or_default(),
        library_name: lib.store.meta("name").unwrap_or_default(),
        schema: crate::store::SCHEMA_VERSION,
        created: now(),
        producer: format!("Prism Reference Manager {}", env!("CARGO_PKG_VERSION")),
        journal_watermark: watermark,
        counts,
        checksums: BTreeMap::new(),
        includes_files: include_files,
        attachments_expected: lib.store.count("attachments")? as usize,
    };
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    let part = dest.with_extension("zip.part");
    {
        let f = std::fs::File::create(&part)?;
        let mut z = zip::ZipWriter::new(f);
        m.checksums.insert("library.db".into(), sha256_file(&snap)?);
        z.start_file("library.db", zopt())?;
        std::io::copy(&mut std::fs::File::open(&snap)?, &mut z)?;
        if include_files {
            let files = lib.files_dir();
            if files.exists() {
                for entry in walk(&files)? {
                    let rel = entry.strip_prefix(&files).unwrap().to_string_lossy().replace('\\', "/");
                    let name = format!("files/{rel}");
                    m.checksums.insert(name.clone(), sha256_file(&entry)?);
                    z.start_file(name, zopt())?;
                    std::io::copy(&mut std::fs::File::open(&entry)?, &mut z)?;
                }
            }
        }
        z.start_file("manifest.json", zopt())?;
        z.write_all(serde_json::to_string_pretty(&m)?.as_bytes())?;
        z.finish()?;
    }
    std::fs::rename(&part, dest)?;
    let _ = std::fs::remove_dir_all(&tmpdir);
    Ok(m)
}

fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read_manifest(z: &mut zip::ZipArchive<std::fs::File>) -> Result<Manifest> {
    let mut s = String::new();
    z.by_name("manifest.json").map_err(|_| Error::user("This file has no manifest; it is not a Prism backup."))?.read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

/// Verify a backup without changing anything: checksums, schema, entity counts, links and
/// attachment presence.
pub fn verify(path: &Path) -> Result<VerifyReport> {
    let f = std::fs::File::open(path)?;
    let mut z = zip::ZipArchive::new(f)?;
    let m = read_manifest(&mut z)?;
    let mut checks = Vec::new();
    checks.push(("Backup format".into(), m.format == "prism-reference-manager-backup", m.format.clone()));
    checks.push(("Schema supported".into(), m.schema <= crate::store::SCHEMA_VERSION, format!("schema {}", m.schema)));
    let mut all_ok = true;
    let mut bad = Vec::new();
    for (name, sum) in &m.checksums {
        let mut h = sha2::Sha256::new();
        use sha2::Digest;
        match z.by_name(name) {
            Ok(mut e) => {
                let mut buf = vec![0u8; 1 << 16];
                loop {
                    let n = e.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
                if hex::encode(h.finalize()) != *sum {
                    all_ok = false;
                    bad.push(name.clone());
                }
            }
            Err(_) => {
                all_ok = false;
                bad.push(format!("{name} (missing)"));
            }
        }
    }
    checks.push(("Checksums".into(), all_ok, if bad.is_empty() { format!("{} files verified", m.checksums.len()) } else { format!("Mismatch: {}", bad.join(", ")) }));
    // Open the database copy and compare counts and links.
    let tmpdir = std::env::temp_dir().join(format!("prm-verify-{}", crate::util::new_id()));
    std::fs::create_dir_all(&tmpdir)?;
    {
        let mut e = z.by_name("library.db")?;
        let mut out = std::fs::File::create(tmpdir.join("library.db"))?;
        std::io::copy(&mut e, &mut out)?;
    }
    let conn = rusqlite::Connection::open(tmpdir.join("library.db"))?;
    let mut counts_ok = true;
    let mut detail = Vec::new();
    for (t, n) in &m.counts {
        let c: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap_or(-1);
        if c != *n {
            counts_ok = false;
            detail.push(format!("{t}: expected {n}, found {c}"));
        }
    }
    checks.push(("Entity counts".into(), counts_ok, if detail.is_empty() { "All entity counts match".into() } else { detail.join("; ") }));
    let orphans: i64 = conn
        .query_row("SELECT COUNT(*) FROM project_sources ps WHERE NOT EXISTS (SELECT 1 FROM refs r WHERE r.id=json_extract(ps.data,'$.ref_id'))", [], |r| r.get(0))
        .unwrap_or(0);
    checks.push(("Project links".into(), orphans == 0, format!("{orphans} memberships point to missing sources")));
    let anchor_orphans: i64 = conn
        .query_row("SELECT COUNT(*) FROM annotations a WHERE NOT EXISTS (SELECT 1 FROM attachments t WHERE t.id=json_extract(a.data,'$.attachment_id'))", [], |r| r.get(0))
        .unwrap_or(0);
    checks.push(("Annotation anchors".into(), anchor_orphans == 0, format!("{anchor_orphans} annotations point to missing file versions")));
    if m.includes_files {
        let managed: Vec<String> = {
            let mut st = conn.prepare("SELECT json_extract(data,'$.path') FROM attachments WHERE json_extract(data,'$.mode')='managed'")?;
            let v: Vec<String> = st.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
            v
        };
        let missing = managed.iter().filter(|p| !m.checksums.contains_key(&format!("files/{p}"))).count();
        checks.push(("Attachments present".into(), missing == 0, format!("{} managed files, {missing} missing", managed.len())));
    } else {
        checks.push(("Attachments present".into(), true, "This backup holds metadata only; files are not included.".into()));
    }
    drop(conn);
    let _ = std::fs::remove_dir_all(&tmpdir);
    let ok = checks.iter().all(|c| c.1);
    Ok(VerifyReport { ok, manifest: m, checks })
}

/// Restore a verified backup into a new, empty folder. The live library is never overwritten.
pub fn restore(path: &Path, target: &Path) -> Result<VerifyReport> {
    let report = verify(path)?;
    if !report.ok {
        return Err(Error::user("The backup did not pass verification, so nothing was restored. See the verification report."));
    }
    if target.exists() && std::fs::read_dir(target)?.next().is_some() {
        return Err(Error::user("Choose an empty folder to restore into; the current library is never overwritten."));
    }
    std::fs::create_dir_all(target)?;
    let f = std::fs::File::open(path)?;
    let mut z = zip::ZipArchive::new(f)?;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        let Some(name) = e.enclosed_name() else { continue };
        if name.to_string_lossy() == "manifest.json" {
            continue;
        }
        let out = target.join(name);
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut e, &mut w)?;
    }
    // Rebuild caches and the mirror from the restored store.
    let s = Store::open(target, "restore")?;
    crate::search::rebuild(&s.conn)?;
    s.rebuild_mirror()?;
    let log = format!("Restored from {} on {}\n", path.display(), now());
    std::fs::write(target.join("restore-log.txt"), log)?;
    Ok(report)
}

/// Keep at most `keep` backups in `dir` whose names start with `prefix`.
pub fn prune(dir: &Path, prefix: &str, keep: usize) -> Result<usize> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with(prefix)).unwrap_or(false) && p.extension().map(|e| e == "zip").unwrap_or(false)).collect();
    v.sort();
    let mut removed = 0;
    while v.len() > keep {
        let p = v.remove(0);
        std::fs::remove_file(p)?;
        removed += 1;
    }
    Ok(removed)
}

// ---- complete export package ------------------------------------------------------------------

/// Export everything as an open package: canonical JSON per entity, notes as Markdown, style
/// dependencies, attachments and a checksum manifest.
pub fn export_package(lib: &Library, dest: &Path, include_files: bool) -> Result<Manifest> {
    let mut m = Manifest {
        format: "prism-reference-manager-package".into(),
        version: 1,
        library_id: lib.store.meta("library_id").unwrap_or_default(),
        library_name: lib.store.meta("name").unwrap_or_default(),
        schema: crate::store::SCHEMA_VERSION,
        created: now(),
        producer: format!("Prism Reference Manager {}", env!("CARGO_PKG_VERSION")),
        journal_watermark: lib.store.conn.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))?,
        counts: BTreeMap::new(),
        checksums: BTreeMap::new(),
        includes_files: include_files,
        attachments_expected: lib.store.count("attachments")? as usize,
    };
    let part = dest.with_extension("zip.part");
    {
        let f = std::fs::File::create(&part)?;
        let mut z = zip::ZipWriter::new(f);
        let mut put = |z: &mut zip::ZipWriter<std::fs::File>, m: &mut Manifest, name: &str, bytes: &[u8]| -> Result<()> {
            m.checksums.insert(name.to_string(), sha256_hex(bytes));
            z.start_file(name, zopt())?;
            z.write_all(bytes)?;
            Ok(())
        };
        let dump = lib.store.canonical_dump()?;
        for (t, rows) in &dump {
            m.counts.insert(t.clone(), rows.len() as i64);
            let arr: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::from_str(r).unwrap_or_default()).collect();
            put(&mut z, &mut m, &format!("data/{t}.json"), serde_json::to_string_pretty(&arr)?.as_bytes())?;
        }
        // Notes as Markdown for reading without the program.
        let notes: Vec<crate::model::Note> = lib.store.all()?;
        for n in &notes {
            let md = format!("---\nid: {}\nkind: {}\nproject: {}\nsources: {:?}\nlocator: {} {}\norigin: {}\n---\n\n{}{}\n", n.id, n.kind, n.project_id, n.ref_ids, n.locator_label, n.locator, n.origin, if n.exact_text.is_empty() { String::new() } else { format!("> {}\n\n", n.exact_text) }, n.body);
            put(&mut z, &mut m, &format!("notes/{}.md", n.id), md.as_bytes())?;
        }
        let refs: Vec<crate::model::Reference> = lib.store.all()?;
        let rr: Vec<&crate::model::Reference> = refs.iter().collect();
        put(&mut z, &mut m, "references.csl.json", crate::formats::csljson::export(&rr).as_bytes())?;
        put(&mut z, &mut m, "references.bib", crate::formats::bibtex::export(&rr, true, &[]).as_bytes())?;
        put(&mut z, &mut m, "styles/apa-7.json", serde_json::to_string_pretty(&crate::render::apa::coverage())?.as_bytes())?;
        if include_files {
            for a in lib.store.all::<crate::model::Attachment>()? {
                let p = lib.attachment_path(&a);
                if let Ok(bytes) = std::fs::read(&p) {
                    let name = if a.mode == "managed" { format!("files/{}", a.path) } else { format!("linked/{}/{}", &a.hash[..16], a.original_name) };
                    if !m.checksums.contains_key(&name) {
                        put(&mut z, &mut m, &name, &bytes)?;
                    }
                }
            }
        }
        z.start_file("manifest.json", zopt())?;
        z.write_all(serde_json::to_string_pretty(&m)?.as_bytes())?;
        z.finish()?;
    }
    std::fs::rename(&part, dest)?;
    Ok(m)
}

/// Import a package into an empty library at `target`.
pub fn import_package(path: &Path, target: &Path) -> Result<Manifest> {
    if target.join(crate::store::DB_FILE).exists() {
        return Err(Error::user("Choose an empty folder; packages are imported into a new library."));
    }
    let f = std::fs::File::open(path)?;
    let mut z = zip::ZipArchive::new(f)?;
    let mut s = String::new();
    z.by_name("manifest.json").map_err(|_| Error::user("This file has no manifest; it is not a Prism package."))?.read_to_string(&mut s)?;
    let m: Manifest = serde_json::from_str(&s)?;
    if m.format != "prism-reference-manager-package" {
        return Err(Error::user("This is not a Prism export package."));
    }
    for (name, sum) in &m.checksums {
        let mut buf = Vec::new();
        z.by_name(name)?.read_to_end(&mut buf)?;
        if sha256_hex(&buf) != *sum {
            return Err(Error::user(format!("The package is damaged: {name} does not match its checksum. Nothing was imported.")));
        }
    }
    let mut store = Store::open(target, "import")?;
    if let Some(name) = Some(m.library_name.clone()).filter(|n| !n.is_empty()) {
        store.set_meta("name", &name)?;
    }
    let mut tables: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    for t in TABLES {
        let name = format!("data/{t}.json");
        if let Ok(mut e) = z.by_name(&name) {
            let mut s = String::new();
            e.read_to_string(&mut s)?;
            tables.insert(t.to_string(), serde_json::from_str(&s)?);
        }
    }
    store.write("Import package", |tx| {
        for (t, rows) in &tables {
            for r in rows {
                let id = r.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                tx.put_raw(t, &id, Some(&canonical_json(r)))?;
            }
        }
        Ok(())
    })?;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        let Some(name) = e.enclosed_name() else { continue };
        let ns = name.to_string_lossy().replace('\\', "/");
        if let Some(rel) = ns.strip_prefix("files/") {
            let out = target.join("files").join(rel);
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut w = std::fs::File::create(&out)?;
            std::io::copy(&mut e, &mut w)?;
        }
    }
    store.rebuild_mirror()?;
    Ok(m)
}
