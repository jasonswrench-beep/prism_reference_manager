//! PDF inspection, annotation export into copies, flattened copies, and reading annotations
//! made in other PDF readers. The original file is never modified.

use crate::error::{Error, Result};
use crate::model::{Annotation, Rect};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::Serialize;
use std::path::Path;

const LARGE_BYTES: u64 = 250 * 1024 * 1024;
const LARGE_PAGES: usize = 3000;

#[derive(Debug, Clone, Serialize, Default)]
pub struct PdfInfo {
    /// ok, protected, malformed, large, image-only
    pub state: String,
    pub page_count: u32,
    pub page_labels: Vec<String>,
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub dois: Vec<String>,
    /// arXiv identifiers printed on the first pages (for example 2401.01234v2).
    #[serde(default)]
    pub arxiv: Vec<String>,
    pub message: String,
}

fn obj_str(o: &Object) -> String {
    match o {
        Object::String(b, _) => decode_pdf_string(b),
        Object::Name(n) => String::from_utf8_lossy(n).to_string(),
        _ => String::new(),
    }
}

/// PDF text strings: UTF-16BE with a byte-order mark, otherwise PDFDocEncoding (Latin-1 subset).
pub fn decode_pdf_string(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let u: Vec<u16> = b[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else if b.len() >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF {
        String::from_utf8_lossy(&b[3..]).to_string()
    } else {
        b.iter().map(|c| *c as char).collect()
    }
}

fn encode_pdf_string(s: &str) -> Object {
    if s.is_ascii() {
        Object::string_literal(s)
    } else {
        let mut v = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(v, lopdf::StringFormat::Hexadecimal)
    }
}

fn roman(mut n: u32, upper: bool) -> String {
    let table = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper { s.to_uppercase() } else { s }
}

fn letters(n: u32, upper: bool) -> String {
    let c = (b'a' + ((n - 1) % 26) as u8) as char;
    let s: String = std::iter::repeat(c).take(((n - 1) / 26 + 1) as usize).collect();
    if upper { s.to_uppercase() } else { s }
}

fn page_labels(doc: &Document, count: usize) -> Vec<String> {
    let mut labels: Vec<String> = (1..=count).map(|i| i.to_string()).collect();
    let Ok(cat) = doc.catalog() else { return labels };
    let Ok(pl) = cat.get(b"PageLabels") else { return labels };
    let pl = match pl {
        Object::Reference(r) => doc.get_object(*r).ok().cloned(),
        o => Some(o.clone()),
    };
    let Some(Object::Dictionary(pl)) = pl else { return labels };
    let Ok(Object::Array(nums)) = pl.get(b"Nums").map(|o| match o {
        Object::Reference(r) => doc.get_object(*r).cloned().unwrap_or(Object::Null),
        o => o.clone(),
    }) else {
        return labels;
    };
    let mut ranges: Vec<(usize, Dictionary)> = Vec::new();
    let mut i = 0;
    while i + 1 < nums.len() {
        let start = nums[i].as_i64().unwrap_or(0) as usize;
        let d = match &nums[i + 1] {
            Object::Reference(r) => doc.get_dictionary(*r).ok().cloned(),
            Object::Dictionary(d) => Some(d.clone()),
            _ => None,
        };
        if let Some(d) = d {
            ranges.push((start, d));
        }
        i += 2;
    }
    for (k, (start, d)) in ranges.iter().enumerate() {
        let end = ranges.get(k + 1).map(|r| r.0).unwrap_or(count);
        let style = d.get(b"S").ok().map(obj_str).unwrap_or_default();
        let prefix = d.get(b"P").ok().map(obj_str).unwrap_or_default();
        let st = d.get(b"St").ok().and_then(|o| o.as_i64().ok()).unwrap_or(1) as u32;
        for p in *start..end.min(count) {
            let n = st + (p - start) as u32;
            let num = match style.as_str() {
                "D" => n.to_string(),
                "r" => roman(n, false),
                "R" => roman(n, true),
                "a" => letters(n, false),
                "A" => letters(n, true),
                _ => String::new(),
            };
            labels[p] = format!("{prefix}{num}");
        }
    }
    labels
}

/// Inspect a PDF: state, page count, labels, document information and DOIs in metadata.
/// Text of the given pages with line and word breaks kept (each text-showing operator is a word
/// group; positioning operators start a new line), so identifiers are not glued to the next word.
fn page_text(doc: &Document, pages: &std::collections::BTreeMap<u32, ObjectId>, numbers: &[u32]) -> Option<String> {
    let mut out = String::new();
    for n in numbers {
        let id = pages.get(n)?;
        let bytes = doc.get_page_content(*id);
        let Ok(content) = lopdf::content::Content::decode(&bytes) else { continue };
        for op in content.operations {
            match op.operator.as_str() {
                "Tj" | "'" | "\"" => {
                    for o in &op.operands {
                        if let Object::String(b, _) = o {
                            out.push_str(&decode_pdf_string(b));
                        }
                    }
                    out.push(' ');
                }
                "TJ" => {
                    if let Some(Object::Array(items)) = op.operands.first() {
                        for it in items {
                            match it {
                                Object::String(b, _) => out.push_str(&decode_pdf_string(b)),
                                Object::Integer(k) if *k < -200 => out.push(' '),
                                Object::Real(k) if *k < -200.0 => out.push(' '),
                                _ => {}
                            }
                        }
                    }
                    out.push(' ');
                }
                "Td" | "TD" | "T*" | "Tm" | "ET" => out.push('\n'),
                _ => {}
            }
        }
        out.push('\n');
    }
    if out.trim().is_empty() { None } else { Some(out) }
}

/// Inspect a PDF. A file built to crash the PDF reader gives a "malformed" result instead.
pub fn inspect(path: &Path) -> PdfInfo {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inspect_inner(path))).unwrap_or_else(|_| PdfInfo { state: "malformed".into(), ..Default::default() })
}

