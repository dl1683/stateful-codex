//! Host admission of direct project declarations: the frozen capture-c1-v3 positive grammar
//! (CONTRACT §3), project productions only. This is a finite syntax contract, not a speech
//! classifier: a sealed user text part either matches a supported production completely, or
//! nothing is admitted and the part stays ordinary sealed history. Literal syntax compares
//! ASCII-case-insensitively with flexible spaces between words; stored words are the exact
//! original bytes of each instruction unit.

use std::ops::Range;

#[cfg(test)]
#[path = "capture_admission_tests.rs"]
mod tests;

/// The grammar production a declaration matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Production {
    /// A project header followed by a list of supported units.
    ProjectList,
    /// One supported unit with an explicit project scope.
    ScopedImperative,
}

impl Production {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ProjectList => "projectList",
            Self::ScopedImperative => "scopedImperative",
        }
    }
}

/// A complete supported declaration: byte ranges into the sealed part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Declaration {
    pub(crate) production: Production,
    /// The governing header (list) or scope words (scoped imperative).
    pub(crate) envelope: Range<usize>,
    /// Each instruction unit's exact bytes, in the order written.
    pub(crate) units: Vec<Range<usize>>,
}

const MAX_UNITS: usize = 24;

const PROJECT_HEADERS: [&str; 4] = [
    "Standing rules for this project, please follow them in every session:",
    "Ground rules for this whole project, in every session from now on:",
    "Ground rules for this project:",
    "My standing rules for this project:",
];

/// The one informational preface allowed before an embedded header (CONTRACT §3).
const HORIZON_PREFACE: &str = "I maintain an internal fork of python-humanize for our dashboards, and I'll be working on it with you over the next few weeks, roughly one session a day.";

const TASK_SECTIONS: [&str; 3] = ["First task:", "Today's task:", "For today,"];

/// The supported declaration in `text`, or `None` when any part of the enclosing message is
/// outside the grammar. Unknown outer framing, quotation, questions, nested or mixed lists,
/// trailing qualifiers and unsupported bodies all fail closed.
pub(crate) fn project_declaration(text: &str) -> Option<Declaration> {
    let lines = lines(text);
    let (first, rest) = lines.split_first()?;
    let first_text = &text[first.clone()];
    if let Some(envelope) = header(first_text) {
        let envelope = first.start + envelope.start..first.start + envelope.end;
        let (units, tail) = list(text, rest)?;
        task_tail(text, tail)?;
        return Some(Declaration {
            production: Production::ProjectList,
            envelope,
            units,
        });
    }
    let (envelope, unit) = scoped_imperative(first_text)?;
    let unit_range = first.start + unit.start..first.start + unit.end;
    task_tail(text, rest)?;
    Some(Declaration {
        production: Production::ScopedImperative,
        envelope: first.start + envelope.start..first.start + envelope.end,
        units: vec![unit_range],
    })
}

/// Line ranges without their terminators; a CR before LF is excluded from the line.
fn lines(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            let end = if index > start && text.as_bytes()[index - 1] == b'\r' {
                index - 1
            } else {
                index
            };
            lines.push(start..end);
            start = index + 1;
        }
    }
    if start < text.len() {
        lines.push(start..text.len());
    }
    lines
}

/// A standalone project header, or the allowed preface followed by one, on the first line.
fn header(line: &str) -> Option<Range<usize>> {
    for header in PROJECT_HEADERS {
        if literal(line, 0, header) == Some(line.len()) {
            return Some(0..line.len());
        }
    }
    let mut at = 0;
    if let Some(end) = literal(line, at, "Hi.") {
        at = spaces(line, end)?;
    }
    at = literal(line, at, HORIZON_PREFACE)?;
    let start = spaces(line, at)?;
    PROJECT_HEADERS.iter().find_map(|header| {
        (literal(line, start, header) == Some(line.len())).then_some(start..line.len())
    })
}

/// `For this project, <unit>` or `<unit> for this project.` as the whole first line.
fn scoped_imperative(line: &str) -> Option<(Range<usize>, Range<usize>)> {
    if let Some(start) = literal(line, 0, "For this project,") {
        let start = spaces(line, start)?;
        let unit = start..line.trim_end().len();
        return supported_body(&line[unit.clone()]).then_some((0..start, unit));
    }
    let trimmed = line.trim_end();
    let suffix = " for this project.";
    let body_end = trimmed.len().checked_sub(suffix.len())?;
    if !trimmed.is_char_boundary(body_end) || !trimmed[body_end..].eq_ignore_ascii_case(suffix) {
        return None;
    }
    let body = &trimmed[..body_end];
    // A terminal period is not allowed before a trailing scope suffix.
    if body.ends_with('.') || !supported_body(body) {
        return None;
    }
    Some((body_end + 1..trimmed.len(), 0..body_end))
}

