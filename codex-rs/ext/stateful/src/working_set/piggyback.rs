//! The bounded extraction addition to a native LOCAL compaction request, and the contributor
//! that validates the separated trailer into semantic records.
//!
//! The addition is the design's normative instruction, the strict schema, the trailer framing
//! with a request-scoped marker, and an index of eligible original parts. The complete
//! addition is held under 1,000 continuing-model tokens and 4,096 bytes; parts that do not fit
//! are unindexed debt and can never become new records. Nothing here retries, repairs or adds
//! a model call. Not registered: activation is stage (3).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_extension_api::CompactionExtractionContributor;
use codex_extension_api::CompactionExtractionPreparation;
use codex_extension_api::CompactionExtractionResult;
use codex_extension_api::CompactionRequestSnapshot;
use codex_extension_api::CompactionSourceClass;
use codex_extension_api::CompactionTrailer;
use codex_extension_api::ExtensionData;
use codex_extension_api::MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES;
use sha2::Digest;
use sha2::Sha256;

use super::semantic::ExtractionBatch;
use super::semantic::SemanticLedger;
use super::source_validation::FrozenIndex;
use super::source_validation::MAX_INDEXED_PARTS;
use super::source_validation::SIDECAR_CLOSING;
use super::source_validation::SidecarOutcome;
use super::source_validation::validate_sidecar;

pub(crate) const MAX_ADDITION_TOKENS: usize = 1_000;
const SIDECAR_TAG: &str = "capsule_sidecar_v1";

const INSTRUCTION: &str = "Produce the ordinary handoff summary requested above first, with the same completeness and qualifications. Then append the delimited capsule JSON trailer described here. Select only exact contiguous quotations from indexed original source parts in this request. Preserve subjects, uncertainty, reasons, units and run identity. Assistant statements are reported claims, not verified facts or user authority. Do not source records from summaries, context packets, the trailer instructions or omitted input. Empty records are valid. Proposed supersession is advisory. Do not infer an old/new rule link or a working replacement merely from recency.";

const SCHEMA: &str = r#"Trailer JSON, one line: {"version":1,"records":[R,...],"supersessions":[P,...]}
Span = {"q":int,"start":int,"end":int,"quote":string}; quote is exactly the UTF-8 bytes start..end (half-open) of indexed part q.
R = {"kind":"diagnosis","finding":Span,"subject":Span,"uncertainty":Span|null}
 | {"kind":"choice","choice":Span,"reason":Span,"subject":Span}
 | {"kind":"rejection","rejected":Span,"reason":Span,"subject":Span}
 | {"kind":"measurement","value":Span,"subject":Span,"run":Span|null,"qualifier":Span|null}
P = {"old":Span,"new":Span,"correction":Span}; correction quotes the user.
A measurement value includes its number and unit; run quotes an explicit run identifier or null. At most 32 records and 4 supersessions; at most 4 distinct spans and 512 quoted bytes per record. No other keys."#;

/// Continuing-model tokenizer over final text; `None` counts one token per byte.
pub(crate) type SharedTokenCounter = Arc<dyn Fn(&str) -> usize + Send + Sync>;

/// One attempt's addition and the host-only map behind its index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Addition {
    pub preparation: CompactionExtractionPreparation,
    pub index: FrozenIndex,
    /// Request part position of each `q`.
    pub positions: Vec<usize>,
    /// Eligible parts left out of the index by the bounds.
    pub unindexed: usize,
    pub request_sha256: String,
    pub tokens: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdditionError {
    /// The instruction, schema and framing alone exceed a bound.
    Capacity,
    /// No marker absent from the request could be produced.
    Marker,
}

