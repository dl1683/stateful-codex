//! Shortening of quoted turn text in the continuity record.
//!
//! A turn's text usually ends with where it left off ("Next step: ..."), so a shortened
//! quote keeps its head and its tail, and a trailing next-step line whole, instead of the
//! head alone. The excerpt stays inside the size envelope a head-only quote of the same text
//! is allowed.

use crate::continuity::escape;
use crate::continuity::quote;

/// Below this many bytes for the two quoted parts a split says too little, so the text is
/// shortened to its head alone.
const MIN_SPLIT_BYTES: usize = 128;
/// Smallest first part of an overlong next-step line worth keeping.
const MIN_LINE_BYTES: usize = 32;
const NEXT_STEP: &str = "next step";
const NEXT: &str = "next:";

/// JSON-quotes and markup-escapes `text`. Text whose quote exceeds `limit` bytes is shortened
/// to a head and a tail around a marker naming the omitted and the original byte counts,
/// inside the size envelope a head-only quote at the same limit is allowed
/// (`limit` plus its marker); tiny limits fall back to that head-only quote unchanged.
pub(crate) fn quote_excerpt(text: &str, limit: usize) -> String {
    let whole = encode(text);
    if whole.len() <= limit {
        return whole;
    }
    let total = text.len();
    let bound = limit + format!(" [shortened at 0 of {total} bytes]").len();
    // Space for the quoted parts once the middle marker's longest form is reserved.
    let budget = bound.saturating_sub(middle_marker(total, total).len());
    let default_tail =
        (budget >= MIN_SPLIT_BYTES).then(|| fit_suffix(text, 0, total, budget * 2 / 5));
    let tail = match next_step(text) {
        // A next step the default tail already holds needs no special room.
        Some((start, end)) if default_tail.is_none_or(|cut| start < cut) => {
            next_step_tail(text, start, end, budget)
        }
        _ => default_tail.map(|cut| (cut, total)),
    };
    let Some((tail_start, tail_end)) = tail else {
        return quote(text, limit);
    };
    let trailing = if tail_end < total {
        trailing_marker(total - tail_end, total)
    } else {
        String::new()
    };
    let tail_text = encode(&text[tail_start..tail_end]);
    let head_end = fit_prefix(
        text,
        0,
        tail_start,
        budget - trailing.len() - tail_text.len(),
    );

    let mut excerpt = encode(&text[..head_end]);
    excerpt.push_str(&middle_marker(tail_start - head_end, total));
    excerpt.push_str(&tail_text);
    excerpt.push_str(&trailing);
    excerpt
}

/// The kept range starting at the next-step line `text[start..end]`, within `budget` bytes
/// for both quoted parts. A line that fits is kept whole: the head then gets the space left
/// and text after the line at most a third of it. A longer line keeps its first part, up to
/// three quarters of the space.
fn next_step_tail(text: &str, start: usize, end: usize, budget: usize) -> Option<(usize, usize)> {
    let total = text.len();
    let trailing = trailing_marker(total, total).len();
    let line = encode(&text[start..end]).len();
    let reserve = if end < total { trailing } else { 0 };
    // The head needs at least its empty quote.
    let room = budget.checked_sub(reserve + 2)?;
    if line <= room {
        let tail_end = fit_prefix(text, start, total, line + (room - line) / 3);
        return Some((start, tail_end));
    }
    let room = budget.checked_sub(trailing)? * 3 / 4;
    (room >= MIN_LINE_BYTES).then(|| (start, fit_prefix(text, start, end, room)))
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

/// The last line or sentence in the second half of `text` that begins with "Next step(s)"
/// or "Next:" (any case, after list or emphasis markup), as its start and the end of its
/// line. This is lexical selection: it marks text worth keeping, not an authorized plan.
fn next_step(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    for (index, ch) in text.char_indices() {
        let after_sentence =
            ch.is_whitespace() && index > 0 && matches!(bytes[index - 1], b'.' | b'!' | b'?');
        if ch == '\n' || after_sentence {
            starts.push(index + ch.len_utf8());
        }
    }
    let start = starts
        .into_iter()
        .rev()
        .take_while(|start| *start >= text.len() / 2)
        .find(|start| {
            let line = text[*start..].trim_start_matches([' ', '\t', '*', '-', '#', '>', '_']);
            let step = line
                .get(..NEXT_STEP.len())
                .filter(|head| head.eq_ignore_ascii_case(NEXT_STEP))
                .map(|_| &line[NEXT_STEP.len()..])
                .map(|rest| rest.strip_prefix(['s', 'S']).unwrap_or(rest))
                .is_some_and(|rest| rest.chars().next().is_none_or(|ch| !ch.is_alphanumeric()));
            step || line
                .get(..NEXT.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(NEXT))
        })?;
    let end = text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset);
    Some((start, end))
}

#[cfg(test)]
#[path = "continuity_excerpt_tests.rs"]
mod tests;
