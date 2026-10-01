//! Byte-level parsers for the Git output used by repository observation.
//!
//! Each parser accounts for the whole output and rejects anything it cannot,
//! so malformed or truncated output is never mistaken for a known state.

use std::path::PathBuf;

use codex_protocol::protocol::GitSha;

/// Why Git output could not be turned into an observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParseFailure {
    /// The output does not match Git's documented format.
    InvalidOutput,
    /// A well-formed path that this platform cannot represent.
    UnsupportedPath,
}

/// The repository's storage object format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GitObjectFormat {
    Sha1,
    Sha256,
}

/// The `# branch.oid` status header.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum StatusBranchOid {
    Commit(GitSha),
    /// HEAD names a branch that has no commit yet.
    Initial,
}

/// The `# branch.head` status header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum StatusBranchHead {
    /// The branch name, without `refs/heads/` when HEAD points under it.
    Branch(String),
    Detached,
}

/// Whether status reported any tracked change or untracked file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StatusWorktree {
    Clean,
    Dirty,
}

/// A complete `status --porcelain=v2 --branch -z` response.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PorcelainStatus {
    pub(super) oid: StatusBranchOid,
    pub(super) head: StatusBranchHead,
    pub(super) worktree: StatusWorktree,
}

/// Removes the single terminating newline; no other whitespace is touched.
fn single_line(output: &[u8]) -> Result<&[u8], ParseFailure> {
    match output.strip_suffix(b"\n") {
        Some(line) if !line.is_empty() => Ok(line),
        Some(_) | None => Err(ParseFailure::InvalidOutput),
    }
}

/// Parses `rev-parse --show-toplevel` output, preserving embedded whitespace.
pub(super) fn parse_worktree_root(output: &[u8]) -> Result<PathBuf, ParseFailure> {
    let line = single_line(output)?;
    if line.contains(&b'\0') {
        return Err(ParseFailure::UnsupportedPath);
    }
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(line))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(std::str::from_utf8(line).map_err(|_| ParseFailure::UnsupportedPath)?);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(ParseFailure::InvalidOutput)
    }
}

/// Parses `rev-parse --show-object-format=storage` output.
pub(super) fn parse_object_format(output: &[u8]) -> Result<GitObjectFormat, ParseFailure> {
    match single_line(output)? {
        b"sha1" => Ok(GitObjectFormat::Sha1),
        b"sha256" => Ok(GitObjectFormat::Sha256),
        _ => Err(ParseFailure::InvalidOutput),
    }
}

/// Parses a full object ID printed on its own line.
pub(super) fn parse_oid_line(
    output: &[u8],
    format: GitObjectFormat,
) -> Result<GitSha, ParseFailure> {
    parse_oid(single_line(output)?, format)
}

fn parse_oid(oid: &[u8], format: GitObjectFormat) -> Result<GitSha, ParseFailure> {
    let is_lower_hex = |byte: &u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte);
    let hex_len = match format {
        GitObjectFormat::Sha1 => 40,
        GitObjectFormat::Sha256 => 64,
    };
    if oid.len() != hex_len || !oid.iter().all(is_lower_hex) {
        return Err(ParseFailure::InvalidOutput);
    }
    let oid = std::str::from_utf8(oid).map_err(|_| ParseFailure::InvalidOutput)?;
    Ok(GitSha::new(oid))
}

/// Parses `symbolic-ref -q HEAD` output from a successful run.
pub(super) fn parse_symbolic_ref(output: &[u8]) -> Result<String, ParseFailure> {
    let line = single_line(output)?;
    if !line.starts_with(b"refs/") {
        return Err(ParseFailure::InvalidOutput);
    }
    parse_ref_name(line)
}

/// Accepts a reference name that `git check-ref-format --allow-onelevel`
/// would accept, as UTF-8.
fn parse_ref_name(name: &[u8]) -> Result<String, ParseFailure> {
    let forbidden = |byte: &u8| byte.is_ascii_control() || b" ~^:?*[\\".contains(byte);
    let valid_component = |component: &[u8]| {
        !component.is_empty() && !component.starts_with(b".") && !component.ends_with(b".lock")
    };
    let valid = name != b"@"
        && !name.ends_with(b".")
        && !name.windows(2).any(|pair| pair == b".." || pair == b"@{")
        && !name.iter().any(forbidden)
        && name.split(|byte| *byte == b'/').all(valid_component);
    if !valid {
        return Err(ParseFailure::InvalidOutput);
    }
    String::from_utf8(name.to_vec()).map_err(|_| ParseFailure::InvalidOutput)
}

/// Parses `status --porcelain=v2 --branch -z` output.
///
/// Every record must be NUL-terminated. Unknown headers are ignored; any
/// unrecognized entry, malformed required field, or missing branch header
/// rejects the whole response.
pub(super) fn parse_porcelain_v2_status(
    output: &[u8],
    format: GitObjectFormat,
) -> Result<PorcelainStatus, ParseFailure> {
    let complete = output
        .strip_suffix(b"\0")
        .ok_or(ParseFailure::InvalidOutput)?;
    let mut records = complete.split(|byte| *byte == b'\0');
    let mut oid = None;
    let mut head = None;
    let mut worktree = StatusWorktree::Clean;
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix(b"# ") {
            parse_header(header, format, &mut oid, &mut head)?;
            continue;
        }
        let (kind, rest) = record
            .split_at_checked(2)
            .ok_or(ParseFailure::InvalidOutput)?;
        match kind {
            b"1 " => parse_change_fields(rest, format, ChangeEntry::Ordinary)?,
            b"2 " => {
                parse_change_fields(rest, format, ChangeEntry::RenameOrCopy)?;
                // The original path follows as its own NUL-terminated record.
                non_empty(records.next().unwrap_or_default()).map(drop)?;
            }
            b"u " => parse_change_fields(rest, format, ChangeEntry::Unmerged)?,
            b"? " => non_empty(rest).map(drop)?,
            // Ignored files are only listed on request and are not changes.
            b"! " => {
                non_empty(rest)?;
                continue;
            }
            _ => return Err(ParseFailure::InvalidOutput),
        }
        worktree = StatusWorktree::Dirty;
    }
    Ok(PorcelainStatus {
        oid: oid.ok_or(ParseFailure::InvalidOutput)?,
        head: head.ok_or(ParseFailure::InvalidOutput)?,
        worktree,
    })
}

