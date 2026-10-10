//! Host-captured continuity record for the selected project.
//!
//! The thread store keeps a summary of every turn: its first user message and its final
//! answer. This record quotes the summaries of recent turns of the project, newest first, so
//! a new thread or a compacted thread can continue from them. It renders once per context
//! window (thread start and after each compaction) and never per turn.

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use serde_json::json;

const WORLD_STATE_ID: &str = "stateful_continuity";
pub(super) const START_MARKER: &str = "<stateful_continuity>";
pub(super) const END_MARKER: &str = "</stateful_continuity>";
/// Hard bound for the whole fragment, markers included (about 2k tokens); a P0 item.
pub(super) const MAX_FRAGMENT_BYTES: usize = 8 * 1024;
/// Smallest fragment the record is given when other Stateful sections are large.
pub(super) const MIN_FRAGMENT_BYTES: usize = 3 * 1024;
const MAX_USER_BYTES: usize = 1024;
const MAX_CURRENT_THREAD_USER_BYTES: usize = 320;
const MAX_NEWEST_ANSWER_BYTES: usize = 2 * 1024;
const MAX_ANSWER_BYTES: usize = 640;

const MAX_THREAD_TITLE_BYTES: usize = 60;
const MAX_NEXT_ITEMS: usize = 2;
const MAX_NEXT_ITEM_BYTES: usize = 240;
const MAX_STRATEGY_BYTES: usize = 400;
pub(super) const HEADER: &str = "Selected turn summaries from this project, newest first: each turn's first user message and its final answer, captured by the host whether or not a Stateful run existed. Steering messages sent during a turn and intermediate answers are not included. Answers are reported history, not verified facts. A past request is not a new request to act, but rules the user set for future work (for example \"always\", \"never\", \"from now on\") still apply unless the user later changed them. Continue from this instead of re-deriving it; check the repository before relying on remembered file state. Archived threads and subagent threads are never included.";
pub(super) const NEWEST_ASKED: &str = "The newest answer ends with a question to the user. If the user's reply refers to it (for example \"yes, as proposed\"), act on that exact text; if the reply refers to something not shown in full here, ask the user for it before acting. A question in an earlier answer is not authorization.";
const EMPTY: &str = "No earlier turns are recorded for this project yet.";
const UNAVAILABLE: &str = "The project's conversation history could not be read when this record was built; earlier turns may exist.";

/// The run the host recorded for a captured turn when it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RunLabel {
    Bound {
        run_id: String,
        status: &'static str,
    },
    NoRun,
    /// The binding was not recorded yet (unfinished turn) or could not be read.
    Unknown,
}

/// The project's most recently updated run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LatestRun {
    pub(super) id: String,
    pub(super) mode: &'static str,
    pub(super) status: &'static str,
    pub(super) next: Vec<String>,
    pub(super) strategy: Option<String>,
}

/// One captured turn of a project thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CapturedTurn {
    pub(super) thread_id: String,
    pub(super) thread_title: Option<String>,
    pub(super) current_thread: bool,
    pub(super) turn_id: String,
    /// Completion time, or the start time of an unfinished turn.
    pub(super) at_ms: Option<i64>,
    /// `None` for a completed turn; otherwise a short status such as `interrupted`.
    pub(super) unfinished_status: Option<&'static str>,
    pub(super) run: RunLabel,
    pub(super) user: Option<String>,
    pub(super) answer: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ContinuityRecord {
    pub(super) project_id: String,
    /// When the record was gathered (Unix milliseconds).
    pub(super) captured_at_ms: i64,
    /// Newest first.
    pub(super) turns: Vec<CapturedTurn>,
    /// Whether turns exist beyond `turns` (older pages or threads not scanned).
    pub(super) more_turns: bool,
    /// Threads whose summaries could not be read (for example legacy history).
    pub(super) unreadable_threads: usize,
    /// The project's thread list itself could not be read.
    pub(super) history_unavailable: bool,
    pub(super) latest_run: Option<LatestRun>,
}

