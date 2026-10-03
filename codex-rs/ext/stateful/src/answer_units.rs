//! The outcomes a completed final answer states under plain headings, as complete units: each
//! item of a "Ruled out:" list, each item of an "Open checks:" list, and each "Decision:" with
//! the reason, alternatives and reconsideration condition written beside it. Every unit keeps
//! the byte range of the answer it was read from.
//!
//! Only explicit structure counts. A tentative heading ("Not ruled out", "Possible causes"),
//! a tentative item ("possibly the proxy; not checked"), an inline list after a colon, quoted
//! (`>` or a line opening with a quotation mark) or fenced text, and prose without a label are
//! left to the retained conversation; nothing is guessed. Units are never cut: one longer
//! than an entry can hold is reported as omitted.

use std::ops::Range;

use crate::user_rules::Fence;
use crate::user_rules::has_phrase;
use crate::user_rules::list_item_body;
use crate::user_rules::normalize;

/// Longest unit kept; the store's entry limit leaves room for a decision's labels.
pub(crate) const MAX_UNIT_BYTES: usize = 3_800;
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
const QUOTATION_OPENINGS: &[char] = &['"', '\'', '\u{201c}', '\u{2018}', '\u{ab}', '`'];

/// Words of the answer and where they are in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceText {
    pub(crate) text: String,
    /// Byte range of the answer the words were read from (list markers and labels excluded).
    pub(crate) span: Range<usize>,
}

impl SourceText {
    fn extend(&mut self, line: &Line<'_>) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(line.trimmed);
        self.span.end = line.span.end;
    }
}

/// What a list holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnswerUnitKind {
    RuledOut,
    OpenCheck,
    Decision,
}

/// One decision as the answer wrote it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DecisionUnit {
    pub(crate) choice: SourceText,
    /// `None` when the answer gave no reason; never filled in.
    pub(crate) reason: Option<SourceText>,
    pub(crate) alternatives: Vec<SourceText>,
    pub(crate) reconsider_if: Option<SourceText>,
}

