use std::path::PathBuf;

use codex_protocol::protocol::GitSha;
use pretty_assertions::assert_eq;

use super::*;

const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ZERO_SHA1: &str = "0000000000000000000000000000000000000000";

fn status(records: &[String]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|record| [record.as_bytes(), b"\0"].concat())
        .collect()
}

fn branch_headers() -> Vec<String> {
    vec![
        format!("# branch.oid {SHA1}"),
        "# branch.head main".to_string(),
    ]
}

fn parse_sha1_status(records: &[String]) -> Result<PorcelainStatus, ParseFailure> {
    parse_porcelain_v2_status(&status(records), GitObjectFormat::Sha1)
}

fn committed(worktree: StatusWorktree) -> PorcelainStatus {
    PorcelainStatus {
        oid: StatusBranchOid::Commit(GitSha::new(SHA1)),
        head: StatusBranchHead::Branch("main".to_string()),
        worktree,
    }
}

#[test]
fn clean_status_with_unknown_headers_is_clean() {
    let mut records = branch_headers();
    records.push("# branch.upstream origin/main".to_string());
    records.push("# branch.ab +0 -0".to_string());
    records.push("# stash 2".to_string());
    records.push("# some.future.extension with values".to_string());
    assert_eq!(
        parse_sha1_status(&records),
        Ok(committed(StatusWorktree::Clean))
    );
}

#[test]
fn every_change_kind_is_dirty() {
    let entries = [
        format!("1 .M N... 100644 100644 100644 {SHA1} {SHA1} a file.txt"),
        format!("1 A. S.M. 000000 160000 160000 {ZERO_SHA1} {SHA1} sub"),
        format!("u UU N... 100644 100644 100644 100644 {SHA1} {SHA1} {SHA1} both.txt"),
        "? new file.txt".to_string(),
    ];
    for entry in entries {
        let mut records = branch_headers();
        records.push(entry.clone());
        assert_eq!(
            parse_sha1_status(&records),
            Ok(committed(StatusWorktree::Dirty)),
            "{entry}"
        );
    }

    let mut records = branch_headers();
    records.push(format!(
        "2 R. N... 100644 100644 100644 {SHA1} {SHA1} R100 new name"
    ));
    records.push("old name".to_string());
    assert_eq!(
        parse_sha1_status(&records),
        Ok(committed(StatusWorktree::Dirty))
    );
}

#[test]
fn ignored_entries_do_not_make_the_worktree_dirty() {
    let mut records = branch_headers();
    records.push("! target/".to_string());
    assert_eq!(
        parse_sha1_status(&records),
        Ok(committed(StatusWorktree::Clean))
    );
}

#[test]
fn unborn_and_detached_headers_are_recognized() {
    let records = [
        "# branch.oid (initial)".to_string(),
        "# branch.head (detached)".to_string(),
    ];
    assert_eq!(
        parse_sha1_status(&records),
        Ok(PorcelainStatus {
            oid: StatusBranchOid::Initial,
            head: StatusBranchHead::Detached,
            worktree: StatusWorktree::Clean,
        })
    );
}

#[test]
fn sha256_status_requires_sha256_object_ids() {
    let records = [
        format!("# branch.oid {SHA256}"),
        "# branch.head main".to_string(),
        format!("1 M. N... 100644 100644 100644 {SHA256} {SHA256} file"),
    ];
    assert_eq!(
        parse_porcelain_v2_status(&status(&records), GitObjectFormat::Sha256),
        Ok(PorcelainStatus {
            oid: StatusBranchOid::Commit(GitSha::new(SHA256)),
            head: StatusBranchHead::Branch("main".to_string()),
            worktree: StatusWorktree::Dirty,
        })
    );
    assert_eq!(
        parse_sha1_status(&records),
        Err(ParseFailure::InvalidOutput)
    );
}

