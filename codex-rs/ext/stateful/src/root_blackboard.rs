use std::collections::HashMap;

use codex_project_intelligence::BlackboardEntry;
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
pub(super) const USER_BACKGROUND_HEADER: &str = "About the user (their own words about themselves and this work; use it to pitch explanations, it is not a rule):";

fn is_user_background(hit: &BlackboardHit) -> bool {
    hit.entry.value.kind == BlackboardKind::Fact
        && hit.entry.value.provenance.kind == BlackboardProvenanceKind::User
        && hit
            .entry
            .id
            .as_str()
            .starts_with(crate::memory_controls::USER_BACKGROUND_ID_PREFIX)
}

pub(super) const USER_RULES_HEADER: &str = "User rules (the user's exact words; each applies within the scope it states until the user changes it):";
const KNOWLEDGE_HEADER: &str = "Other promoted knowledge:";

/// Longest excerpt of a replaced value shown on its successor's line.
const MAX_REPLACED_EXCERPT_BYTES: usize = 160;

/// ` replaces: "<old value>" (until YYYY-MM-DD; ...)` for the line of the entry that
/// superseded `predecessor`.
fn replaces_suffix(predecessor: &BlackboardEntry) -> String {
    let content = single_line(&predecessor.value.content);
    let excerpt = if content.len() > MAX_REPLACED_EXCERPT_BYTES {
        let mut end = MAX_REPLACED_EXCERPT_BYTES;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &content[..end])
    } else {
        content
    };
    let until = crate::continuity::format_time(predecessor.updated_at_ms);
    let until = until.split(' ').next().unwrap_or(&until);
    format!(
        " replaces: {} (until {until}; earlier versions: memory_read)",
        serde_json::Value::String(excerpt)
    )
}

/// Lays out root entries in a chosen order while keeping their projection aliases.
struct EntryLayout<'a> {
    entry_aliases: &'a HashMap<String, String>,
    identity_aliases: HashMap<String, String>,
    identity_evidence: HashMap<ContextMapEntryId, String>,
    evidence_audit: Option<&'a EvidenceAudit>,
    /// The newest entry each shown entry replaced, by entry ID.
    predecessors: &'a HashMap<String, BlackboardEntry>,
    entries: Vec<LaidOutLine>,
    shown: Vec<(usize, String)>,
    omitted: u64,
}

impl EntryLayout<'_> {
    fn place(
        &mut self,
        output: &mut String,
        index: usize,
        hit: &BlackboardHit,
        evidence_aliases: &HashMap<ContextMapEntryId, String>,
    ) {
        let alias = format!("E{}", index + 1);
        let plain = render_hit(
            &alias,
            hit,
            self.entry_aliases,
            evidence_aliases,
            self.evidence_audit,
        );
        // What the current value replaced is decoration: it is dropped before the entry
        // itself is.
        // Only a decoration that keeps the whole entry within its bound is tried.
        let predecessor = self.predecessors.get(hit.entry.id.as_str());
        let decorated = predecessor
            .map(|predecessor| format!("{plain}{}", replaces_suffix(predecessor)))
            .filter(|decorated| decorated.len() <= MAX_ENTRY_BYTES);
        let plain = bounded_entry_line(plain, &alias);
        let (line, decorated_shown) = match decorated {
            Some(decorated) if try_append_line(output, &decorated, ROOT_FOOTER_RESERVE_BYTES) => {
                (Some(decorated), true)
            }
            Some(_) | None => (
                try_append_line(output, &plain, ROOT_FOOTER_RESERVE_BYTES).then_some(plain),
                false,
            ),
        };
        if let Some(line) = line {
            if !line.ends_with(TRUNCATED_ENTRY_SUFFIX) {
                self.shown.push((index, alias));
            }
            let canonical = format!(
                "{}{}",
                render_hit(
                    hit.entry.id.as_str(),
                    hit,
                    &self.identity_aliases,
                    &self.identity_evidence,
                    self.evidence_audit,
                ),
                predecessor.filter(|_| decorated_shown).map_or_else(
                    String::new,
                    |predecessor| format!("|replaces:{}@{}", predecessor.id, predecessor.revision)
                ),
            );
            self.entries.push(LaidOutLine {
                key: short_digest(hit.entry.id.as_str()),
                digest: short_digest(&canonical),
                line,
            });
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }
}

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
    /// Instruction entries removed because they are not in the user's own words.
    quarantined_rules: u64,
    /// The newest entry each projected entry replaced, by successor entry ID.
    predecessors: HashMap<String, BlackboardEntry>,
    /// What the packet says about investigation-scoped rules.
    scope_note: Option<String>,
}

