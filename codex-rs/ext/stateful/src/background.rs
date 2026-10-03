//! What the user says about themselves or about the whole work, in their own words
//! ("I know Python well but only a little Rust"): background a later session needs, kept
//! apart from rules.

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
    "i work as",
    "my background",
    "i'm not changing",
    "i am not changing",
    "i won't be changing",
    "i will not be changing",
    "i'm only reading",
    "i'm just reading",
];

/// Clauses of `text` in which the user describes themselves or the whole work, in their
/// words. Questions, rules and relayed speech are not background.
pub(crate) fn background_statements(text: &str) -> Vec<String> {
    // A message that quotes or shows code anywhere may carry someone else's words on any
    // line, so it contributes no background at all; missing a statement costs less than
    // attributing a stranger's self-description to the user.
    if quotes_or_shows_code(text) {
        return Vec::new();
    }
    let rules = marked_rules(text)
        .into_iter()
        .map(|rule| rule.text)
        .collect::<Vec<_>>();
    // Only the user's own plain prose counts: fenced blocks, quoted lines, indented or list
    // lines and the block a colon-terminated line introduces ("She wrote:", until a blank
    // line) may be someone else's words.
    let mut fenced = false;
    let mut introduced = false;
    let mut own_lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if trimmed.is_empty() {
            // An introduced block ends at a blank line.
            introduced = false;
            continue;
        }
        let plain = !fenced
            && !introduced
            && !trimmed.starts_with('>')
            && !line.starts_with([' ', '\t'])
            && list_item_body(trimmed).is_none();
        introduced = introduced || trimmed.ends_with(':');
        // A line that quotes, shows code or introduces words after a colon ("She wrote: ...")
        // is left out whole: splitting it into sentences would lose that context.
        if plain && !trimmed.contains(['"', '\u{201c}', '\u{201d}', '`', ':']) {
            own_lines.push(trimmed);
        }
    }
    own_lines
        .into_iter()
        .flat_map(clauses)
        .filter(|clause| {
            !clause.contains(['"', '\u{201c}', '\u{201d}'])
                && !clause.ends_with('?')
                && !clause.ends_with(':')
                && clause.len() <= MAX_RULE_BYTES
                && !rules.iter().any(|rule| rule == clause)
                && !is_reported_speech(&normalize(clause))
                && clause
                    .split([':', ',', ';', '-'])
                    .map(normalize)
                    .any(|phrase| {
                        let body = strip_list_marker(&phrase);
                        let body = ["and ", "also ", "oh and ", "but ", "so ", "well "]
                            .iter()
                            .find_map(|lead| body.strip_prefix(lead))
                            .unwrap_or(body);
                        BACKGROUND_OPENINGS.iter().any(|opening| {
                            body == *opening || body.starts_with(&format!("{opening} "))
                        })
                    })
        })
        .map(str::to_string)
        .collect()
}

/// Whether `text` holds a quotation (double or curly quotes, or a single quote opening a
/// word) or code (backticks or a fence).
fn quotes_or_shows_code(text: &str) -> bool {
    if text.contains([
        '"', '\u{201c}', '\u{201d}', '\u{2018}', '`', '\u{ab}', '\u{bb}',
    ]) || text.contains("~~~")
    {
        return true;
    }
    // An apostrophe inside a word ("I'm") is not a quotation; one opening a word is.
    let mut previous = ' ';
    for character in text.chars() {
        if matches!(character, '\'' | '\u{2019}') && !previous.is_alphanumeric() {
            return true;
        }
        previous = character;
    }
    false
}
