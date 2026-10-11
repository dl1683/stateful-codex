//! Semantic capsule records validated from a compaction trailer: speaker-derived labels,
//! kind-specific completeness, merge and carry-forward with the original provenance, and the
//! text of each whole record.
//!
//! A record is admitted only from a `SidecarOutcome::Validated` proposal, so every quotation
//! is an exact span of an original user or published assistant message in the request it was
//! produced from. That proves faithful quotation and attribution, not truth: assistant-origin
//! records always render `reported`, and an unquoted uncertainty or run is rendered as unknown,
//! never as certain. Prior records carry forward on their own provenance; nothing here reads a
//! summary or calls a model.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use super::Exclusions;
use super::MAX_CANDIDATES;
use super::render::escape;
use super::source_validation::Record;
use super::source_validation::RecordError;
use super::source_validation::SidecarOutcome;
use super::source_validation::SourceSpan;
use super::source_validation::Speaker;
use super::source_validation::Supersession;

/// Version of the persisted semantic record schema; another version is never reinterpreted.
pub(crate) const SEMANTIC_VERSION: u32 = 1;
/// Proposed supersession links kept for the directive policy.
const MAX_LINKS: usize = 16;

/// Host-owned copy of one accepted span: its original identity, digests and order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Quote {
    pub text: String,
    pub user: bool,
    pub item_id: String,
    pub turn_id: String,
    pub part: usize,
    pub start: usize,
    pub end: usize,
    pub part_sha256: String,
    pub span_sha256: String,
    pub revision: String,
    pub sequence: u64,
}

impl Quote {
    fn identity(&self) -> (&str, usize, usize, usize) {
        (&self.item_id, self.part, self.start, self.end)
    }
}

impl From<SourceSpan> for Quote {
    fn from(span: SourceSpan) -> Self {
        Self {
            text: span.quote,
            user: span.speaker == Speaker::User,
            item_id: span.item_id,
            turn_id: span.turn_id,
            part: span.part_index,
            start: span.byte_start,
            end: span.byte_end,
            part_sha256: span.source_part_sha256,
            span_sha256: span.selected_span_sha256,
            revision: span.source_revision,
            sequence: span.host_sequence,
        }
    }
}

/// Kinds in packing priority: an unresolved rejection outranks a diagnosis, which outranks a
/// choice, which outranks a measurement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticKind {
    Rejection,
    Diagnosis,
    Choice,
    Measurement,
}

/// One whole validated record. `main` is the finding, choice, rejected item or value;
/// `qualifier` is the uncertainty, reason or run; `detail` is a measurement's qualifier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticRecord {
    pub kind: SemanticKind,
    pub main: Quote,
    pub subject: Quote,
    pub qualifier: Option<Quote>,
    pub detail: Option<Quote>,
    /// Fingerprint of the compaction request whose index validated this record.
    pub request_sha256: String,
}

impl SemanticRecord {
    fn quotes(&self) -> impl Iterator<Item = &Quote> {
        [Some(&self.main), Some(&self.subject)]
            .into_iter()
            .chain([self.qualifier.as_ref(), self.detail.as_ref()])
            .flatten()
    }

    /// Earliest original source, the stable rank inside a kind.
    pub(crate) fn first_sequence(&self) -> u64 {
        self.quotes().map(|q| q.sequence).min().unwrap_or_default()
    }

    fn reported(&self) -> bool {
        self.quotes().any(|quote| !quote.user)
    }

    pub(crate) fn excluded_by(&self, exclusions: &Exclusions) -> bool {
        self.quotes().any(|quote| {
            exclusions.turns.contains(&quote.turn_id)
                || exclusions
                    .retired_texts
                    .iter()
                    .any(|retired| !retired.is_empty() && quote.text.contains(retired.as_str()))
        })
    }

