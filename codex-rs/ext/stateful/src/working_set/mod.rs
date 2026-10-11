//! Current-thread working-set capsule: a deterministic, bounded projection of what the
//! thread has observed (next action, tool routes, delivered values, constraints and changed
//! file content), reduced from host observations with no model call, and its checked
//! model-visible rendering. Neither representation is authority; the projection is a
//! rebuildable derived cache persisted through `codex_project_intelligence::WorkingSetStore`.
//!
//! Inactive: nothing registers or installs the capsule yet (stage (c) wires lifecycle
//! callbacks, executor file reads and the checked install).
#![allow(dead_code)]

mod commands;
mod directives;
mod grammar;
mod offline;
mod piggyback;
mod reduce;
mod render;
mod retain;
mod semantic;
mod source_validation;
mod working_files;

use codex_project_intelligence::MAX_WORKING_SET_PAYLOAD_BYTES;
use serde::Deserialize;
use serde::Serialize;

use working_files::FileWitness;

pub use offline::OfflineCapsule;
pub use offline::OfflineCapsuleInput;
pub use offline::OfflineFileWitness;
pub use offline::offline_capsule;

/// Version of the finite extraction grammar and payload schema; a change invalidates caches.
pub(crate) const GRAMMAR_VERSION: u32 = 1;
pub(crate) const MAX_CANDIDATES: usize = 128;
pub(crate) const MAX_CANDIDATE_TEXT_BYTES: usize = 512;
pub(crate) const MAX_REFERENCES: usize = 4;

/// Authoritative location of the original bytes a candidate quotes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SourceRef {
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    /// Host invocation identity for command/tool output; never the provider call ID alone.
    pub invocation: Option<String>,
    /// Content part index inside the item, and the UTF-8 byte range inside that part.
    pub part: usize,
    pub start: usize,
    pub end: usize,
    /// SHA-256 of the complete source enclosure (item text or command output).
    pub digest: String,
    pub source_revision: u64,
    pub binding_generation: u64,
}