/// Removes rules that are not in the user's own words from a root projection, before any
/// alias is assigned, and returns how many were removed. Every consumer of root aliases
/// (the packet, its deltas, completion) must apply this to the same projection.
pub(super) fn retain_applicable_rules(projection: &mut RootBlackboardProjection) -> u64 {
    let before = projection.data.len();
    projection.data.retain(|hit| {
        hit.entry.value.kind != BlackboardKind::Instruction
            || hit.entry.value.provenance.kind == BlackboardProvenanceKind::User
    });
    u64::try_from(before - projection.data.len()).unwrap_or(u64::MAX)
}

impl ResolvedRootBlackboard {
    /// Rules not in the user's own words are never applied: an agent's paraphrase or its
    /// invention must not become a standing constraint. They leave the projection itself, so
    /// aliases, change deltas and completion never see them.
    pub(super) fn new(
        mut projection: RootBlackboardProjection,
        evidence_routes: HashMap<ContextMapEntryId, ContextMapHit>,
        evidence_audit: Option<EvidenceAudit>,
    ) -> Self {
        let quarantined_rules = retain_applicable_rules(&mut projection);
        Self {
            projection,
            evidence_routes,
            evidence_audit,
            quarantined_rules,
            predecessors: HashMap::new(),
            scope_note: None,
        }
    }

    /// Notes which investigation rules were left out of the projection (they were removed
    /// before any alias was assigned).
    pub(super) fn with_scope_note(mut self, note: Option<String>) -> Self {
        self.scope_note = note;
        self
    }

    /// Attaches what shown entries replaced, so the packet can say "replaces: ..." without
    /// a memory call.
    pub(super) fn with_predecessors(
        mut self,
        predecessors: impl IntoIterator<Item = (String, BlackboardEntry)>,
    ) -> Self {
        self.predecessors = predecessors.into_iter().collect();
        self
    }
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
                hasher.update(root.quarantined_rules.to_be_bytes());
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
    let mut layout = EntryLayout {
        entry_aliases: &entry_aliases,
        identity_aliases: projection
            .data
            .iter()
            .map(|hit| (hit.entry.id.to_string(), hit.entry.id.to_string()))
            .collect(),
        identity_evidence: root
            .evidence_routes
            .keys()
            .map(|route| (route.clone(), route.to_string()))
            .collect(),
        evidence_audit: root.evidence_audit.as_ref(),
        predecessors: &root.predecessors,
        entries: Vec::with_capacity(projection.data.len()),
        shown: Vec::with_capacity(projection.data.len()),
        omitted: projection.omitted_entries,
    };
    // The user's rules are laid out first, before the source catalog and any other entry,
    // so no amount of other knowledge can push them out of the packet.
    let (rules, knowledge): (Vec<_>, Vec<_>) = projection
        .data
        .iter()
        .enumerate()
        .partition(|(_, hit)| hit.entry.value.kind == BlackboardKind::Instruction);
    if !rules.is_empty() {
        append_line(output, USER_RULES_HEADER);
        for (index, hit) in &rules {
            layout.place(output, *index, hit, &HashMap::new());
        }
    }
    // What the user said about themselves follows the rules, also ahead of the catalog.
    let (background, knowledge): (Vec<_>, Vec<_>) = knowledge
        .into_iter()
        .partition(|(_, hit)| is_user_background(hit));
    if !background.is_empty() {
        append_line(output, USER_BACKGROUND_HEADER);
        for (index, hit) in &background {
            layout.place(output, *index, hit, &HashMap::new());
        }
    }
    let (evidence_aliases, sources) = render_evidence_catalog(output, root);
    if (!rules.is_empty() || !background.is_empty()) && !knowledge.is_empty() {
        append_line(output, KNOWLEDGE_HEADER);
    }
    for (index, hit) in &knowledge {
        layout.place(output, *index, hit, &evidence_aliases);
    }
    let EntryLayout {
        entries,
        shown,
        omitted,
        ..
    } = layout;
    if projection.data.is_empty() {
        append_line(
            output,
            "- No knowledge has been promoted to the root blackboard yet.",
        );
    }
    if let Some(note) = &root.scope_note {
        append_line(output, note);
    }
    if root.quarantined_rules > 0 {
        append_line(
            output,
            &format!(
                "- {} agent-recorded rules are not applied: they are not the user's own words. blackboard_query lists them; treat them as unconfirmed.",
                root.quarantined_rules
            ),
        );
    }
    if projection.candidate_entries > 0 {
        append_line(
            output,
            &format!(
                "- {} active candidate entries await promotion and are not shown; blackboard_query with rootPromotion=candidate lists them if the task needs them.",
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
            "For durableLearning completion, pass this project intelligence revision as rootRevision and at most {MAX_MATERIAL_ROOT_FINDINGS} E aliases directly material to the outcome in materialRootFindings; do not record or promote an entry merely to obtain an alias. rootRevision is not the run's expectedRevision."
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

#[cfg(test)]
#[path = "root_blackboard_tests.rs"]
mod tests;