    /// Whole-record text with speaker labels; the subject is shown only when no other field
    /// already quotes it.
    pub(crate) fn render(&self, alias: &str) -> String {
        let origin = if self.reported() { "reported" } else { "user" };
        let quoted = |quote: &Quote| format!("\"{}\"", escape(&quote.text));
        let others = [
            Some(&self.main),
            self.qualifier.as_ref(),
            self.detail.as_ref(),
        ];
        let subject_shown = !others
            .iter()
            .flatten()
            .any(|quote| quote.text.contains(self.subject.text.as_str()));
        let subject = |joiner: &str| {
            if subject_shown {
                format!(" {joiner} {}", quoted(&self.subject))
            } else {
                String::new()
            }
        };
        let main = quoted(&self.main);
        match self.kind {
            SemanticKind::Diagnosis => {
                let uncertainty = match &self.qualifier {
                    Some(quote) => quoted(quote),
                    None => "uncertainty unspecified".to_string(),
                };
                format!(
                    "diagnosis({origin}; {uncertainty}): {main}{} {alias}",
                    subject("re")
                )
            }
            SemanticKind::Choice | SemanticKind::Rejection => {
                let label = if self.kind == SemanticKind::Choice {
                    "choice"
                } else {
                    "rejected"
                };
                let reason = self.qualifier.as_ref().map(quoted).unwrap_or_default();
                format!(
                    "{label}({origin}): {main}{} because {reason} {alias}",
                    subject("for")
                )
            }
            SemanticKind::Measurement => {
                let run = match &self.qualifier {
                    Some(quote) => format!("run {}", quoted(quote)),
                    None => "run unknown".to_string(),
                };
                let detail = self
                    .detail
                    .as_ref()
                    .map(|quote| format!("; {}", quoted(quote)))
                    .unwrap_or_default();
                format!(
                    "measured({origin}; {run}): {main}{}{detail} {alias}",
                    subject("of")
                )
            }
        }
    }
}

/// A validated, advisory supersession proposal: exact old and new wording plus a user
/// correction. It never mutates stored rules; the directive policy decides what it means.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SupersessionLink {
    pub old: Quote,
    pub new: Quote,
    pub correction: Quote,
}

/// Coverage debt reason codes; counts are aggregate and never repeat source text.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DebtReason {
    MissingSource,
    Unindexed,
    InvalidSpan,
    SemanticUnavailable,
    UnknownRun,
    UnresolvedAuthority,
    Ineligible,
    Oversize,
    Packing,
    SourceGap,
    TokenizerUnavailable,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CoverageDebt(pub BTreeMap<DebtReason, u32>);

impl CoverageDebt {
    pub(crate) fn add(&mut self, reason: DebtReason, count: u32) {
        if count > 0 {
            *self.0.entry(reason).or_default() += count;
        }
    }

    fn absorb(&mut self, other: &Self) {
        for (reason, count) in &other.0 {
            self.add(*reason, *count);
        }
    }
}

/// The semantic result of one compaction boundary.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExtractionBatch {
    pub request_sha256: String,
    /// False for a missing or malformed trailer: no new semantic coverage at this boundary.
    pub semantic_available: bool,
    pub records: Vec<SemanticRecord>,
    pub links: Vec<SupersessionLink>,
    pub debt: CoverageDebt,
}

impl ExtractionBatch {
    /// Converts a validator outcome. `unindexed` counts eligible parts left out of the index.
    pub(crate) fn from_outcome(
        outcome: SidecarOutcome,
        request_sha256: &str,
        unindexed: usize,
    ) -> Self {
        let mut batch = Self {
            request_sha256: request_sha256.to_string(),
            ..Self::default()
        };
        batch.debt.add(DebtReason::Unindexed, count(unindexed));
        let (records, supersessions, rejected) = match outcome {
            SidecarOutcome::Absent | SidecarOutcome::Floor(_) => {
                batch.debt.add(DebtReason::SemanticUnavailable, 1);
                return batch;
            }
            SidecarOutcome::Validated {
                records,
                supersessions,
                rejected_records,
                rejected_supersessions,
            } => (
                records,
                supersessions,
                rejected_records.into_iter().chain(rejected_supersessions),
            ),
        };
        batch.semantic_available = true;
        for (_, error) in rejected {
            let reason = match error {
                RecordError::Quota => DebtReason::Oversize,
                RecordError::CorrectionSpeaker => DebtReason::UnresolvedAuthority,
                RecordError::Span(_) | RecordError::Order => DebtReason::InvalidSpan,
            };
            batch.debt.add(reason, 1);
        }
        for record in records {
            let record = semantic_record(record, request_sha256);
            if record.kind == SemanticKind::Measurement && record.qualifier.is_none() {
                batch.debt.add(DebtReason::UnknownRun, 1);
            }
            if !complete(&record) {
                batch.debt.add(DebtReason::InvalidSpan, 1);
                continue;
            }
            batch.records.push(record);
        }
        for Supersession {
            old,
            new,
            correction,
        } in supersessions
        {
            let link = SupersessionLink {
                old: old.into(),
                new: new.into(),
                correction: correction.into(),
            };
            // A proposed successor must itself be user wording to become current direction.
            if link.new.user {
                batch.links.push(link);
            } else {
                batch.debt.add(DebtReason::UnresolvedAuthority, 1);
            }
        }
        batch
    }
}

