//! Slot packing and rendering under independent 2,048-byte and 512-token caps. Whole records
//! only: an exact value is never shortened into an apparently exact answer; a record that
//! does not fit stays stored and is counted as omitted.

use codex_utils_string::take_bytes_at_char_boundary;

use super::Candidate;
use super::CandidateKind;
use super::Exclusions;
use super::Origin;
use super::Projection;
use super::SourceRef;
use super::Status;
use super::grammar::family;
use super::grammar::is_locator;
use super::grammar::programs;
use super::grammar::statements;

pub(crate) const MAX_CAPSULE_BYTES: usize = 2048;
pub(crate) const MAX_CAPSULE_TOKENS: usize = 512;
const OPEN: &str = "<stateful_capsule>";
const CLOSE: &str = "</stateful_capsule>";
const QUALIFICATION: &str =
    "Historical observations; proposals are not authorization. File state needs checking.";
/// (bytes, tokens) reserved per slot, in packing order after the envelope; the first two
/// (next action, unavailable route) are required slots.
const ENVELOPE: (usize, usize) = (224, 56);
const SLOTS: [(CandidateKind, (usize, usize)); 4] = [
    (CandidateKind::Next, (288, 72)),
    // Unavailable programs, with a same-purpose replacement when one is known.
    (CandidateKind::Route, (384, 96)),
    // An external file change is checked before delivered values.
    (CandidateKind::Change, (384, 96)),
    (CandidateKind::Value, (704, 176)),
];
const COVERAGE: (usize, usize) = (64, 16);
/// The attributing user statement is quoted only this far; its alias names the whole.
const MAX_ATTRIBUTION_QUOTE_BYTES: usize = 48;

/// The continuing model's tokenizer over final text; `None` counts one token per byte.
pub(crate) type TokenCounter<'a> = Option<&'a dyn Fn(&str) -> usize>;

