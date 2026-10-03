//! Where a message quotes someone else's words, so a quoted or relayed preference is never
//! taken as the user's own rule.
//!
//! A quotation is a span between double quotes, curly double quotes, or single quotes that
//! open at a word start. Double quotes may cross sentences and lines (an unclosed one runs to
//! the end); a single quote closes before punctuation, at the end of the text, or before a
//! capitalized word, and otherwise at the end of its line, so a possessive ("users' docs")
//! does not end it.
//!
//! The rule is deliberately conservative: when a message attributes words to someone
//! (a speech verb outside every quotation, such as "wrote" or "said"), no clause that
//! begins inside or contains a quotation is the user's rule. A user who quotes a term while
//! also reporting speech can restate the rule without quotes; a stranger's preference must
//! never be applied as theirs.

/// Words that attribute words to someone.
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
    "quote",
    "quoted",
    "quoting",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    /// Byte offset of the opening quote.
    open: usize,
    /// Byte offset just past the closing quote.
    close: usize,
}

/// The quotations of one message, computed once.
pub(crate) struct Quotations<'a> {
    text: &'a str,
    spans: Vec<Span>,
    attributes_speech: bool,
}

impl<'a> Quotations<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let spans = spans(text);
        let mut outside = String::with_capacity(text.len());
        let mut position = 0;
        for span in &spans {
            outside.push_str(&text[position..span.open]);
            outside.push(' ');
            position = span.close;
        }
        outside.push_str(&text[position.min(text.len())..]);
        let attributes_speech = outside
            .split(|character: char| !character.is_alphanumeric())
            .any(|word| SPEECH_VERBS.contains(&word.to_lowercase().as_str()));
        Self {
            text,
            spans,
            attributes_speech,
        }
    }

    /// Whether the part from `start` to `end` (byte offsets) may be someone else's words:
    /// it begins inside a quotation, or the message attributes speech and the part holds a
    /// quotation.
    pub(crate) fn relays(&self, start: usize, end: usize) -> bool {
        self.spans.iter().any(|span| {
            (span.open < start && start < span.close)
                || (self.attributes_speech && span.open >= start && span.open < end)
        })
    }

    /// Like `relays`, for a clause found in the message (judged alone if not found).
    pub(crate) fn relays_clause(&self, clause: &str) -> bool {
        match self.text.find(clause) {
            Some(start) => self.relays(start, start + clause.len()),
            None => {
                let alone = Quotations::new(clause);
                alone.relays(0, clause.len())
            }
        }
    }
}

fn spans(text: &str) -> Vec<Span> {
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut open: Option<(usize, char)> = None;
    for (index, &(offset, character)) in characters.iter().enumerate() {
        let before = index.checked_sub(1).map(|previous| characters[previous].1);
        let after = characters.get(index + 1).map(|next| next.1);
        let after_next = characters.get(index + 2).map(|next| next.1);
        let end = offset + character.len_utf8();
        match open {
            Some((start, '"')) if character == '"' => {
                spans.push(Span {
                    open: start,
                    close: end,
                });
                open = None;
            }
            Some((start, '\u{201c}')) if character == '\u{201d}' => {
                spans.push(Span {
                    open: start,
                    close: end,
                });
                open = None;
            }
            Some((start, '\'' | '\u{2018}')) => {
                let candidate = matches!(character, '\'' | '\u{2019}')
                    && before.is_some_and(|before| !before.is_whitespace())
                    && !after.is_some_and(char::is_alphanumeric);
                let closes = candidate
                    && match after {
                        None => true,
                        Some(next) if next.is_ascii_punctuation() => true,
                        Some(next) if next.is_whitespace() => {
                            after_next.is_none_or(|word| !word.is_lowercase())
                        }
                        Some(_) => false,
                    };
                if closes {
                    spans.push(Span {
                        open: start,
                        close: end,
                    });
                    open = None;
                } else if character == '\n' {
                    // An unmatched single quote ends with its line.
                    spans.push(Span {
                        open: start,
                        close: offset,
                    });
                    open = None;
                }
            }
            Some(_) => {}
            None => {
                let opens = match character {
                    '"' | '\u{201c}' => true,
                    '\'' | '\u{2018}' => {
                        !before.is_some_and(char::is_alphanumeric)
                            && after.is_some_and(char::is_alphanumeric)
                    }
                    _ => false,
                };
                if opens {
                    open = Some((offset, character));
                }
            }
        }
    }
    if let Some((start, _)) = open {
        spans.push(Span {
            open: start,
            close: text.len(),
        });
    }
    spans
}

#[cfg(test)]
#[path = "quotation_tests.rs"]
mod tests;
