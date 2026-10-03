//! Topic-first recall of one kind of knowledge (ruled-out items, decisions, open checks,
//! rules, background) for `memory_read`, decided before any hit cap or byte budget.
//!
//! Every current entry of the kind is read first (bounded), whole capture groups are kept
//! together in the order they were written, groups that mention the question's topic come
//! first and the rest are left out, and the thread's own investigation leads. The result is
//! an ordered list the caller pages through under its byte budget, with an honest count.

use std::collections::HashMap;
use std::collections::HashSet;

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::CategorizedEntry;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::MAX_CATEGORIZED_ENTRIES;
use codex_project_intelligence::MemberOutcome;
use codex_project_intelligence::ScopeState;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::user_rules::normalize;

use super::memory_read::mentions_any;

/// One kind of knowledge a question can ask for.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) enum RecallKind {
    RuledOut,
    Decision,
    OpenCheck,
    Rule,
    Background,
}

impl RecallKind {
    /// The kind a question asks about, if it plainly asks about one.
    pub(super) fn of_question(question: &str) -> Option<Self> {
        let words = format!(" {} ", normalize(question));
        let has = |phrases: &[&str]| phrases.iter().any(|phrase| words.contains(phrase));
        if has(&[
            " ruled out ",
            " rule out ",
            " rejected ",
            " eliminated ",
            " excluded ",
            " hypothes",
            " dead end",
        ]) {
            Some(Self::RuledOut)
        } else if has(&[
            " open check",
            " still open ",
            " unresolved ",
            " not yet verified ",
            " outstanding ",
            " open question",
            " left to check",
            " left to verify",
        ]) {
            Some(Self::OpenCheck)
        } else if has(&[
            " decid",
            " decision",
            " chose ",
            " chosen ",
            " choice ",
            " why ",
        ]) {
            Some(Self::Decision)
        } else if has(&[" rule ", " rules ", " instruction"]) {
            Some(Self::Rule)
        } else if has(&[" about me ", " my background ", " know about me "]) {
            Some(Self::Background)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::RuledOut => "ruledOut",
            Self::Decision => "decision",
            Self::OpenCheck => "openCheck",
            Self::Rule => "rule",
            Self::Background => "background",
        }
    }

    fn categories(self) -> (&'static [KnowledgeCategory], &'static [BlackboardKind]) {
        match self {
            Self::RuledOut => (
                &[KnowledgeCategory::RuledOut],
                &[BlackboardKind::RejectedApproach],
            ),
            Self::Decision => (&[KnowledgeCategory::Decision], &[BlackboardKind::Decision]),
            Self::OpenCheck => (&[KnowledgeCategory::OpenCheck], &[BlackboardKind::Question]),
            Self::Rule => (&[KnowledgeCategory::Rule], &[BlackboardKind::Instruction]),
            Self::Background => (&[KnowledgeCategory::Background], &[]),
        }
    }
}

/// Words that ask for a kind rather than name a topic.
const INTENT_WORDS: &[&str] = &[
    "ruled",
    "rule",
    "rules",
    "out",
    "rejected",
    "eliminated",
    "excluded",
    "hypotheses",
    "hypothesis",
    "dead",
    "end",
    "ends",
    "decided",
    "decide",
    "decision",
    "decisions",
    "chose",
    "chosen",
    "choice",
    "reason",
    "reasons",
    "open",
    "check",
    "checks",
    "still",
    "unresolved",
    "verified",
    "verify",
    "outstanding",
    "question",
    "questions",
    "left",
    "instruction",
    "instructions",
    "background",
    "list",
    "far",
    "items",
    "remind",
    "recall",
    "everything",
    "gave",
    "given",
    "standing",
    "know",
];

/// The ordered recall of one kind, ready for paging.
pub(super) struct KindRecall {
    pub(super) items: Vec<Value>,
    /// Identifies the set the items were drawn from; a cursor from another set restarts.
    pub(super) snapshot: String,
    pub(super) coverage: Value,
}