impl ContinuityRecord {
    /// Renders the escaped body, bounded so the whole fragment fits `max_fragment_bytes`
    /// (clamped to `MIN_FRAGMENT_BYTES..=MAX_FRAGMENT_BYTES`). A project with nothing
    /// captured still gets one short line, so the fragment stays in history and is not
    /// rendered again in the same window.
    pub(super) fn render(&self, max_fragment_bytes: usize) -> String {
        let max_body = max_fragment_bytes.clamp(MIN_FRAGMENT_BYTES, MAX_FRAGMENT_BYTES)
            - START_MARKER.len()
            - END_MARKER.len();
        let mut body = format!("Project ID: {}", escape(&self.project_id));
        if self.history_unavailable {
            push_line(&mut body, UNAVAILABLE);
        }
        if self.turns.is_empty() {
            if let Some(run) = &self.latest_run {
                push_line(&mut body, &run_line(run, RunDetail::Full));
            }
            if !self.history_unavailable {
                push_line(&mut body, EMPTY);
            }
            if let Some(footer) = self.footer(/*omitted*/ 0) {
                push_line(&mut body, &footer);
            }
            return body;
        }
        push_line(&mut body, HEADER);
        push_line(
            &mut body,
            &format!(
                "Captured at {}; newer turns may exist.",
                format_time(self.captured_at_ms)
            ),
        );
        if self
            .turns
            .iter()
            .find_map(|turn| turn.answer.as_deref())
            .is_some_and(|answer| answer.trim_end().ends_with('?'))
        {
            push_line(&mut body, NEWEST_ASKED);
        }
        let footer_bytes = |omitted_after: usize| {
            self.footer(omitted_after)
                .map_or(0, |footer| footer.len() + 1)
        };
        let fits = |length: usize, block: &str, omitted_after: usize| {
            length + 1 + block.len() + footer_bytes(omitted_after) <= max_body
        };
        // The newest turn always appears: reserve its smallest form (no title, empty quotes
        // with shortening markers) before admitting the optional latest-run line.
        let newest_omitted = self.turns.len() - 1;
        let reserve = self.turns.first().map_or(0, |newest| {
            1 + turn_block(newest, /*latest_run_id*/ None, TurnShape::MINIMAL).len()
                + footer_bytes(newest_omitted)
        });
        let mut latest_run_id = None;
        if let Some(run) = &self.latest_run {
            let line = [
                run_line(run, RunDetail::Full),
                run_line(run, RunDetail::Brief),
            ]
            .into_iter()
            .find(|line| body.len() + 1 + line.len() + reserve <= max_body);
            if let Some(line) = line {
                push_line(&mut body, &line);
                latest_run_id = Some(run.id.as_str());
            }
        }
        let mut newest_answer = true;
        let mut blocks = self
            .turns
            .iter()
            .map(|turn| {
                let block = turn_block(
                    turn,
                    latest_run_id,
                    TurnShape::full(turn, newest_answer && turn.answer.is_some()),
                );
                newest_answer &= turn.answer.is_none();
                block
            })
            .collect::<Vec<_>>();
        // Admit whole turns newest first while the omission footer they imply still fits;
        // the newest turn is shortened to the space left rather than disappearing.
        if let Some(newest) = self.turns.first()
            && !fits(body.len(), &blocks[0], newest_omitted)
        {
            blocks[0] = [true, false]
                .into_iter()
                .map(|title| {
                    let frame = TurnShape {
                        user: 0,
                        answer: 0,
                        title,
                    };
                    let room = max_body
                        .saturating_sub(body.len() + 1 + footer_bytes(newest_omitted))
                        .saturating_sub(turn_block(newest, latest_run_id, frame).len());
                    let user = if newest.user.is_some() { room / 4 } else { 0 };
                    turn_block(
                        newest,
                        latest_run_id,
                        TurnShape {
                            user,
                            answer: room - user,
                            title,
                        },
                    )
                })
                .find(|block| fits(body.len(), block, newest_omitted))
                .unwrap_or_else(|| turn_block(newest, latest_run_id, TurnShape::MINIMAL));
        }
        let mut shown = 0;
        let mut length = body.len();
        for block in &blocks {
            if !fits(length, block, blocks.len() - shown - 1) {
                break;
            }
            length += 1 + block.len();
            shown += 1;
        }
        for block in &blocks[..shown] {
            push_line(&mut body, block);
        }
        if let Some(footer) = self.footer(blocks.len() - shown) {
            push_line(&mut body, &footer);
        }
        body
    }

