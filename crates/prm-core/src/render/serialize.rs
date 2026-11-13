//! Serializers for rendered output. Each consumes the same runs and paragraph properties.
//! Bibliography indents are real paragraph properties, never spaces or tabs.

use super::{Paragraph, Run};
use crate::util::{html_escape, xml_escape};
use std::io::Write;

pub fn plain(runs: &[Run]) -> String {
    super::runs_text(runs)
}

pub fn html_runs(runs: &[Run]) -> String {
    let mut o = String::new();
    for r in runs {
        let mut t = html_escape(&r.text).replace('\n', "<br>");
        if r.sup {
            t = format!("<sup>{t}</sup>");
        }
        if r.sub {
            t = format!("<sub>{t}</sub>");
        }
        if r.italic {
            t = format!("<i>{t}</i>");
        }
        if r.bold {
            t = format!("<b>{t}</b>");
        }
        if r.small_caps {
            t = format!("<span style=\"font-variant:small-caps\">{t}</span>");
        }
        if let Some(l) = &r.link {
            t = format!("<a href=\"{}\">{}</a>", html_escape(l), t);
        }
        o.push_str(&t);
    }
    o
}

/// Inline HTML fragment for a citation (no paragraph wrapper, no font: the receiving document's
/// body font applies).
pub fn html_inline(runs: &[Run]) -> String {
    format!("<span>{}</span>", html_runs(runs))
}

/// HTML for bibliography paragraphs with a real hanging indent.
pub fn html_paragraphs(ps: &[Paragraph]) -> String {
    let mut o = String::new();
    for p in ps {
        o.push_str(&format!(
            "<p style=\"margin:0 0 0 {ind}in;text-indent:-{ind}in;line-height:{ls}\">{}</p>",
            html_runs(&p.runs),
            ind = p.hanging_indent,
            ls = if p.line_spacing > 0.0 { p.line_spacing } else { 1.0 }
        ));
    }
    o
}

pub fn html_document(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n{}\n</body></html>\n",
        html_escape(title),
        body
    )
}

/// Windows "HTML Format" clipboard payload with byte offsets.
pub fn cf_html(fragment: &str) -> String {
    let header_tmpl = "Version:0.9\r\nStartHTML:{SH}\r\nEndHTML:{EH}\r\nStartFragment:{SF}\r\nEndFragment:{EF}\r\n";
    let pre = "<html><body>\r\n<!--StartFragment-->";
    let post = "<!--EndFragment-->\r\n</body></html>";
    let header_len = header_tmpl.replace("{SH}", "0000000000").replace("{EH}", "0000000000").replace("{SF}", "0000000000").replace("{EF}", "0000000000").len();
    let sh = header_len;
    let sf = sh + pre.len();
    let ef = sf + fragment.len();
    let eh = ef + post.len();
    let header = header_tmpl
        .replace("{SH}", &format!("{sh:010}"))
        .replace("{EH}", &format!("{eh:010}"))
        .replace("{SF}", &format!("{sf:010}"))
        .replace("{EF}", &format!("{ef:010}"));
    format!("{header}{pre}{fragment}{post}")
}

fn rtf_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '{' => o.push_str("\\{"),
            '}' => o.push_str("\\}"),
            '\n' => o.push_str("\\line "),
            c if (c as u32) < 128 => o.push(c),
            c => {
                // UTF-16 code units as signed 16-bit values, with "?" as the fallback character.
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    o.push_str(&format!("\\u{}?", *u as i16));
                }
            }
        }
    }
    o
}

fn rtf_runs(runs: &[Run]) -> String {
    let mut o = String::new();
    for r in runs {
        let mut t = rtf_escape(&r.text);
        if r.sup {
            t = format!("{{\\super {t}}}");
        }
        if r.sub {
            t = format!("{{\\sub {t}}}");
        }
        if r.italic {
            t = format!("{{\\i {t}}}");
        }
        if r.bold {
            t = format!("{{\\b {t}}}");
        }
        if r.small_caps {
            t = format!("{{\\scaps {t}}}");
        }
        if let Some(l) = &r.link {
            t = format!("{{\\field{{\\*\\fldinst{{HYPERLINK \"{}\"}}}}{{\\fldrslt {}}}}}", rtf_escape(l), t);
        }
        o.push_str(&t);
    }
    o
}

