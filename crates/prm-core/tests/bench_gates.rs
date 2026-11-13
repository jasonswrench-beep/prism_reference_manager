//! Release-gate measurements (spec PRF-002, -005, -006, -007, -008, -013, -015). Ignored by
//! default; run on the reference machine with
//! `cargo test -p prm-core --release --test bench_gates -- --ignored --nocapture --test-threads 1`.
//! Each line starting with GATE gives the measurement and the spec target.
//! Sizes can be reduced for a quick run: PRM_BENCH_N, PRM_BENCH_PDFS, PRM_BENCH_PAGES,
//! PRM_BENCH_MIGRATE.

use prm_core::library::{Library, ListQuery};
use prm_core::model::*;
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn env(k: &str, d: usize) -> usize {
    std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d)
}

fn dir(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("prm-gates-{name}-{}", prm_core::util::new_id()));
    std::fs::create_dir_all(&p).unwrap();
    p.join("lib")
}

/// A vocabulary large enough that full-text search is not trivially selective.
fn vocabulary(rng: &mut Rng) -> Vec<String> {
    let syll = ["ka", "ri", "to", "men", "sa", "lo", "vin", "der", "pa", "qu", "el", "tion", "ra", "go", "ne", "sti", "ber", "mo", "lu", "fa"];
    let mut v: Vec<String> = ["teaching", "learning", "faculty", "reflection", "assessment", "generative", "communication", "pedagogy", "practice", "students", "evidence", "design"].iter().map(|s| s.to_string()).collect();
    while v.len() < 3000 {
        let n = 2 + rng.below(3);
        v.push((0..n).map(|_| syll[rng.below(syll.len())]).collect());
    }
    v
}

/// Messy, realistic-looking records: missing years, group authors, accents, near duplicates.
fn records(n: usize, rng: &mut Rng, vocab: &[String]) -> Vec<Reference> {
    let surnames = ["Chen", "Patel", "Garcia", "Kim", "Baker", "Müller", "Øberg", "Nguyen", "Rossi", "Smith", "O'Neil", "van der Berg", "Dubois", "Kowalski", "Okafor"];
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut r = Reference { id: prm_core::util::new_id(), rtype: if i % 9 == 0 { "book".into() } else { "article-journal".into() }, revision: 1, ..Default::default() };
        let words: Vec<&str> = (0..(5 + rng.below(8))).map(|_| vocab[rng.below(vocab.len())].as_str()).collect();
        r.fields.insert("title".into(), format!("{} {}", words.join(" "), i));
        r.fields.insert("container_title".into(), format!("Journal of {}", vocab[rng.below(400)]));
        if i % 13 == 0 {
            r.creators.push(Creator { role: "author".into(), name: Name::org("Center for Teaching Excellence") });
        } else {
            for _ in 0..(1 + rng.below(4)) {
                r.creators.push(Creator { role: "author".into(), name: Name::person(surnames[rng.below(surnames.len())], "A.") });
            }
        }
        if i % 17 != 0 {
            r.issued = Some(PartialDate::year(1980 + rng.below(45) as i32));
        }
        if i % 3 == 0 {
            r.fields.insert("doi".into(), format!("10.5555/gate.{i}"));
        }
        r.citekey = format!("k{i}");
        out.push(r);
    }
    out
}

fn fill(l: &mut Library, recs: &[Reference]) {
    for chunk in recs.chunks(10_000) {
        l.write("gate insert", |tx| {
            for r in chunk {
                tx.put(r)?;
            }
            Ok(())
        })
        .unwrap();
    }
}

fn p95(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[v.len() / 2], v[((v.len() as f64) * 0.95).ceil() as usize - 1])
}