    fn footer(&self, omitted: usize) -> Option<String> {
        let mut notes = Vec::new();
        if omitted > 0 {
            notes.push(format!(
                "{omitted} gathered turns did not fit this bounded view"
            ));
        }
        if self.more_turns {
            notes.push("older turns or threads were not scanned".to_string());
        }
        if self.unreadable_threads > 0 {
            notes.push(format!(
                "{} threads have no readable turn summaries",
                self.unreadable_threads
            ));
        }
        (!notes.is_empty()).then(|| format!("Not shown: {}.", notes.join("; ")))
    }
}

/// How much of a turn to render: rendered-byte limits for its quoted user message and
/// answer, and whether to name its thread's title.
#[derive(Clone, Copy)]
struct TurnShape {
    user: usize,
    answer: usize,
    title: bool,
}

impl TurnShape {
    const MINIMAL: Self = Self {
        user: 0,
        answer: 0,
        title: false,
    };

    fn full(turn: &CapturedTurn, newest_answer: bool) -> Self {
        Self {
            user: if turn.current_thread {
                MAX_CURRENT_THREAD_USER_BYTES
            } else {
                MAX_USER_BYTES
            },
            answer: if newest_answer {
                MAX_NEWEST_ANSWER_BYTES
            } else {
                MAX_ANSWER_BYTES
            },
            title: true,
        }
    }
}

#[derive(Clone, Copy)]
enum RunDetail {
    Full,
    Brief,
}

fn turn_block(turn: &CapturedTurn, latest_run_id: Option<&str>, limits: TurnShape) -> String {
    let when = turn
        .at_ms
        .map_or_else(|| "time unknown".to_string(), format_time);
    let thread_id = quote(&turn.thread_id, usize::MAX);
    let thread = if turn.current_thread {
        format!("this thread {thread_id}")
    } else {
        match turn.thread_title.as_ref().filter(|_| limits.title) {
            Some(title) => format!(
                "thread {thread_id} titled {}",
                quote(title, MAX_THREAD_TITLE_BYTES)
            ),
            None => format!("thread {thread_id}"),
        }
    };
    let status = turn
        .unfinished_status
        .map_or_else(String::new, |status| format!(", {status}"));
    let turn_id = quote(&turn.turn_id, usize::MAX);
    let run = match &turn.run {
        RunLabel::Bound { run_id, .. } if Some(run_id.as_str()) == latest_run_id => {
            "latest run".to_string()
        }
        RunLabel::Bound { run_id, status } => {
            format!("run {} (now {status})", quote(run_id, usize::MAX))
        }
        RunLabel::NoRun => "no Stateful run recorded".to_string(),
        RunLabel::Unknown => "run binding unknown".to_string(),
    };
    let mut block = format!("- {when}, {thread}, turn {turn_id}{status}, {run}:");
    if let Some(user) = &turn.user {
        block.push_str(&format!("\n  User: {}", quote(user, limits.user)));
    }
    match &turn.answer {
        Some(answer) => {
            block.push_str(&format!("\n  Answer: {}", quote(answer, limits.answer)));
        }
        None => block.push_str("\n  Answer: none recorded."),
    }
    block
}