impl SourceRef {
    /// Complete provenance: unknown identity disqualifies a record instead of being guessed.
    fn is_complete(&self, needs_invocation: bool) -> bool {
        !self.thread_id.is_empty()
            && !self.turn_id.is_empty()
            && !self.item_id.is_empty()
            && self.digest.len() == 64
            && self.binding_generation > 0
            && self.start <= self.end
            && (!needs_invocation || self.invocation.as_deref().is_some_and(|i| !i.is_empty()))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Origin {
    User,
    /// Reported by the assistant: never promoted to a verified fact or a user rule.
    Assistant,
    Structured,
    Host,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CandidateKind {
    Next,
    Route,
    Value,
    /// A user message naming a task file, kept only to attribute a change witness. Directives
    /// themselves come from the root packet, not the capsule.
    Attribution,
    Change,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Active,
    /// A question or conditional proposal: never authorization.
    Proposed,
    /// Pinned historical baseline; a newer measurement never supersedes it.
    Baseline,
    /// An unavailable route; `resolved_by` names its working replacement.
    Failed,
    Resolved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Candidate {
    pub id: u64,
    pub kind: CandidateKind,
    pub status: Status,
    pub origin: Origin,
    /// Normalized subject (route program, file path) where mechanically available.
    pub subject: Option<String>,
    /// Observed scope: executor cwd for a route; `None` where the source carries none.
    pub scope: Option<String>,
    /// Exact selected source text; `None` when the record exceeded the per-candidate cap.
    pub text: Option<String>,
    pub partial: bool,
    pub sources: Vec<SourceRef>,
    /// Dependency identities: the working route that replaced a failed one, and the user
    /// statement attributing a change witness. They count toward `MAX_REFERENCES`.
    pub resolved_by: Option<u64>,
    pub linked: Option<u64>,
}

impl Candidate {
    fn references(&self) -> usize {
        self.sources.len()
            + usize::from(self.resolved_by.is_some())
            + usize::from(self.linked.is_some())
    }
}

/// Serialized projection payload stored by `WorkingSetStore`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Projection {
    pub grammar_version: u32,
    pub next_id: u64,
    pub candidates: Vec<Candidate>,
    /// Paths named by applied file changes, commands or the user, oldest first.
    pub touched_files: Vec<String>,
    /// Records that could not be retained or exceeded caps.
    pub capacity_debt: u32,
    /// Completed items whose middle was not inspected.
    pub uninspected_items: u32,
    /// The newest accepted user messages, captured whole with no word list.
    #[serde(default)]
    pub statements: Vec<UserStatement>,
    /// Every accepted user message seen, including those no longer retained.
    #[serde(default)]
    pub statements_seen: u32,
}

/// One accepted user message. It is a user statement, not by itself an adopted rule.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct UserStatement {
    pub source: SourceRef,
    /// The exact message; `None` when it exceeds the per-statement bound (reference only).
    pub text: Option<String>,
    /// `Projection::next_id` when it arrived: candidates with an id up to it came before it.
    pub after_candidate: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapacityDebt {
    Candidates,
    Candidate,
    Payload,
}

/// Turn and retired wording the host's eligibility policy excludes at render time.
#[derive(Clone, Debug, Default)]
pub(crate) struct Exclusions {
    pub turns: Vec<String>,
    pub retired_texts: Vec<String>,
}

impl Exclusions {
    fn excludes(&self, candidate: &Candidate) -> bool {
        candidate
            .sources
            .iter()
            .any(|source| self.turns.contains(&source.turn_id))
            || candidate.text.as_deref().is_some_and(|text| {
                self.retired_texts
                    .iter()
                    .any(|retired| !retired.is_empty() && text.contains(retired.as_str()))
            })
    }
}

/// One host observation. Reasoning, arbitrary tool/MCP payloads and memory proposals have no
/// variant: they are never captured.
#[derive(Clone, Debug)]
pub(crate) enum Observation {
    UserMessage {
        source: SourceRef,
        text: String,
    },
    /// Published assistant text (intermediate or final).
    AssistantMessage {
        source: SourceRef,
        text: String,
    },
    /// Structured obligation `next` or plan step; replaces the previous structured next.
    StructuredNext {
        source: SourceRef,
        text: String,
    },
    PlanStepCompleted {
        text: String,
    },
    Command {
        source: SourceRef,
        argv: Vec<String>,
        cwd: String,
        exit_code: Option<i32>,
        output: String,
    },
    AppliedFileChange {
        path: String,
    },
    FileWitness {
        source: SourceRef,
        witness: FileWitness,
    },
}

impl Projection {
    pub(crate) fn new() -> Self {
        Self {
            grammar_version: GRAMMAR_VERSION,
            ..Self::default()
        }
    }

    /// Decodes a stored payload; another grammar version is refused (rebuild instead).
    pub(crate) fn from_payload(payload: &str) -> Option<Self> {
        serde_json::from_str::<Self>(payload)
            .ok()
            .filter(|projection| projection.grammar_version == GRAMMAR_VERSION)
    }

    pub(crate) fn to_payload(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Validates the typed bounds and the complete serialization before a store write.
    /// Overflow is refused as capacity debt, never written truncated.
    pub(crate) fn validated_payload(&self) -> Result<String, CapacityDebt> {
        if self.candidates.len() > MAX_CANDIDATES {
            return Err(CapacityDebt::Candidates);
        }
        if self.candidates.iter().any(|c| {
            c.text
                .as_ref()
                .is_some_and(|t| t.len() > MAX_CANDIDATE_TEXT_BYTES)
                || c.sources.is_empty()
                || c.references() > MAX_REFERENCES
        }) {
            return Err(CapacityDebt::Candidate);
        }
        let payload = self.to_payload();
        if payload.is_empty() || payload.len() > MAX_WORKING_SET_PAYLOAD_BYTES {
            return Err(CapacityDebt::Payload);
        }
        Ok(payload)
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod commands_tests;
#[cfg(test)]
#[path = "directives_tests.rs"]
mod directives_tests;
#[cfg(test)]
#[path = "grammar_tests.rs"]
mod grammar_tests;
#[cfg(test)]
#[path = "offline_tests.rs"]
mod offline_tests;
#[cfg(test)]
#[path = "piggyback_tests.rs"]
mod piggyback_tests;
#[cfg(test)]
#[path = "reduce_tests.rs"]
mod reduce_tests;
#[cfg(test)]
#[path = "render_tests.rs"]
mod render_tests;
#[cfg(test)]
#[path = "semantic_tests.rs"]
mod semantic_tests;
#[cfg(test)]
#[path = "source_validation_tests.rs"]
mod source_validation_tests;
#[cfg(test)]
#[path = "working_files_tests.rs"]
mod working_files_tests;
