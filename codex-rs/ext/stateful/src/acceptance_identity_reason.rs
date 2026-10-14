//! Closed identity failures and bounded, display-only explanations. Paths stay exact in identity.
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Bound {
    Visits,
    Entries,
    Directories,
    Pending,
    RetainedBytes,
    PathLength,
    Bytes,
    GitOutput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EntryKind {
    EscapingLink,
    DirectoryLink,
    Special,
    Unreadable,
    Reparse,
    NotContained,
    Submodule,
    EmbeddedRepository,
    InexactName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum IdentityGap {
    Roots,
    Git,
    Bound(Bound),
    Entry { path: PathBuf, kind: EntryKind },
    Slots,
    Worker,
    Time,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Side {
    Start,
    End,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum IdentityChange {
    Gap(Side, IdentityGap),
    Roots,
    Repository,
    Path(PathBuf),
}

/// The observer's two refusal sources, without changing its outcome matrix.
pub(crate) enum IdentityRefusal {
    Overlap,
    Change(IdentityChange),
}

impl IdentityRefusal {
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Overlap => "another workspace change was observed while the check ran, so its effects overlap other evidence".to_string(),
            Self::Change(change) => change.render(),
        }
    }
}

impl IdentityChange {
    pub(crate) fn render(&self) -> String {
        match self {
            Self::Gap(side, gap) => {
                let side = match side { Side::Start => "start", Side::End => "end" };
                let reason = match gap {
                    IdentityGap::Roots => "project roots could not be identified".to_string(),
                    IdentityGap::Git => "git state could not be read".to_string(),
                    IdentityGap::Bound(bound) => format!("identity bound exceeded: {bound:?}"),
                    IdentityGap::Entry { path, kind } => format!("identity entry {}: {kind:?}", display_path(path)),
                    IdentityGap::Slots => "no identity reader slot is free".to_string(),
                    IdentityGap::Worker => "identity worker failed".to_string(),
                    IdentityGap::Time => "identity deadline expired".to_string(),
                };
                format!("workspace identity unavailable at {side}: {reason}")
            }
            Self::Roots => "the check changed the project roots".to_string(),
            Self::Repository => "the check changed repository state".to_string(),
            Self::Path(path) => format!("the check changed a file no criterion pins: {}", display_path(path)),
        }
    }
}

fn display_path(path: &Path) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if text.len() > 200 {
        let mut end = 200;
        while !text.is_char_boundary(end) { end -= 1; }
        text.truncate(end);
    }
    text
}

#[cfg(test)]
#[path = "acceptance_identity_reason_tests.rs"]
mod tests;
