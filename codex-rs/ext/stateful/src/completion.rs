use std::collections::HashSet;
use std::fmt::Write;
use std::path::PathBuf;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardVerification;
use codex_stateful_runtime::ObligationPacket;
use serde_json::Value;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::AuditedEvidenceFreshness;
use crate::source_freshness::AuditedPremiseFreshness;
use crate::source_freshness::audited_blackboard_freshness;
use crate::source_freshness::audited_premise_freshness;
use crate::source_freshness::audited_verification;
use crate::source_freshness::observe_evidence;
use crate::visible_root::VisibleRoot;

const MAX_FINAL_CHECKLIST_ITEMS: usize = 16;
const MAX_FINAL_CHECKLIST_ITEM_BYTES: usize = 640;
pub(crate) const MAX_MATERIAL_ROOT_FINDINGS: usize = 8;
/// Shared completion-disposition rule, rendered verbatim by the run update tool
/// and the run world-state packet so the model never sees divergent guidance.
pub(crate) const REUSABLE_LEARNING_RULE: &str = "Choose the completion disposition by what the run learned, not by whether files changed or the answer is short. Use noReusableLearning for an answer drawn from adequate existing project knowledge, a narrow source citation that adds no reusable understanding, or a trivial answer or cheap-to-recompute inventory. Reading sources can produce reusable learning even when no source files change: when an orientation establishes project purpose, module responsibilities and relationships (not a directory listing), or how to run the tests (stating whether that procedure is documented, executed successfully, or blocked), record the findings worth reusing and complete with durableLearning. Do not create duplicate entries or persist routine inventories merely to qualify for completion.";

pub(crate) struct CompletionRecord {
    pub(crate) result: String,
    pub(crate) checklist: Vec<Value>,
    pub(crate) omitted_checklist_items: usize,
}

pub(crate) struct CompletionRequest<'a> {
    pub(crate) project_id: &'a str,
    /// The thread completing; scoped rules of other investigations are not in its root.
    pub(crate) thread_id: &'a str,
    pub(crate) project_roots: &'a [PathBuf],
    pub(crate) result: &'a str,
    pub(crate) packet: &'a ObligationPacket,
    pub(crate) root_revision: u64,
    pub(crate) material_root_findings: &'a [String],
    /// The root the model was last shown; selected findings shown in full at the
    /// same revision are echoed back by alias instead of repeating their prose.
    pub(crate) visible_root: Option<&'a VisibleRoot>,
}

struct ChecklistItem {
    category: &'static str,
    text: String,
    /// Tool-output form for a finding already visible in full; the durable result
    /// always keeps `text`.
    compact_text: Option<String>,
}

struct MaterialChecklist {
    items: Vec<ChecklistItem>,
    source_fingerprints: HashSet<String>,
}

