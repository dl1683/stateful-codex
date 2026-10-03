//! Rule units: where in a message a rule can come from. A prose sentence is one unit; a list
//! item with its indented continuation lines is one unit and one whole rule. Host capture and
//! the model's verified-quote path both resolve a rule to the same unit, so they store the
//! same words under the same identity.

use std::ops::Range;

use crate::quotation::Quotations;
use crate::user_rules::Fence;
use crate::user_rules::HeaderScope;
use crate::user_rules::MAX_RULE_BYTES;
use crate::user_rules::RuleClause;
use crate::user_rules::RuleStanding;
use crate::user_rules::asks_about_rules;
use crate::user_rules::clauses;
use crate::user_rules::has_explicit_task_limit;
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

/// Whether a rule's words name a piece of work it is limited to (an investigation, this bug,
/// ...), the way a header or sentence that scopes rules does.
pub(crate) fn names_limited_scope(normalized: &str) -> bool {
    has_phrase(normalized, crate::user_rules::INVESTIGATION_PHRASES)
        || LIMITED_SCOPE_PHRASES
            .iter()
            .any(|phrase| normalized.contains(phrase))
}

/// A rule found in a message, with the investigation it is limited to, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MarkedRule {
    pub(crate) clause: RuleClause,
    pub(crate) scope: Option<ScopeHint>,
}

/// Where a rule applies, in the user's words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScopeHint {
    /// The words naming the investigation ("Some ground rules for this whole investigation").
    pub(crate) title: String,
    /// When the rule stops applying ("until we have agreed on the root cause").
    pub(crate) end_condition: Option<String>,
}

/// Everything one message says about rules: the rules in the order written, the units that
/// were recognized but could not be kept whole, and a count the user declared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MarkedRules {
    pub(crate) rules: Vec<MarkedRule>,
    /// Openings of recognized rules too long to store whole.
    pub(crate) omitted: Vec<String>,
    /// "Two standing rules ..." declares 2.
    pub(crate) declared_count: Option<u32>,
}

/// Clauses of `text` that the user explicitly marked as standing (or pending) rules, in the
/// order the user wrote them.
pub(crate) fn marked_rules(text: &str) -> Vec<RuleClause> {
    marked_rule_units(text)
        .rules
        .into_iter()
        .map(|rule| rule.clause)
        .collect()
}

/// The rules `text` marks, in the order written. A list item is one rule, whole (all its
/// sentences and indented continuation lines). In prose each marked sentence is a rule;
/// independent directives joined in one sentence ("never commit and always end with Next:")
/// are separate rules, and a following sentence that qualifies a rule ("If you need an
/// environment, ...") stays with it and decides its standing and scope with it. Fenced and
/// blockquoted lines are never the user's rules, and a unit longer than `MAX_RULE_BYTES` is
/// reported as omitted, never stored in part.
pub(crate) fn marked_rule_units(text: &str) -> MarkedRules {
    let located = locate_rules(text);
    MarkedRules {
        rules: located.rules.into_iter().map(|(rule, _)| rule).collect(),
        omitted: located
            .omitted
            .iter()
            .map(|(full, _)| opening(full))
            .collect(),
        declared_count: located.declared_count,
    }
}

/// The rules of a message with the byte range of the user's words each came from.
#[derive(Default)]
struct LocatedRules {
    rules: Vec<(MarkedRule, Range<usize>)>,
    /// Recognized rules too long to store whole, in full.
    omitted: Vec<(String, Range<usize>)>,
    declared_count: Option<u32>,
}

