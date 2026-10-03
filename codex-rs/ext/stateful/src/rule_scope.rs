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

use crate::quotation::Quotations;
use crate::request_scope::RequestScope;
use crate::rule_capture::user_message_source;
use crate::user_rules::Fence;
use crate::user_rules::INVESTIGATION_PHRASES;
use crate::user_rules::has_phrase;
use crate::user_rules::normalize;

/// Root entries a packet or completion considers.
pub(crate) const ROOT_ENTRIES: u32 = 256;
/// Most entries read to fill `ROOT_ENTRIES` with applicable ones.
const MAX_ROOT_CANDIDATES: u32 = 1024;
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
    /// Open investigations this thread is not part of, by title.
    pub(crate) other_open_titles: Vec<String>,
    /// Older rules naming an investigation that none was recorded for.
    pub(crate) unscoped_legacy: u64,
    /// Scopes could not be read, so every rule naming an investigation is held back.
    pub(crate) unavailable: bool,
}

/// The project's root projection with only the rules that apply in `thread_id`, filled to
/// `ROOT_ENTRIES` from further candidates when inapplicable rules took places, and the view
/// that explains what was left out.
pub(crate) async fn applicable_projection(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
) -> Result<(RootBlackboardProjection, ScopeView), BlackboardStoreError> {
    let mut max_entries = ROOT_ENTRIES;
    loop {
        let mut projection = store
            .root_projection(RootBlackboardQuery {
                project_id: project_id.to_string(),
                max_entries,
            })
            .await?;
        let view = ScopeView::load(store, &projection, thread_id).await;
        let removed = view.retain_applicable(&mut projection);
        let truncated = projection.omitted_entries > 0;
        let wanted = ROOT_ENTRIES
            .saturating_add(u32::try_from(removed).unwrap_or(u32::MAX))
            .min(MAX_ROOT_CANDIDATES);
        if removed == 0 || !truncated || wanted <= max_entries {
            let limit = usize::try_from(ROOT_ENTRIES).unwrap_or(usize::MAX);
            if projection.data.len() > limit {
                let extra = projection.data.len() - limit;
                projection.data.truncate(limit);
                projection.omitted_entries = projection
                    .omitted_entries
                    .saturating_add(u64::try_from(extra).unwrap_or(u64::MAX));
            }
            return Ok((projection, view));
        }
        max_entries = wanted;
    }
}

impl ScopeView {
    /// Computes the view for `projection` in `thread_id`. Rules without a scope apply
    /// everywhere, except older rules that name an investigation without a recorded one,
    /// which wait for the user.
    pub(crate) async fn load(
        store: &BlackboardStore,
        projection: &RootBlackboardProjection,
        thread_id: &str,
    ) -> Self {
        let project_id = projection.project_id.as_str();
        let rules = projection
            .data
            .iter()
            .filter(|hit| hit.entry.value.kind == BlackboardKind::Instruction)
            .collect::<Vec<_>>();
        let rule_ids = rules
            .iter()
            .map(|hit| hit.entry.id.clone())
            .collect::<Vec<BlackboardEntryId>>();
        let names_investigation = |content: &str| {
            let normalized = normalize(content);
            has_phrase(&normalized, INVESTIGATION_PHRASES) || normalized.contains("investigation")
        };
        let (contexts, bound, scopes) = match (
            store.knowledge_contexts(project_id, &rule_ids).await,
            store.thread_scope(project_id, thread_id).await,
            store.scopes(project_id, /*state*/ None).await,
        ) {
            (Ok(contexts), Ok(bound), Ok(scopes)) => (contexts, bound, scopes),
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                tracing::warn!(%project_id, %error, "failed to load rule scopes");
                // Unknown applicability is never widened to everywhere.
                return Self {
                    inapplicable: rules
                        .iter()
                        .filter(|hit| names_investigation(&hit.entry.value.content))
                        .map(|hit| hit.entry.id.to_string())
                        .collect(),
                    unavailable: true,
                    ..Self::default()
                };
            }
        };
        let bound_open = bound.filter(|scope| scope.state == ScopeState::Open);
        let mut inapplicable = HashSet::new();
        let mut unscoped_legacy = 0;
        for hit in &rules {
            let id = hit.entry.id.as_str();
            match contexts.get(id) {
                Some(context) => {
                    if let Some(scope_id) = &context.scope_id
                        && bound_open
                            .as_ref()
                            .is_none_or(|bound| &bound.scope_id != scope_id)
                    {
                        inapplicable.insert(id.to_string());
                    }
                }
                None if names_investigation(&hit.entry.value.content) => {
                    unscoped_legacy += 1;
                    inapplicable.insert(id.to_string());
                }
                None => {}
            }
        }
        let other_open_titles = scopes
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
        Self {
            inapplicable,
            bound_title: bound_open.map(|scope| shown_title(&scope.title)),
            other_open_titles,
            unscoped_legacy,
            unavailable: false,
        }
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
        if self.unavailable {
            return (held_back > 0).then(|| format!(
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
        let other = held_back.saturating_sub(self.unscoped_legacy);
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
    END_PHRASES
        .iter()
        .chain(AGREEMENT_PHRASES.iter().filter(|_| agreement_condition))
        .any(|phrase| {
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
                let questioning = lower[sentence_end..].starts_with('?');
                !questioning
                    && !before
                        .split(' ')
                        .any(|word| NOT_AFFIRMATIVE.contains(&word))
            })
        })
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
