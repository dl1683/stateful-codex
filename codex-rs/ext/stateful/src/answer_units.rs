//! The outcomes a completed final answer states under plain headings, as complete units: each
//! item of a "Ruled out:" list, each item of an "Open checks:" list, and each "Decision:" with
//! the reason, alternatives and reconsideration condition written beside it. Every unit keeps
//! the byte range of the answer it was read from.
//!
//! Only explicit structure counts. A tentative or negated heading ("Not ruled out", "Possible
//! causes"), an inline list after a colon, quoted (`>` or a line opening with a quotation
//! mark), fenced or indented-code text, and prose without a label are left to the retained
//! conversation; nothing is guessed. Units are never cut: one longer than an entry can hold,
//! or a ruled-out item that is itself tentative, is reported as not kept.

use std::ops::Range;

use crate::answer_phrases::Label;
use crate::answer_phrases::heading_kind;
use crate::answer_phrases::is_tentative_rejection;
use crate::answer_phrases::is_unmade_choice;
use crate::answer_phrases::label_of;
use crate::user_rules::Fence;
use crate::user_rules::list_item_body;

/// Longest unit kept; the store's entry limit leaves room for a decision's labels.
pub(crate) const MAX_UNIT_BYTES: usize = 3_800;

pub(crate) const QUOTATION_OPENINGS: &[char] = &['"', '\'', '\u{201c}', '\u{2018}', '\u{ab}', '`'];

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
    /// Units not kept (too long to keep whole, or tentative), by kind, each with why.
    pub(crate) omitted: Vec<(AnswerUnitKind, String)>,
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
        let leading = &text[..text.len() - text.trim_start().len()];
        let indent = leading
            .chars()
            .map(|character| if character == '\t' { 4 } else { 1 })
            .sum();
        let line = Line {
            trimmed: text.trim(),
            indent,
            span: start + leading.len()..start + text.trim_end().len(),
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
        // An indented code block (an example of a format) is not this answer's structure.
        if line.indent >= 4 && !line.trimmed.is_empty() {
            finish(&mut block, &mut units);
            blank_before = false;
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
                if item.text.is_empty() {
                    continue;
                }
                if kind == AnswerUnitKind::RuledOut && is_tentative_rejection(&item.text) {
                    units.omitted.push((
                        kind,
                        format!("tentative, not saved as ruled out: {}", opening(&item.text)),
                    ));
                    continue;
                }
                if item.text.len() > MAX_UNIT_BYTES {
                    units.omitted.push((
                        kind,
                        format!("too long to keep whole: {}", opening(&item.text)),
                    ));
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
            if unit.choice.text.trim().is_empty() || is_unmade_choice(&unit.choice.text) {
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
                units.omitted.push((
                    AnswerUnitKind::Decision,
                    format!("too long to keep whole: {}", opening(&unit.choice.text)),
                ));
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
    let kind = label_of(label.trim_end_matches(['*', '_']))?;
    let rest = rest.trim_start_matches(['*', '_']);
    // A choice needs its words on the same line; the other parts may follow on later lines.
    if kind == Label::Choice && rest.trim().is_empty() {
        return None;
    }
    Some((kind, line.tail(rest)))
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