fn locate_rules(text: &str) -> LocatedRules {
    let quotations = Quotations::new(text);
    let mut located = LocatedRules {
        declared_count: declared_count(text),
        ..LocatedRules::default()
    };
    let offset = |part: &str| part.as_ptr() as usize - text.as_ptr() as usize;
    let range = |part: &str| offset(part)..offset(part) + part.len();
    let keep = |rule: MarkedRule, at: Range<usize>, located: &mut LocatedRules| {
        if rule.clause.text.len() <= MAX_RULE_BYTES {
            located.rules.push((rule, at));
        } else {
            located.omitted.push((rule.clause.text, at));
        }
    };
    // The prose rule the previous sentence of this paragraph produced, which a qualifying
    // sentence extends, even on the next line.
    let mut extendable: Option<(MarkedRule, Range<usize>)> = None;
    for unit in rule_units(text) {
        let kept = kept_clauses(text, &quotations, &unit);
        if (unit.item || unit.separated)
            && let Some((rule, at)) = extendable.take()
        {
            keep(rule, at, &mut located);
        }
        if unit.item {
            if let (Some(rule), Some(first), Some(last)) = (
                item_rule(&unit, &kept, Marker::Required),
                unit.lines.first(),
                unit.lines.last(),
            ) {
                keep(rule, offset(first)..offset(last) + last.len(), &mut located);
            }
            continue;
        }
        for clause in kept {
            let normalized = normalize(clause);
            if let Some((rule, at)) = extendable.as_mut()
                && qualifies_previous(&normalized)
                && !has_standing_marker(&normalized)
            {
                // The qualification is part of the rule: its standing, scope and ending are
                // judged on the whole.
                *rule = prose_rule(&format!("{} {clause}", rule.clause.text));
                at.end = range(clause).end;
                continue;
            }
            if let Some((rule, at)) = extendable.take() {
                keep(rule, at, &mut located);
            }
            if !has_standing_marker(&normalized) {
                continue;
            }
            // "Some ground rules for this essay: second person; British spelling; ..." lists
            // its rules after a framing colon; each item is its own rule, keeping the framing
            // so its scope stays in the user's words.
            if let Some(items) = framed_list(text, &quotations, clause) {
                for (item, at) in items {
                    keep(prose_rule(&item), at, &mut located);
                }
                continue;
            }
            let mut pieces = coordinated_directives(text, &quotations, clause);
            let last = pieces.pop();
            for piece in pieces {
                keep(prose_rule(piece), range(piece), &mut located);
            }
            extendable = last.map(|piece| (prose_rule(piece), range(piece)));
        }
    }
    if let Some((rule, at)) = extendable {
        keep(rule, at, &mut located);
    }
    located
}

/// The rule the user's words at `quote` (inside `clause`, a sentence of `text`) belong to:
/// the rule host capture keeps for that place (the list item with its header's scope, the
/// coordinated directive or framed item the quote is in, with its qualifications), so both
/// paths store the same words. A rule host capture found too long comes back whole, for the
/// caller to refuse rather than store a part. Otherwise the unit holding the sentence: the
/// whole list item, or the sentence itself in prose. None when the clause is not in `text`
/// or its list relays someone else's words. The caller checks the length.
pub(crate) fn rule_for_clause(text: &str, clause: &str, quote: &str) -> Option<MarkedRule> {
    let start = text.find(clause)?;
    let end = start + clause.len();
    let at = find_collapsed(&text[start..end], quote)
        .map(|found| start + found.start..start + found.end)
        .unwrap_or(start..end);
    let located = locate_rules(text);
    let within = |range: &Range<usize>| range.start <= at.start && at.end <= range.end;
    if let Some((rule, _)) = located.rules.iter().find(|(_, range)| within(range)) {
        return Some(rule.clone());
    }
    if let Some((full, _)) = located.omitted.iter().find(|(_, range)| within(range)) {
        return Some(MarkedRule {
            clause: RuleClause {
                text: full.clone(),
                standing: RuleStanding::Standing,
            },
            scope: None,
        });
    }
    let quotations = Quotations::new(text);
    let unit = rule_units(text).into_iter().find(|unit| {
        unit.lines.iter().any(|line| {
            let line_start = line.as_ptr() as usize - text.as_ptr() as usize;
            line_start <= start && end <= line_start + line.len()
        })
    })?;
    if !unit.item {
        return Some(prose_rule(clause));
    }
    let kept = kept_clauses(text, &quotations, &unit);
    item_rule(&unit, &kept, Marker::NotRequired)
}

/// Where `needle`'s words appear in `haystack`, whatever whitespace separates them.
fn find_collapsed(haystack: &str, needle: &str) -> Option<Range<usize>> {
    let words = needle.split_whitespace().collect::<Vec<_>>();
    let (first, rest) = words.split_first()?;
    haystack.match_indices(first).find_map(|(found, _)| {
        let mut position = found + first.len();
        for word in rest {
            let after = &haystack[position..];
            let trimmed = after.trim_start();
            if trimmed.len() == after.len() || !trimmed.starts_with(word) {
                return None;
            }
            position += after.len() - trimmed.len() + word.len();
        }
        Some(found..position)
    })
}

