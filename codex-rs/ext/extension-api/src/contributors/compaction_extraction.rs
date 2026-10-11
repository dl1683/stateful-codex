//! Bounded extraction from the host's native LOCAL compaction request, and admission of a
//! boundary candidate prepared from it.
//!
//! The host owns the request, its source identities and the native summary. A contributor may
//! add one bounded instruction to the request and receives the separated output trailer of the
//! successful attempt together with the exact request it was produced from. Nothing here
//! interprets the trailer; validation and any later disclosure belong to the contributor.

use std::future::Future;
use std::sync::Arc;

use codex_protocol::ThreadId;

use crate::ExtensionData;

/// Hard cap on the instruction a contributor may append to one compaction request.
pub const MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES: usize = 4_096;
/// Hard cap on a separated trailer handed back to the contributor.
pub const MAX_COMPACTION_EXTRACTION_TRAILER_BYTES: usize = 16_384;

/// Host classification of a request text part. Only the first three can be original sources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionSourceClass {
    /// Text the user submitted and the host accepted.
    User,
    /// Assistant text published to the user; a reported claim, never authority.
    Assistant,
    /// Tool or command output observed by the host.
    CommandOutput,
    /// Compaction summaries and other compaction-generated text.
    CompactionOutput,
    /// Host-rendered context, instructions and the compaction prompt itself.
    HostContext,
    /// Context copied from a parent thread; never child-local authority.
    Inherited,
}

/// Host-owned identity of the durable original behind one request part.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionSourceOrigin {
    pub item_id: String,
    pub turn_id: String,
    pub origin_thread_id: Option<ThreadId>,
    /// Durable acceptance or delivery order; `None` on legacy records.
    pub sequence: Option<u64>,
    /// Opaque host revision of the original message.
    pub source_revision: String,
    /// The recorded original was complete and the sent bytes equal it byte for byte. False after
    /// truncation, carry-forward shortening or a normalization that changed this part.
    pub byte_identical: bool,
}

/// One text part exactly as sent in the final request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionSourcePart {
    /// Position of the item in the request `input`.
    pub item_index: usize,
    /// Position of the content part inside that item.
    pub part_index: usize,
    pub class: CompactionSourceClass,
    /// Decoded UTF-8 text of the part as sent.
    pub text: String,
    /// `None` when the host cannot map the part to a durable original.
    pub origin: Option<CompactionSourceOrigin>,
}

/// Immutable final request of one native local compaction attempt, frozen before sending.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionRequestSnapshot {
    pub thread_id: ThreadId,
    pub turn_id: String,
    /// One-based compaction stream attempt, counting stream and context-window retries.
    pub attempt: u32,
    /// Highest durable sequence the host had accepted when the request was frozen.
    pub sequence_watermark: Option<u64>,
    pub parts: Vec<CompactionSourcePart>,
}

/// A contributor's addition to one request attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionExtractionPreparation {
    /// Appended as its own input item after the native compaction instruction.
    pub instruction: String,
    /// Exact opening line of the trailer. The host ignores the preparation when this line is
    /// empty, spans lines, or already occurs in a request part.
    pub trailer_opening: String,
}

/// Output after the native summary, separated by the host and never recorded as history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompactionTrailer {
    /// The summary carried no trailer opening.
    Absent,
    /// Raw text from the opening line to the end of the summary message.
    Present(String),
    /// A trailer existed but exceeded `MAX_COMPACTION_EXTRACTION_TRAILER_BYTES`.
    Oversize,
}

/// The successful attempt's request, preparation and separated trailer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionExtractionResult {
    pub request: Arc<CompactionRequestSnapshot>,
    pub preparation: CompactionExtractionPreparation,
    /// Compaction stream attempts made by this compaction, including failed ones. Requests the
    /// transport layer resends inside one attempt (HTTP or auth retries) are not counted.
    pub native_attempts: u32,
    pub trailer: CompactionTrailer,
}

/// Contributes bounded extraction to native LOCAL compaction requests.
///
/// The host asks `prepare` once per native attempt, after the attempt's final input is fixed,
/// and calls `complete` at most once, only for the attempt that completed with a separable
/// native summary. Failed or interrupted attempts never reach `complete`. Implementations must
/// not request retries or additional model calls, and must treat every trailer as untrusted.
pub trait CompactionExtractionContributor: Send + Sync {
    fn prepare(
        &self,
        request: &CompactionRequestSnapshot,
        thread_store: &ExtensionData,
    ) -> Option<CompactionExtractionPreparation>;

    fn complete(&self, result: CompactionExtractionResult, thread_store: &ExtensionData);
}

/// A boundary candidate prepared off live state, to be admitted before installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedBoundary {
    pub thread_id: ThreadId,
    pub window_number: u64,
    /// Live history version the candidate was prepared against.
    pub expected_history_version: u64,
    /// Digest of the exact model-visible bytes the host would install.
    pub fragment_sha256: String,
}

/// Outcome of admitting or confirming a prepared boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BoundaryAdmission {
    Admitted,
    /// Stale or ineligible; the host installs no extension content for this boundary.
    Refused,
}

/// Admits prepared boundary candidates against the contributor's durable state.
///
/// The host admits a candidate before installing it and confirms the exact installed candidate
/// afterwards; anything but `Admitted` from either call withholds the extension content.
/// Implementations must order admission against their own invalidations and hold no lock
/// across the returned future's completion.
pub trait BoundaryAdmissionContributor: Send + Sync {
    fn admit(
        &self,
        boundary: &PreparedBoundary,
        thread_store: &ExtensionData,
    ) -> impl Future<Output = BoundaryAdmission> + Send;

    fn confirm_installed(
        &self,
        boundary: &PreparedBoundary,
        thread_store: &ExtensionData,
    ) -> impl Future<Output = BoundaryAdmission> + Send;
}