/// Validated records carried across boundaries with their original provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SemanticLedger {
    pub version: u32,
    pub records: Vec<SemanticRecord>,
    pub links: Vec<SupersessionLink>,
    /// Boundaries whose trailer was missing or malformed.
    pub unavailable_boundaries: u32,
    pub debt: CoverageDebt,
}

impl Default for SemanticLedger {
    fn default() -> Self {
        Self {
            version: SEMANTIC_VERSION,
            records: Vec::new(),
            links: Vec::new(),
            unavailable_boundaries: 0,
            debt: CoverageDebt::default(),
        }
    }
}

impl SemanticLedger {
    /// Merges one boundary's batch. Exact source-identity duplicates and verbatim restatements
    /// coalesce into the earliest record; a new measurement never replaces an older one.
    pub(crate) fn merge(&mut self, batch: ExtractionBatch) {
        if !batch.semantic_available {
            self.unavailable_boundaries += 1;
        }
        self.debt.absorb(&batch.debt);
        for record in batch.records {
            if self.records.iter().any(|known| same_record(known, &record)) {
                continue;
            }
            if self.records.len() == MAX_CANDIDATES {
                self.debt.add(DebtReason::Oversize, 1);
                continue;
            }
            self.records.push(record);
        }
        for link in batch.links {
            let duplicate = self.links.iter().any(|known| {
                known.old.identity() == link.old.identity()
                    && known.new.identity() == link.new.identity()
            });
            if duplicate {
                continue;
            }
            if self.links.len() == MAX_LINKS {
                self.debt.add(DebtReason::Oversize, 1);
                continue;
            }
            self.links.push(link);
        }
    }

    /// Records still eligible for disclosure, in packing order: kind, then original order.
    pub(crate) fn eligible(&self, exclusions: &Exclusions) -> Vec<&SemanticRecord> {
        let mut records: Vec<&SemanticRecord> = self
            .records
            .iter()
            .filter(|record| !record.excluded_by(exclusions))
            .collect();
        records.sort_by_key(|record| (record.kind, record.first_sequence()));
        records
    }
}

fn same_record(known: &SemanticRecord, record: &SemanticRecord) -> bool {
    fn identities(r: &SemanticRecord) -> Vec<(&str, usize, usize, usize)> {
        r.quotes().map(Quote::identity).collect()
    }
    let texts = |r: &SemanticRecord| r.quotes().map(|q| q.text.clone()).collect::<Vec<_>>();
    known.kind == record.kind
        && (identities(known) == identities(record) || texts(known) == texts(record))
}

fn semantic_record(record: Record<SourceSpan>, request_sha256: &str) -> SemanticRecord {
    let (kind, main, subject, qualifier, detail) = match record {
        Record::Diagnosis {
            finding,
            subject,
            uncertainty,
        } => (SemanticKind::Diagnosis, finding, subject, uncertainty, None),
        Record::Choice {
            choice,
            reason,
            subject,
        } => (SemanticKind::Choice, choice, subject, Some(reason), None),
        Record::Rejection {
            rejected,
            reason,
            subject,
        } => (
            SemanticKind::Rejection,
            rejected,
            subject,
            Some(reason),
            None,
        ),
        Record::Measurement {
            value,
            subject,
            run,
            qualifier,
        } => (SemanticKind::Measurement, value, subject, run, qualifier),
    };
    SemanticRecord {
        kind,
        main: main.into(),
        subject: subject.into(),
        qualifier: qualifier.map(Into::into),
        detail: detail.map(Into::into),
        request_sha256: request_sha256.to_string(),
    }
}

/// Kind-specific completeness beyond the schema: a measured value quotes a number and a unit.
fn complete(record: &SemanticRecord) -> bool {
    if record.kind != SemanticKind::Measurement {
        return true;
    }
    let value = &record.main.text;
    value.chars().any(|c| c.is_ascii_digit())
        && value
            .chars()
            .any(|c| c.is_alphabetic() || matches!(c, '%' | '/' | '$' | '\u{00b5}'))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