/// Openings of a sentence that qualifies the rule before it rather than stating a new one.
const QUALIFIER_OPENINGS: &[&str] = &[
    "if",
    "unless",
    "except",
    "otherwise",
    "that way",
    "this means",
    "in that case",
    "instead",
    "but if",
    "when",
    "only if",
];

fn qualifies_previous(normalized: &str) -> bool {
    QUALIFIER_OPENINGS
        .iter()
        .any(|opening| normalized == *opening || normalized.starts_with(&format!("{opening} ")))
}

/// Joins inside one sentence after which an independent directive starts.
const DIRECTIVE_JOINS: &[&str] = &[
    ", and always ",
    ", and never ",
    " and always ",
    " and never ",
    "; always ",
    "; never ",
    ", and please always ",
    ", and please never ",
];

/// Splits a marked sentence into independent directives ("never X and always Y"), outside
/// quotations, keeping each part in the user's words. A sentence with no such join, or whose
/// first part is not itself a directive, is one rule.
fn coordinated_directives<'a>(
    text: &str,
    quotations: &Quotations<'_>,
    clause: &'a str,
) -> Vec<&'a str> {
    let base = clause.as_ptr() as usize - text.as_ptr() as usize;
    let lower = clause.to_ascii_lowercase();
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut search = 0;
    while let Some((offset, join)) = DIRECTIVE_JOINS
        .iter()
        .filter_map(|join| {
            lower[search..]
                .find(join)
                .map(|found| (search + found, *join))
        })
        .min_by_key(|(found, _)| *found)
    {
        // The right part starts at its directive word ("please always", "always", "never").
        let next = offset
            + ["please", "always", "never"]
                .iter()
                .find_map(|word| join.find(word))
                .unwrap_or(join.len());
        let left = clause[start..offset].trim();
        let inside_quotation =
            quotations.touches_quotation(base + offset, base + offset + join.len());
        if !inside_quotation && has_standing_marker(&normalize(left)) {
            pieces.push(left);
            start = next;
        }
        search = offset + join.len();
    }
    pieces.push(clause[start..].trim());
    pieces
}

/// The items of "<framing that names rules>: a; b; c", each with the framing and the range
/// of its own words, when the framing names rules or preferences and at least two items
/// follow. Items split only at "; " outside every quotation and code span of `text`.
fn framed_list(
    text: &str,
    quotations: &Quotations<'_>,
    clause: &str,
) -> Option<Vec<(String, Range<usize>)>> {
    let (framing, list) = clause.split_once(": ")?;
    let framing_words = normalize(framing);
    let names_rules = ["rules", "preferences", "conventions", "guidelines"]
        .iter()
        .any(|noun| framing_words.split(' ').any(|word| word == *noun));
    let base = list.as_ptr() as usize - text.as_ptr() as usize;
    let mut parts = Vec::new();
    let mut start = 0;
    for (index, _) in list.match_indices("; ") {
        if !quotations.touches_quotation(base + index, base + index + 1) {
            parts.push(&list[start..index]);
            start = index + 2;
        }
    }
    parts.push(&list[start..]);
    let items = parts
        .into_iter()
        .map(|item| item.trim().trim_end_matches(['.', ';']).trim())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    (names_rules && items.len() >= 2).then(|| {
        items
            .into_iter()
            .map(|item| {
                let at = item.as_ptr() as usize - text.as_ptr() as usize;
                (format!("{framing}: {item}."), at..at + item.len())
            })
            .collect()
    })
}

fn prose_rule(clause: &str) -> MarkedRule {
    let normalized = normalize(clause);
    MarkedRule {
        scope: has_phrase(&normalized, crate::user_rules::INVESTIGATION_PHRASES).then(|| {
            ScopeHint {
                title: clause.trim_end_matches(['.', '!']).to_string(),
                end_condition: end_condition(clause),
            }
        }),
        clause: RuleClause {
            text: clause.to_string(),
            standing: standing_of(&normalized),
        },
    }
}