pub(crate) struct CapsuleHeader<'a> {
    pub project: &'a str,
    pub thread: &'a str,
    pub window: &'a str,
    pub eligibility_generation: u64,
    pub watermark: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RenderedCapsule {
    pub text: String,
    pub tokens: usize,
    /// Complete alias map persisted beside the capsule; aliases are not retrieval tools.
    pub aliases: Vec<(String, SourceRef)>,
    pub source_turn_ids: Vec<String>,
    pub source_texts: Vec<String>,
    pub omitted: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CapsuleError {
    /// The required envelope alone exceeds a cap.
    Capacity,
}

pub(crate) fn render_capsule<'a>(
    projection: &'a Projection,
    header: &CapsuleHeader<'_>,
    exclusions: &Exclusions,
    counter: TokenCounter<'_>,
) -> Result<RenderedCapsule, CapsuleError> {
    let count = |text: &str| counter.map_or(text.len(), |count| count(text));
    let ids = |short: bool| {
        let cut = |id: &str| {
            if short {
                id.chars().take(8).collect()
            } else {
                id.to_string()
            }
        };
        format!(
            "P={} T={} W={} E={} S={}",
            escape(&cut(header.project)),
            escape(&cut(header.thread)),
            escape(&cut(header.window)),
            header.eligibility_generation,
            header.watermark
        )
    };
    let envelope = |ids: &str| format!("{OPEN}\n{ids}\n{QUALIFICATION}\n{CLOSE}");
    let mut id_line = ids(false);
    if envelope(&id_line).len() > ENVELOPE.0 {
        id_line = ids(true);
    }
    let base = envelope(&id_line);
    let (base_bytes, base_tokens) = (base.len(), count(&base));
    if base_bytes > ENVELOPE.0 || base_tokens + COVERAGE.1 > MAX_CAPSULE_TOKENS {
        return Err(CapsuleError::Capacity);
    }
    // An envelope over its token reservation (the byte-per-token fallback) is charged to
    // the lowest-priority slots first, never to the next action.
    let (mut bytes, mut tokens) = (ENVELOPE.0, base_tokens.max(ENVELOPE.1));
    let mut reserves = SLOTS;
    let mut deficit = base_tokens.saturating_sub(ENVELOPE.1);
    for (_, (_, slot_tokens)) in reserves.iter_mut().rev() {
        let charged = deficit.min(*slot_tokens);
        *slot_tokens -= charged;
        deficit -= charged;
    }
    let visible: Vec<&Candidate> = projection
        .candidates
        .iter()
        .filter(|c| !exclusions.excludes(c))
        .collect();
    let mut queue: Vec<(usize, Vec<&Candidate>)> = Vec::new();
    for (slot, (kind, _)) in reserves.iter().enumerate() {
        for record in slot_records(*kind, &visible) {
            queue.push((slot, record));
        }
    }
    let mut lines: Vec<Line<'a>> = Vec::new();
    let mut aliases: Vec<(String, SourceRef)> = Vec::new();
    let mut deferred = Vec::new();
    let (mut used_bytes, mut used_tokens) = (base_bytes, base_tokens);
    let (free_bytes, free_tokens) = (
        MAX_CAPSULE_BYTES - COVERAGE.0,
        MAX_CAPSULE_TOKENS - COVERAGE.1,
    );
    let mut offer = |rank: usize, slot_cap: Option<(usize, usize)>, lines: &mut Vec<Line<'a>>| {
        let (slot, record) = &queue[rank];
        let aliases_before = aliases.len();
        let suspended = record[0].kind == CandidateKind::Next
            && lines
                .iter()
                .any(|l| l.record[0].kind == CandidateKind::Next);
        let text = record_line(record, &visible, suspended, &mut aliases);
        let (line_bytes, line_tokens) = (text.len() + 1, count(&format!("{text}\n")));
        let (cap_bytes, cap_tokens) = slot_cap.unwrap_or((free_bytes, free_tokens));
        let fits = used_bytes + line_bytes <= cap_bytes.min(free_bytes)
            && used_tokens + line_tokens <= cap_tokens.min(free_tokens);
        if fits {
            used_bytes += line_bytes;
            used_tokens += line_tokens;
            lines.push(Line {
                text,
                record: record.clone(),
                aliases_before,
                slot: *slot,
                rank,
            });
        } else {
            aliases.truncate(aliases_before);
        }
        fits
    };
    // First pass: slot reservations in priority order; unused space rolls forward. The top
    // record of the next-action and route slots may use any free space.
    for (rank, (slot, _)) in queue.iter().enumerate() {
        let (_, (slot_bytes, slot_tokens)) = reserves[*slot];
        if lines.iter().all(|l| l.slot != *slot) && queue[..rank].iter().all(|(s, _)| s != slot) {
            bytes += slot_bytes;
            tokens += slot_tokens;
        }
        let required = *slot < 2 && lines.iter().all(|l| l.slot != *slot);
        let cap = (!required).then_some((bytes, tokens));
        if !offer(rank, cap, &mut lines) {
            deferred.push(rank);
        }
    }
    // Second pass: leftover space goes to omitted records in the same priority order.
    let mut omitted = 0;
    for rank in deferred {
        if !offer(rank, None, &mut lines) {
            omitted += 1;
        }
    }
    // A real tokenizer is not additive across lines: drop the last records until it fits.
    loop {
        let coverage = coverage_line(projection, omitted, counter.is_none());
        let mut ordered: Vec<&Line<'_>> = lines.iter().collect();
        ordered.sort_by_key(|line| (line.slot, line.rank));
        let body: Vec<&str> = ordered
            .iter()
            .map(|line| line.text.as_str())
            .chain(coverage.as_deref())
            .collect();
        let text = format!(
            "{OPEN}\n{id_line}\n{QUALIFICATION}\n{}{}{CLOSE}",
            body.join("\n"),
            if body.is_empty() { "" } else { "\n" }
        );
        let total = count(&text);
        if text.len() <= MAX_CAPSULE_BYTES && total <= MAX_CAPSULE_TOKENS {
            return Ok(finish(text, total, &lines, aliases, omitted));
        }
        let Some(dropped) = lines.pop() else {
            return Err(CapsuleError::Capacity);
        };
        aliases.truncate(dropped.aliases_before);
        omitted += 1;
    }
}

/// Records of one slot, highest priority first; each is one or two linked candidates.
fn slot_records<'a>(kind: CandidateKind, visible: &[&'a Candidate]) -> Vec<Vec<&'a Candidate>> {
    let newest = visible
        .iter()
        .rev()
        .filter(|c| c.kind == kind && c.text.is_some());
    let with = |status: Status| newest.clone().filter(move |c| c.status == status).copied();
    let single = |c: &'a Candidate| vec![c];
    match kind {
        CandidateKind::Next => with(Status::Active)
            .take(2)
            .chain(with(Status::Proposed).take(1))
            .map(single)
            .collect(),
        // A route is shown working only as the same-purpose replacement of an unavailable
        // program; any other success is history and is not shown.
        CandidateKind::Route => {
            let replacement = |failed: &Candidate| {
                visible
                    .iter()
                    .find(|c| Some(c.id) == failed.resolved_by)
                    .copied()
            };
            with(Status::Failed)
                .map(|f| replacement(f).map_or_else(|| vec![f], |r| vec![f, r]))
                .collect()
        }
        CandidateKind::Attribution => Vec::new(),
        // Historical answer locators are pinned like baselines.
        CandidateKind::Value => with(Status::Baseline)
            .chain(with(Status::Active).filter(|c| c.subject.as_deref().is_some_and(is_locator)))
            .chain(with(Status::Active).filter(|c| !c.subject.as_deref().is_some_and(is_locator)))
            .map(single)
            .collect(),
        CandidateKind::Change => with(Status::Active).map(single).collect(),
    }
}

