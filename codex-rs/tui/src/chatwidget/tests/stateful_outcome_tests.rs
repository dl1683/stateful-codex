//! A trailing Stateful outcome block is never rendered: not while it streams, not in the
//! completed answer, and not on replay.

use super::*;
use pretty_assertions::assert_eq;

const ANSWER: &str = "A leap year has 366 days.";
const MESSAGE: &str = "A leap year has 366 days.\n\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

fn rendered(lines: &[ratatui::text::Line<'static>]) -> String {
    lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn a_streamed_outcome_block_is_never_shown() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    handle_turn_started(&mut chat, "turn-1");
    while rx.try_recv().is_ok() {}

    // Split inside the opener, inside a body line and inside the closing line.
    let mut frames = Vec::new();
    let mut shown = Vec::new();
    for delta in [
        "A leap year has 366 days.\n\n[state",
        "ful-outcome]\ndisposition: an",
        "swer\nopen-issues: none\n[/stateful-out",
        "come]",
    ] {
        handle_agent_message_delta(&mut chat, delta);
        chat.run_commit_tick();
        frames.push(
            chat.active_cell_transcript_lines(/*width*/ 80)
                .map(|lines| rendered(&lines))
                .unwrap_or_default(),
        );
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::InsertHistoryCell(cell) => {
                    shown.push(rendered(&cell.display_lines(/*width*/ 80)));
                }
                AppEvent::ConsolidateAgentMessage { source, .. } => shown.push(source),
                _ => {}
            }
        }
    }
    complete_assistant_message(&mut chat, "msg-1", MESSAGE, Some(MessagePhase::FinalAnswer));
    let mut consolidated = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                shown.push(rendered(&cell.display_lines(/*width*/ 80)));
            }
            AppEvent::ConsolidateAgentMessage { source, .. } => consolidated.push(source),
            _ => {}
        }
    }

    for text in frames.iter().chain(&shown).chain(&consolidated) {
        assert!(
            !text.contains("[stateful") && !text.contains("disposition"),
            "{text}"
        );
    }
    assert_eq!(consolidated, vec![ANSWER.to_string()]);
}

#[tokio::test]
async fn a_replayed_answer_shows_no_outcome_block() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.note_rendered_width(/*width*/ 80);
    replay_user_message_text(
        &mut chat,
        "user",
        "How many days are in a leap year?",
        ReplayKind::ThreadSnapshot,
    );
    replay_agent_message(&mut chat, "answer", MESSAGE, ReplayKind::ThreadSnapshot);
    let cells = drain_insert_history(&mut rx);
    insta::assert_snapshot!(
        cells.into_iter().flatten().map(|line| line.to_string()).collect::<Vec<_>>().join("\n").trim(),
        @r"
    › How many days are in a leap year?


    • A leap year has 366 days.
    "
    );
}
