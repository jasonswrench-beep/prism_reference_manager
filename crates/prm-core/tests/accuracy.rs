//! Accuracy measurements for the release gates (spec QAL-005, QAL-006). Ignored by default
//! because they use the network or a local corpus of real files:
//! `cargo test -p prm-core --release --test accuracy -- --ignored --nocapture`.

use serde_json::Value;

fn norm(s: &str) -> String {
    prm_core::util::sort_key(s)
}

/// QAL-005: identifier lookup returns the right title, first author, year and DOI.
#[test]
#[ignore]
fn metadata_accuracy_corpus() {
    let v: Value = serde_json::from_str(include_str!("fixtures/metadata/corpus.json")).unwrap();
    let entries = v["entries"].as_array().unwrap();
    let (mut all_ok, mut abstained, mut wrong, mut reviewed) = (0, 0, 0, 0);
    let mut field_ok = [0usize; 4];
    for e in entries {
        if !e["reviewed_by"].as_str().unwrap_or("").is_empty() {
            reviewed += 1;
        }
        let doi = e["doi"].as_str().unwrap();
        let res = prm_core::lookup::resolve("doi", doi, &mut |_, _, _, _, _| {});
        let Some(c) = res.candidate else {
            abstained += 1;
            println!("ABSTAIN {doi}: {}", res.message);
            continue;
        };
        let title_ok = strsim::normalized_levenshtein(&norm(c.title()), &norm(e["title"].as_str().unwrap())) >= 0.95;
        let first = c.creators.first().map(|x| if x.name.literal.is_empty() { x.name.surname() } else { x.name.literal.clone() }).unwrap_or_default();
        let author_ok = norm(&first) == norm(e["first_author"].as_str().unwrap());
        let year_ok = c.year() == e["year"].as_i64().map(|y| y as i32);
        let doi_ok = prm_core::util::normalize_doi(c.f("doi")) == prm_core::util::normalize_doi(doi);
        for (i, ok) in [title_ok, author_ok, year_ok, doi_ok].iter().enumerate() {
            if *ok {
                field_ok[i] += 1;
            }
        }
        if title_ok && author_ok && year_ok && doi_ok {
            all_ok += 1;
        } else {
            wrong += 1;
            println!("MISMATCH {doi}: title={title_ok} author={author_ok} (got {first}) year={year_ok} (got {:?}) doi={doi_ok}", c.year());
        }
    }
    let n = entries.len();
    let answered = n - abstained;
    println!(
        "GATE QAL-005 metadata accuracy: {all_ok}/{answered} answered records fully correct ({:.0}%), {abstained} abstentions, {wrong} wrong; fields correct: title {}, first author {}, year {}, DOI {} of {answered}; {reviewed}/{n} corpus entries independently reviewed (target >= 95% on a reviewed corpus)",
        if answered > 0 { all_ok as f64 * 100.0 / answered as f64 } else { 0.0 },
        field_ok[0], field_ok[1], field_ok[2], field_ok[3]
    );
}

/// QAL-006: DOIs recovered from real PDFs. Point PRM_PDF_CORPUS at a folder containing the PDFs
/// and a labels.csv with lines `file name,expected DOI` (empty DOI = the file has none).
#[test]
#[ignore]
fn pdf_doi_recovery_corpus() {
    let Ok(dir) = std::env::var("PRM_PDF_CORPUS") else {
        println!("GATE QAL-006 not measured: set PRM_PDF_CORPUS to a folder of PDFs with labels.csv");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let labels = std::fs::read_to_string(dir.join("labels.csv")).expect("labels.csv in the corpus folder");
    let (mut correct, mut wrong, mut abstain, mut n) = (0, 0, 0, 0);
    for line in labels.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let (file, expected) = line.split_once(',').unwrap_or((line, ""));
        let info = prm_core::pdf::inspect(&dir.join(file.trim()));
        let expected = prm_core::util::normalize_doi(expected.trim());
        let found = info.dois.first().and_then(|d| prm_core::util::normalize_doi(d));
        n += 1;
        match (&found, &expected) {
            (None, _) => abstain += 1,
            (Some(f), Some(e)) if f == e => correct += 1,
            _ => {
                wrong += 1;
                println!("WRONG {file}: found {found:?}, expected {expected:?}");
            }
        }
    }
    println!("GATE QAL-006 PDF DOI recovery: {correct}/{n} correct ({:.0}%), {wrong} wrong, {abstain} abstained (target >= 90%)", correct as f64 * 100.0 / n.max(1) as f64);
}