pub(crate) async fn prepare_completion(
    services: &ProjectIntelligenceServices,
    request: CompletionRequest<'_>,
) -> Result<CompletionRecord, FunctionCallError> {
    let CompletionRequest {
        project_id,
        thread_id,
        project_roots,
        result,
        packet,
        root_revision,
        material_root_findings,
        visible_root,
    } = request;
    if material_root_findings.len() > MAX_MATERIAL_ROOT_FINDINGS {
        return Err(respond(format!(
            "materialRootFindings accepts at most {MAX_MATERIAL_ROOT_FINDINGS} root aliases; select the highest-priority findings directly material to the outcome and preserve the remainder in the final semantic obligation"
        )));
    }
    let mut unique_references = HashSet::new();
    if let Some(reference) = material_root_findings
        .iter()
        .find(|reference| !unique_references.insert(reference.as_str()))
    {
        return Err(respond(format!(
            "materialRootFindings contains duplicate reference {reference}"
        )));
    }

    if !packet.learning.is_empty() && material_root_findings.is_empty() {
        return Err(respond(
            "finalObligation.learning contains reusable project knowledge, but completion selected no blackboard finding; record and promote the smallest durable conclusion, then retry with its current root alias in materialRootFindings",
        ));
    }

    let material = material_root_checklist(
        project_id,
        thread_id,
        services,
        project_roots,
        root_revision,
        material_root_findings,
        visible_root,
    )
    .await?;
    validate_source_fingerprint_references(result, packet, &material.source_fingerprints)?;

    let mut checklist = material.items;
    checklist.extend(packet_checklist(packet));
    if checklist.is_empty() {
        return Err(respond(
            "completed requires a final semantic obligation with at least one learning, implication, uncertainty, or blocker",
        ));
    }
    let omitted_checklist_items = checklist.len().saturating_sub(MAX_FINAL_CHECKLIST_ITEMS);
    checklist.truncate(MAX_FINAL_CHECKLIST_ITEMS);

    let mut durable_result = result.to_string();
    durable_result.push_str("\n\nDurable completion basis:");
    for item in &checklist {
        let label = match item.category {
            "rootFinding" => "Root finding",
            "learning" => "Learning",
            "implication" => "Implication",
            "uncertainty" => "Uncertainty",
            "blocker" => "Blocker",
            _ => unreachable!("completion checklist categories are static"),
        };
        let _ = write!(durable_result, "\n- {label}: {}", item.text);
    }
    if omitted_checklist_items > 0 {
        let _ = write!(
            durable_result,
            "\n- {omitted_checklist_items} additional final-obligation items remain in the structured obligation record."
        );
    }

    Ok(CompletionRecord {
        result: durable_result,
        checklist: checklist
            .into_iter()
            .map(|item| {
                json!({
                    "category": item.category,
                    "text": item.compact_text.unwrap_or(item.text),
                })
            })
            .collect(),
        omitted_checklist_items,
    })
}

/// The root blackboard revision completion validates `rootRevision` against, with the
/// number of E aliases it currently offers.
pub(crate) async fn completion_root(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    thread_id: &str,
) -> Result<(u64, usize), FunctionCallError> {
    let store = services.blackboard().await.map_err(respond)?;
    let (mut projection, _) =
        crate::rule_scope::applicable_projection(store, project_id, thread_id)
            .await
            .map_err(respond)?;
    crate::root_blackboard::retain_applicable_rules(&mut projection);
    Ok((projection.revision, projection.data.len()))
}

