//! System clipboard: Unicode text, HTML and RTF written together from one rendering. The
//! write is confirmed by reading the text back before a copy event is recorded.

#[cfg(windows)]
pub fn write(text: &str, html_fragment: Option<&str>, rtf: Option<&str>) -> Result<(), String> {
    use clipboard_win::{options::NoClear, raw, Clipboard};
    let _clip = Clipboard::new_attempts(20).map_err(|e| format!("The clipboard is in use by another program ({e}). Try again."))?;
    raw::empty().map_err(|e| format!("The clipboard could not be cleared ({e})."))?;
    raw::set_string_with(text, NoClear).map_err(|e| format!("Text could not be placed on the clipboard ({e})."))?;
    if let Some(h) = html_fragment {
        if let Some(fmt) = clipboard_win::register_format("HTML Format") {
            let payload = prm_core::render::serialize::cf_html(h);
            let mut bytes = payload.into_bytes();
            bytes.push(0);
            raw::set_without_clear(fmt.get(), &bytes).map_err(|e| format!("HTML could not be placed on the clipboard ({e})."))?;
        }
    }
    if let Some(r) = rtf {
        if let Some(fmt) = clipboard_win::register_format("Rich Text Format") {
            let mut bytes = r.as_bytes().to_vec();
            bytes.push(0);
            raw::set_without_clear(fmt.get(), &bytes).map_err(|e| format!("Rich text could not be placed on the clipboard ({e})."))?;
        }
    }
    drop(_clip);
    // Confirm the write.
    let back: String = clipboard_win::get_clipboard_string().map_err(|e| format!("The clipboard write could not be confirmed ({e})."))?;
    if back.replace("\r\n", "\n") != text.replace("\r\n", "\n") {
        return Err("Another program changed the clipboard during the copy. Try again.".into());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn write(_text: &str, _html: Option<&str>, _rtf: Option<&str>) -> Result<(), String> {
    Err("Clipboard writing is only implemented for Windows in this build.".into())
}
