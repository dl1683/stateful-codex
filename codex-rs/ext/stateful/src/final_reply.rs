//! Shared classification of final-answer bodies, trailers and turn-local model progress.

const START: &str = "[stateful-outcome]";
const END: &str = "[/stateful-outcome]";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FinalReply<'a> {
    Deliver { body: &'a str, issues: Vec<String> },
    KeepRunning(&'static str),
    StatusOnly(&'a str),
}

#[derive(Clone, Copy)]
pub(crate) enum ModelProgress {
    Absent,
    Recorded,
}

/// Both finalization and the status-only diagnostic classify the same body here.
pub(crate) fn final_reply(message: &str) -> FinalReply<'_> {
    classify(message, ModelProgress::Absent)
}

pub(crate) fn classify(message: &str, progress: ModelProgress) -> FinalReply<'_> {
    let text = message.trim();
    let (body, issues, explicit_answer) = match text.rfind(START) {
        None => (text, Vec::new(), false),
        Some(start) => {
            let body = text[..start].trim();
            let block = &text[start..];
            match parse_trailer(text, start) {
                Some(("answer", issues)) => (body, issues, true),
                Some(("continue" | "blocked", _)) => {
                    return FinalReply::KeepRunning(
                        "the model declared continuing or blocked work",
                    );
                }
                Some((_, _)) | None => {
                    if block.lines().any(|line| {
                        matches!(
                            line.trim(),
                            "disposition: continue" | "disposition: blocked"
                        )
                    }) {
                        return FinalReply::KeepRunning(
                            "the malformed trailer declares continuing or blocked work",
                        );
                    }
                    (body, Vec::new(), false)
                }
            }
        }
    };
    if body.is_empty() || crate::deliverable::is_status_only(body) {
        return FinalReply::StatusOnly(body);
    }
    if !explicit_answer && matches!(progress, ModelProgress::Recorded) {
        return FinalReply::KeepRunning("model recorded next work this turn");
    }
    FinalReply::Deliver { body, issues }
}

/// Exact trailing grammar. Structural malformation is distinct from malformed answer text,
/// whose admission belongs to the runtime.
fn parse_trailer(text: &str, start: usize) -> Option<(&str, Vec<String>)> {
    let before = &text[..start];
    let block = &text[start..];
    if before.contains(START)
        || (!before.is_empty() && !before.ends_with('\n'))
        || block.len() > 4 * 1024
    {
        return None;
    }
    let mut lines = block.lines().map(str::trim_end);
    if lines.next()? != START {
        return None;
    }
    let disposition = lines.next()?.strip_prefix("disposition: ")?;
    if !matches!(disposition, "answer" | "continue" | "blocked") {
        return None;
    }
    let mut issues = Vec::new();
    match lines.next()? {
        "open-issues: none" => {
            if lines.next()? != END {
                return None;
            }
        }
        "open-issues:" => {
            loop {
                let line = lines.next()?;
                if line == END {
                    break;
                }
                let issue = line.strip_prefix("- ")?.trim();
                if issue.is_empty() || issues.len() == 16 {
                    return None;
                }
                issues.push(issue.to_string());
            }
            if issues.is_empty() {
                return None;
            }
        }
        _ => return None,
    }
    if lines.next().is_some() {
        return None;
    }
    Some((disposition, issues))
}

#[cfg(test)]
#[path = "final_reply_tests.rs"]
mod tests;
