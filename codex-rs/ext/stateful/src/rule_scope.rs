//! Which investigation-scoped rules apply in a thread, and the user's acts that bind a thread
//! to an investigation or end one. A rule limited to an investigation applies only in threads
//! bound to that investigation while it is open; it never constrains unrelated work, and it
//! ends only when the user releases it.

use std::collections::HashSet;

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeScope;
use codex_project_intelligence::RootBlackboardProjection;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::ScopeState;
use codex_project_intelligence::ThreadScopes;

use crate::quotation::Quotations;
use crate::request_scope::RequestScope;
use crate::rule_capture::user_message_source;
use crate::rule_units::names_limited_scope;
use crate::user_rules::Fence;
use crate::user_rules::normalize;

/// Root entries a packet or completion considers.
pub(crate) const ROOT_ENTRIES: u32 = 256;
/// Longest scope title shown in the packet.
const MAX_SHOWN_TITLE_CHARS: usize = 120;
/// Investigations named in one packet note.
const MAX_SHOWN_SCOPES: usize = 3;

/// Why some rules of a projection do not apply in a thread.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeView {
    /// Rules that do not apply here, by entry ID.
    pub(crate) inapplicable: HashSet<String>,
    /// The open investigation this thread continues, if any.
    pub(crate) bound_title: Option<String>,
    /// The investigation this thread continued until the user ended it.
    pub(crate) ended_title: Option<String>,
    /// Open investigations this thread is not part of, by title.
    pub(crate) other_open_titles: Vec<String>,
    /// Rules storage left out because they belong to an investigation this thread does not
    /// continue.
    pub(crate) scoped_elsewhere: u64,
    /// Older rules naming an investigation that none was recorded for.
    pub(crate) unscoped_legacy: u64,
    /// Rule meanings could not be read, so every rule naming an investigation is held back.
    pub(crate) unavailable: bool,
}

/// The project's root projection with only the rules that apply in `thread_id`, and the view
/// that explains what was left out. Storage leaves out rules of other investigations before
/// its cap, in the same snapshot as the thread's binding; older rules that name an
/// investigation without a recorded one are then held back here. A projection whose revision
/// moved while it was explained is read again, so aliases never mix two states.
pub(crate) async fn applicable_projection(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
) -> Result<(RootBlackboardProjection, ScopeView), BlackboardStoreError> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        let (mut projection, scopes) = store
            .root_projection_for_thread(
                RootBlackboardQuery {
                    project_id: project_id.to_string(),
                    max_entries: ROOT_ENTRIES,
                },
                thread_id,
            )
            .await?;
        let view = ScopeView::load(store, &projection, scopes).await;
        view.retain_applicable(&mut projection);
        if attempts >= MAX_SNAPSHOT_ATTEMPTS
            || store.project_revision(project_id).await? == projection.revision
        {
            return Ok((projection, view));
        }
    }
}

/// Reads of a projection before one that did not move is used anyway.
const MAX_SNAPSHOT_ATTEMPTS: u32 = 3;

