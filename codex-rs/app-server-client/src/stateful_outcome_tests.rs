use super::OutcomeTrailerStream;
use super::visible_answer;
use pretty_assertions::assert_eq;

const ANSWER: &str = "A leap year has 366 days.";
const BLOCK: &str =
    "[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

#[test]
fn a_complete_trailing_outcome_block_is_hidden() {
    let messages = [
        format!("{ANSWER}\n\n{BLOCK}"),
        format!("{ANSWER}\r\n\r\n{}\r\n", BLOCK.replace('\n', "\r\n")),
        format!("{ANSWER}\n{BLOCK}\n\n"),
        format!(
            "{ANSWER}\n[stateful-outcome]\ndisposition: blocked\nopen-issues:\n- The source is missing.\n[/stateful-outcome]"
        ),
    ];
    assert_eq!(
        messages
            .iter()
            .map(|message| visible_answer(message))
            .collect::<Vec<_>>(),
        vec![ANSWER; 4]
    );
}

#[test]
fn prose_and_quoted_examples_stay_visible() {
    let messages = [
        format!("Write it like this:\n```\n{BLOCK}\n```\nThen stop."),
        "The block starts with [stateful-outcome] on its own line.".to_string(),
        format!("{ANSWER}\n[stateful-outcome]\ndisposition: answer"),
        format!("{ANSWER}\n[stateful-outcome]\nsomething else\n[/stateful-outcome]"),
        format!(
            "{ANSWER}\n[stateful-outcome]\n{}[/stateful-outcome]",
            "- x\n".repeat(1_500)
        ),
        ANSWER.to_string(),
    ];
    for message in &messages {
        assert_eq!(visible_answer(message), message.as_str());
    }
}

#[test]
fn a_streamed_trailer_is_never_shown_at_any_split() {
    let message = format!("{ANSWER}\n\n{BLOCK}");
    // Every split point, byte by byte, including inside the opener and the closing line.
    for split in 1..message.len() {
        let mut stream = OutcomeTrailerStream::default();
        let shown = [
            stream.push(&message[..split]),
            stream.push(&message[split..]),
        ]
        .concat();
        assert_eq!(shown.trim_end(), ANSWER, "split at {split}");
    }
}

#[test]
fn streamed_text_that_is_not_a_trailer_is_released() {
    let mut stream = OutcomeTrailerStream::default();
    let shown = [
        stream.push("Paris.\n[state"),
        stream.push(" of the art]\n"),
        stream.push("[stateful-outcome]\nnot a trailer line\n"),
        stream.push("more"),
    ]
    .concat();
    assert_eq!(
        shown,
        "Paris.\n[state of the art]\n[stateful-outcome]\nnot a trailer line\nmore"
    );
}
