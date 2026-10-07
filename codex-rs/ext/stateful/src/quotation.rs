//! Where a message quotes someone else's words, so a quoted or relayed preference is never
//! taken as the user's own rule.
//!
//! A quotation is a span between double quotes, curly double quotes, guillemets or
//! backticks (code shows someone's words just as a quotation does), or single quotes that
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
pub(crate) const SPEECH_VERBS: &[&str] = &[
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
    /// Inline code (a backtick run) rather than quoted words: a literal, never a quoted
    /// instruction.
    code: bool,
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
    /// Any speech verb appears in it, the user's own included ("I wrote last week: ..."):
    /// a quotation it introduces is reported words, not a statement made now.
    quotes_speech: bool,
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
}

impl<'a> Quotations<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let spans = spans(text);
        let sentences = sentences(text, &spans);
        let attributed = spans
            .iter()
            .map(|span| {
                let index = sentences.partition_point(|sentence| sentence.end <= span.open);
                let own = sentences
                    .get(index)
                    .is_some_and(|sentence| sentence.quotes_speech);
                let introduced = index
                    .checked_sub(1)
                    .and_then(|previous| sentences.get(previous))
                    .is_some_and(|previous| previous.introduces && previous.quotes_speech);
                let referred_back = sentences
                    .get(index + 1)
                    .is_some_and(|next| next.refers_back && next.quotes_speech);
                // A quoted instruction ("\"From now on, never commit.\"") is someone's
                // words even unattributed; a quoted term ('Next:') is not.
                let instruction = !span.code
                    && crate::attributed_text::reads_as_instruction(
                        &crate::attributed_text::normalize(&text[span.open..span.close]),
                    );
                own || introduced || referred_back || instruction
            })
            .collect();
        Self {
            text,
            spans,
            attributed,
        }
    }

    /// Quotations attributed to someone else (not code), each with the sentence that
    /// attributes it.
    pub(crate) fn attributed_quotes(&self) -> Vec<(&'a str, &'a str)> {
        const MARKS: [char; 8] = [
            '"', '\'', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '\u{ab}', '\u{bb}',
        ];
        let sentences = sentences(self.text, &self.spans);
        self.spans
            .iter()
            .zip(&self.attributed)
            .filter(|(span, attributed)| **attributed && !span.code)
            .map(|(span, _)| {
                let index = sentences.partition_point(|sentence| sentence.end <= span.open);
                // A quotation on the line after "Priya wrote:" is named by that line.
                let named_before = !sentences
                    .get(index)
                    .is_some_and(|sentence| sentence.quotes_speech);
                let first = if named_before {
                    index.saturating_sub(1)
                } else {
                    index
                };
                let start = first
                    .checked_sub(1)
                    .and_then(|previous| sentences.get(previous))
                    .map_or(0, |previous| previous.end);
                let end = sentences
                    .get(index)
                    .map_or(self.text.len(), |sentence| sentence.end)
                    .max(span.close);
                let quote = self.text[span.open..span.close].trim_matches(MARKS).trim();
                (quote, self.text[start..end].trim())
            })
            .collect()
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
            quotes_speech: words
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

/// Inline code spans: a run of backticks closes at the next run of the same length. A run
/// with no partner is a stray mark and opens nothing.
fn code_spans(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut runs = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'`' {
            let start = index;
            while index < bytes.len() && bytes[index] == b'`' {
                index += 1;
            }
            runs.push((start, index - start));
        } else {
            index += 1;
        }
    }
    let mut spans = Vec::new();
    let mut next = 0;
    while next < runs.len() {
        let (open, length) = runs[next];
        match runs[next + 1..]
            .iter()
            .position(|(_, other)| *other == length)
        {
            Some(offset) => {
                let (close, _) = runs[next + 1 + offset];
                spans.push(Span {
                    open,
                    close: close + length,
                    code: true,
                });
                next += offset + 2;
            }
            None => next += 1,
        }
    }
    spans
}

fn spans(text: &str) -> Vec<Span> {
    let code = code_spans(text);
    let in_code = |offset: usize| {
        code.iter()
            .any(|span| span.open <= offset && offset < span.close)
    };
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut quotes = Vec::new();
    let mut open: Option<(usize, char)> = None;
    for (index, &(offset, character)) in characters.iter().enumerate() {
        // Quotation marks inside code are part of the code.
        if in_code(offset) {
            continue;
        }
        let before = index.checked_sub(1).map(|previous| characters[previous].1);
        let after = characters.get(index + 1).map(|next| next.1);
        let end = offset + character.len_utf8();
        let closes = match open {
            Some((_, '"')) => character == '"',
            Some((_, '\u{201c}')) => character == '\u{201d}',
            Some((_, '\u{ab}')) => character == '\u{bb}',
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
                quotes.push(Span {
                    open: start,
                    close: end,
                    code: false,
                });
            }
            continue;
        }
        if open.is_none() {
            let opens = match character {
                '"' | '\u{201c}' | '\u{ab}' => true,
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
    if let Some((start, '"' | '\u{201c}' | '\u{ab}')) = open {
        quotes.push(Span {
            open: start,
            close: text.len(),
            code: false,
        });
    }
    // Code inside a quotation belongs to the quotation; spans stay disjoint and sorted.
    let kept_code = code
        .into_iter()
        .filter(|code| {
            !quotes
                .iter()
                .any(|quote| quote.open <= code.open && code.close <= quote.close)
        })
        .collect::<Vec<_>>();
    let mut spans = quotes;
    spans.extend(kept_code);
    spans.sort_by_key(|span| span.open);
    spans
}
