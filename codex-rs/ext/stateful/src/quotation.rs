//! Where a message quotes someone else's words, so a quoted or relayed preference is never
//! taken as the user's own rule.
//!
//! A quotation is a span between double quotes, curly double quotes, or single quotes that
//! open and close at word boundaries (an apostrophe inside a word, as in "I'm", is not a
//! quote). Spans may cross sentences and lines; an unclosed quote runs to the end.

/// Words that introduce what someone said or wrote.
const SPEECH_VERBS: &[&str] = &[
    "wrote",
    "writes",
    "said",
    "says",
    "asked",
    "asks",
    "told",
    "tells",
    "mentioned",
    "posted",
    "replied",
    "commented",
    "suggested",
    "noted",
];

/// A quoted phrase this long is someone's statement; a shorter one names a term or UI text
/// ("never use the word \"simply\"", "start with 'Next:'").
const MIN_STATEMENT_WORDS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    /// Byte offset of the opening quote.
    open: usize,
    /// Byte offset just past the closing quote (or the end of the text).
    close: usize,
    words: usize,
}

fn spans(text: &str) -> Vec<Span> {
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut open: Option<(usize, char)> = None;
    for (index, &(offset, character)) in characters.iter().enumerate() {
        let before = index.checked_sub(1).map(|previous| characters[previous].1);
        let after = characters.get(index + 1).map(|next| next.1);
        let word_before = before.is_some_and(char::is_alphanumeric);
        let word_after = after.is_some_and(char::is_alphanumeric);
        let closing = match (open, character) {
            (Some((_, '"')), '"') | (Some((_, '\u{201c}')), '\u{201d}') => true,
            (Some((_, '\'' | '\u{2018}')), '\'' | '\u{2019}') => {
                before.is_some_and(|before| !before.is_whitespace()) && !word_after
            }
            _ => false,
        };
        if closing {
            if let Some((start, _)) = open.take() {
                spans.push(span(text, start, offset + character.len_utf8()));
            }
            continue;
        }
        if open.is_none() {
            let opens = match character {
                '"' | '\u{201c}' => true,
                '\'' | '\u{2018}' => !word_before && word_after,
                _ => false,
            };
            if opens {
                open = Some((offset, character));
            }
        }
    }
    if let Some((start, _)) = open {
        spans.push(span(text, start, text.len()));
    }
    spans
}

fn span(text: &str, open: usize, close: usize) -> Span {
    Span {
        open,
        close,
        words: text[open..close].split_whitespace().count(),
    }
}

/// Whether the part of `text` from `start` to `end` (byte offsets) is someone else's words:
/// it begins inside a quotation, or it holds a quoted statement and names who said it
/// (a speech verb, before or after the quote).
pub(crate) fn is_relayed(text: &str, start: usize, end: usize) -> bool {
    let spans = spans(text);
    if spans
        .iter()
        .any(|span| span.open < start && start < span.close)
    {
        return true;
    }
    let statement = spans
        .iter()
        .any(|span| span.open >= start && span.open < end && span.words >= MIN_STATEMENT_WORDS);
    statement && mentions_speech(&text[start..end])
}

/// Whether `clause`, found in `text`, is someone else's words; a clause not found in the
/// text is judged on its own.
pub(crate) fn is_relayed_in(text: &str, clause: &str) -> bool {
    match text.find(clause) {
        Some(start) => is_relayed(text, start, start + clause.len()),
        None => is_relayed(clause, 0, clause.len()),
    }
}

fn mentions_speech(text: &str) -> bool {
    text.split(|character: char| !character.is_alphanumeric())
        .any(|word| SPEECH_VERBS.contains(&word.to_lowercase().as_str()))
}

#[cfg(test)]
#[path = "quotation_tests.rs"]
mod tests;