#[test]
fn malformed_or_incomplete_status_is_rejected() {
    let malformed = [
        vec![format!("# branch.oid {SHA1}")],
        vec!["# branch.head main".to_string()],
        [branch_headers(), branch_headers()].concat(),
        [branch_headers(), vec!["1 .M N... 100644 file".to_string()]].concat(),
        [
            branch_headers(),
            vec![format!("1 .Z N... 100644 100644 100644 {SHA1} {SHA1} file")],
        ]
        .concat(),
        [
            branch_headers(),
            vec![format!("1 .M X... 100644 100644 100644 {SHA1} {SHA1} file")],
        ]
        .concat(),
        [
            branch_headers(),
            vec![format!("1 .M N... 100648 100644 100644 {SHA1} {SHA1} file")],
        ]
        .concat(),
        [
            branch_headers(),
            vec![format!("1 .M N... 100644 100644 100644 {SHA1} {SHA1} ")],
        ]
        .concat(),
        [
            branch_headers(),
            vec![format!(
                "2 R. N... 100644 100644 100644 {SHA1} {SHA1} R100 renamed"
            )],
        ]
        .concat(),
        [branch_headers(), vec!["? ".to_string()]].concat(),
        [branch_headers(), vec!["x unknown".to_string()]].concat(),
        [branch_headers(), vec![String::new()]].concat(),
    ];
    for records in malformed {
        assert_eq!(
            parse_sha1_status(&records),
            Err(ParseFailure::InvalidOutput),
            "{records:?}"
        );
    }

    let mut truncated = status(&branch_headers());
    truncated.extend_from_slice(b"? partial");
    assert_eq!(
        parse_porcelain_v2_status(&truncated, GitObjectFormat::Sha1),
        Err(ParseFailure::InvalidOutput)
    );
    assert_eq!(
        parse_porcelain_v2_status(b"", GitObjectFormat::Sha1),
        Err(ParseFailure::InvalidOutput)
    );
}

#[test]
fn worktree_root_keeps_whitespace_and_strips_only_the_line_ending() {
    #[cfg(unix)]
    let root = "/tmp/ spaced\n\tdir ";
    #[cfg(windows)]
    let root = "C:/Users/me/ spaced dir ";
    assert_eq!(
        parse_worktree_root(format!("{root}\n").as_bytes()),
        Ok(PathBuf::from(root))
    );
    assert_eq!(
        parse_worktree_root(root.as_bytes()),
        Err(ParseFailure::InvalidOutput)
    );
    assert_eq!(parse_worktree_root(b"\n"), Err(ParseFailure::InvalidOutput));
    assert_eq!(
        parse_worktree_root(b"relative/path\n"),
        Err(ParseFailure::InvalidOutput)
    );
}

#[cfg(windows)]
#[test]
fn non_utf8_worktree_root_is_unsupported_on_windows() {
    assert_eq!(
        parse_worktree_root(b"C:/bad\xff\n"),
        Err(ParseFailure::UnsupportedPath)
    );
}

#[test]
fn object_format_accepts_only_known_formats() {
    assert_eq!(parse_object_format(b"sha1\n"), Ok(GitObjectFormat::Sha1));
    assert_eq!(
        parse_object_format(b"sha256\n"),
        Ok(GitObjectFormat::Sha256)
    );
    for output in [
        &b"sha1"[..],
        b"sha1 \n",
        b"SHA1\n",
        b"blake3\n",
        b"--show-object-format=storage\n",
    ] {
        assert_eq!(
            parse_object_format(output),
            Err(ParseFailure::InvalidOutput),
            "{output:?}"
        );
    }
}

#[test]
fn object_ids_must_match_the_storage_format() {
    assert_eq!(
        parse_oid_line(format!("{SHA1}\n").as_bytes(), GitObjectFormat::Sha1),
        Ok(GitSha::new(SHA1))
    );
    assert_eq!(
        parse_oid_line(format!("{SHA256}\n").as_bytes(), GitObjectFormat::Sha256),
        Ok(GitSha::new(SHA256))
    );
    let invalid = [
        (format!("{SHA1}\n"), GitObjectFormat::Sha256),
        (format!("{SHA256}\n"), GitObjectFormat::Sha1),
        (format!("{}\n", SHA1.to_uppercase()), GitObjectFormat::Sha1),
        (format!("{}g\n", &SHA1[..39]), GitObjectFormat::Sha1),
        (SHA1.to_string(), GitObjectFormat::Sha1),
    ];
    for (output, format) in invalid {
        assert_eq!(
            parse_oid_line(output.as_bytes(), format),
            Err(ParseFailure::InvalidOutput),
            "{output:?}"
        );
    }
}

#[test]
fn symbolic_ref_requires_one_ref_line() {
    assert_eq!(
        parse_symbolic_ref(b"refs/heads/feature/x\n"),
        Ok("refs/heads/feature/x".to_string())
    );
    for output in [
        &b"refs/heads/main"[..],
        b"\n",
        b"HEAD\n",
        b"refs/heads/a\nrefs/heads/b\n",
        b"refs/heads/\xff\n",
    ] {
        assert_eq!(
            parse_symbolic_ref(output),
            Err(ParseFailure::InvalidOutput),
            "{output:?}"
        );
    }
}
