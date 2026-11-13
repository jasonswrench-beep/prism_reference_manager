//! Parser robustness (malformed and random input never panics), term merging, and a scale
//! benchmark (ignored by default; run with `cargo test --release -- --ignored --nocapture`).

use prm_core::library::{Library, ListQuery};
use prm_core::model::*;

/// Small deterministic generator so failures are reproducible.
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

const SEEDS: &[&str] = &[
    "TY  - JOUR\nAU  - Chen, Li\nTI  - Title\nPY  - 2024///\nSP  - 1\nEP  - 2\nER  - \n",
    "@article{k, author = {M{\\\"u}ller, J. and {Org}}, title = {A {B} c}, year = 2020, pages = {1--2}}\n@string{x = \"y\"}",
    "[{\"type\":\"article-journal\",\"title\":\"T\",\"author\":[{\"family\":\"A\",\"given\":\"B\"}],\"issued\":{\"date-parts\":[[2020,1]]}}]",
    "<?xml version=\"1.0\"?><xml><records><record><ref-type name=\"Journal Article\">17</ref-type><titles><title><style>T</style></title></titles></record></records></xml>",
    "Title,Authors,Year\n\"A, B\",\"Chen, L; Patel, M\",2020\n",
    "Chen, L., & Patel, M. (2024). Teaching with AI. Journal of Teaching, 12(2), 115-132. https://doi.org/10.1/x",
];

#[test]
fn parsers_survive_mutations_and_random_bytes() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for round in 0..3000 {
        let seed = SEEDS[round % SEEDS.len()];
        let mut bytes = seed.as_bytes().to_vec();
        for _ in 0..(1 + rng.below(12)) {
            let i = rng.below(bytes.len().max(1));
            match rng.below(4) {
                0 if !bytes.is_empty() => {
                    bytes.remove(i.min(bytes.len() - 1));
                }
                1 => bytes.insert(i.min(bytes.len()), (rng.next() & 0xff) as u8),
                2 if !bytes.is_empty() => {
                    let j = i.min(bytes.len() - 1);
                    bytes[j] = b"{}[]\"\\,;\n<>/@=#-"[rng.below(16)];
                }
                _ => {
                    let j = i.min(bytes.len());
                    let cut = bytes[j..].to_vec();
                    bytes.truncate(j);
                    bytes.extend(cut.iter().take(rng.below(cut.len() + 1)));
                }
            }
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        for fmt in ["ris", "bibtex", "csljson", "xml", "csv", "paste"] {
            let _ = prm_core::formats::parse(fmt, &text, None);
        }
        let _ = prm_core::search::parse(&text);
    }
    // Pure random input.
    for _ in 0..500 {
        let n = rng.below(400);
        let bytes: Vec<u8> = (0..n).map(|_| (rng.next() & 0xff) as u8).collect();
        let text = String::from_utf8_lossy(&bytes).to_string();
        for fmt in ["ris", "bibtex", "csljson", "xml", "csv", "paste"] {
            let _ = prm_core::formats::parse(fmt, &text, None);
        }
    }
}

