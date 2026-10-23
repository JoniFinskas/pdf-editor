//! Local Windows dictionaries. Public ranges are UTF-8 byte offsets.
use crate::{Result, err};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellingIssue {
    pub range: Range<usize>,
    pub word: String,
    pub suggestions: Vec<String>,
}

pub fn replace_spelling(text: &mut String, issue: &SpellingIssue, replacement: &str) -> Result<()> {
    if text.get(issue.range.clone()) != Some(issue.word.as_str()) {
        return Err(err("Spelling result is out of date"));
    }
    if replacement.len() > 1024 || replacement.contains('\0') {
        return Err(err("Invalid spelling replacement"));
    }
    text.replace_range(issue.range.clone(), replacement);
    Ok(())
}

#[cfg(any(windows, test))]
fn utf16_range(text: &str, start: u32, length: u32) -> Option<Range<usize>> {
    let end = start.checked_add(length)?;
    let mut units = 0;
    let mut first = None;
    for (byte, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        if units == start {
            first = Some(byte);
        }
        if units == end {
            return first.map(|first| first..byte);
        }
        units += ch.len_utf16() as u32;
    }
    None
}

pub fn check_spelling(text: &str, language: &str) -> Result<Vec<SpellingIssue>> {
    if !matches!(language, "fi-FI" | "en-US") {
        return Err(err("Choose Finnish or English spelling"));
    }
    if text.len() > 65536 || text.contains('\0') {
        return Err(err("Text exceeds spelling limits"));
    }
    native(text, language)
}

#[cfg(not(windows))]
fn native(_: &str, _: &str) -> Result<Vec<SpellingIssue>> {
    Err(err("Spell checking requires Windows"))
}

#[cfg(windows)]
fn native(text: &str, language: &str) -> Result<Vec<SpellingIssue>> {
    use windows::{
        Win32::{
            Globalization::{ISpellCheckerFactory, SpellCheckerFactory},
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
        },
        core::{HSTRING, PWSTR},
    };
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    let failure = |e: windows::core::Error| err(&format!("Windows spell checking: {e}"));
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(failure)?;
        let _apartment = Apartment;
        let factory: ISpellCheckerFactory =
            CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER).map_err(failure)?;
        let language = HSTRING::from(language);
        if !factory.IsSupported(&language).map_err(failure)?.as_bool() {
            return Err(err(
                "This Windows spelling dictionary is not installed. Choose another language or install it in Windows language settings.",
            ));
        }
        let checker = factory.CreateSpellChecker(&language).map_err(failure)?;
        let errors = checker.Check(&HSTRING::from(text)).map_err(failure)?;
        let mut result = Vec::new();
        for _ in 0..256 {
            let mut item = None;
            errors.Next(&mut item).ok().map_err(failure)?;
            let Some(item) = item else {
                break;
            };
            let range = utf16_range(
                text,
                item.StartIndex().map_err(failure)?,
                item.Length().map_err(failure)?,
            )
            .ok_or_else(|| err("Invalid spelling range"))?;
            let word = text[range.clone()].to_string();
            let mut suggestions = Vec::new();
            if let Ok(values) = checker.Suggest(&HSTRING::from(&word)) {
                for _ in 0..8 {
                    let mut value = [PWSTR::null()];
                    let mut fetched = 0;
                    values
                        .Next(&mut value, Some(&mut fetched))
                        .ok()
                        .map_err(failure)?;
                    if fetched == 0 {
                        break;
                    }
                    let suggestion = value[0].to_string();
                    CoTaskMemFree(Some(value[0].0.cast()));
                    if let Ok(suggestion) = suggestion {
                        suggestions.push(suggestion);
                    }
                }
            }
            result.push(SpellingIssue {
                range,
                word,
                suggestions,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_ranges_and_stale_replacement() {
        let mut text = "😀 ää wrng end".to_string();
        assert_eq!(utf16_range(&text, 6, 4), Some(10..14));
        assert_eq!(utf16_range(&text, 1, 1), None);
        let issue = SpellingIssue {
            range: 10..14,
            word: "wrng".into(),
            suggestions: vec![],
        };
        replace_spelling(&mut text, &issue, "wrong").unwrap();
        assert_eq!(text, "😀 ää wrong end");
        assert!(replace_spelling(&mut text, &issue, "x").is_err());
        assert!(check_spelling("abc\0def", "en-US").is_err());
        assert!(check_spelling("text", "unknown").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn installed_windows_dictionaries() {
        std::thread::spawn(|| {
            for language in ["en-US", "fi-FI"] {
                match check_spelling("qzxqzxqzx hello maailma", language) {
                    Ok(issues) => {
                        assert!(issues.iter().any(|i| i.word == "qzxqzxqzx"));
                        eprintln!("{language}: installed dictionary verified");
                    }
                    Err(error) => {
                        assert!(error.to_string().contains("not installed"), "{error}");
                        eprintln!("{language}: dictionary unavailable; error verified");
                    }
                }
            }
        })
        .join()
        .unwrap();
    }
}