/// Builds the addition for one attempt. User parts are indexed first, newest first; assistant
/// parts follow in delivery order; indexing stops before either bound.
pub(crate) fn prepare_addition(
    request: &CompactionRequestSnapshot,
    salt: u64,
    counter: Option<&(dyn Fn(&str) -> usize + Send + Sync)>,
) -> Result<Addition, AdditionError> {
    let marker = (0..8)
        .map(|tries| marker(request, salt.wrapping_add(tries)))
        .find(|marker| request.parts.iter().all(|part| !part.text.contains(marker)))
        .ok_or(AdditionError::Marker)?;
    let opening = format!("<{SIDECAR_TAG} request=\"{marker}\">");
    let eligible = eligible_parts(request);
    let (users, assistants): (Vec<usize>, Vec<usize>) = eligible
        .iter()
        .partition(|&&position| request.parts[position].class == CompactionSourceClass::User);
    let priority: Vec<usize> = users.into_iter().rev().chain(assistants).collect();
    let count = |text: &str| counter.map_or(text.len(), |count| count(text));
    let fits = |text: &str| {
        let tokens = count(text);
        (text.len() <= MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES && tokens <= MAX_ADDITION_TOKENS)
            .then_some(tokens)
    };
    let mut selected: Vec<usize> = Vec::new();
    let mut tokens = fits(&render(request, &opening, &selected)).ok_or(AdditionError::Capacity)?;
    for &position in priority.iter().take(MAX_INDEXED_PARTS) {
        let mut trial = selected.clone();
        trial.push(position);
        trial.sort_unstable();
        match fits(&render(request, &opening, &trial)) {
            Some(trial_tokens) => {
                selected = trial;
                tokens = trial_tokens;
            }
            None => break,
        }
    }
    let instruction = render(request, &opening, &selected);
    Ok(Addition {
        index: FrozenIndex::new(request, &opening, &selected),
        unindexed: eligible.len() - selected.len(),
        positions: selected,
        request_sha256: request_fingerprint(request),
        tokens,
        preparation: CompactionExtractionPreparation {
            instruction,
            trailer_opening: opening,
        },
    })
}

/// Parts that can validate: user or assistant text mapped byte-identically to an original at
/// or before the watermark. Command output has no durable invocation identity yet.
fn eligible_parts(request: &CompactionRequestSnapshot) -> Vec<usize> {
    request
        .parts
        .iter()
        .enumerate()
        .filter(|(_, part)| {
            matches!(
                part.class,
                CompactionSourceClass::User | CompactionSourceClass::Assistant
            ) && !part.text.is_empty()
                && part.origin.as_ref().is_some_and(|origin| {
                    origin.byte_identical
                        && origin
                            .sequence
                            .zip(request.sequence_watermark)
                            .is_some_and(|(sequence, watermark)| sequence <= watermark)
                })
        })
        .map(|(position, _)| position)
        .collect()
}

fn render(request: &CompactionRequestSnapshot, opening: &str, selected: &[usize]) -> String {
    let mut text = format!(
        "{INSTRUCTION}\n{SCHEMA}\nFraming: after the summary write the line {opening} then the JSON line then the line {SIDECAR_CLOSING}\nIndex (q: input item position.content part, bytes, speaker u=user a=assistant):"
    );
    for (q, &position) in selected.iter().enumerate() {
        let part = &request.parts[position];
        let speaker = if part.class == CompactionSourceClass::User {
            "u"
        } else {
            "a"
        };
        text.push_str(&format!(
            "\n{q}: {}.{} {}B {speaker}",
            part.item_index,
            part.part_index,
            part.text.len()
        ));
    }
    text
}

fn marker(request: &CompactionRequestSnapshot, salt: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request.thread_id.to_string());
    hasher.update(&request.turn_id);
    hasher.update(request.attempt.to_le_bytes());
    hasher.update(salt.to_le_bytes());
    for part in &request.parts {
        hasher.update(Sha256::digest(part.text.as_bytes()));
    }
    format!("{:x}", hasher.finalize())[..24].to_string()
}