/// Every current item of `kind`, topic and investigation first, whole groups together.
pub(super) async fn kind_recall(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
    kind: RecallKind,
    terms: &[String],
) -> Result<KindRecall, String> {
    let (categories, legacy_kinds) = kind.categories();
    let (entries, more) = store
        .categorized_entries(
            project_id,
            categories,
            legacy_kinds,
            MAX_CATEGORIZED_ENTRIES,
        )
        .await
        .map_err(|error| error.to_string())?;
    let scope = store
        .thread_scope(project_id, thread_id)
        .await
        .map_err(|error| error.to_string())?
        .filter(|scope| scope.state == ScopeState::Open);
    let mut titles = HashMap::new();
    for scope in store
        .scopes(project_id, /*state*/ None)
        .await
        .map_err(|error| error.to_string())?
    {
        titles.insert(scope.scope_id, scope.title);
    }

    // Groups in the order their newest member was written; a member listed by its capture
    // group follows that group's order even when an earlier group saved it first.
    let by_id = entries
        .iter()
        .map(|candidate| (candidate.entry.id.to_string(), candidate))
        .collect::<HashMap<_, _>>();
    let mut groups: Vec<(Option<String>, Vec<&CategorizedEntry>)> = Vec::new();
    let mut placed = HashSet::new();
    for candidate in &entries {
        if placed.contains(&candidate.entry.id.to_string()) {
            continue;
        }
        let group_id = candidate
            .context
            .as_ref()
            .and_then(|context| context.group_id.clone());
        let mut members = Vec::new();
        if let Some(group_id) = &group_id
            && let Ok(Some(capture)) = store.capture(project_id, group_id).await
        {
            for member in capture.members {
                let listed = matches!(
                    member.outcome,
                    MemberOutcome::Saved | MemberOutcome::AlreadyPresent
                );
                if let Some(entry) = member
                    .entry_id
                    .filter(|_| listed)
                    .and_then(|id| by_id.get(&id).copied())
                    && placed.insert(entry.entry.id.to_string())
                {
                    members.push(entry);
                }
            }
        }
        if let Some(group_id) = &group_id {
            let mut rest = entries
                .iter()
                .filter(|other| {
                    other
                        .context
                        .as_ref()
                        .is_some_and(|context| context.group_id.as_ref() == Some(group_id))
                })
                .filter(|other| placed.insert(other.entry.id.to_string()))
                .collect::<Vec<_>>();
            rest.sort_by_key(|other| {
                other
                    .context
                    .as_ref()
                    .and_then(|context| context.unit_ordinal)
            });
            members.extend(rest);
        }
        if placed.insert(candidate.entry.id.to_string()) {
            members.push(candidate);
        }
        groups.push((group_id, members));
    }

    let topic = terms
        .iter()
        .filter(|term| !INTENT_WORDS.contains(&term.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    // A group is on the topic when any member, or the answer it came from, mentions it.
    let on_topic = |members: &[&CategorizedEntry]| {
        topic.is_empty()
            || members.iter().any(|member| {
                mentions_any(&member.entry.value.content, &topic)
                    || mentions_any(&answer_opening(member), &topic)
            })
    };
    let topic_matched = groups.iter().any(|(_, members)| on_topic(members));
    // On-topic groups first (other groups only after them), then the thread's own
    // investigation, then project-wide items, then other investigations.
    let rank = |members: &[&CategorizedEntry]| {
        let item_scope = members
            .first()
            .and_then(|member| member.context.as_ref())
            .and_then(|context| context.scope_id.as_deref());
        let scope_rank = match (item_scope, scope.as_ref()) {
            (Some(item), Some(own)) if item == own.scope_id => 0,
            (None, _) => 1,
            (Some(_), _) => 2,
        };
        (!on_topic(members), scope_rank)
    };
    groups.sort_by_key(|(_, members)| rank(members));
    let on_topic_items = groups
        .iter()
        .filter(|(_, members)| on_topic(members))
        .map(|(_, members)| members.len())
        .sum::<usize>();

    let mut items = Vec::new();
    let mut digest = Sha256::new();
    let mut groups_shown = Vec::new();
    for (group_id, members) in &groups {
        if let Some(group_id) = group_id
            && let Ok(Some(capture)) = store.capture(project_id, group_id).await
        {
            groups_shown.push(json!({
                "groupId": group_id,
                "source": capture.source.map(|source| source.locator),
                "recognized": capture.group.recognized,
                "saved": capture.group.saved,
                "alreadyPresent": capture.group.already_present,
                "notKept": capture.group.omitted + capture.group.failed,
            }));
        }
        for member in members {
            digest.update(member.entry.id.as_str());
            digest.update(member.entry.revision.to_le_bytes());
            items.push(item(member, group_id.as_deref(), &titles));
        }
    }
    let coverage = json!({
        "requestedKind": kind.name(),
        "topicTerms": topic,
        "onTopic": on_topic_items,
        "topicNote": if topic.is_empty() || topic_matched {
            json!("items on the topic come first; the rest of this kind follows them")
        } else {
            json!("no item of this kind mentions the topic; every current item of the kind is listed")
        },
        "threadInvestigation": scope.map(|scope| scope.title),
        "matching": items.len(),
        "moreOfThisKindThanRead": more,
        "captureGroups": groups_shown,
    });
    Ok(KindRecall {
        items,
        snapshot: format!("{:x}", digest.finalize())[..16].to_string(),
        coverage,
    })
}

/// The opening of the answer a captured unit came from.
fn answer_opening(member: &CategorizedEntry) -> String {
    member
        .context
        .as_ref()
        .and_then(|context| context.payload.as_deref())
        .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
        .and_then(|payload| payload["answerOpening"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn item(
    member: &CategorizedEntry,
    group_id: Option<&str>,
    titles: &HashMap<String, String>,
) -> Value {
    let value = &member.entry.value;
    let context = member.context.as_ref();
    let said_by = match context.map(|context| context.authority) {
        Some(KnowledgeAuthority::HumanDirect) => "the user's own words",
        Some(KnowledgeAuthority::AssistantReported) => {
            "the assistant's answer (reported, not verified)"
        }
        Some(KnowledgeAuthority::ReportedThirdParty) => "someone the user quoted",
        Some(KnowledgeAuthority::HostObserved) => "observed by the host",
        Some(KnowledgeAuthority::LegacyUnknown) | None => "agent record",
    };
    let status = match context.map(|context| context.validity) {
        Some(KnowledgeValidity::NeedsCheck) => "needs a check before relying on it",
        Some(KnowledgeValidity::Current) | None => "current",
        Some(KnowledgeValidity::Obsolete) | Some(KnowledgeValidity::Historical) => "historical",
    };
    let details = context
        .and_then(|context| context.payload.as_deref())
        .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
        .map(|payload| payload["details"].clone())
        .unwrap_or(Value::Null);
    let mut item = json!({
        "entryId": member.entry.id.to_string(),
        "revision": member.entry.revision,
        // Whole: an entry is at most 4 KiB, and a page moves an item that does not fit to
        // the next page rather than cutting it.
        "content": value.content,
        "status": status,
        "saidBy": said_by,
        "recorded": crate::continuity::format_time(member.entry.created_at_ms),
        "scope": context
            .and_then(|context| context.scope_id.as_ref())
            .map(|scope_id| titles.get(scope_id).cloned().unwrap_or_else(|| scope_id.clone()))
            .unwrap_or_else(|| "project-wide".to_string()),
    });
    if let Some(group_id) = group_id {
        item["groupId"] = json!(group_id);
    }
    if let Some(state) = details["state"].as_str() {
        item["checkState"] = json!(state);
    }
    if let Some(reason) = details["reasonStatus"].as_str() {
        item["reasonStatus"] = json!(reason);
    }
    item
}

#[cfg(test)]
#[path = "recall_plan_tests.rs"]
mod tests;
