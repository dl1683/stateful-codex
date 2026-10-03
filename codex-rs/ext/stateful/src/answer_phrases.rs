//! The words that mark the outcomes an answer states: which headings introduce ruled-out
//! items or open checks, which labels name a decision and its parts, and which wordings are
//! tentative, negated or unmade and so state no outcome.

use crate::answer_units::AnswerUnitKind;
use crate::answer_units::QUOTATION_OPENINGS;
use crate::user_rules::has_phrase;
use crate::user_rules::list_item_body;
use crate::user_rules::normalize;

/// Longest line read as a heading.
const MAX_HEADING_BYTES: usize = 120;

const RULED_OUT_PHRASES: &[&str] = &[
    "ruled out",
    "ruled it out",
    "rejected",
    "eliminated",
    "excluded",
    "dismissed",
];
/// An item with these is not a settled rejection, even under a ruled-out heading.
const NOT_REJECTED_PHRASES: &[&str] = &[
    "not ruled out",
    "not yet ruled out",
    "not been ruled out",
    "not rejected",
    "not excluded",
    "not eliminated",
    "cannot rule out",
    "cannot be ruled out",
    "can't rule out",
    "can't be ruled out",
    "could not rule out",
    "couldn't rule out",
    "yet to rule out",
    "be ruled out",
    "still possible",
    "possibly",
    "probably",
    "likely",
    "might",
    "may be",
    "suspect",
    "not checked",
    "not yet checked",
    "unverified",
    "unclear",
];
/// A heading with these does not introduce rejections, even beside a ruled-out phrase.
const TENTATIVE_PHRASES: &[&str] = &[
    "not ruled out",
    "not yet ruled out",
    "not been ruled out",
    "not rejected",
    "not excluded",
    "not eliminated",
    "cannot rule out",
    "cannot be ruled out",
    "can't rule out",
    "can't be ruled out",
    "could not rule out",
    "couldn't rule out",
    "yet to rule out",
    "to rule out",
    "be ruled out",
    "still possible",
    "possible",
    "possibly",
    "candidates",
    "might",
    "may",
    "unclear",
];
const OPEN_CHECK_PHRASES: &[&str] = &[
    "open checks",
    "open check",
    "still open",
    "open questions",
    "open question",
    "not yet verified",
    "not verified",
    "unverified",
    "still to verify",
    "still to check",
    "to be verified",
    "needs verification",
    "remaining checks",
    "outstanding checks",
    "unresolved",
];
const CHOICE_LABELS: &[&str] = &["decision", "decided", "final decision", "chosen approach"];
/// A choice that opens with these has not been made.
const UNMADE_CHOICES: &[&str] = &[
    "pending",
    "none",
    "none yet",
    "not yet",
    "not made",
    "tbd",
    "to be decided",
    "undecided",
    "your call",
];
const REASON_LABELS: &[&str] = &["reason", "why", "rationale", "because"];
const ALTERNATIVE_LABELS: &[&str] = &[
    "alternatives",
    "alternatives considered",
    "rejected alternatives",
    "options considered",
];
const RECONSIDER_LABELS: &[&str] = &[
    "reconsider if",
    "reconsider when",
    "revisit if",
    "revisit when",
];
/// Words that negate a heading, wherever they appear in it.
const NEGATIONS: &[&str] = &["not", "no", "never", "cannot", "nor", "without"];
/// A choice containing these says no decision was made.
const UNMADE_PHRASES: &[&str] = &[
    "not decided",
    "not yet decided",
    "have not decided",
    "haven't decided",
    "not been decided",
    "no decision",
    "undecided",
    "to be decided",
    "not made",
    "not yet made",
    "pending your",
    "your call",
    "tbd",
];

/// The part of an answer a decision label opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Label {
    Choice,
    Reason,
    Alternatives,
    Reconsider,
}

/// The decision part `label` names ("Decision", "Why", "Revisit if"), if any.
pub(crate) fn label_of(label: &str) -> Option<Label> {
    let label = normalize(label);
    if CHOICE_LABELS.contains(&label.as_str()) {
        Some(Label::Choice)
    } else if REASON_LABELS.contains(&label.as_str()) {
        Some(Label::Reason)
    } else if ALTERNATIVE_LABELS.contains(&label.as_str()) {
        Some(Label::Alternatives)
    } else if RECONSIDER_LABELS.contains(&label.as_str()) {
        Some(Label::Reconsider)
    } else {
        None
    }
}