/// Logical request fingerprint: positions, classes and text of every part, no transport data.
fn request_fingerprint(request: &CompactionRequestSnapshot) -> String {
    let mut hasher = Sha256::new();
    for part in &request.parts {
        hasher.update(format!(
            "{}.{}:{:?}:{}\n",
            part.item_index,
            part.part_index,
            part.class,
            part.text.len()
        ));
        hasher.update(part.text.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// How a proposal's span coordinates are read before strict validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpanResolution {
    /// The model's `q`, `start` and `end` are used as given.
    Strict,
    /// A quote that occurs exactly once among all indexed parts is located by the host; the
    /// quote bytes themselves are never changed. Ambiguous quotes stay as given.
    UniqueQuote,
}

/// Validates the separated trailer of the attempt `addition` was built for.
pub(crate) fn validate_result(
    result: &CompactionExtractionResult,
    addition: &Addition,
    resolution: SpanResolution,
) -> (SidecarOutcome, usize) {
    let outcome = validate_sidecar(result, &addition.index);
    let (SpanResolution::UniqueQuote, SidecarOutcome::Validated { .. }) = (resolution, &outcome)
    else {
        return (outcome, 0);
    };
    let CompactionTrailer::Present(trailer) = &result.trailer else {
        return (outcome, 0);
    };
    let Some((resolved, moved)) = resolve_spans(trailer, &result.request, addition) else {
        return (outcome, 0);
    };
    let resolved_result = CompactionExtractionResult {
        trailer: CompactionTrailer::Present(resolved),
        ..result.clone()
    };
    (validate_sidecar(&resolved_result, &addition.index), moved)
}

/// Rewrites span coordinates of an already well-formed trailer; returns the new trailer and the
/// number of spans the host located.
fn resolve_spans(
    trailer: &str,
    request: &CompactionRequestSnapshot,
    addition: &Addition,
) -> Option<(String, usize)> {
    let opening = &addition.preparation.trailer_opening;
    let body = trailer
        .strip_prefix(opening)?
        .trim_start_matches(['\r', '\n']);
    let json = &body[..body.rfind(SIDECAR_CLOSING)?];
    let mut envelope: serde_json::Value = serde_json::from_str(json).ok()?;
    let texts: Vec<&str> = addition
        .positions
        .iter()
        .map(|&position| request.parts[position].text.as_str())
        .collect();
    let mut moved = 0;
    for list in ["records", "supersessions"] {
        for proposal in envelope.get_mut(list)?.as_array_mut()? {
            for span in proposal.as_object_mut()?.values_mut() {
                moved += usize::from(resolve_span(span, &texts));
            }
        }
    }
    Some((format!("{opening}\n{envelope}\n{SIDECAR_CLOSING}"), moved))
}

fn resolve_span(span: &mut serde_json::Value, texts: &[&str]) -> bool {
    let Some(span) = span.as_object_mut() else {
        return false;
    };
    let field = |name: &str| span.get(name).and_then(serde_json::Value::as_u64);
    let Some(quote) = span
        .get("quote")
        .and_then(|q| q.as_str())
        .map(str::to_string)
    else {
        return false;
    };
    let (q, start, end) = (field("q"), field("start"), field("end"));
    let exact = q
        .and_then(|q| texts.get(usize::try_from(q).ok()?))
        .zip(start.zip(end))
        .and_then(|(text, (start, end))| {
            text.get(usize::try_from(start).ok()?..usize::try_from(end).ok()?)
        })
        .is_some_and(|selected| selected == quote);
    if exact || quote.is_empty() {
        return false;
    }
    let mut found = texts.iter().enumerate().flat_map(|(q, text)| {
        text.match_indices(quote.as_str())
            .map(move |(start, _)| (q, start))
    });
    let (Some((q, start)), None) = (found.next(), found.next()) else {
        return false;
    };
    span.insert("q".to_string(), q.into());
    span.insert("start".to_string(), start.into());
    span.insert("end".to_string(), (start + quote.len()).into());
    true
}

/// Per-thread extraction state: the pending attempt's addition and the carried ledger.
#[derive(Default)]
pub(crate) struct PiggybackState {
    pub pending: Option<Addition>,
    pub ledger: SemanticLedger,
}

/// Adds the bounded instruction to each native LOCAL compaction attempt and merges the
/// validated records of the successful attempt into the thread's ledger.
pub(crate) struct PiggybackExtractor {
    counter: Option<SharedTokenCounter>,
    resolution: SpanResolution,
    salt: AtomicU64,
}

impl PiggybackExtractor {
    pub(crate) fn new(counter: Option<SharedTokenCounter>, resolution: SpanResolution) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_nanos()).unwrap_or_default()
            });
        Self {
            counter,
            resolution,
            salt: AtomicU64::new(nanos),
        }
    }
}

impl CompactionExtractionContributor for PiggybackExtractor {
    fn prepare(
        &self,
        request: &CompactionRequestSnapshot,
        thread_store: &ExtensionData,
    ) -> Option<CompactionExtractionPreparation> {
        let salt = self.salt.fetch_add(8, Ordering::Relaxed);
        let addition = prepare_addition(request, salt, self.counter.as_deref()).ok()?;
        let preparation = addition.preparation.clone();
        let state = thread_store.get_or_init(|| Mutex::new(PiggybackState::default()));
        if let Ok(mut state) = state.lock() {
            state.pending = Some(addition);
        }
        Some(preparation)
    }

    fn complete(&self, result: CompactionExtractionResult, thread_store: &ExtensionData) {
        let state = thread_store.get_or_init(|| Mutex::new(PiggybackState::default()));
        let Ok(mut state) = state.lock() else { return };
        let Some(addition) = state.pending.take() else {
            return;
        };
        // The validator refuses a trailer of another attempt or marker as a floor.
        let (outcome, _) = validate_result(&result, &addition, self.resolution);
        let batch =
            ExtractionBatch::from_outcome(outcome, &addition.request_sha256, addition.unindexed);
        state.ledger.merge(batch);
    }
}