async fn material_root_checklist(
    project_id: &str,
    thread_id: &str,
    services: &ProjectIntelligenceServices,
    project_roots: &[PathBuf],
    expected_root_revision: u64,
    requested_references: &[String],
    visible_root: Option<&VisibleRoot>,
) -> Result<MaterialChecklist, FunctionCallError> {
    let store = services.blackboard().await.map_err(respond)?;
    // Aliases are positions in the projection the packet showed, which never holds rules
    // that are not in the user's own words, nor rules of an investigation this thread is not
    // part of.
    let (mut projection, _) =
        crate::rule_scope::applicable_projection(store, project_id, thread_id)
            .await
            .map_err(respond)?;
    crate::root_blackboard::retain_applicable_rules(&mut projection);
    if projection.revision != expected_root_revision {
        return Err(respond(format!(
            "root blackboard changed from revision {expected_root_revision} to {}; review the current root aliases before completing",
            projection.revision
        )));
    }
    let mut selected = Vec::with_capacity(requested_references.len());
    for reference in requested_references {
        let Some(raw_index) = reference.strip_prefix('E') else {
            return Err(invalid_root_alias(
                reference,
                expected_root_revision,
                projection.data.len(),
            ));
        };
        let Ok(index) = raw_index.parse::<usize>() else {
            return Err(invalid_root_alias(
                reference,
                expected_root_revision,
                projection.data.len(),
            ));
        };
        if index == 0 || raw_index.starts_with('0') {
            return Err(invalid_root_alias(
                reference,
                expected_root_revision,
                projection.data.len(),
            ));
        }
        let Some(hit) = projection.data.get(index - 1) else {
            return Err(respond(format!(
                "unknown material root finding alias {reference} at root revision {expected_root_revision}: {}",
                root_alias_guidance(projection.data.len())
            )));
        };
        selected.push((reference, hit));
    }
    let mut evidence_ids = Vec::new();
    for (_, hit) in &selected {
        evidence_ids.extend(
            hit.entry
                .value
                .evidence
                .iter()
                .chain(hit.premise_evidence())
                .map(|evidence| evidence.context_map_entry_id.clone()),
        );
    }
    let evidence_audit = if evidence_ids.is_empty() {
        None
    } else {
        Some(observe_evidence(services, project_id, project_roots, evidence_ids).await)
    };
    let context_map = services.context_map().await.map_err(respond)?;
    let mut material = MaterialChecklist {
        items: Vec::with_capacity(requested_references.len()),
        source_fingerprints: HashSet::new(),
    };
    for (reference, hit) in selected {
        let evidence_freshness = audited_blackboard_freshness(hit, evidence_audit.as_ref());
        let premise_freshness = audited_premise_freshness(hit, evidence_audit.as_ref());
        if hit.entry.value.verification == BlackboardVerification::SourceVerified
            && evidence_freshness != AuditedEvidenceFreshness::Current
        {
            return Err(respond(format!(
                "material root finding {reference} evidence is {}; read current evidence and revise or supersede the finding before completing",
                freshness_name(evidence_freshness)
            )));
        }
        if !matches!(
            premise_freshness,
            AuditedPremiseFreshness::NotApplicable | AuditedPremiseFreshness::Current
        ) {
            return Err(respond(format!(
                "material root finding {reference} premises are {}; revise or supersede the finding before completing",
                premise_freshness_name(premise_freshness)
            )));
        }
        let effective_verification = audited_verification(
            hit.entry.value.verification,
            evidence_freshness,
            premise_freshness,
        );
        let mut sources = Vec::new();
        for evidence in hit
            .entry
            .value
            .evidence
            .iter()
            .chain(hit.premise_evidence())
        {
            material
                .source_fingerprints
                .insert(evidence.source_fingerprint.as_str().to_ascii_lowercase());
            let Some(route) = context_map
                .get_hit(project_id, &evidence.context_map_entry_id)
                .await
                .map_err(respond)?
            else {
                continue;
            };
            let locator = match evidence.line_range {
                Some(range) => format!(
                    "{}:L{}-L{}",
                    route.source.relative_path, range.start, range.end
                ),
                None => route.source.relative_path.to_string(),
            };
            let source = format!("{locator}@{}", evidence.source_fingerprint);
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        let sources = if sources.is_empty() {
            String::new()
        } else {
            format!(" sources=[{}]", sources.join(","))
        };
        let labels = format!(
            "{reference} [{}; verification={}; evidence={}; premises={}]",
            importance_name(hit.entry.value.importance),
            verification_name(effective_verification),
            freshness_name(evidence_freshness),
            premise_freshness_name(premise_freshness),
        );
        let shown_in_root = visible_root.is_some_and(|visible| {
            visible.project_revision == expected_root_revision
                && visible.alias_for(hit.entry.id.as_str(), hit.entry.revision)
                    == Some(reference.as_str())
        });
        material.items.push(ChecklistItem {
            category: "rootFinding",
            text: bounded_item(&format!("{labels} {}{sources}", hit.entry.value.content)),
            compact_text: shown_in_root.then(|| {
                bounded_item(&format!(
                    "{labels} (content as shown in the root packet){sources}"
                ))
            }),
        });
    }
    Ok(material)
}

fn premise_freshness_name(freshness: AuditedPremiseFreshness) -> &'static str {
    match freshness {
        AuditedPremiseFreshness::NotApplicable => "notApplicable",
        AuditedPremiseFreshness::Current => "current",
        AuditedPremiseFreshness::Stale => "stale",
        AuditedPremiseFreshness::SourceUnavailable => "sourceUnavailable",
        AuditedPremiseFreshness::UncheckedThisTurn => "uncheckedThisTurn",
    }
}