#[test]
#[ignore]
fn gates_library_100k() {
    let n = env("PRM_BENCH_N", 100_000);
    let mut rng = Rng(7);
    let vocab = vocabulary(&mut rng);
    let path = dir("lib");
    let mut l = Library::open(&path, "bench", "bench").unwrap();
    let t = Instant::now();
    fill(&mut l, &records(n, &mut rng, &vocab));
    println!("SETUP inserted {n} records in {:.1}s", t.elapsed().as_secs_f64());
    l.store.advance_mirror().unwrap();

    // PRF-015: one ordinary edit, including its write to the text mirror.
    let ids: Vec<String> = l.list_references(&ListQuery { limit: 20, ..Default::default() }).unwrap().rows.into_iter().map(|r| r.id).collect();
    let mut times = vec![];
    for id in &ids {
        let mut r = l.store.must::<Reference>(id).unwrap();
        r.fields.insert("note".into(), format!("edited {}", rng.next()));
        let t = Instant::now();
        l.update_reference(r, "user").unwrap();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let (m, p) = p95(times);
    println!("GATE PRF-015 single edit incl. mirror write: median {m:.1} ms, p95 {p:.1} ms (target <= 100 ms)");

    // PRF-007: importing 1,000 references with the duplicate check.
    let mut ris = String::new();
    for (i, r) in records(1000, &mut Rng(99), &vocab).iter().enumerate() {
        ris.push_str(&format!("TY  - JOUR\nID  - imp{i}\nTI  - {}\nAU  - {}\nPY  - {}\nT2  - {}\nER  - \n", r.title(), r.creators.first().map(|c| c.name.surname()).unwrap_or_default(), r.year().unwrap_or(2000), r.f("container_title")));
    }
    let t = Instant::now();
    let b = l.stage_import("gate.ris", &ris, "ris", "", "", "", None).unwrap();
    let staged = t.elapsed().as_secs_f64();
    let t2 = Instant::now();
    l.commit_import(&b.id).unwrap();
    let committed = t2.elapsed().as_secs_f64();
    println!("GATE PRF-007 import 1,000 into {n}: review with duplicate check {staged:.1}s + commit {committed:.1}s = {:.1}s (target <= 30 s)", staged + committed);

    // PRF-013: the health report (main checks), and the full report with the duplicate scan.
    let t = Instant::now();
    let h = prm_core::health::run_with(&l, false).unwrap();
    println!("GATE PRF-013 health report main checks: {:.1}s, {} items (target <= 60 s)", t.elapsed().as_secs_f64(), h.items.len());
    let t = Instant::now();
    let _ = prm_core::health::run(&l).unwrap();
    println!("INFO health report including duplicate scan: {:.1}s", t.elapsed().as_secs_f64());

    // PRF-002: opening a different library of this size.
    l.close();
    drop(l);
    let mut times = vec![];
    for _ in 0..3 {
        let t = Instant::now();
        let l2 = Library::open(&path, "bench", "bench").unwrap();
        let _ = l2.list_references(&ListQuery { limit: 400, ..Default::default() }).unwrap();
        times.push(t.elapsed().as_secs_f64());
        l2.close();
    }
    let (m, p) = p95(times);
    println!("GATE PRF-002 open a {n}-record library and show the first page: median {m:.2}s, worst {p:.2}s (target <= 3 s)");
}

#[test]
#[ignore]
fn gates_fulltext_20k_pdfs() {
    let pdfs = env("PRM_BENCH_PDFS", 20_000);
    let pages = env("PRM_BENCH_PAGES", 8);
    let mut rng = Rng(11);
    let vocab = vocabulary(&mut rng);
    let mut l = Library::open(&dir("ft"), "bench", "bench").unwrap();
    let recs = records(pdfs, &mut rng, &vocab);
    fill(&mut l, &recs);
    // Each PDF is attached to its source, so full-text hits lead back to sources.
    for chunk in recs.chunks(10_000) {
        l.write("gate attachments", |tx| {
            for r in chunk {
                tx.put(&Attachment { id: format!("att-{}", r.id), ref_id: r.id.clone(), hash: r.id.clone(), path: format!("x/{}.pdf", r.id), mode: "managed".into(), mime: "application/pdf".into(), role: "full-text".into(), original_name: "paper.pdf".into(), indexed: true, ..Default::default() })?;
            }
            Ok(())
        })
        .unwrap();
    }
    // PRF-006: core indexing rate for extracted page text (text extraction itself runs in the
    // window with pdf.js and is measured separately on real files).
    let t = Instant::now();
    let mut total_pages = 0usize;
    for chunk in recs.chunks(500) {
        l.store.conn.execute_batch("BEGIN").unwrap();
        for r in chunk {
            let ps: Vec<String> = (0..pages).map(|_| (0..300).map(|_| vocab[rng.below(vocab.len())].as_str()).collect::<Vec<_>>().join(" ")).collect();
            prm_core::search::index_pages(&l.store.conn, &format!("att-{}", r.id), &ps).unwrap();
            total_pages += ps.len();
        }
        l.store.conn.execute_batch("COMMIT").unwrap();
    }
    let secs = t.elapsed().as_secs_f64();
    println!("GATE PRF-006 index {total_pages} pages (300 words each) from {pdfs} PDFs: {:.0} pages/minute, {secs:.0}s (target >= 2,000 pages/minute; pdf.js extraction not included)", total_pages as f64 / secs * 60.0);
    // PRF-005: full-text search, time to results.
    let mut times = vec![];
    for q in [vocab[20].clone(), format!("{} {}", vocab[100], vocab[200]), format!("fulltext:{}", vocab[300]), format!("\"{} {}\"", vocab[5], vocab[6])] {
        for _ in 0..3 {
            let t = Instant::now();
            let page = l.list_references(&ListQuery { search: q.clone(), sort: "relevance".into(), limit: 50, ..Default::default() }).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            assert!(page.total > 0, "full-text search should find sources for {q}");
        }
        let last = &times[times.len() - 3..];
        println!("INFO query {q:?}: {:.0} / {:.0} / {:.0} ms", last[0], last[1], last[2]);
    }
    let (m, p) = p95(times);
    println!("GATE PRF-005 full-text search over {pdfs} PDFs: median {m:.0} ms, p95 {p:.0} ms to the first page of results (target <= 1,000 ms)");
}

#[test]
#[ignore]
fn gates_migration_50k() {
    let n = env("PRM_BENCH_MIGRATE", 50_000);
    let mut rng = Rng(5);
    let vocab = vocabulary(&mut rng);
    let mut ris = String::with_capacity(n * 160);
    for (i, r) in records(n, &mut rng, &vocab).iter().enumerate() {
        ris.push_str(&format!("TY  - JOUR\nID  - m{i}\nTI  - {}\n", r.title()));
        for c in &r.creators {
            ris.push_str(&format!("AU  - {}\n", if c.name.literal.is_empty() { format!("{}, {}", c.name.surname(), c.name.given) } else { c.name.literal.clone() }));
        }
        if let Some(y) = r.year() {
            ris.push_str(&format!("PY  - {y}\n"));
        }
        ris.push_str(&format!("T2  - {}\n{}ER  - \n", r.f("container_title"), if r.has("doi") { format!("DO  - {}\n", r.f("doi")) } else { String::new() }));
    }
    let mut l = Library::open(&dir("mig"), "bench", "bench").unwrap();
    let t = Instant::now();
    let b = l.stage_import("migration.ris", &ris, "ris", "", "", "", None).unwrap();
    let staged = t.elapsed().as_secs_f64();
    let t2 = Instant::now();
    let rep = l.commit_import(&b.id).unwrap();
    let committed = t2.elapsed().as_secs_f64();
    println!("GATE PRF-008 migrate {n} records ({:.1} MB file): review {staged:.0}s + commit {committed:.0}s = {:.1} min, {} added (target <= 30 min; stoppable, review resumable)", ris.len() as f64 / 1e6, (staged + committed) / 60.0, rep.added);
}
