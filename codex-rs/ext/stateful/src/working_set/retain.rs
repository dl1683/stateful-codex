//! Semantic retention: admission with complete provenance, duplicate coalescing, bounded
//! eviction and supersession. Retention is by meaning, never a recency window.

use codex_utils_string::take_bytes_at_char_boundary;

use super::Candidate;
use super::CandidateKind;
use super::CandidateKind as K;
use super::MAX_CANDIDATE_TEXT_BYTES;
use super::MAX_CANDIDATES;
use super::MAX_REFERENCES;
use super::Origin;
use super::Origin as O;
use super::Projection;
use super::SourceRef;
use super::Status;
use super::Status as S;
use super::grammar::mentions;

/// Active assistant commitments kept unresolved: the latest and one suspended across a detour.
const MAX_OPEN_NEXT: usize = 2;

/// What a record is: kind, status and origin.
pub(super) type Head = (CandidateKind, Status, Origin);

impl Projection {
    /// Stores one whole record with complete provenance. Replaying an already reduced
    /// source span is a no-op and exact duplicates coalesce references; incomplete
    /// provenance or saturation is counted as debt. Saturation evicts the oldest resolved
    /// record only, never an unresolved one.
    pub(super) fn admit(
        &mut self,
        (kind, status, origin): Head,
        (subject, scope): (Option<&str>, Option<&str>),
        text: &str,
        source: &SourceRef,
        (start, end): (usize, usize),
    ) -> Option<u64> {
        let text = text.trim();
        let source = SourceRef {
            start,
            end,
            ..source.clone()
        };
        if !source.is_complete(kind == K::Route) {
            self.capacity_debt += 1;
            return None;
        }
        if let Some(seen) = self
            .candidates
            .iter()
            .find(|c| c.kind == kind && c.sources.contains(&source))
        {
            return Some(seen.id);
        }
        if let Some(same) = self.candidates.iter_mut().find(|c| {
            c.kind == kind
                && c.origin == origin
                && c.status == status
                && c.text.as_deref() == Some(text)
        }) {
            same.add_source(source);
            return Some(same.id);
        }
        let oversized = text.len() > MAX_CANDIDATE_TEXT_BYTES;
        // A long next action or user message is kept as an explicitly partial quotation; any other
        // oversized record keeps only its locator.
        let (kept, partial) = match (oversized, kind) {
            (false, _) => (Some(text), false),
            (true, K::Next | K::Attribution) => (
                Some(take_bytes_at_char_boundary(text, MAX_CANDIDATE_TEXT_BYTES)),
                true,
            ),
            (true, _) => (None, false),
        };
        if kept.is_none() {
            self.capacity_debt += 1;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.candidates.push(Candidate {
            id,
            kind,
            status,
            origin,
            subject: subject.map(str::to_string),
            scope: scope.map(str::to_string),
            text: kept.map(str::to_string),
            partial,
            sources: vec![source],
            resolved_by: None,
            linked: None,
        });
        while self.validated_payload().is_err() {
            // A resolved record another record depends on (a replacement route, an
            // attribution) is not evicted while that record is open.
            let referenced: Vec<u64> = self
                .candidates
                .iter()
                .filter(|c| c.status != S::Resolved)
                .flat_map(|c| c.resolved_by.into_iter().chain(c.linked))
                .collect();
            let evictable = |c: &Candidate| c.status == S::Resolved && !referenced.contains(&c.id);
            match self.candidates.iter().position(evictable) {
                Some(index) => {
                    self.candidates.remove(index);
                }
                None => {
                    self.candidates.pop();
                    self.capacity_debt += 1;
                    return None;
                }
            }
        }
        Some(id)
    }

    /// The unresolved record of `kind` with this subject and scope, if any.
    pub(super) fn open_mut(
        &mut self,
        kind: CandidateKind,
        subject: &str,
        scope: Option<&str>,
    ) -> Option<&mut Candidate> {
        self.candidates.iter_mut().find(|c| {
            c.kind == kind
                && c.status != S::Resolved
                && c.subject.as_deref() == Some(subject)
                && (scope.is_none() || c.scope.as_deref() == scope)
        })
    }

    /// Keeps the latest active assistant commitment and one suspended across a detour.
    pub(super) fn supersede_open_next(&mut self) {
        let open: Vec<usize> = (0..self.candidates.len())
            .filter(|&i| {
                let c = &self.candidates[i];
                c.kind == K::Next && c.origin == O::Assistant && c.status == S::Active
            })
            .collect();
        for &i in open
            .iter()
            .take((open.len() + 1).saturating_sub(MAX_OPEN_NEXT))
        {
            self.candidates[i].status = S::Resolved;
        }
    }

    pub(super) fn touch(&mut self, path: &str) {
        if !self.touched_files.iter().any(|p| p == path) {
            self.touched_files.push(path.to_string());
        }
        if self.touched_files.len() > MAX_CANDIDATES {
            self.touched_files.remove(0);
        }
    }

    /// Links unattributed change witnesses to the newest user statement naming their file.
    pub(super) fn link_attributions(&mut self) {
        let users: Vec<(u64, String)> = self
            .candidates
            .iter()
            .filter(|c| c.kind == K::Attribution)
            .filter_map(|c| Some((c.id, c.text.clone()?)))
            .collect();
        for change in &mut self.candidates {
            if change.kind != K::Change || change.linked.is_some() {
                continue;
            }
            let path = change.subject.clone().unwrap_or_default();
            let user = users
                .iter()
                .rev()
                .find(|(_, text)| mentions(Some(text), &path));
            if let Some((id, _)) = user
                && change.references() < MAX_REFERENCES
            {
                change.linked = Some(*id);
            }
        }
    }
}

impl Candidate {
    pub(super) fn add_source(&mut self, source: SourceRef) {
        if self.references() < MAX_REFERENCES && !self.sources.contains(&source) {
            self.sources.push(source);
        }
    }
}