/// RTF for an inline citation. No font table entry is forced on the receiving document.
pub fn rtf_inline(runs: &[Run]) -> String {
    format!("{{\\rtf1\\ansi\\uc1 {}}}", rtf_runs(runs))
}

/// RTF bibliography: each entry a paragraph with a 0.5 inch hanging indent and line spacing.
pub fn rtf_paragraphs(ps: &[Paragraph], font: Option<&str>) -> String {
    let mut o = String::from("{\\rtf1\\ansi\\uc1");
    if let Some(f) = font {
        o.push_str(&format!("{{\\fonttbl{{\\f0 {};}}}}\\f0\\fs24", rtf_escape(f)));
    }
    o.push('\n');
    for p in ps {
        let twips = (p.hanging_indent * 1440.0).round() as i32;
        let line = (p.line_spacing.max(1.0) * 240.0).round() as i32;
        o.push_str(&format!("{{\\pard\\li{twips}\\fi-{twips}\\sl{line}\\slmult1 {}\\par}}\n", rtf_runs(&p.runs)));
    }
    o.push('}');
    o
}

pub fn markdown_runs(runs: &[Run]) -> String {
    let mut o = String::new();
    for r in runs {
        let esc = r.text.replace('*', "\\*").replace('_', "\\_");
        let mut t = if r.italic && !esc.trim().is_empty() {
            // Keep surrounding spaces outside the emphasis markers.
            let lead = &esc[..esc.len() - esc.trim_start().len()];
            let trail = &esc[esc.trim_end().len()..];
            format!("{lead}*{}*{trail}", esc.trim())
        } else {
            esc
        };
        if r.bold && !t.trim().is_empty() {
            let lead = t[..t.len() - t.trim_start().len()].to_string();
            let trail = t[t.trim_end().len()..].to_string();
            t = format!("{lead}**{}**{trail}", t.trim());
        }
        if r.sup {
            t = format!("<sup>{t}</sup>");
        }
        if let Some(l) = &r.link {
            if l == &r.text {
                t = format!("<{l}>");
            } else {
                t = format!("[{t}]({l})");
            }
        }
        o.push_str(&t);
    }
    o
}

pub fn markdown_paragraphs(ps: &[Paragraph]) -> String {
    ps.iter().map(|p| markdown_runs(&p.runs)).collect::<Vec<_>>().join("\n\n") + "\n"
}

// ---- DOCX --------------------------------------------------------------------------------------

/// A block of a DOCX document.
pub enum DocBlock {
    Heading(u8, String),
    Para(Vec<Run>),
    /// Bibliography paragraph with hanging indent and spacing.
    RefPara(Paragraph),
    Quote(Vec<Run>),
}

fn docx_runs(runs: &[Run], rels: &mut Vec<String>) -> String {
    let mut o = String::new();
    for r in runs {
        let mut rpr = String::new();
        if r.bold {
            rpr.push_str("<w:b/>");
        }
        if r.italic {
            rpr.push_str("<w:i/>");
        }
        if r.small_caps {
            rpr.push_str("<w:smallCaps/>");
        }
        if r.sup {
            rpr.push_str("<w:vertAlign w:val=\"superscript\"/>");
        }
        if r.sub {
            rpr.push_str("<w:vertAlign w:val=\"subscript\"/>");
        }
        let mut text_xml = String::new();
        for (i, line) in r.text.split('\n').enumerate() {
            if i > 0 {
                text_xml.push_str("<w:br/>");
            }
            text_xml.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", xml_escape(line)));
        }
        let run = format!("<w:r>{}{}</w:r>", if rpr.is_empty() { String::new() } else { format!("<w:rPr>{rpr}</w:rPr>") }, text_xml);
        if let Some(l) = &r.link {
            rels.push(l.clone());
            o.push_str(&format!("<w:hyperlink r:id=\"rIdL{}\">{}</w:hyperlink>", rels.len(), run));
        } else {
            o.push_str(&run);
        }
    }
    o
}

