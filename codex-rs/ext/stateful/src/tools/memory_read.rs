//! One host-assembled recall of earlier work: matching knowledge with its status, history,
//! provenance and evidence routes, and the earlier turns that match, in one bounded result.
//!
//! It replaces chains of query, read and conversation calls for questions such as "what is
//! the default and why" or "summarize what we decided since Monday". Nothing here calls a
//! model; everything is read from the project's stores within fixed scan bounds, and the
//! result says what it covered.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::BlackboardEntryScope;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardQuery;
use codex_project_intelligence::BlackboardStore;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::conversation_summaries::is_top_level;
use crate::conversation_summaries::project_threads_params;
use crate::conversation_summaries::turn_status;
use crate::conversation_summaries::turn_texts;
use crate::conversation_summaries::turn_time_ms;
use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::bounded_json_output;
use super::fits_response;
use super::parse_arguments;
use super::respond;

const TOOL_NAME: &str = "memory_read";
/// Enough for a multi-topic question ("why sign, and why mth and yr?").
const MAX_TERMS: usize = 24;
const MAX_SEARCH_HITS: u32 = 30;
/// Full predecessor entries and hits loaded by history expansion across all groups.
const MAX_HISTORY_PREDECESSORS: u32 = 30;
/// Successor lookups one call may make while resolving matches to their current entries.
const MAX_SUCCESSOR_LOOKUPS: usize = 64;
const MAX_ENTRY_CONTENT_BYTES: usize = 600;
const MAX_RELATION_EXCERPT_BYTES: usize = 160;
const MAX_RELATIONS_PER_ENTRY: usize = 4;
/// Entries changed in the requested period that are ranked by the question.
const MAX_SINCE_CANDIDATES: u32 = 200;
const MAX_THREADS: usize = 20;
const MAX_TURNS_PER_THREAD: usize = 30;
const MAX_TURNS_SCANNED: usize = 300;
const MAX_TURNS_RETURNED: usize = 12;
const MAX_USER_BYTES: usize = 320;
const MAX_ANSWER_BYTES: usize = 480;
/// Serialized bytes kept free for coverage, routes and closing fields.
const RESERVED_BYTES: usize = 900;
/// Serialized bytes entries may use before turns are added.
const ENTRY_BUDGET_BYTES: usize = 5_000;
const ANSWER_LABEL: &str =
    "assistant's final answer (reported history, not evidence of the user's preferences)";

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "was", "were", "what", "why", "how", "did", "does", "our", "you",
    "your", "with", "that", "this", "from", "have", "has", "had", "into", "about", "which", "when",
    "where", "who", "can", "could", "would", "should", "will", "there", "their", "them", "then",
    "than", "they", "its", "it's", "not", "but", "all", "any", "each", "some", "since", "been",
    "also", "just", "on", "off", "is", "to", "of", "in", "it", "we", "be", "do", "go", "my", "me",
    "up", "so", "no", "or", "an", "as", "at", "by", "if", "us", "am", "he", "she", "her", "him",
    "his", "out", "too", "one", "now", "new", "get", "got", "let", "say", "said", "use", "used",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryReadArguments {
    question: Option<String>,
    since: Option<String>,
    include_history: Option<bool>,
}

pub(super) struct MemoryReadTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    threads: Arc<dyn ThreadStore>,
}