/// A later clause with these turns to another candidate ("likely a parser bug instead"),
/// so its uncertainty is not about the rejected item. Words such as "other" do not: "still
/// possible on the other host" is still about the item.
const CONTRAST_WORDS: &[&str] = &["instead"];

/// Whether a ruled-out item does not settle its rejection: a clause says the item is still
/// possible, unchecked or not ruled out, or negates the rejection ("not dismissed"). A later
/// clause about another cause ("likely a parser bug instead") does not unsettle it.
pub(crate) fn is_tentative_rejection(item: &str) -> bool {
    item.split([';', '\n'])
        .flat_map(|part| part.split(". "))
        .enumerate()
        .any(|(index, clause)| {
            let words = normalized(clause);
            let about_another =
                index > 0 && words.split(' ').any(|word| CONTRAST_WORDS.contains(&word));
            !about_another
                && (has_phrase(&words, NOT_REJECTED_PHRASES) || negates_rejection(&words))
        })
}

/// Whether normalized `words` deny a rejection ("not dismissed", "never eliminated").
fn negates_rejection(words: &str) -> bool {
    RULED_OUT_PHRASES.iter().any(|phrase| {
        [
            "not",
            "never",
            "not yet",
            "not been",
            "wasn't",
            "isn't",
            "hasn't been",
        ]
        .iter()
        .any(|negation| has_phrase(words, &[&format!("{negation} {phrase}")]))
    })
}

/// A later clause with these says the decision itself was not made ("we have not decided
/// yet"), unlike a clause about an undecided alternative ("Redis is still undecided").
const UNMADE_DECISION_PHRASES: &[&str] = &[
    "we have not decided",
    "we haven't decided",
    "i have not decided",
    "i haven't decided",
    "not decided yet",
    "not yet decided",
    "no decision",
    "nothing is decided",
    "nothing decided",
    "decision is pending",
    "decision pending",
];

/// Whether a stated choice says that nothing was chosen: its choice clause is unmade, or a
/// later clause (not the reason after "because") says the decision was not made.
pub(crate) fn is_unmade_choice(choice: &str) -> bool {
    let without_reason = choice.split(" because ").next().unwrap_or_default();
    let mut clauses = without_reason
        .split([';', '\n'])
        .flat_map(|part| part.split(". "));
    let first = normalized(clauses.next().unwrap_or_default());
    has_phrase(&first, UNMADE_PHRASES)
        || UNMADE_CHOICES
            .iter()
            .any(|unmade| first == *unmade || first.starts_with(&format!("{unmade} ")))
        || clauses.any(|clause| has_phrase(&normalized(clause), UNMADE_DECISION_PHRASES))
}

/// The list a heading line introduces: a Markdown heading, a bold line, or a short line
/// ending with a colon, with nothing after the colon.
pub(crate) fn heading_kind(trimmed: &str) -> Option<AnswerUnitKind> {
    if trimmed.len() > MAX_HEADING_BYTES
        || list_item_body(trimmed).is_some()
        || trimmed.starts_with(QUOTATION_OPENINGS)
    {
        return None;
    }
    let markdown = trimmed.starts_with('#');
    let bold = trimmed.starts_with("**") || trimmed.starts_with("__");
    let text = trimmed
        .trim_start_matches('#')
        .trim()
        .trim_matches(['*', '_'])
        .trim();
    if !(markdown || bold || text.ends_with(':')) {
        return None;
    }
    let words = normalized(text.trim_end_matches(':'));
    if words.is_empty() || (text.contains(':') && !text.ends_with(':')) {
        return None;
    }
    let negated = words
        .split(' ')
        .any(|word| NEGATIONS.contains(&word) || word.ends_with("n't"));
    let ruled_out =
        has_phrase(&words, RULED_OUT_PHRASES) && !negated && !has_phrase(&words, TENTATIVE_PHRASES);
    let open_check = has_phrase(&words, OPEN_CHECK_PHRASES);
    match (ruled_out, open_check) {
        (true, false) => Some(AnswerUnitKind::RuledOut),
        (false, true) => Some(AnswerUnitKind::OpenCheck),
        _ => None,
    }
}

/// Normalized words, with typographic apostrophes read as plain ones.
fn normalized(text: &str) -> String {
    normalize(&text.replace('\u{2019}', "'"))
}