fn inspect_inner(path: &Path) -> PdfInfo {
    let mut info = PdfInfo { state: "ok".into(), ..Default::default() };
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            info.state = "missing".into();
            info.message = format!("The file could not be read: {e}");
            return info;
        }
    };
    if bytes.is_empty() || bytes.len() < 1024 && !bytes.starts_with(b"%PDF") {
        info.state = "placeholder".into();
        info.message = "The file looks like a cloud placeholder that has not been downloaded. Make it available offline, then try again.".into();
        return info;
    }
    // DOIs in the first megabyte (XMP metadata and first-page text streams often carry them).
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1 << 20)]);
    let re = re!(r"(?i)\b(10\.\d{4,9}/[^\s<>\]\)\(\x22]+)");
    for c in re.captures_iter(&head) {
        let d = c[1].trim_end_matches(['.', ',', ';']).to_lowercase();
        if !info.dois.contains(&d) && d.len() < 100 {
            info.dois.push(d);
        }
    }
    let doc = match Document::load_mem(&bytes) {
        Ok(d) => d,
        Err(e) => {
            let msg = e.to_string();
            if msg.to_lowercase().contains("encrypt") || head.contains("/Encrypt") {
                info.state = "protected".into();
                info.message = "The PDF is password-protected or encrypted. It can still be opened for reading if the reader can decrypt it; annotations are kept in the library layer.".into();
            } else {
                info.state = "malformed".into();
                info.message = format!("The PDF structure could not be read ({msg}). The file is kept unchanged.");
            }
            return info;
        }
    };
    if doc.is_encrypted() {
        info.state = "protected".into();
        info.message = "The PDF is encrypted. Annotations are kept in the library layer; an annotated copy may not be possible.".into();
    }
    let pages = doc.get_pages();
    info.page_count = pages.len() as u32;
    // First-page text: DOIs and arXiv identifiers are usually printed there, inside compressed
    // content streams the raw scan above cannot see.
    let first: Vec<u32> = pages.keys().copied().take(2).collect();
    if let Some(text) = page_text(&doc, &pages, &first) {
        for c in re!(r"(?i)\b(10\.\d{4,9}/[^\s<>\]\)\(\x22]+)").captures_iter(&text) {
            let d = c[1].trim_end_matches(['.', ',', ';']).to_lowercase();
            if !info.dois.contains(&d) && d.len() < 100 {
                info.dois.push(d);
            }
        }
        for c in re!(r"(?i)arXiv:\s*(\d{4}\.\d{4,5}(?:v\d+)?|[a-z\-]+(?:\.[A-Z]{2})?/\d{7}(?:v\d+)?)").captures_iter(&text) {
            let a = c[1].to_string();
            if !info.arxiv.contains(&a) {
                info.arxiv.push(a);
            }
        }
    }
    info.page_labels = page_labels(&doc, pages.len());
    if let Ok(Object::Reference(r)) = doc.trailer.get(b"Info") {
        if let Ok(d) = doc.get_dictionary(*r) {
            let g = |k: &[u8]| d.get(k).ok().map(obj_str).unwrap_or_default();
            info.title = g(b"Title");
            info.author = g(b"Author");
            info.subject = g(b"Subject");
            info.keywords = g(b"Keywords");
        }
    }
    if info.state == "ok" {
        if size > LARGE_BYTES || pages.len() > LARGE_PAGES {
            info.state = "large".into();
            info.message = "This is a very large PDF. Reading and indexing may be slow.".into();
        } else {
            // Image-only check: no font resources on the first pages.
            let mut fonts = false;
            for (_, id) in pages.iter().take(3) {
                if let Ok((Some(res), _)) = doc.get_page_resources(*id) {
                    if res.get(b"Font").is_ok() {
                        fonts = true;
                    }
                }
                if let Ok(f) = doc.get_page_fonts(*id) {
                    if !f.is_empty() {
                        fonts = true;
                    }
                }
            }
            if !fonts && !pages.is_empty() {
                info.state = "image-only".into();
                info.message = "No text layer was found; this looks like a scanned PDF. Text search and quotation capture need OCR.".into();
            }
        }
    }
    info
}

