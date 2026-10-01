use std::collections::HashMap;

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
use crate::source_freshness::AuditedEvidenceFreshness;
use crate::source_freshness::AuditedPremiseFreshness;
use crate::source_freshness::EvidenceAudit;
use crate::source_freshness::audited_blackboard_freshness;
use crate::source_freshness::audited_context_freshness;
use crate::source_freshness::audited_premise_freshness;
use crate::source_freshness::audited_verification;
use crate::world_state::append_line;
use crate::world_state::hash_component;
use crate::world_state::try_append_line;

const MAX_ENTRY_BYTES: usize = 3 * 1024;
const ROOT_KNOWLEDGE_RESERVE_BYTES: usize = 4 * 1024;
const ROOT_FOOTER_RESERVE_BYTES: usize = 512;
const TRUNCATED_ENTRY_SUFFIX: &str = " truncated; query blackboard by content]";

pub(super) enum RootBlackboardStatus {
    Available(ResolvedRootBlackboard),
    NotConfigured,
    Unavailable,
}

/// Root lines that reached the model-visible packet, with alias-independent digests
/// so a later render can describe only what changed.
#[derive(Default)]
pub(super) struct RootLayout {
    pub(super) entries: Vec<LaidOutLine>,
    pub(super) sources: Vec<LaidOutLine>,
    /// Entries shown in full (entry ID, alias, entry revision).
    pub(super) complete_entries: Vec<(String, String, u64)>,
}

pub(super) struct LaidOutLine {
    /// Short stable identity (entry or route ID digest).
    pub(super) key: String,
    /// Digest of the line rendered with identities instead of positional aliases.
    pub(super) digest: String,
    pub(super) line: String,
}

pub(super) struct ResolvedRootBlackboard {
    pub(super) projection: RootBlackboardProjection,
    pub(super) evidence_routes: HashMap<ContextMapEntryId, ContextMapHit>,
    pub(super) evidence_audit: Option<EvidenceAudit>,
}

impl RootBlackboardStatus {
    pub(super) fn update_fingerprint(&self, hasher: &mut Sha256) {
        match self {
            Self::Available(root) => {
                let projection = &root.projection;
                hasher.update(b"blackboard-available\0");
                hasher.update(projection.revision.to_be_bytes());
                hasher.update(projection.omitted_entries.to_be_bytes());
                hasher.update(projection.candidate_entries.to_be_bytes());
                let mut rendered = String::new();
                render_projection(&mut rendered, root);
                hash_component(hasher, &rendered);
            }
            Self::NotConfigured => hasher.update(b"blackboard-not-configured\0"),
            Self::Unavailable => hasher.update(b"blackboard-unavailable\0"),
        }
    }
}

pub(super) fn render_root_blackboard(
    output: &mut String,
    status: &RootBlackboardStatus,
) -> RootLayout {
    match status {
        RootBlackboardStatus::Available(root) => render_projection(output, root),
        RootBlackboardStatus::NotConfigured => {
            append_line(
                output,
                "Project intelligence is unavailable because persistent state is disabled. Use source files as ground truth and do not claim memory readiness.",
            );
            RootLayout::default()
        }
        RootBlackboardStatus::Unavailable => {
            append_line(
                output,
                "Project intelligence could not be loaded. Use source files as ground truth and do not claim memory or evidence readiness.",
            );
            RootLayout::default()
        }
    }
}

