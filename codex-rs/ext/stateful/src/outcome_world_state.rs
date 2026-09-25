use std::collections::HashSet;

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulRunOutcome;
use serde_json::Value;
use serde_json::json;

const WORLD_STATE_ID: &str = "stateful_project_outcomes";
const START_MARKER: &str = "<stateful_project_outcomes>";
const END_MARKER: &str = "</stateful_project_outcomes>";
const UPDATE_START_MARKER: &str = "<stateful_project_outcomes_update>";
const UPDATE_END_MARKER: &str = "</stateful_project_outcomes_update>";
const MAX_BODY_BYTES: usize = 10 * 1024;
const MAX_GOAL_BYTES: usize = 320;
const MAX_RESULT_BYTES: usize = 1_024;
const MAX_PACKET_ITEM_BYTES: usize = 320;
const MAX_PACKET_ITEMS: usize = 6;
const TRUNCATION_MARKER: &str =
    "\n[Older or longer outcome detail omitted by the bounded continuity view.]";

pub(super) enum ProjectOutcomesStatus {
    Available {
        project_id: String,
        outcomes: Vec<StatefulRunOutcome>,
        has_more: bool,
    },
    Unavailable {
        project_id: String,
    },
}

impl ProjectOutcomesStatus {
    fn project_id(&self) -> &str {
        match self {
            Self::Available { project_id, .. } | Self::Unavailable { project_id } => project_id,
        }
    }

    fn snapshot(&self) -> Value {
        match self {
            Self::Available {
                project_id,
                outcomes,
                has_more,
            } => json!({
                "projectId": project_id,
                "outcomes": outcomes.iter().map(outcome_key).collect::<Vec<_>>(),
                "hasMore": has_more,
            }),
            Self::Unavailable { project_id } => json!({
                "projectId": project_id,
                "unavailable": true,
            }),
        }
    }

    fn render(&self, heading: &str, outcomes: Option<&[StatefulRunOutcome]>) -> String {
        let mut output = String::with_capacity(MAX_BODY_BYTES);
        line(&mut output, heading);
        line(&mut output, &format!("Project ID: {}", self.project_id()));
        line(
            &mut output,
            "These are bounded durable conclusions from prior completed work. Use them to continue reasoning instead of rebuilding unchanged work, but use current blackboard freshness and exact source routes when a claim needs present-source verification.",
        );
        match self {
            Self::Available {
                outcomes: all,
                has_more,
                ..
            } => {
                for outcome in outcomes.unwrap_or(all) {
                    render_outcome(&mut output, outcome);
                }
                if outcomes.is_none() && *has_more {
                    line(
                        &mut output,
                        "- Older completed outcomes are omitted by the bounded recent-history view.",
                    );
                }
            }
            Self::Unavailable { .. } => line(
                &mut output,
                "Recent completed outcomes are temporarily unavailable. Do not claim project-history continuity from this section.",
            ),
        }
        output
    }
}

pub(super) fn project_outcomes_world_state_section(
    status: ProjectOutcomesStatus,
) -> WorldStateSectionContribution {
    let snapshot = status.snapshot();
    let full_body = status.render("Recent completed project outcomes (newest first):", None);
    let project_id = status.project_id().to_string();
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot.clone(), move |previous| {
        match previous {
            PreviousWorldStateSection::Known(previous) if previous == &snapshot => None,
            PreviousWorldStateSection::Known(previous) => {
                let previous_keys = previous
                    .get("outcomes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<HashSet<_>>();
                let ProjectOutcomesStatus::Available { outcomes, .. } = &status else {
                    return Some(RenderedWorldStateFragment::new(
                        "developer",
                        (START_MARKER, END_MARKER),
                        full_body.clone(),
                    ));
                };
                let new_outcomes = outcomes
                    .iter()
                    .filter(|outcome| !previous_keys.contains(&outcome_key(outcome)))
                    .cloned()
                    .collect::<Vec<_>>();
                if new_outcomes.is_empty() {
                    return Some(RenderedWorldStateFragment::new(
                        "developer",
                        (START_MARKER, END_MARKER),
                        full_body.clone(),
                    ));
                }
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (UPDATE_START_MARKER, UPDATE_END_MARKER),
                    status.render(
                        "New completed project outcomes since the prior Stateful snapshot:",
                        Some(&new_outcomes),
                    ),
                ))
            }
            PreviousWorldStateSection::Absent | PreviousWorldStateSection::Unknown => {
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (START_MARKER, END_MARKER),
                    full_body.clone(),
                ))
            }
        }
    })
    .with_legacy_matcher({
        let project_id = project_id.clone();
        move |role, text| matches_fragment(role, text, &project_id)
    })
    .with_retained_fragment_matcher(move |role, text| matches_fragment(role, text, &project_id))
}