impl ScopeView {
    /// Computes the view for `projection` (already limited to the thread's investigation by
    /// storage) from `scopes`, read in the same snapshot. Rules without a recorded meaning
    /// that name a limited piece of work wait for the user; when meanings cannot be read,
    /// every such rule is held back.
    pub(crate) async fn load(
        store: &BlackboardStore,
        projection: &RootBlackboardProjection,
        scopes: ThreadScopes,
    ) -> Self {
        let project_id = projection.project_id.as_str();
        let rules = projection
            .data
            .iter()
            .filter(|hit| hit.entry.value.kind == BlackboardKind::Instruction)
            .filter(|hit| names_limited_scope(&normalize(&hit.entry.value.content)))
            .collect::<Vec<_>>();
        let rule_ids = rules
            .iter()
            .map(|hit| hit.entry.id.clone())
            .collect::<Vec<BlackboardEntryId>>();
        let ended_title = scopes
            .bound
            .as_ref()
            .filter(|scope| scope.state == ScopeState::Ended)
            .map(|scope| shown_title(&scope.title));
        let bound_open = scopes.bound.filter(|scope| scope.state == ScopeState::Open);
        let other_open_titles = scopes
            .scopes
            .iter()
            .filter(|scope| scope.state == ScopeState::Open)
            .filter(|scope| {
                bound_open
                    .as_ref()
                    .is_none_or(|bound| bound.scope_id != scope.scope_id)
            })
            .take(MAX_SHOWN_SCOPES)
            .map(|scope| shown_title(&scope.title))
            .collect();
        let mut view = Self {
            bound_title: bound_open.map(|scope| shown_title(&scope.title)),
            ended_title,
            other_open_titles,
            scoped_elsewhere: scopes.scoped_elsewhere,
            ..Self::default()
        };
        let contexts = match store.knowledge_contexts(project_id, &rule_ids).await {
            Ok(contexts) => contexts,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to load rule meanings");
                // Unknown applicability is never widened to everywhere.
                view.inapplicable = rule_ids.iter().map(ToString::to_string).collect();
                view.unavailable = true;
                return view;
            }
        };
        for id in &rule_ids {
            if !contexts.contains_key(id.as_str()) {
                view.unscoped_legacy += 1;
                view.inapplicable.insert(id.to_string());
            }
        }
        view
    }

    /// Removes the rules that do not apply here from `projection` and returns how many.
    /// Every consumer of root aliases (the packet, its deltas, completion) applies this to
    /// the same projection before aliases are assigned.
    pub(crate) fn retain_applicable(&self, projection: &mut RootBlackboardProjection) -> u64 {
        let before = projection.data.len();
        projection
            .data
            .retain(|hit| !self.inapplicable.contains(hit.entry.id.as_str()));
        u64::try_from(before - projection.data.len()).unwrap_or(u64::MAX)
    }

    /// The packet's note about investigations, if any applies.
    pub(crate) fn note(&self) -> Option<String> {
        let quoted = |titles: &[String]| {
            titles
                .iter()
                .map(|title| serde_json::Value::String(title.clone()).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let held_back = u64::try_from(self.inapplicable.len()).unwrap_or(u64::MAX);
        if self.unavailable && held_back > 0 {
            return Some(format!(
                "- Investigation scopes could not be read: {held_back} rules that name an investigation are not applied now. If the work depends on them, ask the user."
            ));
        }
        let mut lines = Vec::new();
        if let Some(title) = &self.bound_title {
            lines.push(format!(
                "- This thread continues the investigation {}; its rules above apply until the user ends it.",
                serde_json::Value::String(title.clone())
            ));
        }
        if let Some(title) = &self.ended_title {
            lines.push(format!(
                "- The investigation {} ended; its rules no longer apply, even where shown earlier.",
                serde_json::Value::String(title.clone())
            ));
        }
        let other = self.scoped_elsewhere;
        if other > 0 {
            lines.push(format!(
                "- {other} rules belong to an investigation this thread is not part of{}; they do not apply here. If this work continues one, ask the user before following its rules.",
                if self.other_open_titles.is_empty() {
                    String::new()
                } else {
                    format!(" (open: {})", quoted(&self.other_open_titles))
                },
            ));
        }
        if self.unscoped_legacy > 0 {
            lines.push(format!(
                "- {} older rules name an investigation but none was recorded for them; they are not applied. Ask the user whether they still apply.",
                self.unscoped_legacy
            ));
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

fn shown_title(title: &str) -> String {
    match title.char_indices().nth(MAX_SHOWN_TITLE_CHARS) {
        Some((end, _)) => format!("{}...", &title[..end]),
        None => title.to_string(),
    }
}

/// Words with which the user explicitly ends the investigation a thread is bound to.
const END_PHRASES: &[&str] = &[
    "the investigation is over",
    "the investigation is done",
    "the investigation is closed",
    "this investigation is over",
    "this investigation is done",
    "end the investigation",
    "end this investigation",
    "close the investigation",
    "close this investigation",
    "the ground rules no longer apply",
    "the investigation rules no longer apply",
];

/// Words that meet an "until we agree ..." end condition.
const AGREEMENT_PHRASES: &[&str] = &[
    "we have agreed on",
    "we've agreed on",
    "we agree on",
    "i agree with the root cause",
    "i agree on the root cause",
    "agreed on the root cause",
];

/// Words that make a release phrase conditional, hypothetical or negated.
const NOT_AFFIRMATIVE: &[&str] = &[
    "if",
    "when",
    "once",
    "until",
    "till",
    "unless",
    "after",
    "before",
    "whether",
    "not",
    "don't",
    "dont",
    "never",
    "no",
    "can't",
    "cannot",
    "shouldn't",
    "won't",
    "tomorrow",
    "later",
    "maybe",
    "might",
];

/// Words in a request that refer back to an investigation.
const INVESTIGATION_REFERENCES: &[&str] = &[
    "investigation",
    "investigating",
    "ruled out",
    "rule out",
    "root cause",
    "hypothesis",
    "hypotheses",
    "ground rules",
];

/// At turn start: the user's own affirmative words may end the investigation this thread is
/// bound to; otherwise a request that refers back to an investigation joins the project's
/// single open one. With several open, nothing is bound and the packet asks.
pub(crate) async fn observe_turn_start(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    text: &str,
    request: RequestScope,
) {
    let result = async {
        let bound = store.thread_scope(project_id, thread_id).await?;
        if let Some(scope) = bound
            .as_ref()
            .filter(|scope| scope.state == ScopeState::Open)
        {
            // The message that opened an investigation states its end condition; it never
            // ends it.
            let source = user_message_source(thread_id, turn_id);
            if scope.opened_source != source && releases(text, scope.end_condition.as_deref()) {
                store
                    .end_scope(
                        project_id,
                        &scope.scope_id,
                        &source,
                        &release_change(scope, thread_id, turn_id),
                    )
                    .await?;
            }
            return Ok(());
        }
        if request != RequestScope::Continuity || !refers_to_investigation(text) {
            return Ok(());
        }
        let open = store.scopes(project_id, Some(ScopeState::Open)).await?;
        if let [only] = open.as_slice() {
            store
                .bind_thread_scope(project_id, thread_id, &only.scope_id)
                .await?;
        }
        Ok::<(), BlackboardStoreError>(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%project_id, %error, "failed to update investigation scopes");
    }
}

/// Whether a request names an investigation it continues.
pub(crate) fn refers_to_investigation(text: &str) -> bool {
    let lower = text.to_lowercase();
    INVESTIGATION_REFERENCES
        .iter()
        .any(|reference| lower.contains(reference))
}

/// Whether the user's own words affirmatively end the investigation: an explicit end phrase,
/// or, for an "until we agree ..." condition, a statement that the agreement was reached.
/// Quoted, relayed, blockquoted, fenced, conditional, negated and questioning mentions do not
/// count.
pub(crate) fn releases(text: &str, end_condition: Option<&str>) -> bool {
    let quotations = Quotations::new(text);
    let lower = text.to_ascii_lowercase();
    let agreement_condition =
        end_condition.is_some_and(|condition| condition.to_ascii_lowercase().contains("agree"));
    // Lines in a fence or blockquote are someone's text, not the user's statement now.
    let mut fence = Fence::default();
    let mut excluded = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if fence.skips(trimmed) || trimmed.starts_with('>') {
            excluded.push(offset..offset + line.len());
        }
        offset += line.len();
    }
    let subject = end_condition.map(agreement_subject).unwrap_or_default();
    END_PHRASES
        .iter()
        .map(|phrase| (phrase, false))
        .chain(
            AGREEMENT_PHRASES
                .iter()
                .filter(|_| agreement_condition)
                .map(|phrase| (phrase, true)),
        )
        .any(|(phrase, agreement)| {
            lower.match_indices(phrase).any(|(start, matched)| {
                let end = start + matched.len();
                if excluded.iter().any(|range| range.contains(&start))
                    || quotations.relays(start, end)
                    || quotations.in_reported_sentence(start)
                {
                    return false;
                }
                // The sentence around the phrase: from the previous boundary to the next.
                let sentence_start = lower[..start]
                    .rfind(['.', '!', '?', '\n', ';'])
                    .map_or(0, |index| index + 1);
                let sentence_end = lower[end..]
                    .find(['.', '!', '?', '\n', ';'])
                    .map_or(lower.len(), |index| end + index);
                let before = normalize(&lower[sentence_start..start]);
                let after = normalize(&lower[end..sentence_end]);
                let questioning = lower[sentence_end..].starts_with('?');
                // The whole act is affirmative: nothing before or after the phrase makes it
                // conditional ("... if the test passes"), and an agreement is about what the
                // condition names, not anything else ("we agree on lunch").
                let after_words = after.split(' ').collect::<Vec<_>>();
                let negated = [&lower[sentence_start..start], &lower[end..sentence_end]]
                    .iter()
                    .any(|part| part.contains("n't") || part.contains("n\u{2019}t"));
                !questioning
                    && !negated
                    && !before
                        .split(' ')
                        .chain(after_words.iter().copied())
                        .any(|word| NOT_AFFIRMATIVE.contains(&word))
                    && (!agreement
                        || subject.iter().all(|word| {
                            after_words.contains(&word.as_str())
                                || phrase.split(' ').any(|part| part == word)
                        }))
            })
        })
}

/// Words that carry no subject in an agreement condition.
const AGREEMENT_FILLER: &[&str] = &[
    "the", "a", "an", "on", "about", "that", "to", "what", "which", "is", "it", "we", "i", "you",
    "both", "all", "of", "have", "has", "agreed", "agree", "until", "till", "upon", "with",
];

/// What an "until we agree on the root cause" condition is about ("root", "cause"); empty
/// when it names nothing ("until we agree").
fn agreement_subject(condition: &str) -> Vec<String> {
    let lower = normalize(&condition.to_ascii_lowercase());
    let Some(index) = lower.find("agree") else {
        return Vec::new();
    };
    lower[index..]
        .split(' ')
        .map(|word| word.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .filter(|word| !word.is_empty() && !AGREEMENT_FILLER.contains(word))
        .map(str::to_string)
        .collect()
}

fn release_change(scope: &KnowledgeScope, thread_id: &str, turn_id: &str) -> ChangeRecord {
    ChangeRecord {
        operation: ChangeOperation::ScopeEnded,
        origin: ChangeOrigin::HostCapture,
        category: KnowledgeCategory::Rule,
        action_id: None,
        thread_id: Some(thread_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        group_id: None,
        preview: scope.title.clone(),
    }
}

#[cfg(test)]
#[path = "rule_scope_tests.rs"]
mod tests;
