use std::collections::HashMap;

use codex_extension_api::ContentItemKind;
use codex_extension_api::ContextualUserFragment;
use codex_project_intelligence::BlackboardEntryScope;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardQuery;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_protocol::user_input::UserInput;

use crate::services::ProjectIntelligenceServices;

const MAX_QUERY_BYTES: usize = 1024;
const MAX_LEXICAL_CANDIDATES: usize = 3;
const MAX_CANDIDATES: usize = 4;
const MAX_CONTENT_BYTES: usize = 96;
const MAX_FRAGMENT_BYTES: usize = 900;
const OPEN_MARKER: &str = "<stateful_reuse_candidates>";
const CLOSE_MARKER: &str = "</stateful_reuse_candidates>";

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReuseCandidate {
    alias: String,
    content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReuseCandidateSlate {
    project_id: String,
    root_revision: u64,
    candidates: Vec<ReuseCandidate>,
}

impl ContextualUserFragment for ReuseCandidateSlate {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("stateful.reuse_candidates".to_string())
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (OPEN_MARKER, CLOSE_MARKER)
    }

    fn matches_text(text: &str) -> bool {
        let text = text.trim();
        text.starts_with(OPEN_MARKER) && text.ends_with(CLOSE_MARKER)
    }

    fn body(&self) -> String {
        let mut body = format!(
            "Project ID: {}\nRoot revision: {}\nAttention cue only, not evidence. Before concluding, compare or reject these possibly relevant root findings:\n",
            self.project_id, self.root_revision
        );
        let suffix = "Use only with the same current E alias in root; otherwise query the quoted content. Reread source only if its cited range is insufficient.";
        for candidate in &self.candidates {
            let line = format!("- {}: {}\n", candidate.alias, candidate.content);
            if body.len() + line.len() + suffix.len() > MAX_FRAGMENT_BYTES {
                break;
            }
            body.push_str(&line);
        }
        body.push_str(suffix);
        debug_assert!(body.len() <= MAX_FRAGMENT_BYTES);
        body
    }
}

pub(super) async fn reuse_candidate_slate(
    project_id: &str,
    services: &ProjectIntelligenceServices,
    user_input: &[UserInput],
) -> Result<Option<ReuseCandidateSlate>, BlackboardStoreError> {
    let Some(question) = question_text(user_input) else {
        return Ok(None);
    };
    let store = services.blackboard().await?;
    let projection = store
        .root_projection(RootBlackboardQuery {
            project_id: project_id.to_string(),
            max_entries: 256,
        })
        .await?;
    if projection.data.is_empty() {
        return Ok(None);
    }
    let search = store
        .query(BlackboardQuery {
            project_id: project_id.to_string(),
            text: Some(question),
            within_node: None,
            root_promotion: Some(RootPromotion::Promoted),
            entry_scope: BlackboardEntryScope::Active,
            max_results: 12,
        })
        .await?;
    let aliases = projection
        .data
        .iter()
        .enumerate()
        .map(|(index, hit)| (hit.entry.id.clone(), format!("E{}", index + 1)))
        .collect::<HashMap<_, _>>();
    let mut candidates = Vec::with_capacity(MAX_CANDIDATES);
    for hit in search.data.iter().take(MAX_LEXICAL_CANDIDATES) {
        let Some(alias) = aliases.get(&hit.entry.id) else {
            continue;
        };
        push_candidate(&mut candidates, alias, &hit.entry.value.content);
    }
    if let Some(hit) = projection.data.iter().find(|hit| {
        matches!(
            hit.entry.value.kind,
            BlackboardKind::Contradiction | BlackboardKind::Failure
        ) && matches!(
            hit.entry.value.importance,
            BlackboardImportance::Critical | BlackboardImportance::High
        )
    }) && let Some(alias) = aliases.get(&hit.entry.id)
    {
        push_candidate(&mut candidates, alias, &hit.entry.value.content);
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    Ok(Some(ReuseCandidateSlate {
        project_id: project_id.to_string(),
        root_revision: projection.revision,
        candidates,
    }))
}

fn question_text(user_input: &[UserInput]) -> Option<String> {
    let mut output = String::new();
    for item in user_input {
        if let UserInput::Text { text, .. } = item {
            if !output.is_empty() {
                if output.len() == MAX_QUERY_BYTES {
                    break;
                }
                output.push('\n');
            }
            let remaining = MAX_QUERY_BYTES.saturating_sub(output.len());
            let end = floor_char_boundary(text, remaining.min(text.len()));
            output.push_str(&text[..end]);
            if output.len() == MAX_QUERY_BYTES {
                break;
            }
        }
    }
    let output = output.trim();
    (!output.is_empty()).then(|| output.to_string())
}

fn push_candidate(candidates: &mut Vec<ReuseCandidate>, alias: &str, content: &str) {
    if candidates.len() >= MAX_CANDIDATES
        || candidates.iter().any(|candidate| candidate.alias == alias)
    {
        return;
    }
    let content = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let content_len = content.len();
    let end = floor_char_boundary(&content, MAX_CONTENT_BYTES.min(content.len()));
    let mut content = content[..end].to_string();
    if end < content_len {
        content.push('…');
    }
    candidates.push(ReuseCandidate {
        alias: alias.to_string(),
        content,
    });
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

#[cfg(test)]
#[path = "reuse_candidates_tests.rs"]
mod tests;
