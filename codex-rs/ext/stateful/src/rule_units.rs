//! Rule units: where in a message a rule can come from. A prose sentence is one unit; a list
//! item with its indented continuation lines is one unit and one whole rule. Host capture and
//! the model's verified-quote path both resolve a rule to the same unit, so they store the
//! same words under the same identity.

use crate::quotation::Quotations;
use crate::user_rules::EXPLICIT_TASK_PHRASES;
use crate::user_rules::HeaderScope;
use crate::user_rules::MAX_RULE_BYTES;
use crate::user_rules::RuleClause;
use crate::user_rules::RuleStanding;
use crate::user_rules::asks_about_rules;
use crate::user_rules::clauses;
use crate::user_rules::has_phrase;
use crate::user_rules::has_standing_marker;
use crate::user_rules::header_scope;
use crate::user_rules::in_list_item;
use crate::user_rules::is_reported_speech;
use crate::user_rules::list_item_body;
use crate::user_rules::next_header;
use crate::user_rules::normalize;
use crate::user_rules::standing_of;

/// Words in a header that limit its rules to one piece of work; such a header's words are
/// kept with each rule under it.
const LIMITED_SCOPE_PHRASES: &[&str] = &[
    "investigation",
    "this bug",
    "this issue",
    "this incident",
    "this debugging",
];

/// Clauses of `text` that the user explicitly marked as standing (or pending) rules, in the
/// order the user wrote them. A list item is one rule, whole (all its sentences and indented
/// continuation lines); a prose sentence is one rule. Fenced and blockquoted lines are never
/// the user's rules, and a unit longer than `MAX_RULE_BYTES` is not captured at all rather
/// than captured in part.
pub(crate) fn marked_rules(text: &str) -> Vec<RuleClause> {
    let quotations = Quotations::new(text);
    let mut rules = Vec::new();
    for unit in rule_units(text) {
        let kept = kept_clauses(text, &quotations, &unit);
        if unit.item {
            if let Some(rule) = item_rule(&unit, &kept, Marker::Required)
                && rule.text.len() <= MAX_RULE_BYTES
            {
                rules.push(rule);
            }
            continue;
        }
        for clause in kept {
            let normalized = normalize(clause);
            if clause.len() <= MAX_RULE_BYTES && has_standing_marker(&normalized) {
                rules.push(RuleClause {
                    text: clause.to_string(),
                    standing: standing_of(&normalized),
                });
            }
        }
    }
    rules
}

/// The rule the unit holding `clause` (a sentence of `text`) states: the whole list item
/// with its header's scope, or the sentence itself in prose. None when the clause is not in
/// `text` or its list relays someone else's words. The caller checks the length.
pub(crate) fn rule_for_clause(text: &str, clause: &str) -> Option<RuleClause> {
    let start = text.find(clause)?;
    let end = start + clause.len();
    let quotations = Quotations::new(text);
    let unit = rule_units(text).into_iter().find(|unit| {
        unit.lines.iter().any(|line| {
            let line_start = line.as_ptr() as usize - text.as_ptr() as usize;
            line_start <= start && end <= line_start + line.len()
        })
    })?;
    if !unit.item {
        return Some(RuleClause {
            text: clause.to_string(),
            standing: standing_of(&normalize(clause)),
        });
    }
    let kept = kept_clauses(text, &quotations, &unit);
    item_rule(&unit, &kept, Marker::NotRequired)
}

/// Whether an unheaded list item needs a standing marker in one of its sentences.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    /// Host capture: only items the user marked.
    Required,
    /// A verified quote: the user's own words, marked or not.
    NotRequired,
}

/// Clauses of `unit` that may belong to a rule: no question, header, relayed speech or
/// request to retrieve rules.
fn kept_clauses<'a>(text: &str, quotations: &Quotations<'_>, unit: &RuleUnit<'a>) -> Vec<&'a str> {
    unit.lines
        .iter()
        .flat_map(|line| clauses(line))
        .filter(|clause| {
            if clause.ends_with('?') || clause.ends_with(':') {
                return false;
            }
            // Clauses are slices of `text`, so their offsets locate them in its quotations.
            let start = clause.as_ptr() as usize - text.as_ptr() as usize;
            !(is_reported_speech(&normalize(clause))
                || quotations.relays(start, start + clause.len())
                || asks_about_rules(clause))
        })
        .collect()
}

