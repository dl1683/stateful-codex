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

use crate::completion::MAX_MATERIAL_ROOT_FINDINGS;
use crate::world_state::append_line;
use crate::world_state::hash_component;
use crate::world_state::try_append_line;

const MAX_ENTRY_BYTES: usize = 3 * 1024;
const ROOT_KNOWLEDGE_RESERVE_BYTES: usize = 12 * 1024;
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
                let mut rendered = String::new();
                render_projection(&mut rendered, root);
                hash_component(hasher, &rendered);
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
    let entry_aliases = projection
        .data
        .iter()
        .enumerate()
        .map(|(index, hit)| (hit.entry.id.to_string(), format!("E{}", index + 1)))
        .collect::<HashMap<_, _>>();
    let evidence_aliases = render_evidence_catalog(output, root);
    let mut omitted = projection.omitted_entries;
    for (index, hit) in projection.data.iter().enumerate() {
        if !try_append_line(
            output,
            &render_hit(
                &format!("E{}", index + 1),
                hit,
                &entry_aliases,
                &evidence_aliases,
            ),
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
        &format!(
            "For consequential claims, verify against exact source. Use evidence_read with the S paths and source line hints above to open only the smallest decisive line ranges whose wording can change the answer; do not reopen every supporting file by default or call a full-corpus read the smallest set. Use focused deeper-blackboard or context-map queries only when root knowledge or its routes are insufficient. At completion, pass this project intelligence revision as rootRevision and select at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases directly material to the requested outcome in materialRootFindings. Preserve any additional material conclusions in the final semantic obligation; use an empty alias list only after determining that no root finding is material. rootRevision is not expectedRevision: copy expectedRevision from the separate Stateful run World State."
        ),
    );
}

fn render_evidence_catalog(
    output: &mut String,
    root: &ResolvedRootBlackboard,
) -> HashMap<ContextMapEntryId, String> {
    let mut ordered_routes = Vec::new();
    for evidence in root
        .projection
        .data
        .iter()
        .flat_map(|hit| &hit.entry.value.evidence)
    {
        if root
            .evidence_routes
            .contains_key(&evidence.context_map_entry_id)
            && !ordered_routes.contains(&evidence.context_map_entry_id)
        {
            ordered_routes.push(evidence.context_map_entry_id.clone());
        }
    }
    if ordered_routes.is_empty() {
        return HashMap::new();
    }

    append_line(output, "Exact-source aliases:");
    let mut root_aliases = HashMap::new();
    for entry_id in &ordered_routes {
        let route = &root.evidence_routes[entry_id];
        if root_aliases.contains_key(&route.source.project_root) {
            continue;
        }
        let alias = format!("R{}", root_aliases.len() + 1);
        let line = format!("- {alias}={}", single_line(&route.source.project_root));
        if try_append_line(output, &line, ROOT_KNOWLEDGE_RESERVE_BYTES) {
            root_aliases.insert(route.source.project_root.clone(), alias);
        }
    }

    let mut evidence_aliases = HashMap::new();
    for entry_id in ordered_routes {
        let route = &root.evidence_routes[&entry_id];
        let Some(root_alias) = root_aliases.get(&route.source.project_root) else {
            continue;
        };
        let alias = format!("S{}", evidence_aliases.len() + 1);
        let anchor = route
            .source
            .region_anchor
            .as_ref()
            .map(|anchor| format!("#{}:{}", anchor.scheme, anchor.locator))
            .unwrap_or_default();
        let line = format!(
            "- {alias}={root_alias}::{}{anchor} ({})",
            route.source.relative_path,
            context_freshness_name(route.freshness),
        );
        if try_append_line(output, &line, ROOT_KNOWLEDGE_RESERVE_BYTES) {
            evidence_aliases.insert(entry_id, alias);
        }
    }
    evidence_aliases
}

fn render_hit(
    alias: &str,
    hit: &BlackboardHit,
    entry_aliases: &HashMap<String, String>,
    evidence_aliases: &HashMap<ContextMapEntryId, String>,
) -> String {
    let entry = &hit.entry;
    let value = &entry.value;
    let evidence = value
        .evidence
        .iter()
        .filter_map(|link| {
            evidence_aliases
                .get(&link.context_map_entry_id)
                .map(|alias| match link.line_range {
                    Some(range) => format!("{alias}:L{}-L{}", range.start, range.end),
                    None => alias.clone(),
                })
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
            let from = entry_aliases
                .get(relation.value.from_entry_id.as_str())
                .map(String::as_str)
                .unwrap_or("deeper");
            let to = entry_aliases
                .get(relation.value.to_entry_id.as_str())
                .map(String::as_str)
                .unwrap_or("deeper");
            format!("{}:{from}>{to}", relation_kind_name(relation.value.kind))
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut line = format!(
        "- {alias} [{} {}; verification={}; declared={}; evidence={}; confidence={}; provenance={}] content={}{} sources=[{}] relations=[{}]",
        importance_name(value.importance),
        kind_name(value.kind),
        verification_name(hit.effective_verification),
        verification_name(value.verification),
        freshness_name(hit.evidence_freshness),
        value.confidence.basis_points(),
        provenance_name(value.provenance.kind),
        single_line(&value.content),
        structured,
        evidence,
        relations,
    );
    if line.len() > MAX_ENTRY_BYTES {
        let marker = format!("… [{alias} truncated; query blackboard by content]");
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
