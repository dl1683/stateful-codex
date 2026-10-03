//! Where a message quotes someone else's words, so a quoted or relayed preference is never
//! taken as the user's own rule.
//!
//! A quotation is a span between double quotes, curly double quotes, or single quotes that
//! open at a word start. Quotations may cross sentences and lines; an unclosed double quote
//! runs to the end. A single quote closes at the end of a word, except a plural possessive
//! ("users' docs"); a single quote left unclosed is ambiguous, and when the message reports
//! speech everything after it is treated as possibly relayed.
//!
//! Attribution is local to the quotation: a quotation is someone else's words when its own
//! sentence has a speech verb outside every quotation ("Priya wrote: ..."), when the sentence
//! before it introduces it ("She wrote:" ending the previous sentence or line), or when the
//! next sentence refers back to it ("That is what she said."). A user's own rule may quote a
//! term ('Next:') even when another sentence of the same message relays a colleague. A
//! clause that begins inside a quotation is never the user's rule.

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

/// Words that open a sentence referring back to the quotation before it.
const BACK_REFERENCES: &[&str] = &[
    "that", "this", "those", "these", "which", "so", "she", "he", "they", "it",
];

/// One sentence of a message, with quotations kept whole.
#[derive(Clone, Copy, Debug)]
struct Sentence {
    /// Byte offset just past its end.
    end: usize,
    /// A speech verb appears in it outside every quotation.
    reports_speech: bool,
    /// Its text outside quotations ends with a colon ("She wrote:").
    introduces: bool,
    /// Its first word refers back to what came before ("That is what she wrote.").
    refers_back: bool,
}

/// The quotations of one message, computed once.
pub(crate) struct Quotations<'a> {
    text: &'a str,
    /// Sorted by opening offset and non-overlapping.
    spans: Vec<Span>,
    /// Whether the span at the same index is attributed to someone else.
    attributed: Vec<bool>,
    /// Where an unclosed single quote opened, if any.
    ambiguous_from: Option<usize>,
    /// A speech verb appears somewhere outside quotations.
    attributes_speech: bool,
}

impl<'a> Quotations<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let (spans, ambiguous_from) = spans(text);
        let sentences = sentences(text, &spans);
        let attributes_speech = sentences.iter().any(|sentence| sentence.reports_speech);
        let attributed = spans
            .iter()
            .map(|span| {
                let index = sentences.partition_point(|sentence| sentence.end <= span.open);
                let own = sentences
                    .get(index)
                    .is_some_and(|sentence| sentence.reports_speech);
                let introduced = index
                    .checked_sub(1)
                    .and_then(|previous| sentences.get(previous))
                    .is_some_and(|previous| previous.introduces && previous.reports_speech);
                let referred_back = sentences
                    .get(index + 1)
                    .is_some_and(|next| next.refers_back && next.reports_speech);
                // A quoted instruction ("\"From now on, never commit.\"") is someone's
                // words even unattributed; a quoted term ('Next:') is not.
                let instruction = crate::user_rules::has_standing_marker(
                    &crate::user_rules::normalize(&text[span.open..span.close]),
                );
                own || introduced || referred_back || instruction
            })
            .collect();
        Self {
            text,
            spans,
            attributed,
            ambiguous_from,
            attributes_speech,
        }
    }

    /// Whether the part from `start` to `end` (byte offsets) may be someone else's words:
    /// it begins inside a quotation, it holds a quotation that is attributed to someone else
    /// or is itself an instruction, or an ambiguous single quote opened before its end in a
    /// message that reports speech.
    pub(crate) fn relays(&self, start: usize, end: usize) -> bool {
        // Spans are sorted and disjoint: the last one opening before `start` is the only one
        // that can contain it.
        let first_at_or_after = self.spans.partition_point(|span| span.open < start);
        let inside = first_at_or_after
            .checked_sub(1)
            .is_some_and(|index| start < self.spans[index].close);
        let holds_attributed = self.spans[first_at_or_after..]
            .iter()
            .zip(&self.attributed[first_at_or_after..])
            .take_while(|(span, _)| span.open < end)
            .any(|(_, attributed)| *attributed);
        let after_ambiguous = self.ambiguous_from.is_some_and(|open| open < end);
        inside || holds_attributed || (self.attributes_speech && after_ambiguous)
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

/// Splits `text` into sentences without splitting a quotation: a sentence ends at a newline,
/// at `.`, `!` or `?` followed by whitespace or the end, or after a quotation whose last
/// character inside is such a terminal and which is followed by whitespace.
fn sentences(text: &str, spans: &[Span]) -> Vec<Sentence> {
    let bytes = text.as_bytes();
    let mut boundaries = Vec::new();
    let mut index = 0;
    let mut next_span = 0;
    while index < bytes.len() {
        if let Some(span) = spans.get(next_span)
            && span.open == index
        {
            next_span += 1;
            let last_inside = text[span.open..span.close].chars().rev().nth(1);
            let followed_by_space = bytes.get(span.close).is_none_or(u8::is_ascii_whitespace);
            if matches!(last_inside, Some('.' | '!' | '?')) && followed_by_space {
                boundaries.push(span.close);
            }
            index = span.close.max(index + 1);
            continue;
        }
        let byte = bytes[index];
        if byte == b'\n'
            || (matches!(byte, b'.' | b'!' | b'?')
                && bytes.get(index + 1).is_none_or(u8::is_ascii_whitespace))
        {
            boundaries.push(index + 1);
        }
        index += 1;
    }
    let mut sentences = Vec::new();
    let mut start = 0;
    for end in boundaries.into_iter().chain(std::iter::once(text.len())) {
        if end <= start {
            continue;
        }
        let outside = outside_quotations(text, spans, start, end);
        let words = outside
            .split(|character: char| !character.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        sentences.push(Sentence {
            end,
            reports_speech: words
                .iter()
                .any(|word| SPEECH_VERBS.contains(&word.as_str())),
            introduces: outside.trim_end().ends_with(':'),
            refers_back: words
                .first()
                .is_some_and(|word| BACK_REFERENCES.contains(&word.as_str())),
        });
        start = end;
    }
    sentences
}

/// `text[start..end]` with each quotation replaced by a space.
fn outside_quotations(text: &str, spans: &[Span], start: usize, end: usize) -> String {
    let mut outside = String::with_capacity(end - start);
    let mut position = start;
    for span in spans
        .iter()
        .filter(|span| span.close > start && span.open < end)
    {
        if span.open > position {
            outside.push_str(&text[position..span.open]);
        }
        outside.push(' ');
        position = position.max(span.close);
    }
    if position < end {
        outside.push_str(&text[position..end]);
    }
    outside
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