fn parse_header(
    header: &[u8],
    format: GitObjectFormat,
    oid: &mut Option<StatusBranchOid>,
    head: &mut Option<StatusBranchHead>,
) -> Result<(), ParseFailure> {
    let (key, value) = match header.iter().position(|byte| *byte == b' ') {
        Some(space) => (&header[..space], &header[space + 1..]),
        None => (header, &header[header.len()..]),
    };
    let parsed_oid = || match value {
        b"(initial)" => Ok(StatusBranchOid::Initial),
        _ => parse_oid(value, format).map(StatusBranchOid::Commit),
    };
    // Git prints `(unknown)` when it could not resolve the branch.
    let parsed_head = || match value {
        b"(detached)" => Ok(StatusBranchHead::Detached),
        b"(unknown)" => Err(ParseFailure::InvalidOutput),
        _ => parse_ref_name(value).map(StatusBranchHead::Branch),
    };
    match key {
        b"branch.oid" if oid.is_none() => *oid = Some(parsed_oid()?),
        b"branch.head" if head.is_none() => *head = Some(parsed_head()?),
        b"branch.oid" | b"branch.head" => return Err(ParseFailure::InvalidOutput),
        _ => {}
    }
    Ok(())
}

/// The entry kinds that carry modes and object IDs.
#[derive(Clone, Copy)]
enum ChangeEntry {
    Ordinary,
    RenameOrCopy,
    Unmerged,
}

fn parse_change_fields(
    rest: &[u8],
    format: GitObjectFormat,
    entry: ChangeEntry,
) -> Result<(), ParseFailure> {
    let (modes, oids, score) = match entry {
        ChangeEntry::Ordinary => (3, 2, 0),
        ChangeEntry::RenameOrCopy => (3, 2, 1),
        ChangeEntry::Unmerged => (4, 3, 0),
    };
    // XY, submodule state, modes, object IDs, optional score, then the path,
    // which may itself contain spaces.
    let field_count = 2 + modes + oids + score + 1;
    let fields: Vec<&[u8]> = rest.splitn(field_count, |byte| *byte == b' ').collect();
    if fields.len() != field_count {
        return Err(ParseFailure::InvalidOutput);
    }
    let valid = valid_xy(fields[0], entry)
        && valid_submodule(fields[1])
        && fields[2..2 + modes].iter().all(|mode| valid_mode(mode))
        && fields[2 + modes..2 + modes + oids]
            .iter()
            .all(|oid| parse_oid(oid, format).is_ok())
        && fields[2 + modes + oids..field_count - 1]
            .iter()
            .all(|score| valid_score(score, fields[0]));
    if !valid {
        return Err(ParseFailure::InvalidOutput);
    }
    non_empty(fields[field_count - 1]).map(drop)
}

fn non_empty(field: &[u8]) -> Result<&[u8], ParseFailure> {
    if field.is_empty() {
        Err(ParseFailure::InvalidOutput)
    } else {
        Ok(field)
    }
}

fn valid_xy(xy: &[u8], entry: ChangeEntry) -> bool {
    match (entry, xy) {
        (ChangeEntry::Ordinary, [index, worktree]) => {
            b".MTAD".contains(index) && b".MTAD".contains(worktree) && xy != b".."
        }
        (ChangeEntry::RenameOrCopy, [index, worktree]) => {
            b".MTADRC".contains(index)
                && b".MTADRC".contains(worktree)
                && (b"RC".contains(index) || b"RC".contains(worktree))
        }
        // The seven unmerged combinations documented for porcelain status.
        (ChangeEntry::Unmerged, _) => {
            matches!(xy, b"DD" | b"AU" | b"UD" | b"UA" | b"DU" | b"AA" | b"UU")
        }
        (ChangeEntry::Ordinary | ChangeEntry::RenameOrCopy, _) => false,
    }
}

fn valid_submodule(state: &[u8]) -> bool {
    match state {
        b"N..." => true,
        [b'S', commit, tracked, untracked] => {
            matches!(commit, b'C' | b'.')
                && matches!(tracked, b'M' | b'.')
                && matches!(untracked, b'U' | b'.')
        }
        _ => false,
    }
}

fn valid_mode(mode: &[u8]) -> bool {
    mode.len() == 6 && mode.iter().all(|digit| (b'0'..=b'7').contains(digit))
}

/// A rename or copy score: a letter from XY, then a percentage up to 100.
fn valid_score(score: &[u8], xy: &[u8]) -> bool {
    match score {
        [letter @ (b'R' | b'C'), digits @ ..] => {
            xy.contains(letter)
                && (1..=3).contains(&digits.len())
                && digits.iter().all(u8::is_ascii_digit)
                && std::str::from_utf8(digits)
                    .ok()
                    .and_then(|digits| digits.parse::<u8>().ok())
                    .is_some_and(|percent| percent <= 100)
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
