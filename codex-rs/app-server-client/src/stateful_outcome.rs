//! Presentation of the Stateful outcome block (`[stateful-outcome]` ... `[/stateful-outcome]`)
//! that an Autonomous run's final message may end with. Clients never show it: the answer is
//! shown without it, live and on replay, while the raw message, events and history keep it.
//! The Web client mirrors this in `clients/stateful-codex/public/outcome-trailer.mjs`.

const OPEN: &str = "[stateful-outcome]";
const CLOSE: &str = "[/stateful-outcome]";
/// Bound on a block that can be withheld, so a long open block is never held back unbounded.
const MAX_TRAILER_BYTES: usize = 4 * 1024;

/// Whether `text` is a whole message or a prefix of one still arriving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrival {
    Complete,
    Streaming,
}

/// `message` without its trailing outcome block. Only a closed block at the very end, on its
/// own lines and made of `disposition:`, `open-issues:` and `- ` lines, is hidden; prose that
/// merely mentions or quotes the block stays visible.
pub fn visible_answer(message: &str) -> &str {
    match trailer_start(message, Arrival::Complete) {
        Some(start) => message[..start].trim_end(),
        None => message,
    }
}

/// Filters an assistant message's deltas so no part of a trailing outcome block is shown while
/// it streams. Text that turns out not to be a block is released as soon as that is known.
#[derive(Debug, Default)]
pub struct OutcomeTrailerStream {
    seen: String,
    released: usize,
}

impl OutcomeTrailerStream {
    /// Adds `delta` and returns the text that can be shown now.
    pub fn push(&mut self, delta: &str) -> String {
        self.seen.push_str(delta);
        let hold = trailer_start(&self.seen, Arrival::Streaming)
            .unwrap_or(self.seen.len())
            .max(self.released);
        let shown = self.seen[self.released..hold].to_string();
        self.released = hold;
        shown
    }
}

fn is_body_line(line: &str) -> bool {
    line.starts_with("disposition:") || line.starts_with("open-issues:") || line.starts_with("- ")
}

/// Byte offset where the trailing outcome block of `text` starts. A complete message must end
/// with a closed block followed only by blank lines. A streaming prefix also withholds an open
/// block whose lines so far fit, and a last line that may still become its opener.
fn trailer_start(text: &str, arrival: Arrival) -> Option<usize> {
    let mut lines = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let partial = !line.ends_with('\n');
        lines.push((start, line.trim_end_matches(['\n', '\r']), partial));
        start += line.len();
    }
    let pending_opener = match lines.last() {
        Some(&(start, line, true))
            if arrival == Arrival::Streaming && !line.is_empty() && OPEN.starts_with(line) =>
        {
            Some(start)
        }
        Some(_) | None => None,
    };
    let Some(opener) = lines.iter().rposition(|&(_, line, partial)| {
        (!partial || arrival == Arrival::Complete) && line.trim_end() == OPEN
    }) else {
        return pending_opener;
    };
    let opener_start = lines[opener].0;
    if text.len() - opener_start > MAX_TRAILER_BYTES {
        return pending_opener;
    }
    let mut closed = false;
    for &(_, line, partial) in &lines[opener + 1..] {
        if partial && arrival == Arrival::Streaming {
            break;
        }
        let line = line.trim_end();
        if closed {
            if !line.is_empty() {
                return pending_opener;
            }
        } else if line == CLOSE {
            closed = true;
        } else if !is_body_line(line) {
            return pending_opener;
        }
    }
    match arrival {
        Arrival::Complete => closed.then_some(opener_start),
        Arrival::Streaming => Some(opener_start),
    }
}

#[cfg(test)]
#[path = "stateful_outcome_tests.rs"]
mod tests;