/// A list of 1..24 sibling items, all `- ` or all contiguous `N. `, ending at a blank line or
/// the end of the message. Indented lines continue the previous item; any other line fails.
/// A list's unit ranges and the lines after it.
type ListParts<'a> = (Vec<Range<usize>>, &'a [Range<usize>]);

fn list<'a>(text: &str, lines: &'a [Range<usize>]) -> Option<ListParts<'a>> {
    let mut units: Vec<Range<usize>> = Vec::new();
    let mut ordered = None;
    let mut index = 0;
    while let Some(line) = lines.get(index) {
        let content = &text[line.clone()];
        if content.trim().is_empty() {
            break;
        }
        if content.starts_with([' ', '\t']) {
            // A continuation line; nested list markers are unsupported.
            let continued = content.trim_start();
            let digits = continued.bytes().take_while(u8::is_ascii_digit).count();
            if units.is_empty()
                || continued.starts_with("- ")
                || (digits > 0 && continued[digits..].starts_with(". "))
            {
                return None;
            }
            units.last_mut()?.end = line.start + content.trim_end().len();
            index += 1;
            continue;
        }
        let body = marker(content, &mut ordered, units.len())?;
        units.push(line.start + body..line.start + content.trim_end().len());
        index += 1;
    }
    if units.is_empty() || units.len() > MAX_UNITS {
        return None;
    }
    units
        .iter()
        .all(|unit| supported_body(&text[unit.clone()]))
        .then_some((units, &lines[index..]))
}

/// The byte where an item's body starts after its marker. `ordered` fixes the list style.
fn marker(line: &str, ordered: &mut Option<bool>, position: usize) -> Option<usize> {
    if let Some(rest) = line.strip_prefix("- ") {
        if *ordered == Some(true) {
            return None;
        }
        *ordered = Some(false);
        return Some(line.len() - rest.len());
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let number = line.get(..digits)?.parse::<usize>().ok()?;
    if digits == 0 || !line[digits..].starts_with(". ") || *ordered == Some(false) {
        return None;
    }
    if number != position + 1 {
        return None;
    }
    *ordered = Some(true);
    Some(digits + 2)
}

/// After the declaration: nothing, or a blank line then an explicit task section. The task
/// text is a request, never additional standing rules.
fn task_tail(text: &str, lines: &[Range<usize>]) -> Option<()> {
    let Some(next) = lines
        .iter()
        .position(|line| !text[line.clone()].trim().is_empty())
    else {
        return Some(());
    };
    if next == 0 {
        return None;
    }
    let line = &text[lines[next].clone()];
    TASK_SECTIONS
        .iter()
        .any(|section| literal(line, 0, section).is_some())
        .then_some(())
}

/// Whether a unit (with an optional terminal period) is a supported body (CONTRACT §3).
pub(crate) fn supported_body(unit: &str) -> bool {
    let normalized = unit.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = normalized.strip_suffix('.').unwrap_or(&normalized);
    use Piece::Lit;
    use Piece::Param;
    let catalogue: [&[Piece]; 17] = [
        &[Lit("Always test first")],
        &[Lit("Never push")],
        &[Lit("Do not push")],
        &[Lit("Never run git commit")],
        &[Lit("Always include arXiv IDs")],
        &[Lit("Never include arXiv IDs")],
        &[
            Lit("Always use "),
            Param(Slot::CitationStyle),
            Lit(" citations"),
        ],
        &[Lit("Use "), Param(Slot::Spelling), Lit(" spelling")],
        &[
            Lit("Do not adopt "),
            Param(Slot::Speaker),
            Lit("'s workflow"),
        ],
        &[
            Lit("Don't touch "),
            Param(Slot::Path),
            Lit(" or any changelog"),
        ],
        &[Lit(
            "Only run the tests relevant to what you changed, never the whole suite",
        )],
        &[Lit(
            "Never install anything into my global Python. If you need an environment, make a local venv inside the repo",
        )],
        &[
            Lit("End every reply with one line starting with `"),
            Param(Slot::Marker),
            Lit("` that suggests the next step"),
        ],
        &[Lit("Always end each reply with "), Param(Slot::Marker)],
        &[
            Lit("Never run git commit and always end each reply with "),
            Param(Slot::Marker),
        ],
        &[Lit(
            "Cite the paper (arXiv id) and the section for every claim",
        )],
        &[Lit(
            "Clearly distinguish evidence (what a paper actually measured or showed) from speculation or interpretation, whether it's the authors' or yours",
        )],
    ];
    catalogue
        .iter()
        .any(|pattern| matches_pattern(body, pattern))
}

#[derive(Clone, Copy)]
enum Piece {
    Lit(&'static str),
    Param(Slot),
}

#[derive(Clone, Copy)]
enum Slot {
    CitationStyle,
    Spelling,
    Speaker,
    Path,
    Marker,
}

fn matches_pattern(body: &str, pattern: &[Piece]) -> bool {
    let mut at = 0;
    for piece in pattern {
        let next = match piece {
            Piece::Lit(literal_text) => literal(body, at, literal_text),
            Piece::Param(slot) => parameter(body, at, *slot),
        };
        match next {
            Some(next) => at = next,
            None => return false,
        }
    }
    at == body.len()
}

/// A bounded exact parameter starting at `at`; returns its end.
fn parameter(body: &str, at: usize, slot: Slot) -> Option<usize> {
    let rest = body.get(at..)?;
    match slot {
        Slot::CitationStyle => ["author-year", "numeric"]
            .iter()
            .find_map(|style| literal(body, at, style)),
        Slot::Spelling => ["British", "US"]
            .iter()
            .find_map(|spelling| literal(body, at, spelling)),
        Slot::Speaker => {
            let length = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
            (1..=40).contains(&length).then_some(at + length)
        }
        Slot::Marker => {
            let length = rest
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                .count();
            ((1..=32).contains(&length) && rest[length..].starts_with(':'))
                .then_some(at + length + 1)
        }
        Slot::Path => {
            let length = rest
                .bytes()
                .take_while(|byte| !byte.is_ascii_whitespace())
                .count();
            repo_relative_path(&rest[..length]).then_some(at + length)
        }
    }
}

/// A validated 1..512-byte repository-relative path: no whitespace, quotes, control
/// characters, colon, backslash, absolute root or traversal.
fn repo_relative_path(path: &str) -> bool {
    (1..=512).contains(&path.len())
        && !path.starts_with('/')
        && !path.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '"' | '\'' | '`' | ':' | '\\')
        })
        && path
            .split('/')
            .all(|component| component != ".." && component != ".")
}

