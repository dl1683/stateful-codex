//! Reduction of user and assistant text by structure only: assistant text yields marked next
//! actions and `file:line` locators, and a user message naming a task file attributes that
//! file's change. No word list classifies meaning, and nothing here calls a model.

use super::CandidateKind as K;
use super::MAX_REFERENCES;
use super::Observation;
use super::Origin as O;
use super::Projection;
use super::SourceRef;
use super::Status as S;
use super::grammar::NEXT_MARKERS;
use super::grammar::clauses;
use super::grammar::locators;
use super::grammar::marker_end;
use super::grammar::mentions;
use super::grammar::plain;

const MAX_LOCATORS: usize = 16;
/// User messages kept to attribute file changes.
const MAX_ATTRIBUTIONS: usize = 8;

impl Projection {
    pub(crate) fn observe(&mut self, observation: Observation) {
        match observation {
            Observation::UserMessage { source, text } => self.observe_user(&source, &text),
            Observation::AssistantMessage { source, text } => {
                self.observe_assistant(&source, &text);
            }
            Observation::StructuredNext { source, text } => {
                for old in &mut self.candidates {
                    if old.kind == K::Next && old.origin == O::Structured && old.status == S::Active
                    {
                        old.status = S::Resolved;
                    }
                }
                let head = (K::Next, S::Active, O::Structured);
                self.admit(head, (None, None), &text, &source, (0, text.len()));
            }
            Observation::PlanStepCompleted { text } => {
                let done = text.trim().to_lowercase();
                for old in &mut self.candidates {
                    if old.kind == K::Next
                        && old.text.as_deref().map(|t| t.trim().to_lowercase())
                            == Some(done.clone())
                    {
                        old.status = S::Resolved;
                    }
                }
            }
            Observation::Command {
                source,
                argv,
                cwd,
                exit_code,
                output,
            } => self.observe_command(&source, &argv, &cwd, exit_code, &output),
            Observation::AppliedFileChange { path } => self.touch(&path),
            Observation::FileWitness { source, witness } => {
                self.touch(&witness.path);
                let head = (K::Change, S::Active, O::Host);
                let subject = (Some(witness.path.as_str()), None);
                self.admit(head, subject, &witness.summary(), &source, (0, 0));
                self.link_attributions();
            }
        }
    }

    /// A user message is not a directive here (directives come from the root packet). One
    /// that names a task-touched file is kept, unrendered, to attribute that file's change.
    fn observe_user(&mut self, source: &SourceRef, text: &str) {
        let names_file = self
            .touched_files
            .iter()
            .any(|path| mentions(Some(text), path));
        if !names_file {
            return;
        }
        let head = (K::Attribution, S::Active, O::User);
        if self
            .admit(head, (None, None), text, source, (0, text.len()))
            .is_none()
        {
            return;
        }
        let open: Vec<usize> = (0..self.candidates.len())
            .filter(|&i| {
                let c = &self.candidates[i];
                c.kind == K::Attribution && c.status == S::Active
            })
            .collect();
        for &i in open
            .iter()
            .take(open.len().saturating_sub(MAX_ATTRIBUTIONS))
        {
            self.candidates[i].status = S::Resolved;
        }
        self.link_attributions();
    }

    fn observe_assistant(&mut self, source: &SourceRef, text: &str) {
        let mut found = 0;
        for (start, end) in clauses(text, &mut self.uninspected_items) {
            let clause = plain(&text[start..end]);
            let lower = clause.to_lowercase();
            if let Some(marker) = NEXT_MARKERS.iter().find_map(|m| marker_end(&lower, m)) {
                let body = clause[marker..].trim_start();
                // A question is a proposal, never authorization.
                let status = if body.contains('?') {
                    S::Proposed
                } else {
                    S::Active
                };
                if status == S::Active {
                    self.supersede_open_next();
                }
                let head = (K::Next, status, O::Assistant);
                self.admit(head, (None, None), body, source, (start, end));
                continue;
            }
            for locator in locators(&clause) {
                if found == MAX_LOCATORS {
                    break;
                }
                found += 1;
                let span = SourceRef {
                    start,
                    end,
                    ..source.clone()
                };
                // A restated locator adds a source to the record that already holds it.
                if let Some(known) = self.candidates.iter_mut().find(|c| {
                    c.kind == K::Value
                        && c.status != S::Resolved
                        && c.subject.as_deref() == Some(&locator)
                }) {
                    if known.references() < MAX_REFERENCES {
                        known.add_source(span);
                    }
                    continue;
                }
                let head = (K::Value, S::Active, O::Assistant);
                self.admit(head, (Some(&locator), None), &clause, source, (start, end));
            }
        }
    }
}
