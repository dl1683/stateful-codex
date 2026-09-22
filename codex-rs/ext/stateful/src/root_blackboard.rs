use std::collections::HashMap;

use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardRelationKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::ContextMapHit;
use codex_project_intelligence::RootBlackboardProjection;
use sha2::Digest;
use sha2::Sha256;

use crate::world_state::append_line;
use crate::world_state::hash_component;
use crate::world_state::try_append_line;

const MAX_ENTRY_BYTES: usize = 3 * 1024;
const ROOT_FOOTER_RESERVE_BYTES: usize = 512;

pub(super) enum RootBlackboardStatus {
    Available(ResolvedRootBlackboard),
    NotConfigured,
    Unavailable,
}

pub(super) struct ResolvedRootBlackboard {
    pub(super) projection: RootBlackboardProjection,
    pub(super) evidence_routes: HashMap<ContextMapEntryId, ContextMapHit>,
}

impl RootBlackboardStatus {
    pub(super) fn update_fingerprint(&self, hasher: &mut Sha256) {
        match self {
            Self::Available(root) => {
                let projection = &root.projection;
                hasher.update(b"blackboard-available\0");
                hasher.update(projection.revision.to_be_bytes());
                hasher.update(projection.omitted_entries.to_be_bytes());
                for hit in &projection.data {
                    hash_component(hasher, &render_hit(hit, &root.evidence_routes));
                }
            }
            Self::NotConfigured => hasher.update(b"blackboard-not-configured\0"),
            Self::Unavailable => hasher.update(b"blackboard-unavailable\0"),
        }
    }
}

pub(super) fn render_root_blackboard(output: &mut String, status: &RootBlackboardStatus) {
    match status {
        RootBlackboardStatus::Available(root) => render_projection(output, root),
        RootBlackboardStatus::NotConfigured => append_line(
            output,
            "Project intelligence is unavailable because persistent state is disabled. Use source files as ground truth and do not claim memory readiness.",
        ),
        RootBlackboardStatus::Unavailable => append_line(
            output,
            "Project intelligence could not be loaded. Use source files as ground truth and do not claim memory or evidence readiness.",
        ),
    }
}

fn render_projection(output: &mut String, root: &ResolvedRootBlackboard) {
    let projection = &root.projection;
    append_line(
        output,
        &format!("Project intelligence revision: {}", projection.revision),
    );
    append_line(
        output,
        "Root blackboard (active, explicitly promoted knowledge):",
    );
    let mut omitted = projection.omitted_entries;
    for hit in &projection.data {
        if !try_append_line(
            output,
            &render_hit(hit, &root.evidence_routes),
            ROOT_FOOTER_RESERVE_BYTES,
        ) {
            omitted = omitted.saturating_add(1);
        }
    }
    if projection.data.is_empty() {
        append_line(
            output,
            "- No knowledge has been promoted to the root blackboard yet.",
        );
    }
    if omitted > 0 {
        append_line(
            output,
            &format!(
                "- {omitted} root entries omitted by the context bound; query the blackboard for them."
            ),
        );
    }
    append_line(
        output,
        "For consequential claims, verify against exact source. When current source routes are embedded above, open only the smallest decisive source set whose exact wording can change the answer; do not reopen every supporting file by default. Use focused deeper-blackboard or context-map queries only when root knowledge or its routes are insufficient.",
    );
}