fn page_box(doc: &Document, page: ObjectId) -> [f32; 4] {
    let get = |k: &[u8]| -> Option<[f32; 4]> {
        let mut cur = Some(page);
        for _ in 0..16 {
            let id = cur?;
            let d = doc.get_dictionary(id).ok()?;
            if let Ok(o) = d.get(k) {
                let arr = match o {
                    Object::Reference(r) => doc.get_object(*r).ok()?.as_array().ok()?.clone(),
                    Object::Array(a) => a.clone(),
                    _ => return None,
                };
                let v: Vec<f32> = arr.iter().filter_map(|x| x.as_float().ok().or_else(|| x.as_i64().ok().map(|i| i as f32))).collect();
                if v.len() == 4 {
                    return Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])]);
                }
            }
            cur = d.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
        }
        None
    };
    get(b"CropBox").or_else(|| get(b"MediaBox")).unwrap_or([0.0, 0.0, 612.0, 792.0])
}

fn color(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    if h.len() >= 6 {
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(255) as f32 / 255.0;
        [p(0), p(2), p(4)]
    } else {
        [1.0, 0.9, 0.2]
    }
}

fn to_pdf(b: [f32; 4], r: &Rect) -> [f32; 4] {
    let w = b[2] - b[0];
    let h = b[3] - b[1];
    let x1 = b[0] + r.x * w;
    let x2 = b[0] + (r.x + r.w) * w;
    let y_top = b[3] - r.y * h;
    let y_bot = b[3] - (r.y + r.h) * h;
    [x1, y_bot, x2, y_top]
}

fn reals(v: &[f32]) -> Object {
    Object::Array(v.iter().map(|x| Object::Real(*x)).collect())
}

fn union(rs: &[[f32; 4]]) -> [f32; 4] {
    let mut u = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for r in rs {
        u[0] = u[0].min(r[0]);
        u[1] = u[1].min(r[1]);
        u[2] = u[2].max(r[2]);
        u[3] = u[3].max(r[3]);
    }
    if u[0] > u[2] { [0.0, 0.0, 0.0, 0.0] } else { u }
}

/// Appearance stream so viewers that do not generate their own still show the mark.
fn appearance(doc: &mut Document, bbox: [f32; 4], ops: String, alpha: f32, multiply: bool) -> ObjectId {
    let gs = dictionary! { "Type" => "ExtGState", "CA" => Object::Real(alpha), "ca" => Object::Real(alpha), "BM" => if multiply { "Multiply" } else { "Normal" } };
    let res = dictionary! { "ExtGState" => dictionary! { "GS0" => Object::Dictionary(gs) } };
    let d = dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => reals(&bbox), "Resources" => Object::Dictionary(res) };
    doc.add_object(Stream::new(d, format!("/GS0 gs {ops}").into_bytes()))
}