impl DecisionUnit {
    /// The decision as one readable entry, each part under the label it was written with.
    pub(crate) fn content(&self) -> String {
        let mut content = format!("Decision: {}", self.choice.text);
        match &self.reason {
            Some(reason) => content.push_str(&format!("\nReason: {}", reason.text)),
            None => content.push_str("\nReason: not recorded in the answer"),
        }
        if !self.alternatives.is_empty() {
            let alternatives = self
                .alternatives
                .iter()
                .map(|alternative| alternative.text.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            content.push_str(&format!("\nAlternatives considered: {alternatives}"));
        }
        if let Some(condition) = &self.reconsider_if {
            content.push_str(&format!("\nReconsider if: {}", condition.text));
        }
        content
    }

    /// The whole decision's range in the answer.
    pub(crate) fn span(&self) -> Range<usize> {
        let parts = std::iter::once(&self.choice)
            .chain(&self.reason)
            .chain(&self.alternatives)
            .chain(&self.reconsider_if);
        let (start, end) = parts.fold((usize::MAX, 0), |(start, end), part| {
            (start.min(part.span.start), end.max(part.span.end))
        });
        start..end
    }
}

/// Everything one answer states under recognized headings, in the order written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AnswerUnits {
    pub(crate) ruled_out: Vec<SourceText>,
    pub(crate) open_checks: Vec<SourceText>,
    pub(crate) decisions: Vec<DecisionUnit>,
    /// Units too long to keep whole, by kind and opening words.
    pub(crate) omitted: Vec<(AnswerUnitKind, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Label {
    Choice,
    Reason,
    Alternatives,
    Reconsider,
}

/// One line of the answer, trimmed, with the byte range of its trimmed text.
struct Line<'a> {
    trimmed: &'a str,
    indent: usize,
    span: Range<usize>,
}

impl<'a> Line<'a> {
    /// The same line from `rest`, a suffix of its trimmed text.
    fn tail(&self, rest: &'a str) -> Line<'a> {
        let rest_trimmed = rest.trim();
        let skipped = self.trimmed.len() - rest.len() + (rest.len() - rest.trim_start().len());
        Line {
            trimmed: rest_trimmed,
            indent: self.indent,
            span: self.span.start + skipped..self.span.start + skipped + rest_trimmed.len(),
        }
    }

    fn source(&self) -> SourceText {
        SourceText {
            text: self.trimmed.to_string(),
            span: self.span.clone(),
        }
    }
}

/// What the line being read belongs to.
enum Block {
    None,
    /// A list under a heading, with the indentation of its items once the first is seen.
    List {
        kind: AnswerUnitKind,
        indent: Option<usize>,
        items: Vec<SourceText>,
    },
    /// A decision, and the label the last line extended.
    Decision {
        unit: DecisionUnit,
        last: Label,
    },
}

/// The units `answer` states under recognized headings and labels.
pub(crate) fn answer_units(answer: &str) -> AnswerUnits {
    let mut units = AnswerUnits::default();
    let mut block = Block::None;
    let mut fence = Fence::default();
    let mut blank_before = false;
    let mut offset = 0;
    for raw in answer.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let text = raw.trim_end_matches(['\n', '\r']);
        let indent = text.len() - text.trim_start().len();
        let line = Line {
            trimmed: text.trim(),
            indent,
            span: start + indent..start + text.trim_end().len(),
        };
        let item_indent = match &block {
            Block::List {
                indent: Some(indent),
                ..
            } => Some(*indent),
            _ => None,
        };
        // A fence inside a list item is part of the item; anywhere else it is not the
        // answer's own structure.
        if item_indent.is_some_and(|item| line.indent > item)
            && !line.trimmed.is_empty()
            && let Block::List { items, .. } = &mut block
            && let Some(current) = items.last_mut()
        {
            fence.skips(line.trimmed);
            current.extend(&line);
            blank_before = false;
            continue;
        }
        if fence.skips(line.trimmed) {
            finish(&mut block, &mut units);
            continue;
        }
        if line.trimmed.is_empty() {
            blank_before = true;
            continue;
        }
        if line.trimmed.starts_with('>') {
            finish(&mut block, &mut units);
            blank_before = false;
            continue;
        }
        let blank = std::mem::take(&mut blank_before);
        match &mut block {
            Block::List {
                indent: list_indent,
                items,
                ..
            } => match (*list_indent, list_item_body(line.trimmed)) {
                (None, Some(body)) => {
                    *list_indent = Some(line.indent);
                    items.push(line.tail(body).source());
                    continue;
                }
                (Some(first), Some(body)) if line.indent == first => {
                    items.push(line.tail(body).source());
                    continue;
                }
                _ => finish(&mut block, &mut units),
            },
            Block::Decision { unit, last } => {
                if let Some((label, rest)) = labelled(&line)
                    && label != Label::Choice
                {
                    apply_label(unit, label, rest);
                    *last = label;
                    continue;
                }
                if *last == Label::Alternatives
                    && let Some(body) = list_item_body(line.trimmed)
                {
                    unit.alternatives.push(line.tail(body).source());
                    continue;
                }
                if !blank && heading_kind(line.trimmed).is_none() && labelled(&line).is_none() {
                    extend_label(unit, *last, &line);
                    continue;
                }
                finish(&mut block, &mut units);
            }
            Block::None => {}
        }
        if let Some((Label::Choice, rest)) = labelled(&line) {
            let mut unit = DecisionUnit::default();
            apply_label(&mut unit, Label::Choice, rest);
            block = Block::Decision {
                unit,
                last: Label::Choice,
            };
            continue;
        }
        if let Some(kind) = heading_kind(line.trimmed) {
            block = Block::List {
                kind,
                indent: None,
                items: Vec::new(),
            };
        }
    }
    finish(&mut block, &mut units);
    units
}

/// Moves a finished block's units into `units`.
fn finish(block: &mut Block, units: &mut AnswerUnits) {
    match std::mem::replace(block, Block::None) {
        Block::None => {}
        Block::List { kind, items, .. } => {
            for item in items {
                let tentative = kind == AnswerUnitKind::RuledOut
                    && has_phrase(&normalized(&item.text), NOT_REJECTED_PHRASES);
                if item.text.is_empty() || tentative {
                    continue;
                }
                if item.text.len() > MAX_UNIT_BYTES {
                    units.omitted.push((kind, opening(&item.text)));
                    continue;
                }
                match kind {
                    AnswerUnitKind::RuledOut => units.ruled_out.push(item),
                    AnswerUnitKind::OpenCheck => units.open_checks.push(item),
                    AnswerUnitKind::Decision => {}
                }
            }
        }
        Block::Decision { mut unit, .. } => {
            let choice = normalized(&unit.choice.text);
            let unmade = UNMADE_CHOICES
                .iter()
                .any(|unmade| choice == *unmade || choice.starts_with(&format!("{unmade} ")));
            if choice.is_empty() || unmade {
                return;
            }
            if unit.reason.is_none()
                && let Some((before, after)) = unit.choice.text.split_once(" because ")
                && !before.trim().is_empty()
                && !after.trim().is_empty()
                && !unit.choice.text.contains('\n')
            {
                let start = unit.choice.span.start;
                let reason_start = start + before.len() + " because ".len();
                let reason = SourceText {
                    text: after.trim().to_string(),
                    span: reason_start..unit.choice.span.end,
                };
                unit.choice = SourceText {
                    text: before.trim().to_string(),
                    span: start..start + before.trim_end().len(),
                };
                unit.reason = Some(reason);
            }
            if unit.content().len() > MAX_UNIT_BYTES {
                units
                    .omitted
                    .push((AnswerUnitKind::Decision, opening(&unit.choice.text)));
            } else {
                units.decisions.push(unit);
            }
        }
    }
}

fn apply_label(unit: &mut DecisionUnit, label: Label, rest: Line<'_>) {
    let part = (!rest.trimmed.is_empty()).then(|| rest.source());
    match label {
        Label::Choice => unit.choice = rest.source(),
        Label::Reason => unit.reason = part,
        Label::Alternatives => {
            if let Some(part) = part {
                let mut cursor = 0;
                for alternative in part.text.split(';') {
                    let start = part.span.start
                        + cursor
                        + (alternative.len() - alternative.trim_start().len());
                    cursor += alternative.len() + 1;
                    let alternative = alternative.trim();
                    if !alternative.is_empty() {
                        unit.alternatives.push(SourceText {
                            text: alternative.to_string(),
                            span: start..start + alternative.len(),
                        });
                    }
                }
            }
        }
        Label::Reconsider => unit.reconsider_if = part,
    }
}

/// Adds a continuation line to the part the previous line wrote.
fn extend_label(unit: &mut DecisionUnit, label: Label, line: &Line<'_>) {
    let extend = |part: &mut Option<SourceText>| match part {
        Some(part) => part.extend(line),
        None => *part = Some(line.source()),
    };
    match label {
        Label::Choice => unit.choice.extend(line),
        Label::Reason => extend(&mut unit.reason),
        Label::Reconsider => extend(&mut unit.reconsider_if),
        Label::Alternatives => unit.alternatives.push(line.source()),
    }
}

/// The label a line opens with ("Decision:", "**Why:**", "- Reason:") and the words after it.
/// A line opening with a quotation mark is someone's words, not a label.
fn labelled<'a>(line: &Line<'a>) -> Option<(Label, Line<'a>)> {
    let body = list_item_body(line.trimmed).map_or(line.trimmed, str::trim);
    if body.starts_with(QUOTATION_OPENINGS) {
        return None;
    }
    let body = body.trim_start_matches(['*', '_']);
    let (label, rest) = body.split_once(':')?;
    let label = normalize(label.trim_end_matches(['*', '_']));
    let rest = rest.trim_start_matches(['*', '_']);
    let kind = if CHOICE_LABELS.contains(&label.as_str()) {
        Label::Choice
    } else if REASON_LABELS.contains(&label.as_str()) {
        Label::Reason
    } else if ALTERNATIVE_LABELS.contains(&label.as_str()) {
        Label::Alternatives
    } else if RECONSIDER_LABELS.contains(&label.as_str()) {
        Label::Reconsider
    } else {
        return None;
    };
    // A choice needs its words on the same line; the other parts may follow on later lines.
    if kind == Label::Choice && rest.trim().is_empty() {
        return None;
    }
    Some((kind, line.tail(rest)))
}

/// The list a heading line introduces: a Markdown heading, a bold line, or a short line
/// ending with a colon, with nothing after the colon.
fn heading_kind(trimmed: &str) -> Option<AnswerUnitKind> {
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
    let ruled_out = has_phrase(&words, RULED_OUT_PHRASES) && !has_phrase(&words, TENTATIVE_PHRASES);
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

fn opening(text: &str) -> String {
    let mut end = text.len().min(80);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
#[path = "answer_units_tests.rs"]
mod tests;