/// Matches `literal` at `at` ASCII-case-insensitively, letting one run of spaces/tabs in the
/// text stand for each single space of the literal. Returns the end of the match.
fn literal(text: &str, at: usize, literal: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut position = at;
    for expected in literal.bytes() {
        if expected == b' ' {
            let start = position;
            while bytes
                .get(position)
                .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
            {
                position += 1;
            }
            if position == start {
                return None;
            }
            continue;
        }
        if !bytes
            .get(position)
            .is_some_and(|byte| byte.eq_ignore_ascii_case(&expected))
        {
            return None;
        }
        position += 1;
    }
    text.is_char_boundary(position).then_some(position)
}

/// At least one space or tab at `at`; returns the first byte after them.
fn spaces(text: &str, at: usize) -> Option<usize> {
    let end = at
        + text[at..]
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
    (end > at).then_some(end)
}

impl crate::StatefulExtension {
    /// Admits the supported declaration in one freshly sealed part through the common PI
    /// source-group writer, then reports its committed receipt. A redelivered source keeps its
    /// first outcome. Words the user forgot are refused, never restored by a later statement.
    pub(crate) async fn admit_declaration(
        &self,
        services: &crate::services::ProjectIntelligenceServices,
        admission: &codex_state::ThreadProjectAdmission,
        seal: &codex_project_intelligence::SourceSeal,
        text: &str,
    ) {
        use crate::memory_receipts::MemoryReceipt;
        use crate::memory_receipts::ReceiptKind;
        use crate::memory_receipts::ReceiptMember;
        use crate::memory_receipts::ReceiptStatus;
        use codex_project_intelligence::*;
        use serde_json::json;
        use sha2::Digest;
        if !seal.observation.complete_envelope {
            return;
        }
        let Some(declaration) = project_declaration(text) else {
            return;
        };
        let project = admission.project_id();
        let digest = |value: &str| format!("{:x}", sha2::Sha256::digest(value.as_bytes()));
        let group_id = format!("rules-{}", digest(&seal.exact_source_locator));
        let Ok(store) = services.blackboard().await else {
            return;
        };
        match store.capture_group(project, &group_id).await {
            Ok(None) => {}
            Ok(Some(_)) => return,
            Err(error) => {
                tracing::warn!(%error, "declaration admission unavailable");
                return;
            }
        }
        let node = match services.project_node_id(project).await {
            Ok(node) => node,
            Err(error) => {
                tracing::warn!(%error, "declaration admission unavailable: no project node");
                return;
            }
        };
        let Ok(confidence) = ConfidenceScore::from_basis_points(/*value*/ 10_000) else {
            return;
        };
        let temporal = TemporalContext {
            version: 1,
            recorded_at_ms: seal.recorded_at_ms,
            source_time: SourceTime::HostObserved {
                unix_ms: seal.recorded_at_ms,
                precision: TimePrecision::Second,
            },
            event_time: EventTime::Unknown,
            event_status: EventStatus::CurrentAssertion,
        };
        let declared = declaration.units.len();
        let mut members = Vec::with_capacity(declared);
        for (ordinal, unit) in declaration.units.iter().enumerate() {
            let content = &text[unit.clone()];
            let Ok(id) = BlackboardEntryId::parse(format!(
                "stateful-declared-rule-{}",
                digest(&format!("{}:{ordinal}", seal.exact_source_locator))
            )) else {
                return;
            };
            let payload = json!({"admission": {"version": 1, "contract": "capture-c1-v3",
                "production": declaration.production.as_str(), "scope": "project", "by": "user",
                "envelope": {"startByte": declaration.envelope.start, "endByte": declaration.envelope.end},
                "ordinal": ordinal, "declared": declared}, "temporal": temporal});
            members.push(SourceCaptureMember {
                write: CaptureEntryWrite {
                    candidates: vec![id],
                    value: NewBlackboardEntry {
                        project_id: project.to_string(),
                        node_id: node.clone(),
                        kind: BlackboardKind::Instruction,
                        content: content.to_string(),
                        structured_value: None,
                        confidence,
                        verification: BlackboardVerification::Unverified,
                        importance: BlackboardImportance::High,
                        root_promotion: RootPromotion::Promoted,
                        evidence: Vec::new(),
                        premises: Vec::new(),
                        provenance: BlackboardProvenance {
                            kind: BlackboardProvenanceKind::User,
                            source_id: seal.exact_source_locator.clone(),
                        },
                    },
                    context: KnowledgeContext {
                        payload: Some(payload.to_string()),
                        ..KnowledgeContext::new(
                            KnowledgeCategory::Rule,
                            KnowledgeAuthority::HumanDirect,
                        )
                    },
                    change: ChangeRecord {
                        operation: ChangeOperation::Saved,
                        origin: ChangeOrigin::HostCapture,
                        category: KnowledgeCategory::Rule,
                        action_id: None,
                        thread_id: Some(admission.thread_id().to_string()),
                        turn_id: Some(seal.observation.turn_id.clone()),
                        group_id: None,
                        preview: crate::events::receipt_text(content),
                    },
                },
                seal: seal.clone(),
                spans: vec![SourceSpan {
                    start_byte: unit.start as u32,
                    end_byte: unit.end as u32,
                    role: SourceSpanRole::Body,
                }],
            });
        }
        let result = store
            .write_source_group(
                admission,
                SourceCaptureGroup {
                    project_id: project.to_string(),
                    action_id: format!("admit-{}", digest(&seal.exact_source_locator)),
                    group_id: group_id.clone(),
                    members,
                    existing: ExistingWording::AcknowledgeIdentical,
                },
            )
            .await;
        let receipt = match result {
            // Everything was already current: no new receipt, no repeated notification.
            Ok(group) if group.saved == 0 => return,
            Ok(group) => {
                MemoryReceipt::of_group(&group, ReceiptKind::Rules, KnowledgeCategory::Rule)
            }
            Err(BlackboardStoreError::RetiredIdentity | BlackboardStoreError::SourceExcluded) => {
                // Nothing was committed. Name the forgotten words and hold back the rest.
                let mut members = Vec::with_capacity(declared);
                for unit in &declaration.units {
                    let content = &text[unit.clone()];
                    let eligible = store
                        .source_text_eligible(project, content)
                        .await
                        .unwrap_or(false);
                    let status = if eligible {
                        ReceiptStatus::Refused
                    } else {
                        ReceiptStatus::NotRestored
                    };
                    members.push(ReceiptMember::new(
                        /*entry*/ None,
                        KnowledgeCategory::Rule,
                        status,
                        content,
                    ));
                }
                MemoryReceipt {
                    project_id: project.to_string(),
                    thread_id: admission.thread_id().to_string(),
                    turn_id: Some(seal.observation.turn_id.clone()),
                    receipt_id: group_id,
                    kind: ReceiptKind::Rules,
                    members,
                    undoable: false,
                }
            }
            Err(error) => {
                tracing::warn!(%error, "declaration not admitted; source remains history");
                return;
            }
        };
        if let Some(sink) = &self.event_sink {
            sink.emit(crate::StatefulEvent::MemoryReceipt(receipt));
        }
    }
}