fn render_outcome(output: &mut String, outcome: &StatefulRunOutcome) {
    let result = outcome
        .run
        .result
        .as_deref()
        .unwrap_or("No result recorded.");
    line(
        output,
        &format!(
            "- Goal: {}",
            bounded_single_line(&outcome.run.value.goal, MAX_GOAL_BYTES)
        ),
    );
    line(
        output,
        &format!(
            "  Result: {}",
            bounded_single_line(result, MAX_RESULT_BYTES)
        ),
    );
    if let Some(obligation) = &outcome.final_obligation {
        render_packet(output, &obligation.value.packet);
    }
}

fn render_packet(output: &mut String, packet: &ObligationPacket) {
    let categories = [
        ("Learned", &packet.learning),
        ("Implication", &packet.implication),
        ("Strategy", &packet.strategy),
        ("Changed", &packet.changed),
        ("Uncertainty", &packet.uncertainty),
        ("Blocker", &packet.blockers),
    ];
    let total_items = categories
        .iter()
        .map(|(_, items)| items.len())
        .sum::<usize>();
    let mut rendered = 0;
    let mut item_index = 0;
    while rendered < MAX_PACKET_ITEMS {
        let mut found_item = false;
        for (label, items) in &categories {
            if let Some(item) = items.get(item_index) {
                line(
                    output,
                    &format!(
                        "  {label}: {}",
                        bounded_single_line(item, MAX_PACKET_ITEM_BYTES)
                    ),
                );
                rendered += 1;
                found_item = true;
                if rendered == MAX_PACKET_ITEMS {
                    break;
                }
            }
        }
        if !found_item {
            break;
        }
        item_index += 1;
    }
    if rendered < total_items {
        line(
            output,
            "  Prior final obligation shortened by the bounded view.",
        );
    }
}

fn outcome_key(outcome: &StatefulRunOutcome) -> String {
    format!(
        "{}:{}:{}",
        outcome.run.id,
        outcome.run.revision,
        outcome
            .final_obligation
            .as_ref()
            .map_or(0, |obligation| obligation.revision)
    )
}

fn bounded_single_line(value: &str, maximum: usize) -> String {
    let value = value
        .chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            _ => character,
        })
        .collect::<String>();
    if value.len() <= maximum {
        return value;
    }
    let marker = "…";
    let mut boundary = maximum.saturating_sub(marker.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{marker}", &value[..boundary])
}

fn line(output: &mut String, value: &str) {
    if output.ends_with(TRUNCATION_MARKER) {
        return;
    }
    let prefix = usize::from(!output.is_empty());
    let content_limit = MAX_BODY_BYTES.saturating_sub(TRUNCATION_MARKER.len());
    let available = content_limit.saturating_sub(output.len() + prefix);
    if available == 0 {
        output.push_str(TRUNCATION_MARKER);
        return;
    }
    if value.len() <= available {
        if prefix == 1 {
            output.push('\n');
        }
        output.push_str(value);
        return;
    }
    if prefix == 1 {
        output.push('\n');
    }
    let mut boundary = available.min(value.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    output.push_str(&value[..boundary]);
    output.push_str(TRUNCATION_MARKER);
}

fn matches_fragment(role: &str, text: &str, project_id: &str) -> bool {
    role == "developer"
        && text.trim_start().starts_with(START_MARKER)
        && text.contains(&format!("Project ID: {project_id}"))
        && text.trim_end().ends_with(END_MARKER)
}

#[cfg(test)]
#[path = "outcome_world_state_tests.rs"]
mod tests;
