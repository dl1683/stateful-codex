//! Versioned lexical identity. Exact display/source bytes are never normalized in storage.
use std::collections::HashMap;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

pub const CAPTURE_NORMALIZER_VERSION: &str = "nfc-full-fold-17.0.0-v1";

/// NFC, full default Unicode case folding, then Unicode whitespace collapse.
pub fn canonical_capture_words(words: &str) -> String {
    static FOLD: OnceLock<HashMap<char, String>> = OnceLock::new();
    let fold = FOLD.get_or_init(|| {
        include_str!("../data/CaseFolding-17.0.0.txt")
            .lines()
            .filter(|line| !line.starts_with('#') && line.contains(';'))
            .filter_map(|line| {
                let mut fields = line.split(';').map(str::trim);
                let key = char::from_u32(u32::from_str_radix(fields.next()?, 16).ok()?)?;
                let status = fields.next()?;
                let value = fields.next()?;
                if !matches!(status, "C" | "F") {
                    return None;
                }
                let value = value
                    .split_whitespace()
                    .map(|hex| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
                    .collect::<Option<String>>()?;
                Some((key, value))
            })
            .collect()
    });
    let mut folded = String::new();
    for ch in words.nfc() {
        if let Some(value) = fold.get(&ch) {
            folded.push_str(value);
        } else {
            folded.push(ch);
        }
    }
    folded
        .nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Punctuation separates tokens; letters, digits, combining marks and conditions remain.
pub fn retirement_capture_words(words: &str) -> String {
    let words = canonical_capture_words(words);
    words
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || unicode_normalization::char::is_combining_mark(ch) {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
