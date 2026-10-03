//! What the user says about themselves or about the whole work, in their own words
//! ("I know Python well but only a little Rust"): background a later session needs, kept
//! apart from rules.

use crate::quotation::Quotations;
use crate::user_rules::MAX_RULE_BYTES;
use crate::user_rules::clauses;
use crate::user_rules::is_reported_speech;
use crate::user_rules::list_item_body;
use crate::user_rules::marked_rules;
use crate::user_rules::normalize;
use crate::user_rules::strip_list_marker;

/// First-person openings of what the user says about themselves or about the work as a whole
/// ("I know Python well but only a little Rust", "I'm not changing any code"): background a
/// later session needs, kept apart from rules.
const BACKGROUND_OPENINGS: &[&str] = &[
    "i know",
    "i only know",
    "i don't know",
    "i'm new to",
    "i am new to",
    "i'm familiar with",
    "i am familiar with",
    "i'm not familiar with",
    "i'm comfortable with",
    "i'm coming from",
    "i come from",
    "i've used",
    "i have used",
    "i've never",
    "i have never",
    "i'm learning",
    "i am learning",
    "i'm a",
    "i am a",
    "i'm an",
    "i am an",
    "i'm moving into",
    "i'm switching to",
    "i work as",
    "my background",
    "i maintain",
    "i'm the maintainer",
    "i am the maintainer",
    "i work on",
    "i'm rusty",
    "my python is",
];

/// Clauses of `text` in which the user describes themselves or the whole work, in their
/// words. Questions, rules and relayed speech are not background. Each clause is judged on
/// its own: a colleague quoted elsewhere in the message does not hide the user's own
/// description of themselves, while a clause that touches a quotation, sits in a sentence
/// that reports speech, or comes from a block someone else's words may fill is left out.
pub(crate) fn background_statements(text: &str) -> Vec<String> {
    let quotations = Quotations::new(text);
    let rules = marked_rules(text)
        .into_iter()
        .map(|rule| rule.text)
        .collect::<Vec<_>>();
    // Only the user's own plain prose counts: fenced blocks, quoted lines, indented or list
    // lines and the block a colon-terminated line introduces ("She wrote:", until a blank
    // line) may be someone else's words.
    let mut fence = crate::user_rules::Fence::default();
    let mut introduced = false;
    // Under the user's own "About me:" header, its items are the user's words.
    let mut about_me = false;
    let mut statements = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if fence.skips(trimmed) {
            continue;
        }
        if trimmed.is_empty() {
            // An introduced block ends at a blank line.
            introduced = false;
            about_me = false;
            continue;
        }
        let item = list_item_body(trimmed).map(str::trim);
        let plain = (about_me && !trimmed.starts_with('>'))
            || (!introduced
                && !trimmed.starts_with('>')
                && !line.starts_with([' ', '\t'])
                && item.is_none());
        if trimmed.ends_with(':') && !introduced {
            about_me = names_the_user(trimmed);
        }
        introduced = introduced || trimmed.ends_with(':');
        if !plain {
            continue;
        }
        let line_text = if about_me {
            item.unwrap_or(trimmed)
        } else {
            trimmed
        };
        for clause in clauses(line_text) {
            // Clauses are slices of `text`, so their offsets locate them in its quotations.
            let start = clause.as_ptr() as usize - text.as_ptr() as usize;
            if quotations.touches_words(start, start + clause.len())
                || quotations.in_reported_sentence(start)
                || clause.contains(QUOTE_OR_CODE_MARKS)
                || clause.ends_with('?')
                || clause.ends_with(':')
                || clause.len() > MAX_RULE_BYTES
                || rules.iter().any(|rule| rule == clause)
                || is_reported_speech(&normalize(clause))
            {
                continue;
            }
            // A framing before a colon ("Quick intro since this is our first session: I'm
            // a ...") is not part of the description; the words after it are.
            let statement = match clause.split_once(": ") {
                Some((framing, _)) if describes_self(framing) => clause,
                Some((framing, _)) if names_a_source(framing) => continue,
                Some((_, rest)) if describes_self(rest) => rest.trim(),
                Some(_) => continue,
                // An item under "About me:" is the user's description, whatever its opening.
                None if about_me => clause,
                None if describes_self(clause) => clause,
                None => continue,
            };
            statements.push(statement.to_string());
        }
    }
    statements
}

/// Marks that show a quotation or code in a clause.
const QUOTE_OR_CODE_MARKS: [char; 6] =
    ['"', '\u{201c}', '\u{201d}', '\u{2018}', '\u{ab}', '\u{bb}'];

/// Whether a header introduces the user's own description ("About me:", "Background:").
fn names_the_user(header: &str) -> bool {
    let header = normalize(header);
    [
        "about me",
        "a bit about me",
        "about myself",
        "background",
        "my background",
        "some background",
        "who i am",
    ]
    .iter()
    .any(|opening| header == *opening || header.starts_with(&format!("{opening} ")))
}

/// Words of a framing ("From the docs:", "My colleague says:") that make what follows
/// someone else's words.
const SOURCE_WORDS: &[&str] = &[
    "says",
    "say",
    "said",
    "writes",
    "wrote",
    "written",
    "asks",
    "asked",
    "told",
    "tells",
    "from",
    "according",
    "docs",
    "doc",
    "readme",
    "quote",
    "quoted",
    "transcript",
    "message",
    "email",
    "chat",
    "colleague",
    "teammate",
    "she",
    "he",
    "they",
    "her",
    "his",
    "their",
];

fn names_a_source(framing: &str) -> bool {
    normalize(framing)
        .split(' ')
        .any(|word| SOURCE_WORDS.contains(&word))
}

/// Whether a phrase of `clause` opens with a first-person description of the user or the
/// work ("I know Python well", "I'm not changing any code").
fn describes_self(clause: &str) -> bool {
    clause
        .split([':', ',', ';', '-'])
        .map(normalize)
        .any(|phrase| {
            let body = strip_list_marker(&phrase);
            let body = ["and ", "also ", "oh and ", "but ", "so ", "well "]
                .iter()
                .find_map(|lead| body.strip_prefix(lead))
                .unwrap_or(body);
            BACKGROUND_OPENINGS
                .iter()
                .any(|opening| body == *opening || body.starts_with(&format!("{opening} ")))
        })
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
