//! Topic-first recall of one kind of knowledge (ruled-out items, decisions, open checks,
//! rules, background) for `memory_read`, decided before any hit cap or byte budget.
//!
//! Every current entry of the kind is read first (one bounded query), whole capture groups
//! are kept together in the order they were written, groups that mention the question's
//! topic come first and the rest follow them, the thread's own investigation leads, and
//! items of ended investigations come last, marked historical. The result is an ordered list
//! the caller pages through under its byte budget, with honest counts and the captures that
//! could not keep every unit.

use std::collections::HashMap;
use std::collections::HashSet;

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::CandidateLifecycle;
use codex_project_intelligence::CategorizedEntry;
use codex_project_intelligence::CategoryQuery;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeScope;
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
            " why did we choose ",
            " why did we pick ",
            " why did we go with ",
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

    /// The capture-group kind answers record for this kind, when answers record it.
    fn group_kind(self) -> Option<&'static str> {
        match self {
            Self::RuledOut => Some("ruledOut"),
            Self::Decision => Some("decisions"),
            Self::OpenCheck => Some("openChecks"),
            Self::Rule => Some("rules"),
            Self::Background => None,
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

/// Most captures with units not kept that a recall lists by name (the count covers all).
const MAX_INCOMPLETE_CAPTURES: u32 = 5;

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

/// Every current item of `kind` (changed since `since_ms` when given), topic and
/// investigation first, whole groups together.
pub(super) async fn kind_recall(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
    kind: RecallKind,
    terms: &[String],
    since_ms: Option<i64>,
) -> Result<KindRecall, String> {
    let (categories, legacy_kinds) = kind.categories();
    let topic = terms
        .iter()
        .filter(|term| !INTENT_WORDS.contains(&term.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    // Items on the topic are read first, by the store, so newer unrelated items can never
    // cut them off; the newest items of the kind fill the rest.
    let query = |topic| CategoryQuery {
        project_id,
        categories,
        legacy_kinds,
        lifecycle: CandidateLifecycle::Current,
        topic,
        changed_since_ms: since_ms,
        limit: MAX_CATEGORIZED_ENTRIES,
    };
    let (mut entries, mut more) = if topic.is_empty() {
        (Vec::new(), false)
    } else {
        store
            .categorized_entries(query(&topic))
            .await
            .map_err(|error| error.to_string())?
    };
    let (newest, more_newest) = store
        .categorized_entries(query(&[]))
        .await
        .map_err(|error| error.to_string())?;
    more |= more_newest;
    let read = entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<HashSet<_>>();
    entries.extend(newest.into_iter().filter(|entry| !read.contains(&entry.id)));
    // A selected group is shown whole: members the bounded reads left out are read by ID.
    let mut captures = HashMap::new();
    for entry in &entries {
        if let Some(group_id) = entry
            .context
            .as_ref()
            .and_then(|context| context.group_id.as_ref())
            && !captures.contains_key(group_id)
            && let Some(capture) = store
                .capture(project_id, group_id)
                .await
                .map_err(|error| error.to_string())?
        {
            captures.insert(group_id.clone(), capture);
        }
    }
    let read = entries
        .iter()
        .map(|entry| entry.id.to_string())
        .collect::<HashSet<_>>();
    let missing = captures
        .values()
        .flat_map(|capture| &capture.members)
        .filter_map(|member| member.entry_id.clone())
        .filter(|id| !read.contains(id))
        .collect::<Vec<_>>();
    entries.extend(
        store
            .current_entries(project_id, &missing)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|entry| since_ms.is_none_or(|since| entry.updated_at_ms >= since)),
    );
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at_ms));
    let scope = store
        .thread_scope(project_id, thread_id)
        .await
        .map_err(|error| error.to_string())?
        .filter(|scope| scope.state == ScopeState::Open);
    let mut scopes = HashMap::new();
    for scope in store
        .scopes(project_id, /*state*/ None)
        .await
        .map_err(|error| error.to_string())?
    {
        scopes.insert(scope.scope_id.clone(), scope);
    }

    // Groups in the order their newest member was written; a member listed by its capture
    // group follows that group's order even when an earlier group saved it first.
    let by_id = entries
        .iter()
        .map(|candidate| (candidate.id.to_string(), candidate))
        .collect::<HashMap<_, _>>();
    let mut groups: Vec<Group<'_>> = Vec::new();
    let mut placed = HashSet::new();
    for candidate in &entries {
        if placed.contains(&candidate.id.to_string()) {
            continue;
        }
        let group_id = candidate
            .context
            .as_ref()
            .and_then(|context| context.group_id.clone());
        let mut members = Vec::new();
        let mut not_kept = 0;
        if let Some(group_id) = &group_id
            && let Some(capture) = captures.get(group_id)
        {
            not_kept = capture.group.omitted + capture.group.failed;
            for member in capture.members.iter().cloned() {
                let listed = matches!(
                    member.outcome,
                    MemberOutcome::Saved | MemberOutcome::AlreadyPresent
                );
                if let Some(entry) = member
                    .entry_id
                    .filter(|_| listed)
                    .and_then(|id| by_id.get(&id).copied())
                    && placed.insert(entry.id.to_string())
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
                .filter(|other| placed.insert(other.id.to_string()))
                .collect::<Vec<_>>();
            rest.sort_by_key(|other| {
                other
                    .context
                    .as_ref()
                    .and_then(|context| context.unit_ordinal)
            });
            members.extend(rest);
        }
        if placed.insert(candidate.id.to_string()) {
            members.push(candidate);
        }
        groups.push(Group {
            id: group_id,
            members,
            not_kept,
        });
    }

    // A group is on the topic when any member, or the answer it came from, mentions it.
    let on_topic = |group: &Group<'_>| {
        topic.is_empty()
            || group.members.iter().any(|member| {
                mentions_any(&member.content, &topic)
                    || mentions_any(&answer_opening(member), &topic)
            })
    };
    let ended = |group: &Group<'_>| {
        group_scope(group)
            .and_then(|scope_id| scopes.get(scope_id))
            .is_some_and(|scope| scope.state == ScopeState::Ended)
    };
    let topic_matched = groups.iter().any(on_topic);
    // On-topic groups first (other groups only after them); then the thread's own
    // investigation, project-wide items, other open investigations, and last items of
    // investigations that ended.
    let rank = |group: &Group<'_>| {
        let scope_rank = match (group_scope(group), scope.as_ref()) {
            _ if ended(group) => 3,
            (Some(item), Some(own)) if item == own.scope_id => 0,
            (None, _) => 1,
            (Some(_), _) => 2,
        };
        (!on_topic(group), scope_rank)
    };
    groups.sort_by_key(|group| rank(group));
    let on_topic_items = groups
        .iter()
        .filter(|group| on_topic(group))
        .map(|group| group.members.len())
        .sum::<usize>();

    let mut items = Vec::new();
    let mut digest = Sha256::new();
    for group in &groups {
        let ended = ended(group);
        for member in &group.members {
            digest.update(member.id.as_str());
            digest.update(member.revision.to_le_bytes());
            let mut item = item(member, group.id.as_deref(), &scopes);
            if ended {
                item["status"] = json!("historical: its investigation has ended");
            }
            if group.not_kept > 0 {
                item["groupNotKept"] = json!(group.not_kept);
            }
            items.push(item);
        }
    }
    // Captures of this kind that recognized units they could not keep, even when nothing
    // of them was saved, so a complete page is never mistaken for a complete capture.
    let (incomplete, incomplete_total) = match kind.group_kind() {
        Some(group_kind) => store
            .incomplete_captures(project_id, group_kind, since_ms, MAX_INCOMPLETE_CAPTURES)
            .await
            .map_err(|error| error.to_string())?,
        None => (Vec::new(), 0),
    };
    let coverage = json!({
        "requestedKind": kind.name(),
        "topicTerms": topic,
        "onTopic": on_topic_items,
        "topicNote": if topic.is_empty() || topic_matched {
            json!("items on the topic come first; the rest of this kind follows them")
        } else {
            json!("no item of this kind mentions the topic; every current item of the kind is listed")
        },
        "since": since_ms.map(crate::continuity::format_time),
        "threadInvestigation": scope.map(|scope| scope.title),
        "matching": items.len(),
        "moreOfThisKindThanRead": more,
        // Counted over the requested period (all time without since), not only this topic.
        "capturesWithUnitsNotKept": incomplete_total,
        "latestCapturesWithUnitsNotKept": incomplete
            .into_iter()
            .map(|capture| json!({
                "groupId": capture.group.group_id,
                "source": capture.source.map(|source| source.locator),
                "notKept": capture.group.omitted + capture.group.failed,
            }))
            .collect::<Vec<_>>(),
    });
    Ok(KindRecall {
        items,
        snapshot: format!("{:x}", digest.finalize())[..16].to_string(),
        coverage,
    })
}

/// Members of one capture group (or one entry outside any group), in source order.
struct Group<'a> {
    id: Option<String>,
    members: Vec<&'a CategorizedEntry>,
    /// Units the group's capture recognized but did not keep.
    not_kept: u32,
}

fn group_scope<'a>(group: &Group<'a>) -> Option<&'a str> {
    group
        .members
        .first()
        .and_then(|member| member.context.as_ref())
        .and_then(|context| context.scope_id.as_deref())
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
    scopes: &HashMap<String, KnowledgeScope>,
) -> Value {
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
        "entryId": member.id.to_string(),
        "revision": member.revision,
        // Whole: an entry is at most 4 KiB; a page moves an item that does not fit to the
        // next page, and cuts (and says so) only an item no page could hold.
        "content": member.content,
        "status": status,
        "saidBy": said_by,
        "recorded": crate::continuity::format_time(member.created_at_ms),
        "scope": context
            .and_then(|context| context.scope_id.as_ref())
            .map(|scope_id| {
                scopes
                    .get(scope_id)
                    .map_or_else(|| scope_id.clone(), |scope| scope.title.clone())
            })
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