fn record_line(
    record: &[&Candidate],
    visible: &[&Candidate],
    suspended: bool,
    aliases: &mut Vec<(String, SourceRef)>,
) -> String {
    let first = record[0];
    let text = |c: &Candidate| {
        let partial = if c.partial { " [partial quote]" } else { "" };
        format!("{}{partial}", escape(c.text.as_deref().unwrap_or_default()))
    };
    // Stable source aliases in render order; the full map is persisted with the capsule.
    let mut alias = |c: &Candidate| {
        let named: Vec<String> = c
            .sources
            .iter()
            .map(|source| match aliases.iter().find(|(_, s)| s == source) {
                Some((alias, _)) => alias.clone(),
                None => {
                    let alias = format!("s{}", aliases.len() + 1);
                    aliases.push((alias.clone(), source.clone()));
                    alias
                }
            })
            .collect();
        format!("@{}", named.join(","))
    };
    let origin = match first.origin {
        Origin::User => "user",
        Origin::Assistant => "reported",
        Origin::Structured => "obligation",
        Origin::Host => "observed",
    };
    match first.kind {
        CandidateKind::Next => {
            let label = match first.status {
                Status::Proposed => "next(proposed, not authorized)",
                _ if suspended => "next(suspended)",
                _ => "next",
            };
            format!("{label}: {} {}", text(first), alias(first))
        }
        CandidateKind::Route => match record.get(1) {
            Some(fix) => {
                // Only the replacing statement, not its whole compound command.
                let wanted = first.subject.as_deref().and_then(family);
                let command = fix.text.as_deref().unwrap_or_default();
                let command = command.split(" | cwd ").next().unwrap_or_default();
                let statement = statements(command)
                    .into_iter()
                    .find(|s| programs(s).first().and_then(|p| family(p)) == wanted)
                    .map_or_else(|| text(fix), escape);
                format!(
                    "route: unavailable {} {} -> works {statement} {}",
                    text(first),
                    alias(first),
                    alias(fix)
                )
            }
            None => format!("route: unavailable {} {}", text(first), alias(first)),
        },
        CandidateKind::Attribution => String::new(),
        CandidateKind::Value if first.status == Status::Baseline => {
            // A host observation is the first measurement or the first failing run's node,
            // not a baseline someone declared.
            // A host observation is the first value of its command and label, not a
            // baseline someone declared.
            let label = match first.origin {
                Origin::Host => "first-run",
                _ => "baseline",
            };
            format!("values: {label}({origin}) {} {}", text(first), alias(first))
        }
        CandidateKind::Value => format!("values: ({origin}) {} {}", text(first), alias(first)),
        CandidateKind::Change => {
            let path = escape(first.subject.as_deref().unwrap_or_default());
            let by = match visible.iter().find(|c| Some(c.id) == first.linked) {
                Some(user) => {
                    let said = user.text.as_deref().unwrap_or_default();
                    let quote = take_bytes_at_char_boundary(said, MAX_ATTRIBUTION_QUOTE_BYTES);
                    let cut = if quote.len() < said.len() { "..." } else { "" };
                    let quote = escape(quote);
                    format!("attributed by user {}: \"{quote}{cut}\"", alias(user))
                }
                None => "origin unknown".to_string(),
            };
            format!("change: {path} {} {}; {by}", text(first), alias(first))
        }
    }
}

fn coverage_line(projection: &Projection, omitted: usize, byte_tokens: bool) -> Option<String> {
    let mut parts = Vec::new();
    for (label, n) in [
        ("omitted", omitted),
        ("debt", projection.capacity_debt as usize),
        ("unscanned", projection.uninspected_items as usize),
    ] {
        if n > 0 {
            parts.push(format!("{label} {n}"));
        }
    }
    if byte_tokens {
        parts.push("tokens=bytes".to_string());
    }
    // The structural floor carries no directives; they stay in the root packet.
    parts.push("directives: root packet".to_string());
    (!parts.is_empty()).then(|| format!("coverage: {}", parts.join("; ")))
}

struct Line<'a> {
    text: String,
    record: Vec<&'a Candidate>,
    aliases_before: usize,
    slot: usize,
    rank: usize,
}

fn finish(
    text: String,
    tokens: usize,
    lines: &[Line<'_>],
    aliases: Vec<(String, SourceRef)>,
    omitted: usize,
) -> RenderedCapsule {
    let (mut turns, mut texts) = (Vec::new(), Vec::new());
    for candidate in lines.iter().flat_map(|line| &line.record) {
        for source in &candidate.sources {
            if !turns.contains(&source.turn_id) {
                turns.push(source.turn_id.clone());
            }
        }
        texts.extend(candidate.text.clone());
    }
    RenderedCapsule {
        text,
        tokens,
        aliases,
        source_turn_ids: turns,
        source_texts: texts,
        omitted,
    }
}

/// Escapes markup openers and line breaks so no text can close the capsule; budgets count the escaped bytes.
pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
}