fn pdf_date(iso: &str) -> String {
    let d: String = iso.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    format!("D:{d}Z")
}

fn draw_ops(a: &Annotation, quads: &[[f32; 4]], c: [f32; 3]) -> String {
    let mut ops = String::new();
    match a.kind.as_str() {
        "highlight" => {
            ops.push_str(&format!("{} {} {} rg ", c[0], c[1], c[2]));
            for q in quads {
                ops.push_str(&format!("{} {} {} {} re f ", q[0], q[1], q[2] - q[0], q[3] - q[1]));
            }
        }
        "underline" | "strikeout" => {
            ops.push_str(&format!("{} {} {} RG 1 w ", c[0], c[1], c[2]));
            for q in quads {
                let y = if a.kind == "underline" { q[1] + 0.8 } else { (q[1] + q[3]) / 2.0 };
                ops.push_str(&format!("{} {y} m {} {y} l S ", q[0], q[2]));
            }
        }
        "rect" | "region" => {
            ops.push_str(&format!("{} {} {} RG 1.5 w ", c[0], c[1], c[2]));
            for q in quads {
                ops.push_str(&format!("{} {} {} {} re S ", q[0], q[1], q[2] - q[0], q[3] - q[1]));
            }
        }
        "ellipse" => {
            ops.push_str(&format!("{} {} {} RG 1.5 w ", c[0], c[1], c[2]));
            for q in quads {
                let (cx, cy, rx, ry) = ((q[0] + q[2]) / 2.0, (q[1] + q[3]) / 2.0, (q[2] - q[0]) / 2.0, (q[3] - q[1]) / 2.0);
                let k = 0.5523;
                ops.push_str(&format!(
                    "{} {cy} m {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c S ",
                    cx + rx, cx + rx, cy + k * ry, cx + k * rx, cy + ry, cy + ry,
                    cx - k * rx, cy + ry, cx - rx, cy + k * ry, cx - rx,
                    cx - rx, cy - k * ry, cx - k * rx, cy - ry, cy - ry,
                    cx + k * rx, cy - ry, cx + rx, cy - k * ry, cx + rx
                ));
            }
        }
        "note" | "freetext" => {
            ops.push_str(&format!("{} {} {} rg ", c[0], c[1], c[2]));
            for q in quads {
                ops.push_str(&format!("{} {} {} {} re f ", q[0], q[1], (q[2] - q[0]).max(14.0), (q[3] - q[1]).max(14.0)));
            }
        }
        _ => {}
    }
    ops
}

/// Write `annotations` as standard PDF annotation objects into a copy of `src` at `dest`.
/// With `flatten`, marks are drawn into page content instead (a display-only copy).
pub fn export_annotated(src: &Path, dest: &Path, annotations: &[Annotation], flatten: bool) -> Result<usize> {
    unpanic(|| export_annotated_inner(src, dest, annotations, flatten))
}

/// Run PDF work so that a malformed file the PDF library cannot handle gives an error, not a crash.
fn unpanic<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err(Error::user("This PDF's structure could not be read.")))
}