/// The user's words for when a rule stops applying: from "until" to the end of its sentence.
pub(crate) fn end_condition(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(" until ")? + 1;
    let rest = &text[start..];
    let end = rest.find(['.', ';', '!']).unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

/// A count the user declared for their rules ("Two standing rules", "3 ground rules").
fn declared_count(text: &str) -> Option<u32> {
    const NUMBERS: &[(&str, u32)] = &[
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
    ];
    let words = normalize(text);
    let words = words.split(' ').collect::<Vec<_>>();
    words.iter().enumerate().find_map(|(index, word)| {
        let count = NUMBERS
            .iter()
            .find(|(name, _)| name == word)
            .map(|(_, count)| *count)
            .or_else(|| {
                word.parse::<u32>()
                    .ok()
                    .filter(|count| (2..=20).contains(count))
            })?;
        let following = words.get(index + 1..(index + 4).min(words.len()))?;
        let noun = following
            .iter()
            .position(|word| matches!(*word, "rules" | "preferences"))?;
        following[..noun]
            .iter()
            .all(|word| matches!(*word, "standing" | "ground" | "house" | "working"))
            .then_some(count)
    })
}

/// The first words of a unit, for reporting what could not be kept.
fn opening(text: &str) -> String {
    const MAX_OPENING_CHARS: usize = 80;
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match single.char_indices().nth(MAX_OPENING_CHARS) {
        Some((end, _)) => format!("{}...", &single[..end]),
        None => single,
    }
}

/// The complete unit of `text` (a list item with its continuation lines, or a prose line)
/// holding `clause`, in the user's words.
pub(crate) fn unit_text_for_clause(text: &str, clause: &str) -> Option<String> {
    let start = text.find(clause)?;
    let end = start + clause.len();
    rule_units(text)
        .into_iter()
        .find(|unit| {
            unit.lines.iter().any(|line| {
                let line_start = line.as_ptr() as usize - text.as_ptr() as usize;
                line_start <= start && end <= line_start + line.len()
            })
        })
        .map(|unit| unit.lines.join(" "))
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
            let end = start + clause.len();
            // A clause that is wholly a quotation is someone's words, whatever it says.
            let body = list_item_body(clause).map_or(*clause, str::trim_start);
            let body_start = end - body.len();
            !(is_reported_speech(&normalize(clause))
                || quotations.relays(start, end)
                || quotations.wholly_quoted(body_start, end)
                || asks_about_rules(clause))
        })
        .collect()
}

/// The one rule a list item states, judged as a whole so an ending condition or a task limit
/// in any of its sentences applies to all of it.
fn item_rule(unit: &RuleUnit<'_>, kept: &[&str], marker: Marker) -> Option<MarkedRule> {
    if kept.is_empty() {
        return None;
    }
    // The list marker ("1.", "-") is the message's layout, not the rule's words.
    let joined = kept.join(" ");
    let body = list_item_body(&joined)
        .map_or(joined.as_str(), str::trim)
        .to_string();
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
                    if has_explicit_task_limit(&normalized) {
                        RuleStanding::Pending
                    } else {
                        RuleStanding::Standing
                    }
                }
            };
            let (text, scope) = match header.limited_scope {
                // The header's limited scope stays with the rule, in the user's words.
                Some(scope) => {
                    let item = body.as_str();
                    let hint = ScopeHint {
                        title: scope.trim_end_matches(':').trim().to_string(),
                        end_condition: end_condition(item),
                    };
                    (format!("{scope} {item}"), Some(hint))
                }
                None => {
                    let scope = prose_rule(&body).scope;
                    (body, scope)
                }
            };
            Some(MarkedRule {
                clause: RuleClause { text, standing },
                scope,
            })
        }
        None => {
            if marker == Marker::Required
                && !kept
                    .iter()
                    .any(|clause| has_standing_marker(&normalize(clause)))
            {
                return None;
            }
            Some(prose_rule(&body))
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
    /// A blank line, fence or blockquote separates it from the unit before.
    separated: bool,
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
    let mut fence = Fence::default();
    let mut separated = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if fence.skips(trimmed) || trimmed.starts_with('>') {
            item_open = false;
            separated = true;
            continue;
        }
        if trimmed.is_empty() {
            separated = true;
            continue;
        }
        let was_in_list = in_list;
        if !in_list_item(line, trimmed, &mut in_list) {
            list_header = next_list_header(line, trimmed, was_in_list, list_header);
            units.push(RuleUnit {
                header: None,
                item: false,
                lines: vec![trimmed],
                separated,
            });
            separated = false;
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
                separated,
            }),
        }
        separated = false;
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
    let nested = was_in_list && line.starts_with([' ', '\t']);
    let inherited_scope = enclosing
        .filter(|_| nested)
        .and_then(|enclosing| enclosing.limited_scope);
    let own = own.map(|own| ListHeader {
        limited_scope: own.limited_scope.or(inherited_scope),
        ..own
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
