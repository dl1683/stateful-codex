//! Command observations for the structural floor: one learned fact per unavailable program,
//! its replacement only when a same-purpose command succeeded and has not failed since, and
//! strict `label: value unit` output lines. Exit 0 alone is history, never proof of a route.

use codex_utils_string::take_bytes_at_char_boundary;

use super::CandidateKind as K;
use super::MAX_REFERENCES;
use super::Origin as O;
use super::Projection;
use super::SourceRef;
use super::Status as S;
use super::grammar::MAX_INSPECTED_BYTES;
use super::grammar::UNAVAILABLE;
use super::grammar::any_in;
use super::grammar::command_head;
use super::grammar::command_key;
use super::grammar::family;
use super::grammar::labeled_value;
use super::grammar::programs;
use super::grammar::shell_body;
use super::grammar::statements;
use super::grammar::unavailable_program;

const MAX_MEASUREMENTS: usize = 8;
const MAX_ERROR_BYTES: usize = 120;
const MAX_COMMAND_LABEL_BYTES: usize = 60;
/// Replacement commands render as a head of at most this many bytes.
const MAX_ROUTE_HEAD_BYTES: usize = 120;

impl Projection {
    pub(super) fn observe_command(
        &mut self,
        source: &SourceRef,
        argv: &[String],
        cwd: &str,
        exit: Option<i32>,
        output: &str,
    ) {
        // No observed result, no fact: a denial before launch or a kill reports nothing.
        let Some(code) = exit else { return };
        let command = shell_body(argv);
        let programs = programs(&command);
        // Unavailability comes from the shell's error text, whatever the exit status (a
        // shell may report a failed pipeline statement as success), for a program the
        // command runs.
        let missing = unavailable_program(output).filter(|name| programs.contains(name));
        let missing = missing.or_else(|| {
            matches!(code, 127 | 9009)
                .then(|| programs.first().cloned())
                .flatten()
        });
        if code != 0 || missing.is_some() {
            // A later failure of a replacement withdraws it: the route is unknown again.
            let failing: Vec<u64> = self
                .candidates
                .iter()
                .filter(|c| {
                    c.kind == K::Route
                        && c.status == S::Active
                        && c.subject.as_ref().is_some_and(|p| programs.contains(p))
                })
                .map(|c| c.id)
                .collect();
            for failed in &mut self.candidates {
                if failed.resolved_by.is_some_and(|id| failing.contains(&id)) {
                    failed.resolved_by = None;
                }
            }
        }
        if let Some(missing) = missing {
            let error = output
                .lines()
                .find(|line| any_in(UNAVAILABLE, &line.to_lowercase()))
                .unwrap_or_default()
                .trim_matches(|c: char| c == '|' || c.is_whitespace());
            let error = take_bytes_at_char_boundary(error, MAX_ERROR_BYTES);
            let text = format!("{missing} - {error} | exit {code}");
            match self.open_mut(K::Route, &missing, None) {
                Some(known) if known.status == S::Failed => known.add_source(source.clone()),
                _ => {
                    let head = (K::Route, S::Failed, O::Host);
                    self.admit(head, (Some(&missing), Some(cwd)), &text, source, (0, 0));
                }
            }
            return;
        }
        let known_unavailable = self.candidates.iter().any(|c| {
            c.kind == K::Route
                && c.status == S::Failed
                && c.subject
                    .as_ref()
                    .is_some_and(|name| programs.contains(name))
        });
        // A successful command is history; it becomes a route only as the same-purpose
        // replacement of an unavailable program.
        if code == 0
            && !known_unavailable
            && command.split_whitespace().nth(1).is_some()
            && let Some(program) = programs.first()
        {
            let families: Vec<usize> = programs.iter().filter_map(|p| family(p)).collect();
            let replaces = self.candidates.iter().any(|c| {
                c.status == S::Failed
                    && c.subject
                        .as_deref()
                        .and_then(family)
                        .is_some_and(|f| families.contains(&f))
            });
            if replaces {
                for old in &mut self.candidates {
                    if old.kind == K::Route
                        && old.status == S::Active
                        && old.subject.as_deref() == Some(program)
                    {
                        old.status = S::Resolved;
                    }
                }
                // Store a bounded head of the replacing statement, never the whole script;
                // the source alias names the complete command.
                let failed_families: Vec<usize> = self
                    .candidates
                    .iter()
                    .filter(|c| c.status == S::Failed)
                    .filter_map(|c| c.subject.as_deref().and_then(family))
                    .collect();
                let statement = statements(&command)
                    .into_iter()
                    .find(|s| {
                        super::grammar::programs(s)
                            .first()
                            .and_then(|p| family(p))
                            .is_some_and(|f| failed_families.contains(&f))
                    })
                    .unwrap_or(&command);
                let head = command_head(statement, MAX_ROUTE_HEAD_BYTES);
                let text = head;
                let head = (K::Route, S::Active, O::Host);
                let subject = (Some(program.as_str()), Some(cwd));
                if let Some(id) = self.admit(head, subject, &text, source, (0, 0)) {
                    for failed in &mut self.candidates {
                        if failed.status == S::Failed
                            && failed.references() < MAX_REFERENCES
                            && failed
                                .subject
                                .as_deref()
                                .and_then(family)
                                .is_some_and(|f| families.contains(&f))
                        {
                            failed.resolved_by = Some(id);
                        }
                    }
                }
            }
        }
        self.observe_output(source, &command_key(&command), output);
    }

    /// Strict `label: value unit` lines in the inspected head of the output. The first value
    /// of a label from a command stays pinned; a later one supersedes only the previous
    /// later one.
    fn observe_output(&mut self, source: &SourceRef, command: &str, output: &str) {
        let label = if command.len() <= MAX_COMMAND_LABEL_BYTES {
            command.to_string()
        } else {
            format!(
                "{}...",
                take_bytes_at_char_boundary(command, MAX_COMMAND_LABEL_BYTES)
            )
        };
        let (mut offset, mut measured) = (0, 0);
        for line in output.split('\n') {
            let start = offset;
            offset += line.len() + 1;
            if offset > MAX_INSPECTED_BYTES {
                self.uninspected_items += 1;
                break;
            }
            let Some((name, measure)) = labeled_value(line) else {
                continue;
            };
            if measured == MAX_MEASUREMENTS {
                break;
            }
            measured += 1;
            let range = (start, start + line.len());
            // "The same measurement again" is the same label from the same command.
            let scope = format!("{command} :: {}", name.to_lowercase());
            let earlier = self.candidates.iter().any(|c| {
                c.kind == K::Value && c.origin == O::Host && c.scope.as_deref() == Some(&scope)
            });
            for old in &mut self.candidates {
                if old.status == S::Active && old.scope.as_deref() == Some(&scope) {
                    old.status = S::Resolved;
                }
            }
            let status = if earlier { S::Active } else { S::Baseline };
            if let Some(pinned) = self.candidates.iter_mut().find(|c| {
                c.kind == K::Value && c.status == status && c.subject.as_deref() == Some(&measure)
            }) {
                pinned.add_source(SourceRef {
                    start: range.0,
                    end: range.1,
                    ..source.clone()
                });
                continue;
            }
            let text = format!("{} [{label}]", line.trim());
            let head = (K::Value, status, O::Host);
            self.admit(head, (Some(&measure), Some(&scope)), &text, source, range);
        }
    }
}