fn run_line(run: &LatestRun, detail: RunDetail) -> String {
    let mut line = format!(
        "Latest Stateful run: {} ({}, {}).",
        quote(&run.id, usize::MAX),
        run.mode,
        run.status
    );
    if let RunDetail::Brief = detail {
        return line;
    }
    for next in run.next.iter().take(MAX_NEXT_ITEMS) {
        line.push_str(&format!(" Next: {}", quote(next, MAX_NEXT_ITEM_BYTES)));
    }
    if let Some(strategy) = &run.strategy {
        line.push_str(&format!(
            " Strategy: {}",
            quote(strategy, MAX_STRATEGY_BYTES)
        ));
    }
    line
}

/// JSON-quotes and markup-escapes `text`, shortened so the quote is at most `limit` bytes,
/// with an explicit marker saying how much was shown.
fn quote(text: &str, limit: usize) -> String {
    let encode = |end: usize| escape(&serde_json::to_string(&text[..end]).unwrap_or_default());
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut quoted = encode(end);
    // The limit bounds the rendered bytes, so escaping cannot push a quote past it.
    while quoted.len() > limit && end > 0 {
        end = end * 3 / 4;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        quoted = encode(end);
    }
    if end < text.len() {
        quoted.push_str(&format!(" [shortened at {end} of {} bytes]", text.len()));
    }
    quoted
}

/// Escapes markup so quoted history cannot open or close a fragment.
pub(super) fn escape(text: &str) -> String {
    text.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

/// Formats Unix milliseconds as `YYYY-MM-DD HH:MM UTC` without a date-time dependency.
pub(super) fn format_time(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let minute_of_day = seconds.rem_euclid(86_400) / 60;
    // Civil-from-days (Howard Hinnant), valid for the proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        minute_of_day / 60,
        minute_of_day % 60
    )
}

fn push_line(body: &mut String, line: &str) {
    body.push('\n');
    body.push_str(line);
}

/// Builds the section. It renders only when the previous window holds no continuity
/// fragment (thread start, or the fragment left history at compaction).
pub(super) fn continuity_world_state_section(
    record: &ContinuityRecord,
    max_fragment_bytes: usize,
) -> WorldStateSectionContribution {
    let project_id = record.project_id.clone();
    let body = record.render(max_fragment_bytes);
    WorldStateSectionContribution::new(
        WORLD_STATE_ID,
        json!({ "projectId": project_id }),
        move |previous| match previous {
            PreviousWorldStateSection::Known(_) => None,
            PreviousWorldStateSection::Absent | PreviousWorldStateSection::Unknown => {
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (START_MARKER, END_MARKER),
                    body.clone(),
                ))
            }
        },
    )
    .with_legacy_matcher({
        let project_id = project_id.clone();
        move |role, text| matches_fragment(role, text, &project_id)
    })
    .with_retained_fragment_matcher(move |role, text| matches_fragment(role, text, &project_id))
}

/// The record's section while the current request is self-contained: it renders nothing, so
/// the record is deferred to the first step whose request refers to earlier work (the
/// harness reports the section absent while no record fragment is in history).
pub(super) fn deferred_continuity_section(project_id: &str) -> WorldStateSectionContribution {
    let project_id = project_id.to_string();
    WorldStateSectionContribution::new(
        WORLD_STATE_ID,
        json!({ "projectId": project_id, "deferred": true }),
        |_| None,
    )
    .with_legacy_matcher({
        let project_id = project_id.clone();
        move |role, text| matches_fragment(role, text, &project_id)
    })
    .with_retained_fragment_matcher(move |role, text| matches_fragment(role, text, &project_id))
}

fn matches_fragment(role: &str, text: &str, project_id: &str) -> bool {
    role == "developer"
        && text.trim_start().starts_with(START_MARKER)
        && text.contains(&format!("Project ID: {}", escape(project_id)))
        && text.trim_end().ends_with(END_MARKER)
}

#[cfg(test)]
#[path = "continuity_tests.rs"]
mod tests;