#[test]
fn malformed_pdf_is_reported_not_fatal() {
    let dir = std::env::temp_dir().join(format!("prm-bad-pdf-{}", prm_core::util::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("bad.pdf");
    std::fs::write(&p, b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF").unwrap();
    let info = prm_core::pdf::inspect(&p);
    assert!(matches!(info.state.as_str(), "malformed" | "ok" | "image-only"), "{}", info.state);
    let empty = dir.join("empty.pdf");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(prm_core::pdf::inspect(&empty).state, "placeholder");
}

fn lib(name: &str) -> Library {
    let root = std::env::temp_dir().join(format!("prm-rob-{name}-{}", prm_core::util::new_id()));
    Library::open(&root, "dev", "host").unwrap()
}

#[test]
fn term_variants_merge_in_one_step() {
    let mut l = lib("terms");
    for (i, j) in ["Journal of Teaching Practice", "Journal of Teaching  Practice", "journal of teaching practice"].iter().enumerate() {
        let mut r = Reference { rtype: "article-journal".into(), ..Default::default() };
        r.fields.insert("title".into(), format!("Paper {i}"));
        r.fields.insert("container_title".into(), j.to_string());
        l.add_reference(r, None).unwrap();
    }
    let groups = l.term_variants("journal").unwrap();
    assert_eq!(groups.len(), 1);
    let from: Vec<String> = groups[0].iter().map(|t| t.value.clone()).collect();
    let n = l.merge_terms("journal", &from, "Journal of Teaching Practice").unwrap();
    assert_eq!(n, 2);
    assert_eq!(l.terms("journal").unwrap().len(), 1);
    assert!(l.root.join("backups").join("before-changes").read_dir().unwrap().count() >= 1, "a safety snapshot precedes the merge");
    l.undo().unwrap();
    assert_eq!(l.terms("journal").unwrap().len(), 3);
}

#[test]
#[ignore]
fn benchmark_100k_references() {
    let mut l = lib("bench");
    let n: usize = std::env::var("PRM_BENCH_N").ok().and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let words = ["teaching", "learning", "faculty", "reflection", "assessment", "generative", "communication", "pedagogy", "practice", "students", "écoles", "Müller", "研究", "evidence", "design"];
    let surnames = ["Chen", "Patel", "Garcia", "Kim", "Baker", "Müller", "Øberg", "Nguyen", "Rossi", "Smith", "O'Neil", "van der Berg"];
    let mut rng = Rng(42);
    let t0 = std::time::Instant::now();
    let mut batch = Vec::new();
    for i in 0..n {
        let mut r = Reference { id: prm_core::util::new_id(), rtype: "article-journal".into(), revision: 1, ..Default::default() };
        let title: Vec<&str> = (0..6).map(|_| words[rng.below(words.len())]).collect();
        r.fields.insert("title".into(), format!("{} {}", title.join(" "), i));
        r.fields.insert("container_title".into(), format!("Journal {}", rng.below(300)));
        r.creators.push(Creator { role: "author".into(), name: Name::person(surnames[rng.below(surnames.len())], "A.") });
        r.issued = Some(PartialDate::year(1990 + rng.below(35) as i32));
        r.citekey = format!("k{i}");
        batch.push(r);
    }
    for chunk in batch.chunks(10_000) {
        l.write("bench insert", |tx| {
            for r in chunk {
                tx.put(r)?;
            }
            Ok(())
        })
        .unwrap();
    }
    let insert = t0.elapsed();
    println!("BENCH insert done {:.1}s", insert.as_secs_f64());
    let tm = std::time::Instant::now();
    l.store.advance_mirror().unwrap();
    println!("BENCH mirror catch-up (background in the program) {:.1}s", tm.elapsed().as_secs_f64());
    let time = |f: &mut dyn FnMut()| {
        let mut v = Vec::new();
        for _ in 0..10 {
            let t = std::time::Instant::now();
            f();
            v.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (v[v.len() / 2], v[(v.len() * 95) / 100 - 1])
    };
    let mut queries = ["reflection", "author:muller", "faculty learning -assessment", "\"generative pedagogy\"", "year:2010..2015 practice"].iter().cycle();
    let (m1, p1) = time(&mut || {
        let q = queries.next().unwrap();
        let _ = prm_core::search::run(&l.store.conn, q, None, None, 500).unwrap();
    });
    println!("BENCH search done median={m1:.1}ms p95={p1:.1}ms");
    let (m2, p2) = time(&mut || {
        let _ = l.list_references(&ListQuery { sort: "title".into(), limit: 400, ..Default::default() }).unwrap();
    });
    println!("BENCH list done median={m2:.1}ms p95={p2:.1}ms");
    let refs: Vec<Reference> = l.store.all().unwrap();
    let a: Vec<&Reference> = refs.iter().take(1).collect();
    let ctx = prm_core::render::Context { set: refs.iter().take(200).collect(), locale: "en-US".into(), ..Default::default() };
    let (m3, p3) = time(&mut || {
        let _ = prm_core::render::cite("apa-7", &[CiteItem { ref_id: a[0].id.clone(), ..Default::default() }], false, &a, &ctx);
    });
    let t = std::time::Instant::now();
    let d = l.duplicates(0.9).unwrap().len();
    let dup = t.elapsed();
    println!("BENCH n={n} insert={:.1}s", insert.as_secs_f64());
    println!("BENCH metadata search median={m1:.1}ms p95={p1:.1}ms (target p95 <= 250 ms)");
    println!("BENCH list+sort page median={m2:.1}ms p95={p2:.1}ms (target sort <= 500 ms)");
    println!("BENCH citation render (200-source context) median={m3:.2}ms p95={p3:.2}ms (target <= 500 ms)");
    println!("BENCH duplicate scan {:.1}s, {d} candidates (target <= 60 s)", dup.as_secs_f64());
}
