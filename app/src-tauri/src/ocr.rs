//! Local text recognition (OCR) for scanned PDFs, using the recognizer built into Windows.
//! Nothing leaves the computer. The window renders a page to an image; this module returns its
//! text, with each word's position so a marked area can be matched to the words inside it.
//! The PDF file itself is never changed.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct OcrWord {
    pub t: String,
    /// Position on the page in 0..1, origin top-left.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct OcrPage {
    /// Recognized text, one line per recognized line.
    pub text: String,
    pub words: Vec<OcrWord>,
    /// 0..1 estimate of how readable the result is (the recognizer reports no confidence).
    pub quality: f32,
    pub lang: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrLanguage {
    pub tag: String,
    pub name: String,
}

/// Share of recognized words that look like words or numbers. Garbled scans produce many
/// tokens mixing letters, digits and symbols.
pub fn quality(words: &[OcrWord]) -> f32 {
    prm_core::library::word_quality(&words.iter().map(|w| w.t.clone()).collect::<Vec<_>>())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::HSTRING;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    fn e(x: windows::core::Error) -> String {
        x.message().to_string()
    }

    fn init() {
        // Each call runs on its own thread; join the multithreaded apartment there.
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
    }

    pub fn languages() -> Result<Vec<OcrLanguage>, String> {
        init();
        let mut out = vec![];
        for l in OcrEngine::AvailableRecognizerLanguages().map_err(e)? {
            out.push(OcrLanguage { tag: l.LanguageTag().map_err(e)?.to_string(), name: l.DisplayName().map_err(e)?.to_string() });
        }
        Ok(out)
    }

    pub fn recognize(image: &[u8], lang: &str) -> Result<OcrPage, String> {
        init();
        let stream = InMemoryRandomAccessStream::new().map_err(e)?;
        let writer = DataWriter::CreateDataWriter(&stream).map_err(e)?;
        writer.WriteBytes(image).map_err(e)?;
        writer.StoreAsync().map_err(e)?.join().map_err(e)?;
        writer.FlushAsync().map_err(e)?.join().map_err(e)?;
        writer.DetachStream().map_err(e)?;
        stream.Seek(0).map_err(e)?;
        let decoder = BitmapDecoder::CreateAsync(&stream).map_err(e)?.join().map_err(e)?;
        let mut bmp: SoftwareBitmap = decoder.GetSoftwareBitmapAsync().map_err(e)?.join().map_err(e)?;
        if bmp.BitmapPixelFormat().map_err(e)? != BitmapPixelFormat::Bgra8 && bmp.BitmapPixelFormat().map_err(e)? != BitmapPixelFormat::Gray8 {
            bmp = SoftwareBitmap::Convert(&bmp, BitmapPixelFormat::Bgra8).map_err(e)?;
        }
        let (w, h) = (bmp.PixelWidth().map_err(e)? as f32, bmp.PixelHeight().map_err(e)? as f32);
        let max = OcrEngine::MaxImageDimension().map_err(e)? as f32;
        if w > max || h > max {
            return Err(format!("The page image is larger than the recognizer accepts ({max} pixels)."));
        }
        let engine = if lang.is_empty() {
            OcrEngine::TryCreateFromUserProfileLanguages().map_err(e)?
        } else {
            let l = Language::CreateLanguage(&HSTRING::from(lang)).map_err(e)?;
            if !OcrEngine::IsLanguageSupported(&l).map_err(e)? {
                return Err(format!("Text recognition for {lang} is not installed. Add the language in Windows Settings › Time and language › Language, including its optical character recognition feature."));
            }
            OcrEngine::TryCreateFromLanguage(&l).map_err(e)?
        };
        let used = engine.RecognizerLanguage().and_then(|l| l.LanguageTag()).map(|t| t.to_string()).unwrap_or_default();
        let res = engine.RecognizeAsync(&bmp).map_err(e)?.join().map_err(e)?;
        let mut page = OcrPage { lang: used, ..Default::default() };
        let mut lines = vec![];
        for line in res.Lines().map_err(e)? {
            lines.push(line.Text().map_err(e)?.to_string());
            for word in line.Words().map_err(e)? {
                let r = word.BoundingRect().map_err(e)?;
                page.words.push(OcrWord { t: word.Text().map_err(e)?.to_string(), x: r.X / w, y: r.Y / h, w: r.Width / w, h: r.Height / h });
            }
        }
        page.text = lines.join("\n");
        page.quality = quality(&page.words);
        Ok(page)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn languages() -> Result<Vec<OcrLanguage>, String> {
        Ok(vec![])
    }
    pub fn recognize(_image: &[u8], _lang: &str) -> Result<OcrPage, String> {
        Err("Text recognition is available on Windows only.".into())
    }
}

/// Run on a fresh thread: the recognizer's blocking waits must not run on the window's threads.
fn on_thread<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    std::thread::spawn(f).join().map_err(|_| "Text recognition stopped unexpectedly.".to_string())?
}

pub fn languages() -> Result<Vec<OcrLanguage>, String> {
    on_thread(imp::languages)
}

pub fn recognize(image: Vec<u8>, lang: String) -> Result<OcrPage, String> {
    on_thread(move || imp::recognize(&image, &lang))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ws(s: &str) -> Vec<OcrWord> {
        s.split_whitespace().map(|t| OcrWord { t: t.into(), ..Default::default() }).collect()
    }
    #[test]
    fn quality_separates_clean_and_garbled() {
        assert!(quality(&ws("Teaching with reflection improves learning in 2020 (Lee, p. 12).")) > 0.9);
        assert!(quality(&ws("T3a#h ng w!th re7l ct1on i~pr ve5 le@rn")) < 0.5);
    }
}
