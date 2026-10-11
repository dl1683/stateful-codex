//! Shortening of quoted turn text in the continuity record.
//!
//! A turn's text usually ends with where it left off ("Next step: ..."), so a shortened
//! quote keeps its head and its tail, and a trailing next-step line whole, instead of the
//! head alone. The excerpt stays inside the bound a head-only quote of the same text has.

use crate::continuity::escape;
use crate::continuity::quote;

/// Below this many bytes for the two quoted parts a split says too little, so the text is
/// shortened to its head alone.
const MIN_SPLIT_BYTES: usize = 128;

/// JSON-quotes and markup-escapes `text`. Text whose quote exceeds `limit` bytes is shortened
/// to a head and a tail around a marker naming the omitted and the original byte counts; the
/// result is never longer than a head-only quote of the same text at the same limit.
pub(crate) fn quote_excerpt(text: &str, limit: usize) -> String {
    let whole = encode(text);
    if whole.len() <= limit {
        return whole;
    }
    let total = text.len();
    let bound = limit + format!(" [shortened at 0 of {total} bytes]").len();
    let budget = bound.saturating_sub(middle_marker(total, total).len());
    if budget < MIN_SPLIT_BYTES {
        return quote(text, limit);
    }

    let mut tail = (fit_suffix(text, 0, total, budget * 2 / 5), total);
    let mut trailing_reserve = 0;
    if let Some(start) = next_step_start(text).filter(|start| *start < tail.0) {
        // The next-step line may displace head text up to three quarters of the budget; a
        // longer one keeps its first part.
        let room = budget * 3 / 4;
        tail = (start, total);
        if encode(&text[start..]).len() > room {
            trailing_reserve = trailing_marker(total, total).len();
            tail.1 = fit_prefix(text, start, total, room - trailing_reserve);
        }
    }
    let tail_text = encode(&text[tail.0..tail.1]);
    let head_end = fit_prefix(text, 0, tail.0, budget - trailing_reserve - tail_text.len());

    let mut excerpt = encode(&text[..head_end]);
    excerpt.push_str(&middle_marker(tail.0 - head_end, total));
    excerpt.push_str(&tail_text);
    if tail.1 < total {
        excerpt.push_str(&trailing_marker(total - tail.1, total));
    }
    excerpt
}

fn encode(text: &str) -> String {
    escape(&serde_json::to_string(text).unwrap_or_default())
}

fn middle_marker(omitted: usize, total: usize) -> String {
    format!(" [... {omitted} of {total} bytes omitted ...] ")
}

fn trailing_marker(omitted: usize, total: usize) -> String {
    format!(" [... last {omitted} of {total} bytes omitted]")
}

/// The longest `text[start..end']`, `end' <= end`, whose quote fits `max` bytes.
fn fit_prefix(text: &str, start: usize, end: usize, max: usize) -> usize {
    // Every source byte renders as at least one byte, so longer cuts cannot fit.
    let cuts = char_cuts(text, start, end)
        .filter(|cut| cut - start <= max)
        .collect::<Vec<_>>();
    let fitting = cuts.partition_point(|cut| encode(&text[start..*cut]).len() <= max);
    cuts[fitting.saturating_sub(1)]
}

/// The longest `text[start'..end]`, `start' >= start`, whose quote fits `max` bytes.
fn fit_suffix(text: &str, start: usize, end: usize, max: usize) -> usize {
    let cuts = char_cuts(text, start, end)
        .filter(|cut| end - cut <= max)
        .collect::<Vec<_>>();
    let too_long = cuts.partition_point(|cut| encode(&text[*cut..end]).len() > max);
    cuts[too_long.min(cuts.len() - 1)]
}

/// Char boundaries of `text[start..end]`, both ends included, ascending.
fn char_cuts(text: &str, start: usize, end: usize) -> impl Iterator<Item = usize> + '_ {
    text[start..end]
        .char_indices()
        .map(move |(index, _)| start + index)
        .chain([end])
}

/// Start of the last line or sentence in the second half of `text` that begins with "Next
/// step" or "Next:" (any case, after list or emphasis markup).
fn next_step_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    for (index, ch) in text.char_indices() {
        let after_sentence =
            ch.is_whitespace() && index > 0 && matches!(bytes[index - 1], b'.' | b'!' | b'?');
        if ch == '\n' || after_sentence {
            starts.push(index + ch.len_utf8());
        }
    }
    starts
        .into_iter()
        .rev()
        .take_while(|start| *start >= text.len() / 2)
        .find(|start| {
            let line = text[*start..].trim_start_matches([' ', '\t', '*', '-', '#', '>', '_']);
            ["next step", "next:"].iter().any(|prefix| {
                line.get(..prefix.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
            })
        })
}

#[cfg(test)]
#[path = "continuity_excerpt_tests.rs"]
mod tests;