impl MemoryReadTool {
    pub(super) fn new(
        project_id: String,
        services: ProjectIntelligenceServices,
        threads: Arc<dyn ThreadStore>,
    ) -> Self {
        Self {
            project_id,
            services,
            threads,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: MemoryReadArguments = parse_arguments(&call)?;
        let terms = arguments
            .question
            .as_deref()
            .map(question_terms)
            .unwrap_or_default();
        let since_ms = arguments
            .since
            .as_deref()
            .map(parse_utc_day)
            .transpose()
            .map_err(respond)?;
        if terms.is_empty() && since_ms.is_none() {
            return Err(respond(
                "pass a question with content words, or since (YYYY-MM-DD, UTC), or both",
            ));
        }
        let include_history = arguments.include_history.unwrap_or(true);
        let store = self.services.blackboard().await.map_err(respond)?;
        let (groups, knowledge_truncated) = self
            .knowledge(store, &terms, since_ms, include_history)
            .await?;
        let (turns, coverage) = self.conversation(&terms, since_ms).await;

        let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let mut result = json!({
            "answeredFrom": "host stores (no model call): project knowledge with its status and history, and the first user message and final answer of matching earlier turns",
            "entries": [],
            "turns": [],
            "coverage": coverage,
            "use": "This evidence needs no confirmation read. Read more only when an omitted part is critical to the answer: conversation_read with a threadId and turnId for a full turn, evidence_read for an entry's source, git show for a commit.",
        });
        let entry_limit = budget
            .saturating_sub(RESERVED_BYTES)
            .min(ENTRY_BUDGET_BYTES + result.to_string().len());
        let mut omitted_entries = 0usize;
        for group in &groups {
            for item in group {
                if let Some(entries) = result["entries"].as_array_mut() {
                    entries.push(item.clone());
                }
                if result.to_string().len() > entry_limit {
                    if let Some(entries) = result["entries"].as_array_mut() {
                        entries.pop();
                    }
                    omitted_entries += 1;
                }
            }
        }
        let turn_limit = budget.saturating_sub(RESERVED_BYTES);
        let mut omitted_turns = 0usize;
        for turn in turns {
            if let Some(list) = result["turns"].as_array_mut() {
                list.push(turn);
            }
            if result.to_string().len() > turn_limit {
                if let Some(list) = result["turns"].as_array_mut() {
                    list.pop();
                }
                omitted_turns += 1;
            }
        }
        result["coverage"]["entriesOmittedBySize"] = json!(omitted_entries);
        result["coverage"]["moreMatchingKnowledge"] = json!(knowledge_truncated);
        result["coverage"]["turnsOmittedBySize"] = json!(omitted_turns);
        if !fits_response(&result, budget) {
            result["entries"] = json!([]);
            result["turns"] = json!([]);
            result["coverage"]["note"] = json!("the result budget was too small for any item");
        }
        bounded_json_output(&call, result)
    }

    /// Matching or recent entries, each current entry followed by its history, and whether
    /// more candidates existed than were considered.
    async fn knowledge(
        &self,
        store: &BlackboardStore,
        terms: &[String],
        since_ms: Option<i64>,
        include_history: bool,
    ) -> Result<(Vec<Vec<Value>>, bool), FunctionCallError> {
        let mut hits = Vec::new();
        let mut truncated;
        if let Some(since) = since_ms {
            // The date filter comes first; matching then ranks within the period.
            let (entries, more) = store
                .changed_since(&self.project_id, since, MAX_SINCE_CANDIDATES)
                .await
                .map_err(respond)?;
            let mut ranked = entries
                .into_iter()
                .map(|entry| (match_count(&entry.value.content, terms), entry))
                .filter(|(count, _)| terms.is_empty() || *count > 0)
                .collect::<Vec<_>>();
            // Stable: equal counts keep the newest-first order.
            ranked.sort_by(|left, right| right.0.cmp(&left.0));
            // Matches beyond the hit cap are omitted too, not only unconsidered candidates.
            truncated = more || ranked.len() > MAX_SEARCH_HITS as usize;
            for (_, entry) in ranked.into_iter().take(MAX_SEARCH_HITS as usize) {
                if let Some(hit) = store
                    .get_hit(&self.project_id, &entry.id)
                    .await
                    .map_err(respond)?
                {
                    hits.push(hit);
                }
            }
        } else {
            let result = store
                .query(BlackboardQuery {
                    project_id: self.project_id.clone(),
                    text: Some(terms.join(" ")),
                    within_node: None,
                    root_promotion: None,
                    entry_scope: BlackboardEntryScope::All,
                    max_results: MAX_SEARCH_HITS,
                })
                .await
                .map_err(respond)?;
            truncated = result.truncated;
            hits = result.data;
        }

        // Resolve every match to its current entry, remembering each resolution, so matches
        // of one chain share the group of its current entry whatever order they arrive in,
        // and every entry appears once. A match whose current entry cannot be reached within
        // the lookup budget is left out and reported as more matching knowledge.
        let mut resolved: HashMap<String, String> = HashMap::new();
        let mut heads: HashMap<String, usize> = HashMap::new();
        let mut shown: HashSet<String> = HashSet::new();
        let mut groups: Vec<Vec<Value>> = Vec::new();
        let mut lookups = 0usize;
        let mut history_remaining = MAX_HISTORY_PREDECESSORS;
        for hit in hits {
            if !store
                .entry_source_eligible(&self.project_id, &hit.entry.id)
                .await
                .map_err(respond)?
            {
                continue;
            }
            let mut current = hit;
            let mut path = Vec::new();
            let target = loop {
                let id = current.entry.id.to_string();
                if let Some(target) = resolved.get(&id) {
                    break Some(target.clone());
                }
                let Some(successor_id) = current.entry.superseded_by.clone() else {
                    break Some(id);
                };
                if lookups == MAX_SUCCESSOR_LOOKUPS {
                    break None;
                }
                lookups += 1;
                let Some(successor) = store
                    .get_hit(&self.project_id, &successor_id)
                    .await
                    .map_err(respond)?
                else {
                    break Some(id);
                };
                if !store
                    .entry_source_eligible(&self.project_id, &successor.entry.id)
                    .await
                    .map_err(respond)?
                {
                    break None;
                }
                path.push(std::mem::replace(&mut current, successor));
            };
            let Some(target) = target else {
                truncated = true;
                continue;
            };
            for older in &path {
                resolved.insert(older.entry.id.to_string(), target.clone());
            }
            let index = match heads.get(&target) {
                Some(index) => *index,
                None => {
                    // Not resolved earlier, so the walk ended on the current entry itself.
                    let index = groups.len();
                    heads.insert(target.clone(), index);
                    resolved.insert(target.clone(), target.clone());
                    shown.insert(target.clone());
                    groups.push(vec![
                        entry_item(store, &self.project_id, &current, terms).await,
                    ]);
                    if include_history {
                        let (replaced, more) = store
                            .predecessor_page(
                                &self.project_id,
                                &current.entry.id,
                                history_remaining,
                            )
                            .await
                            .map_err(respond)?;
                        history_remaining -= replaced.len() as u32;
                        truncated |= more;
                        for predecessor in replaced {
                            if store
                                .entry_source_eligible(&self.project_id, &predecessor.id)
                                .await
                                .map_err(respond)?
                                && !shown.contains(&predecessor.id.to_string())
                                && let Some(predecessor) = store
                                    .get_hit(&self.project_id, &predecessor.id)
                                    .await
                                    .map_err(respond)?
                            {
                                shown.insert(predecessor.entry.id.to_string());
                                groups[index].push(
                                    entry_item(store, &self.project_id, &predecessor, terms).await,
                                );
                            }
                        }
                    }
                    index
                }
            };
            if include_history {
                for older in path {
                    if shown.insert(older.entry.id.to_string()) {
                        groups[index]
                            .push(entry_item(store, &self.project_id, &older, terms).await);
                    }
                }
            }
        }
        Ok((groups, truncated))
    }

    /// Earlier turns of the project that match, newest first, and what the scan covered.
    async fn conversation(&self, terms: &[String], since_ms: Option<i64>) -> (Vec<Value>, Value) {
        let mut turns = Vec::new();
        let mut threads_scanned = 0usize;
        let mut threads_not_scanned = 0usize;
        let mut threads_with_more_turns = 0usize;
        let mut turns_scanned = 0usize;
        let mut unreadable_threads = 0usize;
        let page = match self
            .threads
            .list_threads(project_threads_params(&self.project_id, MAX_THREADS, None))
            .await
        {
            Ok(page) => page,
            Err(error) => {
                return (
                    Vec::new(),
                    json!({"conversation": format!("history unavailable: {error}")}),
                );
            }
        };
        let more_threads = page.next_cursor.is_some();
        for thread in page.items.iter().filter(|thread| is_top_level(thread)) {
            if since_ms.is_some_and(|since| thread.updated_at.timestamp_millis() < since) {
                continue;
            }
            if turns_scanned >= MAX_TURNS_SCANNED {
                threads_not_scanned += 1;
                continue;
            }
            threads_scanned += 1;
            let listed = match self
                .threads
                .list_turns(ListTurnsParams {
                    thread_id: thread.thread_id,
                    include_archived: false,
                    cursor: None,
                    page_size: MAX_TURNS_PER_THREAD,
                    sort_direction: SortDirection::Desc,
                    items_view: StoredTurnItemsView::Summary,
                })
                .await
            {
                Ok(listed) => listed,
                Err(_) => {
                    unreadable_threads += 1;
                    continue;
                }
            };
            let mut stopped_early = listed.next_cursor.is_some();
            for turn in &listed.turns {
                if turns_scanned >= MAX_TURNS_SCANNED {
                    stopped_early = true;
                    break;
                }
                turns_scanned += 1;
                let at = turn_time_ms(turn);
                if since_ms.is_some_and(|since| at.is_none_or(|at| at < since)) {
                    continue;
                }
                let Ok(store) = self.services.blackboard().await else {
                    unreadable_threads += 1;
                    continue;
                };
                if !store
                    .source_turn_eligible(&self.project_id, &turn.turn_id)
                    .await
                    .unwrap_or(false)
                {
                    continue;
                }
                let (user, answer) = turn_texts(&turn.items);
                let mut user = user;
                let mut answer = answer;
                if let Some(text) = &user
                    && !store
                        .source_text_eligible(&self.project_id, text)
                        .await
                        .unwrap_or(false)
                {
                    user = None;
                }
                if let Some(text) = &answer
                    && !store
                        .source_text_eligible(&self.project_id, text)
                        .await
                        .unwrap_or(false)
                {
                    answer = None;
                }
                let matches = terms.is_empty()
                    || [user.as_deref(), answer.as_deref()]
                        .into_iter()
                        .flatten()
                        .any(|text| mentions_any(text, terms));
                if !matches {
                    continue;
                }
                turns.push((
                    at.unwrap_or_default(),
                    json!({
                        "threadId": thread.thread_id.to_string(),
                        "turnId": turn.turn_id,
                        "at": at.map(crate::continuity::format_time),
                        "status": turn_status(turn).unwrap_or("completed"),
                        "user": user.as_deref().map(|text| excerpt(text, terms, MAX_USER_BYTES)),
                        "answer": answer.as_deref().map(|text| excerpt(text, terms, MAX_ANSWER_BYTES)),
                    }),
                ));
            }
            if stopped_early {
                threads_with_more_turns += 1;
            }
        }
        turns.sort_by(|left, right| right.0.cmp(&left.0));
        let matched = turns.len();
        let turns = turns
            .into_iter()
            .take(MAX_TURNS_RETURNED)
            .map(|(_, turn)| turn)
            .collect::<Vec<_>>();
        let coverage = json!({
            "threadsScanned": threads_scanned,
            "threadsNotScanned": threads_not_scanned,
            "moreThreadsBeyondTheFirstPage": more_threads,
            "threadsWithUnscannedOlderTurns": threads_with_more_turns,
            "unreadableThreads": unreadable_threads,
            "turnsScanned": turns_scanned,
            "turnsMatched": matched,
            "turnsReturned": turns.len(),
            "turnsLabel": format!("each turn shows the user's first message and the {ANSWER_LABEL}; steering messages and intermediate answers are not included"),
            "noMatchMeans": "nothing matched within this coverage, not that nothing happened",
        });
        (turns, coverage)
    }
}

fn current_status(hit: &BlackboardHit) -> String {
    match (hit.entry.state, &hit.entry.superseded_by) {
        (BlackboardEntryState::Active, _) => "current".to_string(),
        (BlackboardEntryState::Superseded, Some(successor)) => {
            format!(
                "replaced by {successor} on {}",
                day(hit.entry.updated_at_ms)
            )
        }
        (BlackboardEntryState::Superseded, None) => "replaced".to_string(),
        (BlackboardEntryState::Tombstoned, _) => {
            format!("retired on {}", day(hit.entry.updated_at_ms))
        }
    }
}

async fn entry_item(
    store: &BlackboardStore,
    project_id: &str,
    hit: &BlackboardHit,
    terms: &[String],
) -> Value {
    let value = &hit.entry.value;
    let source = match (value.provenance.kind, value.kind) {
        (BlackboardProvenanceKind::User, BlackboardKind::Instruction) => "the user's own words",
        (BlackboardProvenanceKind::User, _) => "user",
        (BlackboardProvenanceKind::Agent, _) => "agent record",
        (BlackboardProvenanceKind::Maintenance, _) => "observed by the host",
        (BlackboardProvenanceKind::Import, _) => "import",
    };
    let mut relations = Vec::new();
    for relation in hit.relations.iter().take(MAX_RELATIONS_PER_ENTRY) {
        let other = if relation.value.from_entry_id == hit.entry.id {
            &relation.value.to_entry_id
        } else {
            &relation.value.from_entry_id
        };
        let counterpart = store
            .get_source_eligible_entry(project_id, other)
            .await
            .ok()
            .flatten();
        relations.push(json!({
            "kind": relation.value.kind,
            "otherEntryId": other.to_string(),
            "otherStatus": counterpart.as_ref().map(|entry| entry.state),
            "otherContent": counterpart
                .as_ref()
                .map(|entry| excerpt(&entry.value.content, terms, MAX_RELATION_EXCERPT_BYTES)),
            "note": relation.value.note.as_deref().map(|note| excerpt(note, terms, MAX_RELATION_EXCERPT_BYTES)),
        }));
    }
    let evidence = value
        .evidence
        .iter()
        .map(|link| {
            json!({
                "evidenceRoute": {
                    "contextMapEntryId": link.context_map_entry_id.to_string(),
                    "sourceFingerprint": link.source_fingerprint.to_string(),
                    "lineRange": link.line_range,
                },
            })
        })
        .collect::<Vec<_>>();
    let proposal = store.proposal_context(project_id, &hit.entry.id).await;
    let (proposal, metadata_status) = match proposal {
        Ok(proposal) => (proposal, "supported"),
        Err(_) => (None, "unsupported; original context withheld"),
    };
    let mut result = json!({
        "entryId": hit.entry.id.to_string(),
        "kind": value.kind,
        "status": current_status(hit),
        "source": source,
        "verification": value.verification,
        "storedEvidenceFreshness": hit.evidence_freshness,
        "recorded": day(hit.entry.created_at_ms),
        "updated": day(hit.entry.updated_at_ms),
        "content": excerpt(&value.content, terms, MAX_ENTRY_CONTENT_BYTES),
        "relations": relations,
        "relationsOmitted": hit.relations.len().saturating_sub(MAX_RELATIONS_PER_ENTRY),
        "evidence": evidence,
    });
    if let Some(proposal) = proposal {
        result["source"] = json!(
            "user-delivered source with model-derived interpretation; not standing rules, endorsement or settled decision"
        );
        result["proposal"] = json!(proposal);
        result["temporal"] = json!(store.temporal_context(project_id, &hit.entry.id).await.ok());
        result["applied"] = json!(false);
    } else if metadata_status != "supported" {
        result["metadataStatus"] = json!(metadata_status);
    }
    result
}

/// How many distinct terms `text` mentions.
fn match_count(text: &str, terms: &[String]) -> usize {
    terms
        .iter()
        .filter(|term| mentions_any(text, std::slice::from_ref(term)))
        .count()
}

/// Content words of a question: lowercase, at least two characters (symbols such as `yr`
/// are topics), no stopwords.
pub(super) fn question_terms(question: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    question
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .map(str::to_lowercase)
        .filter(|term| term.chars().count() >= 2 && !STOPWORDS.contains(&term.as_str()))
        .filter(|term| seen.insert(term.clone()))
        .take(MAX_TERMS)
        .collect()
}

/// Whether `text` contains any term as a word prefix (case-insensitive).
pub(super) fn mentions_any(text: &str, terms: &[String]) -> bool {
    let words = text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    terms
        .iter()
        .any(|term| words.iter().any(|word| word.starts_with(term.as_str())))
}

/// At most `maximum` bytes of `text`, centred on the first term it mentions, with cuts
/// marked by "...".
pub(super) fn excerpt(text: &str, terms: &[String], maximum: usize) -> String {
    let text = text.trim();
    if text.len() <= maximum {
        return text.to_string();
    }
    // Lowercase while remembering where each lowered byte came from in the original.
    let mut lower = String::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len());
    for (offset, character) in text.char_indices() {
        for lowered in character.to_lowercase() {
            let before = lower.len();
            lower.push(lowered);
            origin.extend(std::iter::repeat_n(offset, lower.len() - before));
        }
    }
    let anchor = terms
        .iter()
        .filter_map(|term| lower.find(term.as_str()))
        .min()
        .and_then(|position| origin.get(position).copied())
        .unwrap_or(0);
    let budget = maximum.saturating_sub(6);
    let mut start = anchor.saturating_sub(budget / 3);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + budget).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}{}{}",
        if start > 0 { "..." } else { "" },
        &text[start..end],
        if end < text.len() { "..." } else { "" }
    )
}

