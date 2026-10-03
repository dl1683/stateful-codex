//! Where a message quotes someone else's words, so a quoted or relayed preference is never
//! taken as the user's own rule.
//!
//! A quotation is a span between double quotes, curly double quotes, or single quotes that
//! open at a word start. Quotations may cross sentences and lines; an unclosed double quote
//! runs to the end. A single quote closes at the end of a word, except a plural possessive
//! ("users' docs"); a single quote left unclosed is ambiguous, and when the message reports
//! speech everything after it is treated as possibly relayed.
//!
//! The rule is deliberately conservative: when a message attributes words to someone
//! (a speech verb outside every quotation, such as "wrote" or "said"), no clause that
//! begins inside or contains a quotation is the user's rule. A user who quotes a term while
//! also reporting speech can restate the rule without quotes; a stranger's preference must
//! never be applied as theirs.

/// Past-tense verbs that report someone's words. Present forms ("a line that says ...",
/// "if it asks ...") are how instructions describe output, so they do not count.
const SPEECH_VERBS: &[&str] = &[
    "wrote",
    "said",
    "asked",
    "told",
    "mentioned",
    "posted",
    "replied",
    "commented",
    "suggested",
    "noted",
    "quoted",
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
    /// Sorted by opening offset and non-overlapping.
    spans: Vec<Span>,
    /// Where an unclosed single quote opened, if any.
    ambiguous_from: Option<usize>,
    attributes_speech: bool,
}

impl<'a> Quotations<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let (spans, ambiguous_from) = spans(text);
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
            ambiguous_from,
            attributes_speech,
        }
    }

    /// Whether the part from `start` to `end` (byte offsets) may be someone else's words:
    /// it begins inside a quotation, or the message attributes speech and the part holds a
    /// quotation.
    pub(crate) fn relays(&self, start: usize, end: usize) -> bool {
        // Spans are sorted and disjoint: the last one opening before `start` is the only one
        // that can contain it, and the next one tells whether any opens inside the part.
        let first_at_or_after = self.spans.partition_point(|span| span.open < start);
        let inside = first_at_or_after
            .checked_sub(1)
            .is_some_and(|index| start < self.spans[index].close);
        let holds = self
            .spans
            .get(first_at_or_after)
            .is_some_and(|span| span.open < end);
        let after_ambiguous = self.ambiguous_from.is_some_and(|open| open < end);
        inside || (self.attributes_speech && (holds || after_ambiguous))
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

fn spans(text: &str) -> (Vec<Span>, Option<usize>) {
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut open: Option<(usize, char)> = None;
    for (index, &(offset, character)) in characters.iter().enumerate() {
        let before = index.checked_sub(1).map(|previous| characters[previous].1);
        let after = characters.get(index + 1).map(|next| next.1);
        let end = offset + character.len_utf8();
        let closes = match open {
            Some((_, '"')) => character == '"',
            Some((_, '\u{201c}')) => character == '\u{201d}',
            Some((_, '\'' | '\u{2018}')) => {
                let word_end = matches!(character, '\'' | '\u{2019}')
                    && before.is_some_and(|before| !before.is_whitespace())
                    && !after.is_some_and(char::is_alphanumeric);
                // "users' docs": a plural possessive, not the end of the quotation.
                let possessive = before == Some('s') && after.is_some_and(char::is_whitespace);
                word_end && !possessive
            }
            Some(_) => false,
            None => false,
        };
        if closes {
            if let Some((start, _)) = open.take() {
                spans.push(Span {
                    open: start,
                    close: end,
                });
            }
            continue;
        }
        if open.is_none() {
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
    match open {
        Some((start, '"' | '\u{201c}')) => {
            spans.push(Span {
                open: start,
                close: text.len(),
            });
            (spans, None)
        }
        Some((start, _)) => (spans, Some(start)),
        None => (spans, None),
    }
}

#[cfg(test)]
#[path = "quotation_tests.rs"]
mod tests;