fn render_hit(
    hit: &BlackboardHit,
    evidence_routes: &HashMap<ContextMapEntryId, ContextMapHit>,
) -> String {
    let entry = &hit.entry;
    let value = &entry.value;
    let evidence = value
        .evidence
        .iter()
        .map(|link| {
            evidence_routes.get(&link.context_map_entry_id).map_or_else(
                || link.context_map_entry_id.to_string(),
                |route| {
                    let anchor = route
                        .source
                        .region_anchor
                        .as_ref()
                        .map(|anchor| format!("#{}:{}", anchor.scheme, anchor.locator))
                        .unwrap_or_default();
                    format!(
                        "{}@{}::{}{}({})",
                        link.context_map_entry_id,
                        single_line(&route.source.project_root),
                        route.source.relative_path,
                        anchor,
                        context_freshness_name(route.freshness),
                    )
                },
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let structured = value
        .structured_value
        .as_ref()
        .map_or_else(String::new, |item| {
            format!(
                " value={}{}",
                single_line(&item.value),
                item.unit
                    .as_ref()
                    .map(|unit| format!(" {}", single_line(unit)))
                    .unwrap_or_default()
            )
        });
    let relations = hit
        .relations
        .iter()
        .map(|relation| {
            format!(
                "{}:{}->{}",
                relation_kind_name(relation.value.kind),
                relation.value.from_entry_id,
                relation.value.to_entry_id
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut line = format!(
        "- id={} node={} revision={} kind={} verification={} declared={} evidenceFreshness={} importance={} confidence={} provenance={}:{}{} evidence=[{}] relations=[{}] content={}",
        entry.id,
        value.node_id,
        entry.revision,
        kind_name(value.kind),
        verification_name(hit.effective_verification),
        verification_name(value.verification),
        freshness_name(hit.evidence_freshness),
        importance_name(value.importance),
        value.confidence.basis_points(),
        provenance_name(value.provenance.kind),
        single_line(&value.provenance.source_id),
        structured,
        evidence,
        relations,
        single_line(&value.content),
    );
    if line.len() > MAX_ENTRY_BYTES {
        let marker = format!("… [entry truncated; query id={}]", entry.id);
        let maximum = MAX_ENTRY_BYTES.saturating_sub(marker.len());
        line.truncate(floor_char_boundary(&line, maximum));
        line.push_str(&marker);
    }
    line
}

fn context_freshness_name(freshness: ContextMapFreshness) -> &'static str {
    match freshness {
        ContextMapFreshness::Current => "current",
        ContextMapFreshness::Stale => "stale",
        ContextMapFreshness::SourceUnavailable => "sourceUnavailable",
    }
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            _ => character,
        })
        .collect()
}

fn floor_char_boundary(value: &str, maximum: usize) -> usize {
    let mut boundary = maximum.min(value.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

macro_rules! enum_names {
    ($($name:ident($type:ty) { $($variant:path => $value:literal),+ $(,)? })+) => {$ (
        fn $name(value: $type) -> &'static str {
            match value { $($variant => $value,)+ }
        }
    )+ };
}

enum_names! {
    kind_name(BlackboardKind) {
        BlackboardKind::Instruction => "instruction", BlackboardKind::Fact => "fact",
        BlackboardKind::Claim => "claim", BlackboardKind::Number => "number",
        BlackboardKind::Decision => "decision", BlackboardKind::Strategy => "strategy",
        BlackboardKind::Question => "question", BlackboardKind::Contradiction => "contradiction",
        BlackboardKind::Failure => "failure", BlackboardKind::RejectedApproach => "rejectedApproach",
        BlackboardKind::Signal => "signal", BlackboardKind::Note => "note"
    }
    verification_name(BlackboardVerification) {
        BlackboardVerification::Unverified => "unverified",
        BlackboardVerification::SourceVerified => "sourceVerified",
        BlackboardVerification::UserConfirmed => "userConfirmed",
        BlackboardVerification::Disputed => "disputed", BlackboardVerification::Stale => "stale"
    }
    freshness_name(BlackboardEvidenceFreshness) {
        BlackboardEvidenceFreshness::NotApplicable => "notApplicable",
        BlackboardEvidenceFreshness::Current => "current",
        BlackboardEvidenceFreshness::Stale => "stale",
        BlackboardEvidenceFreshness::SourceUnavailable => "sourceUnavailable"
    }
    importance_name(BlackboardImportance) {
        BlackboardImportance::Critical => "critical", BlackboardImportance::High => "high",
        BlackboardImportance::Normal => "normal", BlackboardImportance::Low => "low"
    }
    provenance_name(BlackboardProvenanceKind) {
        BlackboardProvenanceKind::User => "user", BlackboardProvenanceKind::Agent => "agent",
        BlackboardProvenanceKind::Maintenance => "maintenance",
        BlackboardProvenanceKind::Import => "import"
    }
    relation_kind_name(BlackboardRelationKind) {
        BlackboardRelationKind::Supports => "supports",
        BlackboardRelationKind::Contradicts => "contradicts",
        BlackboardRelationKind::DependsOn => "dependsOn",
        BlackboardRelationKind::RelatedTo => "relatedTo"
    }
}