/// Midnight UTC of a `YYYY-MM-DD` day, in Unix milliseconds.
pub(super) fn parse_utc_day(value: &str) -> Result<i64, String> {
    let parts = value
        .split('-')
        .map(str::parse::<i64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("since must be YYYY-MM-DD, got {value}"))?;
    let [year, month, day] = parts[..] else {
        return Err(format!("since must be YYYY-MM-DD, got {value}"));
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if !(1970..=9999).contains(&year) || !(1..=days_in_month).contains(&day) {
        return Err(format!(
            "since must be a real date YYYY-MM-DD from 1970, got {value}"
        ));
    }
    // Days from civil (Howard Hinnant), proleptic Gregorian.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Ok(days * 86_400_000)
}

fn day(ms: i64) -> String {
    let time = crate::continuity::format_time(ms);
    time.split(' ').next().unwrap_or(&time).to_string()
}

impl<'call> ToolExecutor<ToolCall<'call>> for MemoryReadTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::DirectModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Recall missing earlier work once: matching knowledge, status/history, sources/dates and turns. Evidence needs no confirmation read. Proposals are source recall, not settled decisions. Original-source search and exact-source recall are unavailable.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "since": {"type": "string", "description": "YYYY-MM-DD (UTC)"},
                    "includeHistory": {"type": "boolean"}
                },
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static memory_read schema: {error}")),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

#[cfg(test)]
#[path = "memory_read_tests.rs"]
mod tests;