fn invalid_root_alias(
    reference: &str,
    expected_root_revision: u64,
    root_entries: usize,
) -> FunctionCallError {
    respond(format!(
        "invalid material root finding alias {reference} at root revision {expected_root_revision}; materialRootFindings takes E aliases, not entry IDs: {}",
        root_alias_guidance(root_entries)
    ))
}

fn root_alias_guidance(root_entries: usize) -> String {
    let aliases = match root_entries {
        0 => "the root currently has no promoted entries".to_string(),
        1 => "the root currently has only E1".to_string(),
        count => format!("the root currently has E1..E{count}"),
    };
    format!(
        "{aliases}. Only root-promoted entries have E aliases; promote a recorded candidate or deeper finding with blackboard_update_batch setRootPromotion before selecting it, or complete with completionDisposition noReusableLearning when nothing reusable was learned."
    )
}

fn validate_source_fingerprint_references(
    result: &str,
    packet: &ObligationPacket,
    allowed: &HashSet<String>,
) -> Result<(), FunctionCallError> {
    let packet = serde_json::to_string(packet).map_err(respond)?;
    for fingerprint in sha256_references(result).chain(sha256_references(&packet)) {
        if !allowed.contains(&fingerprint.to_ascii_lowercase()) {
            return Err(respond(format!(
                "completion cites unknown source fingerprint {fingerprint}; select a current root finding with that evidence in materialRootFindings or remove the opaque identifier"
            )));
        }
    }
    Ok(())
}

fn sha256_references(value: &str) -> impl Iterator<Item = &str> {
    value.match_indices("sha256:").filter_map(|(start, _)| {
        let end = start + "sha256:".len() + 64;
        let candidate = value.get(start..end)?;
        let digest = candidate.strip_prefix("sha256:")?;
        if digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            && value
                .as_bytes()
                .get(end)
                .is_none_or(|byte| !byte.is_ascii_hexdigit())
        {
            Some(candidate)
        } else {
            None
        }
    })
}

fn packet_checklist(packet: &ObligationPacket) -> Vec<ChecklistItem> {
    [
        ("learning", &packet.learning),
        ("implication", &packet.implication),
        ("uncertainty", &packet.uncertainty),
        ("blocker", &packet.blockers),
    ]
    .into_iter()
    .flat_map(|(category, items)| {
        items.iter().map(move |item| ChecklistItem {
            category,
            text: bounded_item(item),
            compact_text: None,
        })
    })
    .collect()
}

fn bounded_item(item: &str) -> String {
    if item.len() <= MAX_FINAL_CHECKLIST_ITEM_BYTES {
        return item.to_string();
    }
    let marker = "…";
    let mut boundary = MAX_FINAL_CHECKLIST_ITEM_BYTES.saturating_sub(marker.len());
    while !item.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{marker}", &item[..boundary])
}

fn importance_name(importance: BlackboardImportance) -> &'static str {
    match importance {
        BlackboardImportance::Critical => "critical",
        BlackboardImportance::High => "high",
        BlackboardImportance::Normal => "normal",
        BlackboardImportance::Low => "low",
    }
}

fn verification_name(verification: BlackboardVerification) -> &'static str {
    match verification {
        BlackboardVerification::Unverified => "unverified",
        BlackboardVerification::SourceVerified => "sourceVerified",
        BlackboardVerification::UserConfirmed => "userConfirmed",
        BlackboardVerification::Disputed => "disputed",
        BlackboardVerification::Stale => "stale",
    }
}

fn freshness_name(freshness: AuditedEvidenceFreshness) -> &'static str {
    match freshness {
        AuditedEvidenceFreshness::NotApplicable => "notApplicable",
        AuditedEvidenceFreshness::Current => "current",
        AuditedEvidenceFreshness::Stale => "stale",
        AuditedEvidenceFreshness::SourceUnavailable => "sourceUnavailable",
        AuditedEvidenceFreshness::UncheckedThisTurn => "uncheckedThisTurn",
    }
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