fn export_annotated_inner(src: &Path, dest: &Path, annotations: &[Annotation], flatten: bool) -> Result<usize> {
    if src == dest {
        return Err(Error::user("Choose a different file name; the original PDF is never overwritten."));
    }
    let mut doc = Document::load(src)?;
    if doc.is_encrypted() {
        return Err(Error::Pdf("This PDF is encrypted, so an annotated copy cannot be written. Export the notes instead.".into()));
    }
    let pages = doc.get_pages();
    let mut written = 0;
    for a in annotations {
        let Some(page_id) = pages.get(&(a.page_index + 1)).copied() else { continue };
        let b = page_box(&doc, page_id);
        let c = color(&a.color);
        let mut quads: Vec<[f32; 4]> = a.rects.iter().map(|r| to_pdf(b, r)).collect();
        if a.kind == "ink" {
            let w = b[2] - b[0];
            let h = b[3] - b[1];
            let strokes: Vec<Vec<f32>> = a.ink.iter().map(|s| s.iter().flat_map(|p| [b[0] + p[0] * w, b[3] - p[1] * h]).collect()).collect();
            let pts: Vec<[f32; 4]> = strokes.iter().flat_map(|s| s.chunks(2).map(|p| [p[0], p[1], p[0], p[1]]).collect::<Vec<_>>()).collect();
            let mut bb = union(&pts);
            bb = [bb[0] - 2.0, bb[1] - 2.0, bb[2] + 2.0, bb[3] + 2.0];
            if flatten {
                let mut ops = format!("q {} {} {} RG 1.5 w 1 J 1 j ", c[0], c[1], c[2]);
                for s in &strokes {
                    for (i, p) in s.chunks(2).enumerate() {
                        ops.push_str(&format!("{} {} {} ", p[0], p[1], if i == 0 { "m" } else { "l" }));
                    }
                    ops.push_str("S ");
                }
                ops.push_str("Q\n");
                doc.add_page_contents(page_id, ops.into_bytes())?;
            } else {
                let ap = {
                    let mut ops = format!("{} {} {} RG 1.5 w 1 J 1 j ", c[0], c[1], c[2]);
                    for s in &strokes {
                        for (i, p) in s.chunks(2).enumerate() {
                            ops.push_str(&format!("{} {} {} ", p[0], p[1], if i == 0 { "m" } else { "l" }));
                        }
                        ops.push_str("S ");
                    }
                    appearance(&mut doc, bb, ops, 1.0, false)
                };
                let d = dictionary! {
                    "Type" => "Annot", "Subtype" => "Ink", "Rect" => reals(&bb),
                    "InkList" => Object::Array(strokes.iter().map(|s| reals(s)).collect()),
                    "C" => reals(&c), "F" => 4, "NM" => encode_pdf_string(&a.id),
                    "Contents" => encode_pdf_string(&a.comment), "T" => encode_pdf_string(&a.author),
                    "M" => Object::string_literal(pdf_date(&a.modified)),
                    "BS" => dictionary! { "W" => Object::Real(1.5) },
                    "AP" => dictionary! { "N" => ap },
                };
                attach(&mut doc, page_id, d)?;
            }
            written += 1;
            continue;
        }
        if quads.is_empty() {
            continue;
        }
        if a.kind == "note" {
            let q = quads[0];
            quads = vec![[q[0], q[3] - 18.0, q[0] + 18.0, q[3]]];
        }
        let rect = union(&quads);
        if flatten {
            let alpha = if a.kind == "highlight" { 0.35 } else { 1.0 };
            let gs_name = format!("PrismGS{written}");
            let ops = format!("q /{gs_name} gs {}Q\n", draw_ops(a, &quads, c));
            // Register the graphics state on the page resources.
            add_extgstate(&mut doc, page_id, &gs_name, alpha)?;
            doc.add_page_contents(page_id, ops.into_bytes())?;
            written += 1;
            continue;
        }
        let subtype = match a.kind.as_str() {
            "highlight" => "Highlight",
            "underline" => "Underline",
            "strikeout" => "StrikeOut",
            "note" => "Text",
            "freetext" => "FreeText",
            "ellipse" => "Circle",
            _ => "Square",
        };
        let alpha = if a.kind == "highlight" { 0.35 } else { 1.0 };
        let ap = appearance(&mut doc, rect, draw_ops(a, &quads, c), alpha, a.kind == "highlight");
        let mut d = dictionary! {
            "Type" => "Annot", "Subtype" => subtype, "Rect" => reals(&rect), "C" => reals(&c), "F" => 4,
            "NM" => encode_pdf_string(&a.id), "T" => encode_pdf_string(&a.author),
            "M" => Object::string_literal(pdf_date(&a.modified)),
            "Contents" => encode_pdf_string(if a.kind == "freetext" { &a.text } else { &a.comment }),
            "AP" => dictionary! { "N" => ap },
        };
        if matches!(subtype, "Highlight" | "Underline" | "StrikeOut") {
            let qp: Vec<f32> = quads.iter().flat_map(|q| [q[0], q[3], q[2], q[3], q[0], q[1], q[2], q[1]]).collect();
            d.set("QuadPoints", reals(&qp));
            d.set("CA", Object::Real(if subtype == "Highlight" { 0.4 } else { 1.0 }));
        }
        if subtype == "Text" {
            d.set("Name", "Comment");
            d.set("Open", false);
        }
        if subtype == "FreeText" {
            d.set("DA", Object::string_literal("/Helv 11 Tf 0 0 0 rg"));
        }
        if matches!(subtype, "Square" | "Circle") {
            d.set("BS", dictionary! { "W" => Object::Real(1.5) });
        }
        attach(&mut doc, page_id, d)?;
        written += 1;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("pdf.tmp");
    doc.save(&tmp)?;
    std::fs::rename(&tmp, dest)?;
    Ok(written)
}

fn add_extgstate(doc: &mut Document, page_id: ObjectId, name: &str, alpha: f32) -> Result<()> {
    let gs = dictionary! { "Type" => "ExtGState", "CA" => Object::Real(alpha), "ca" => Object::Real(alpha), "BM" => "Multiply" };
    let gs_id = doc.add_object(gs);
    let page = doc.get_dictionary(page_id)?.clone();
    let mut res = match page.get(b"Resources") {
        Ok(Object::Reference(r)) => doc.get_dictionary(*r)?.clone(),
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => {
            // Inherited resources: copy them so the page can be extended.
            match doc.get_page_resources(page_id) {
                Ok((Some(d), _)) => d.clone(),
                _ => Dictionary::new(),
            }
        }
    };
    let mut ext = match res.get(b"ExtGState") {
        Ok(Object::Reference(r)) => doc.get_dictionary(*r)?.clone(),
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    ext.set(name, gs_id);
    res.set("ExtGState", Object::Dictionary(ext));
    let p = doc.get_dictionary_mut(page_id)?;
    p.set("Resources", Object::Dictionary(res));
    Ok(())
}

fn attach(doc: &mut Document, page_id: ObjectId, annot: Dictionary) -> Result<()> {
    let mut annot = annot;
    annot.set("P", page_id);
    let id = doc.add_object(annot);
    let existing = doc.get_dictionary(page_id)?.get(b"Annots").ok().cloned();
    match existing {
        Some(Object::Reference(r)) => {
            if let Ok(Object::Array(arr)) = doc.get_object_mut(r) {
                arr.push(Object::Reference(id));
            }
        }
        Some(Object::Array(mut arr)) => {
            arr.push(Object::Reference(id));
            doc.get_dictionary_mut(page_id)?.set("Annots", Object::Array(arr));
        }
        _ => {
            doc.get_dictionary_mut(page_id)?.set("Annots", Object::Array(vec![Object::Reference(id)]));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, Default)]
pub struct ExternalAnnotation {
    pub page_index: u32,
    pub kind: String,
    pub rects: Vec<Rect>,
    pub ink: Vec<Vec<[f32; 2]>>,
    pub color: String,
    pub comment: String,
    pub author: String,
    pub modified: String,
    /// Annotation name (NM) when present.
    pub name: String,
    /// Stable identity used to prevent duplicate imports.
    pub identity: String,
    pub page_width: f32,
    pub page_height: f32,
    /// Free text annotations carry their text here.
    pub text: String,
}

/// Read annotations from a PDF made in another reader. Unsupported subtypes are reported by name.
pub fn read_annotations(path: &Path) -> Result<(Vec<ExternalAnnotation>, Vec<String>)> {
    unpanic(|| read_annotations_inner(path))
}

fn read_annotations_inner(path: &Path) -> Result<(Vec<ExternalAnnotation>, Vec<String>)> {
    let doc = Document::load(path)?;
    let mut out = Vec::new();
    let mut unsupported = Vec::new();
    for (num, page_id) in doc.get_pages() {
        let b = page_box(&doc, page_id);
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        let Ok(annots) = doc.get_page_annotations(page_id) else { continue };
        for d in annots {
            let subtype = d.get(b"Subtype").ok().map(obj_str).unwrap_or_default();
            let kind = match subtype.as_str() {
                "Highlight" => "highlight",
                "Underline" | "Squiggly" => "underline",
                "StrikeOut" => "strikeout",
                "Text" => "note",
                "FreeText" => "freetext",
                "Square" => "rect",
                "Circle" => "ellipse",
                "Ink" => "ink",
                "Link" | "Widget" | "Popup" => continue,
                other => {
                    if !unsupported.contains(&other.to_string()) {
                        unsupported.push(other.to_string());
                    }
                    continue;
                }
            };
            let nums = |k: &[u8]| -> Vec<f32> {
                d.get(k)
                    .ok()
                    .and_then(|o| match o {
                        Object::Array(a) => Some(a.clone()),
                        Object::Reference(r) => doc.get_object(*r).ok().and_then(|x| x.as_array().ok().cloned()),
                        _ => None,
                    })
                    .map(|a| a.iter().filter_map(|x| x.as_float().ok().or_else(|| x.as_i64().ok().map(|i| i as f32))).collect())
                    .unwrap_or_default()
            };
            let rel = |x1: f32, y1: f32, x2: f32, y2: f32| Rect { x: ((x1.min(x2) - b[0]) / w).clamp(0.0, 1.0), y: ((b[3] - y1.max(y2)) / h).clamp(0.0, 1.0), w: ((x2 - x1).abs() / w).clamp(0.0, 1.0), h: ((y2 - y1).abs() / h).clamp(0.0, 1.0) };
            let mut rects = Vec::new();
            let qp = nums(b"QuadPoints");
            if qp.len() >= 8 {
                for q in qp.chunks(8) {
                    let xs = [q[0], q[2], q[4], q[6]];
                    let ys = [q[1], q[3], q[5], q[7]];
                    let (x1, x2) = (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max));
                    let (y1, y2) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
                    rects.push(rel(x1, y1, x2, y2));
                }
            } else {
                let r = nums(b"Rect");
                if r.len() == 4 {
                    rects.push(rel(r[0], r[1], r[2], r[3]));
                }
            }
            let mut ink = Vec::new();
            if kind == "ink" {
                if let Ok(Object::Array(list)) = d.get(b"InkList") {
                    for s in list {
                        let arr = match s {
                            Object::Array(a) => a.clone(),
                            Object::Reference(r) => doc.get_object(*r).ok().and_then(|x| x.as_array().ok().cloned()).unwrap_or_default(),
                            _ => vec![],
                        };
                        let v: Vec<f32> = arr.iter().filter_map(|x| x.as_float().ok().or_else(|| x.as_i64().ok().map(|i| i as f32))).collect();
                        ink.push(v.chunks(2).filter(|p| p.len() == 2).map(|p| [((p[0] - b[0]) / w).clamp(0.0, 1.0), ((b[3] - p[1]) / h).clamp(0.0, 1.0)]).collect());
                    }
                }
            }
            let c = nums(b"C");
            let color = if c.len() >= 3 { format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8) } else { "#ffd400".into() };
            let contents = d.get(b"Contents").ok().map(obj_str).unwrap_or_default();
            let name = d.get(b"NM").ok().map(obj_str).unwrap_or_default();
            let author = d.get(b"T").ok().map(obj_str).unwrap_or_default();
            let modified = d.get(b"M").ok().map(obj_str).unwrap_or_default();
            let geom: String = rects.iter().map(|r| format!("{:.3},{:.3},{:.3},{:.3};", r.x, r.y, r.w, r.h)).collect();
            let identity = if !name.is_empty() { format!("nm:{name}") } else { format!("geo:{}", &crate::util::sha256_hex(format!("{num}|{kind}|{geom}|{contents}").as_bytes())[..24]) };
            out.push(ExternalAnnotation {
                page_index: num - 1,
                kind: kind.into(),
                rects,
                ink,
                color,
                comment: if kind == "freetext" { String::new() } else { contents.clone() },
                text: if kind == "freetext" { contents } else { String::new() },
                author,
                modified,
                name,
                identity,
                page_width: w,
                page_height: h,
            });
        }
    }
    Ok((out, unsupported))
}

/// Sidecar with semantics standard PDF annotations cannot express.
pub fn sidecar(attachment_hash: &str, annotations: &[Annotation]) -> serde_json::Value {
    serde_json::json!({
        "format": "prism-annotation-sidecar",
        "version": 1,
        "attachment_sha256": attachment_hash,
        "annotations": annotations,
        "note": "Each annotation's PDF annotation name (NM) equals its id. Project scope, meaning, links, locators and review state live here.",
    })
}