fn render_projection(output: &mut String, root: &ResolvedRootBlackboard) -> RootLayout {
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
    let (evidence_aliases, sources) = render_evidence_catalog(output, root);
    let identity_aliases = projection
        .data
        .iter()
        .map(|hit| (hit.entry.id.to_string(), hit.entry.id.to_string()))
        .collect::<HashMap<_, _>>();
    let identity_evidence = root
        .evidence_routes
        .keys()
        .map(|route| (route.clone(), route.to_string()))
        .collect::<HashMap<_, _>>();
    let mut entries = Vec::with_capacity(projection.data.len());
    let mut shown = Vec::with_capacity(projection.data.len());
    let mut omitted = projection.omitted_entries;
    for (index, hit) in projection.data.iter().enumerate() {
        let alias = format!("E{}", index + 1);
        let line = bounded_entry_line(
            render_hit(
                &alias,
                hit,
                &entry_aliases,
                &evidence_aliases,
                root.evidence_audit.as_ref(),
            ),
            &alias,
        );
        if try_append_line(output, &line, ROOT_FOOTER_RESERVE_BYTES) {
            if !line.ends_with(TRUNCATED_ENTRY_SUFFIX) {
                shown.push((index, alias));
            }
            let canonical = render_hit(
                hit.entry.id.as_str(),
                hit,
                &identity_aliases,
                &identity_evidence,
                root.evidence_audit.as_ref(),
            );
            entries.push(LaidOutLine {
                key: short_digest(hit.entry.id.as_str()),
                digest: short_digest(&canonical),
                line,
            });
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    if projection.data.is_empty() {
        append_line(
            output,
            "- No knowledge has been promoted to the root blackboard yet.",
        );
    }
    if projection.candidate_entries > 0 {
        append_line(
            output,
            &format!(
                "- {} active candidate entries await an explicit project-relevance decision. Query with rootPromotion=candidate, then use blackboard_update_batch to promote, keep deeper, revise, supersede, or retire them; do not infer that candidate means verified.",
                projection.candidate_entries
            ),
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
            "For durableLearning completion, pass this project intelligence revision as rootRevision and select at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases directly material to the requested outcome in materialRootFindings. Preserve additional material conclusions in the final semantic obligation. If finalObligation.learning is non-empty, ensure at least one selected current root or exact historical finding preserves that reusable learning. If the run produced no distinct finding likely to improve future project work—especially for a lookup or read-only citation answer from existing project state or source material—do not record or promote an entry merely to obtain an E alias; complete with stateful_run_update passing exactly expectedRevision, status completed, completionDisposition noReusableLearning, and result. rootRevision is not expectedRevision: copy expectedRevision from the separate Stateful run World State."
        ),
    );
    // Certify an entry as fully shown only after layout, against entries actually
    // rendered: every evidence reference must resolve in the packet, every premise
    // must be rendered at exactly its pinned revision, and relation-bearing entries
    // are never certified (relation detail is not rendered).
    let rendered_entries = shown
        .iter()
        .map(|(index, _)| {
            let entry = &projection.data[*index].entry;
            (entry.id.to_string(), entry.revision)
        })
        .collect::<HashMap<_, _>>();
    let complete_entries = shown
        .into_iter()
        .filter_map(|(index, alias)| {
            let hit = &projection.data[index];
            let complete = hit.relations.is_empty()
                && hit
                    .entry
                    .value
                    .evidence
                    .iter()
                    .all(|link| evidence_aliases.contains_key(&link.context_map_entry_id))
                && hit.entry.value.premises.iter().all(|premise| {
                    rendered_entries.get(premise.entry_id.as_str()) == Some(&premise.revision)
                });
            complete.then(|| (hit.entry.id.to_string(), alias, hit.entry.revision))
        })
        .collect();
    RootLayout {
        entries,
        sources,
        complete_entries,
    }
}

fn render_evidence_catalog(
    output: &mut String,
    root: &ResolvedRootBlackboard,
) -> (HashMap<ContextMapEntryId, String>, Vec<LaidOutLine>) {
    let mut ordered_routes = Vec::new();
    for evidence in root.projection.data.iter().flat_map(|hit| {
        hit.entry
            .value
            .evidence
            .iter()
            .chain(hit.premise_evidence())
    }) {
        if root
            .evidence_routes
            .contains_key(&evidence.context_map_entry_id)
            && !ordered_routes.contains(&evidence.context_map_entry_id)
        {
            ordered_routes.push(evidence.context_map_entry_id.clone());
        }
    }
    if ordered_routes.is_empty() {
        return (HashMap::new(), Vec::new());
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
    let mut sources = Vec::new();
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
        let freshness = root
            .evidence_audit
            .as_ref()
            .and_then(|audit| audited_context_freshness(audit, &entry_id, route.freshness))
            .map(context_freshness_name)
            .unwrap_or("uncheckedThisTurn");
        let line = format!(
            "- {alias}={root_alias}::{}{anchor} ({freshness})",
            route.source.relative_path,
        );
        if try_append_line(output, &line, ROOT_KNOWLEDGE_RESERVE_BYTES) {
            let canonical = format!(
                "{entry_id}|{}|{}{anchor}|{freshness}",
                route.source.project_root, route.source.relative_path
            );
            sources.push(LaidOutLine {
                key: short_digest(entry_id.as_str()),
                digest: short_digest(&canonical),
                line,
            });
            evidence_aliases.insert(entry_id, alias);
        }
    }
    (evidence_aliases, sources)
}

fn render_hit(
    alias: &str,
    hit: &BlackboardHit,
    entry_aliases: &HashMap<String, String>,
    evidence_aliases: &HashMap<ContextMapEntryId, String>,
    evidence_audit: Option<&EvidenceAudit>,
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
    let evidence_freshness = audited_blackboard_freshness(hit, evidence_audit);
    let premise_freshness = audited_premise_freshness(hit, evidence_audit);
    let effective_verification =
        audited_verification(value.verification, evidence_freshness, premise_freshness);
    let premises = value
        .premises
        .iter()
        .map(|premise| {
            let alias = entry_aliases
                .get(premise.entry_id.as_str())
                .map(String::as_str)
                .unwrap_or("deeper");
            format!("{alias}@r{}", premise.revision)
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "- {alias} [{} {}; verification={}; declared={}; evidence={}; premises={}; confidence={}; provenance={}] content={}{} sources=[{}] premiseRefs=[{}] relations=[{}]",
        importance_name(value.importance),
        kind_name(value.kind),
        verification_name(effective_verification),
        verification_name(value.verification),
        rendered_freshness_name(evidence_freshness),
        rendered_premise_freshness_name(premise_freshness),
        value.confidence.basis_points(),
        provenance_name(value.provenance.kind),
        single_line(&value.content),
        structured,
        evidence,
        premises,
        relations,
    )
}

/// Bounds a rendered entry line for display. Change detection must digest the
/// untruncated line, since truncation can hide an edited tail.
fn bounded_entry_line(mut line: String, alias: &str) -> String {
    if line.len() > MAX_ENTRY_BYTES {
        let marker = format!("… [{alias}{TRUNCATED_ENTRY_SUFFIX}");
        let maximum = MAX_ENTRY_BYTES.saturating_sub(marker.len());
        line.truncate(floor_char_boundary(&line, maximum));
        line.push_str(&marker);
    }
    line
}

pub(super) fn short_digest(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn rendered_premise_freshness_name(freshness: AuditedPremiseFreshness) -> &'static str {
    match freshness {
        AuditedPremiseFreshness::NotApplicable => "notApplicable",
        AuditedPremiseFreshness::Current => "current",
        AuditedPremiseFreshness::Stale => "stale",
        AuditedPremiseFreshness::SourceUnavailable => "sourceUnavailable",
        AuditedPremiseFreshness::UncheckedThisTurn => "uncheckedThisTurn",
    }
}

fn rendered_freshness_name(freshness: AuditedEvidenceFreshness) -> &'static str {
    match freshness {
        AuditedEvidenceFreshness::NotApplicable => "notApplicable",
        AuditedEvidenceFreshness::Current => "current",
        AuditedEvidenceFreshness::Stale => "stale",
        AuditedEvidenceFreshness::SourceUnavailable => "sourceUnavailable",
        AuditedEvidenceFreshness::UncheckedThisTurn => "uncheckedThisTurn",
    }
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