/// The one rule a list item states, judged as a whole so an ending condition or a task limit
/// in any of its sentences applies to all of it.
fn item_rule(unit: &RuleUnit<'_>, kept: &[&str], marker: Marker) -> Option<RuleClause> {
    if kept.is_empty() {
        return None;
    }
    let body = kept.join(" ");
    let normalized = normalize(&body);
    match unit.header {
        // Items relayed from someone else are never the user's rules.
        Some(ListHeader {
            scope: HeaderScope::Reported,
            ..
        }) => None,
        Some(header) => {
            let standing = match header.scope {
                // A task-limited list header limits every item under it.
                HeaderScope::Pending => RuleStanding::Pending,
                // Under a header the user wrote, a weak task word ("until we agree") is the
                // rule's ending condition; only an explicit limit makes it pending.
                HeaderScope::Standing | HeaderScope::Reported => {
                    if has_phrase(&normalized, EXPLICIT_TASK_PHRASES) {
                        RuleStanding::Pending
                    } else {
                        RuleStanding::Standing
                    }
                }
            };
            let text = match header.limited_scope {
                // The header's limited scope stays with the rule, in the user's words.
                Some(scope) => {
                    let item = list_item_body(&body).map_or(body.as_str(), str::trim);
                    format!("{scope} {item}")
                }
                None => body,
            };
            Some(RuleClause { text, standing })
        }
        None => {
            if marker == Marker::Required
                && !kept
                    .iter()
                    .any(|clause| has_standing_marker(&normalize(clause)))
            {
                return None;
            }
            Some(RuleClause {
                standing: standing_of(&normalized),
                text: body,
            })
        }
    }
}

/// A list header in effect: the scope it gives its items, and its own words when it limits
/// them to a piece of work ("Some ground rules for this whole investigation:").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListHeader<'a> {
    scope: HeaderScope,
    limited_scope: Option<&'a str>,
}

/// One place a rule can come from: a prose line, or a list item with the header in effect.
struct RuleUnit<'a> {
    header: Option<ListHeader<'a>>,
    item: bool,
    lines: Vec<&'a str>,
}

/// Groups `text` into prose lines and list items (an item with its indented continuation
/// lines). Lines inside a fence or a blockquote may be anyone's words and form no unit.
fn rule_units(text: &str) -> Vec<RuleUnit<'_>> {
    let mut units: Vec<RuleUnit<'_>> = Vec::new();
    // The header of the list the current line belongs to. Blank lines keep it (Markdown
    // lists often follow a blank line); any other prose line replaces it.
    let mut list_header: Option<ListHeader<'_>> = None;
    let mut in_list = false;
    let mut item_open = false;
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            item_open = false;
            continue;
        }
        if fenced || trimmed.starts_with('>') {
            item_open = false;
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let was_in_list = in_list;
        if !in_list_item(line, trimmed, &mut in_list) {
            list_header = next_list_header(line, trimmed, was_in_list, list_header);
            units.push(RuleUnit {
                header: None,
                item: false,
                lines: vec![trimmed],
            });
            item_open = false;
            continue;
        }
        let continuation = list_item_body(trimmed).is_none();
        match units.last_mut() {
            Some(unit) if continuation && item_open => unit.lines.push(trimmed),
            _ => units.push(RuleUnit {
                header: list_header,
                item: true,
                lines: vec![trimmed],
            }),
        }
        item_open = true;
    }
    units
}

/// The header a non-item line gives the list items after it.
fn next_list_header<'a>(
    line: &str,
    trimmed: &'a str,
    was_in_list: bool,
    enclosing: Option<ListHeader<'a>>,
) -> Option<ListHeader<'a>> {
    let scope = next_header(
        line,
        trimmed,
        was_in_list,
        enclosing.map(|header| header.scope),
    )?;
    let own = header_scope(trimmed).map(|own_scope| ListHeader {
        scope: own_scope,
        limited_scope: clauses(trimmed)
            .last()
            .copied()
            .filter(|clause| has_phrase(&normalize(clause), LIMITED_SCOPE_PHRASES)),
    });
    match (own, enclosing) {
        (Some(own), _) if own.scope == scope => Some(own),
        (_, Some(enclosing)) => Some(ListHeader { scope, ..enclosing }),
        (Some(own), None) => Some(ListHeader { scope, ..own }),
        (None, None) => Some(ListHeader {
            scope,
            limited_scope: None,
        }),
    }
}

#[cfg(test)]
#[path = "rule_units_tests.rs"]
mod tests;
