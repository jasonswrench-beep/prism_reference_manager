//! Runs the golden APA fixture corpus. A failure in any case blocks release.

#[test]
fn apa_golden_fixtures() {
    let fx = prm_core::render::apa::load_fixtures();
    assert!(fx.len() >= 50, "fixture corpus too small");
    let mut failures = Vec::new();
    for f in &fx {
        let r = prm_core::render::apa::run_fixture(f);
        if !r.passed {
            failures.push(format!("{}\n  expected: {}\n  actual:   {}\n  text_ok={} format_ok={}", r.id, r.expected, r.actual, r.text_ok, r.format_ok));
        }
    }
    assert!(failures.is_empty(), "{} of {} fixtures failed:\n{}", failures.len(), fx.len(), failures.join("\n"));
}