/// Build a DOCX file from blocks. `font` sets the document body font; None leaves the default.
pub fn docx(blocks: &[DocBlock], font: Option<&str>) -> crate::Result<Vec<u8>> {
    let mut rels = Vec::new();
    let mut body = String::new();
    for b in blocks {
        match b {
            DocBlock::Heading(level, text) => {
                body.push_str(&format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"Heading{level}\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                    xml_escape(text)
                ));
            }
            DocBlock::Para(runs) => {
                body.push_str(&format!("<w:p>{}</w:p>", docx_runs(runs, &mut rels)));
            }
            DocBlock::Quote(runs) => {
                body.push_str(&format!("<w:p><w:pPr><w:ind w:left=\"720\"/></w:pPr>{}</w:p>", docx_runs(runs, &mut rels)));
            }
            DocBlock::RefPara(p) => {
                let tw = (p.hanging_indent * 1440.0).round() as i32;
                let line = (p.line_spacing.max(1.0) * 240.0).round() as i32;
                body.push_str(&format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"Bibliography\"/><w:spacing w:after=\"0\" w:line=\"{line}\" w:lineRule=\"auto\"/><w:ind w:left=\"{tw}\" w:hanging=\"{tw}\"/></w:pPr>{}</w:p>",
                    docx_runs(&p.runs, &mut rels)
                ));
            }
        }
    }
    let doc = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let font_xml = font
        .map(|f| format!("<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:cs=\"{0}\" w:eastAsia=\"{0}\"/>", xml_escape(f)))
        .unwrap_or_default();
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr>{font_xml}<w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/><w:lang w:val=\"en-US\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"259\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"32\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"200\" w:after=\"100\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"28\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:b/><w:i/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Bibliography\"><w:name w:val=\"Bibliography\"/><w:basedOn w:val=\"Normal\"/></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style></w:styles>"
    );
    let mut doc_rels = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>");
    for (i, l) in rels.iter().enumerate() {
        doc_rels.push_str(&format!(
            "<Relationship Id=\"rIdL{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            i + 1,
            xml_escape(l)
        ));
    }
    doc_rels.push_str("</Relationships>");
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>";
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            ("[Content_Types].xml", content_types.to_string()),
            ("_rels/.rels", root_rels.to_string()),
            ("word/document.xml", doc),
            ("word/styles.xml", styles),
            ("word/_rels/document.xml.rels", doc_rels),
        ] {
            z.start_file(name, opt)?;
            z.write_all(data.as_bytes())?;
        }
        z.finish()?;
    }
    Ok(buf.into_inner())
}

/// Full RTF document from blocks (used for note and bibliography file exports).
pub fn rtf_document(blocks: &[DocBlock], font: Option<&str>) -> String {
    let mut o = String::from("{\\rtf1\\ansi\\uc1");
    o.push_str(&format!("{{\\fonttbl{{\\f0 {};}}}}\\f0\\fs24\n", rtf_escape(font.unwrap_or("Times New Roman"))));
    for b in blocks {
        match b {
            DocBlock::Heading(level, t) => {
                let size = match level { 1 => 32, 2 => 28, _ => 24 };
                o.push_str(&format!("{{\\pard\\sb240\\sa120\\b\\fs{size} {}\\par}}\n", rtf_escape(t)));
            }
            DocBlock::Para(r) => o.push_str(&format!("{{\\pard\\sa160 {}\\par}}\n", rtf_runs(r))),
            DocBlock::Quote(r) => o.push_str(&format!("{{\\pard\\li720\\sa160 {}\\par}}\n", rtf_runs(r))),
            DocBlock::RefPara(p) => {
                let tw = (p.hanging_indent * 1440.0).round() as i32;
                let line = (p.line_spacing.max(1.0) * 240.0).round() as i32;
                o.push_str(&format!("{{\\pard\\li{tw}\\fi-{tw}\\sl{line}\\slmult1 {}\\par}}\n", rtf_runs(&p.runs)));
            }
        }
    }
    o.push('}');
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cf_html_offsets() {
        let s = cf_html("<i>x</i>");
        let sf: usize = s.lines().find(|l| l.starts_with("StartFragment:")).unwrap()[14..].parse().unwrap();
        let ef: usize = s.lines().find(|l| l.starts_with("EndFragment:")).unwrap()[12..].parse().unwrap();
        assert_eq!(&s[sf..ef], "<i>x</i>");
    }
    #[test]
    fn rtf_unicode() {
        assert_eq!(rtf_escape("é"), "\\u233?");
        assert!(rtf_escape("𝔄").contains("\\u-"));
    }
}
