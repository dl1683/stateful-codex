//! Existing lexical guards for recognizing relayed, nonbinding instruction notes.

pub(crate) const IMPERATIVE_LEADS: &[&str] = &[
    "never",
    "always",
    "please never",
    "please always",
    "so never",
    "so please never",
    "and never",
    "and always",
    "but never",
    "you should never",
    "you must never",
    "you should always",
    "you must always",
    "please don't ever",
    "don't ever",
    "do not ever",
];

const STANDING_PHRASES: &[&str] = &[
    "from now on",
    "going forward",
    "in future",
    "in the future",
    "every reply",
    "every replies",
    "each reply",
    "each of your replies",
    "every one of your replies",
    "every response",
    "each response",
    "each of your responses",
    "every answer",
    "each answer",
    "every time",
    "unless i ask",
    "unless i say",
    "unless i tell you",
    "standing",
    "preference",
    "preferences",
    "i like to work",
    "the whole week",
    "all week",
    "later sessions",
    "all our work",
    "all of our work",
    "ground rules",
    "rules for this",
    "house rules",
    "style rules",
];

const DURATION_PHRASES: &[&str] = &[
    "for the whole time",
    "the whole time we work",
    "while we work on",
    "throughout this project",
    "for this project",
    "for this essay",
    "for this book",
    "for this document",
];

pub(crate) const INVESTIGATION_PHRASES: &[&str] = &[
    "for this investigation",
    "for this whole investigation",
    "for the whole investigation",
    "during this investigation",
    "throughout this investigation",
    "for the rest of this investigation",
    "this whole investigation",
    "the whole investigation",
];

pub(crate) const STRONG_STANDING_PHRASES: &[&str] = &[
    "for the whole time",
    "the whole time we work",
    "throughout this project",
    "from now on",
    "going forward",
    "in future",
    "in the future",
    "the whole week",
    "all week",
    "later sessions",
    "all our work",
    "all of our work",
];

pub(crate) fn reads_as_instruction(normalized: &str) -> bool {
    let body = strip_list_marker(normalized);
    IMPERATIVE_LEADS
        .iter()
        .chain(STRONG_STANDING_PHRASES)
        .chain(["only", "don't", "do not", "please", "make sure"].iter())
        .any(|lead| body.starts_with(&format!("{lead} ")))
}

pub(crate) fn has_standing_marker(normalized: &str) -> bool {
    let body = strip_list_marker(normalized);
    if body.starts_with("never mind") {
        return false;
    }
    IMPERATIVE_LEADS
        .iter()
        .any(|lead| body.starts_with(&format!("{lead} ")))
        || [
            " please never ",
            " so never ",
            " and never ",
            " please always ",
        ]
        .iter()
        .any(|phrase| format!(" {body} ").contains(phrase))
        || has_phrase(normalized, STANDING_PHRASES)
        || has_phrase(normalized, INVESTIGATION_PHRASES)
        || (has_phrase(normalized, DURATION_PHRASES) && !describes_the_user(body))
}

fn describes_the_user(body: &str) -> bool {
    let first_person = [
        "i ", "i'm ", "i've ", "i'd ", "my ", "we ", "we're ", "we've ", "our ",
    ]
    .iter()
    .any(|opening| body.starts_with(opening));
    let directs = [
        " never ",
        " always ",
        " don't ",
        " do not ",
        " must ",
        " should ",
        " want you ",
        " need you ",
        " please ",
    ]
    .iter()
    .any(|word| format!(" {body} ").contains(word));
    first_person && !directs
}

pub(crate) fn strip_list_marker(normalized: &str) -> &str {
    match normalized.split_once(' ') {
        Some((first, rest))
            if first.len() <= 3
                && first
                    .trim_start_matches('p')
                    .chars()
                    .all(|character| character.is_ascii_digit()) =>
        {
            rest
        }
        _ => normalized,
    }
}

pub(crate) fn has_phrase(normalized: &str, phrases: &[&str]) -> bool {
    let padded = format!(" {normalized} ");
    phrases
        .iter()
        .any(|phrase| padded.contains(&format!(" {phrase} ")))
}

pub(crate) fn normalize(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '\'' {
                character.to_lowercase().next().unwrap_or(character)
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
