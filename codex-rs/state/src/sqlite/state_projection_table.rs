//! Ordinary-table entry extraction for a later complete structural inspector.
//! A parsed table is not proof of column properties, constraints, or admission.

use std::borrow::Cow;
use std::ops::Range;

use anyhow::bail;
use anyhow::ensure;

use super::projection_tokens::Kind;
use super::projection_tokens::Limits;
use super::projection_tokens::Token;
use super::projection_tokens::{self};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RowStorage {
    Rowid,
    WithoutRowid,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct TableParts<'a> {
    tokens: Vec<Token<'a>>,
    entries: Vec<Range<usize>>,
    pub storage: RowStorage,
}

fn keyword(token: &Token<'_>, expected: &str) -> bool {
    token.kind == Kind::Identifier && token.raw.eq_ignore_ascii_case(expected)
}

/// This is used only where CREATE TABLE grammar requires a name. SQLite also
/// permits single-quoted names there; expressions must not use this decoder.
fn schema_name<'a>(token: Token<'a>) -> Option<Cow<'a, str>> {
    token.identifier().or_else(|| {
        if token.kind != Kind::Literal {
            return None;
        }
        token
            .raw
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
            .map(|value| Cow::Owned(value.replace("''", "'")))
    })
}

impl<'a> TableParts<'a> {
    pub(super) fn entries(&self) -> impl Iterator<Item = &[Token<'a>]> {
        self.entries.iter().map(|range| &self.tokens[range.clone()])
    }

    /// Locate by declared name, never ordinal. Other entries remain unqualified:
    /// CHECK/FK/index/trigger references still need the complete inspector.
    pub(super) fn column(&self, expected: &str) -> anyhow::Result<Option<&[Token<'a>]>> {
        let mut found = None;
        for entry in self.entries() {
            if schema_name(entry[0]).is_some_and(|name| name.eq_ignore_ascii_case(expected)) {
                ensure!(found.is_none(), "duplicate projected column declaration");
                found = Some(entry);
            }
        }
        Ok(found)
    }
}

pub(super) fn split<'a>(
    sql: &'a str,
    expected_table: &str,
    limits: &Limits,
) -> anyhow::Result<TableParts<'a>> {
    let tokens = projection_tokens::scan(sql, limits)?;
    ensure!(
        tokens.len() >= 5 && keyword(&tokens[0], "CREATE") && keyword(&tokens[1], "TABLE"),
        "projection requires an ordinary CREATE TABLE"
    );
    let name_at = if keyword(&tokens[2], "IF") {
        ensure!(
            tokens.len() >= 8 && keyword(&tokens[3], "NOT") && keyword(&tokens[4], "EXISTS"),
            "unsupported projected table header"
        );
        5
    } else {
        2
    };
    ensure!(
        schema_name(tokens[name_at]).is_some_and(|name| name.eq_ignore_ascii_case(expected_table)),
        "projected table name mismatch"
    );
    let open = name_at + 1;
    ensure!(
        tokens[open].kind == Kind::Symbol && tokens[open].raw == "(",
        "projected table body missing"
    );
    let mut depth = 1_usize;
    let mut start = open + 1;
    let mut entries = Vec::new();
    let mut close = None;
    for (at, token) in tokens.iter().enumerate().skip(start) {
        if token.kind != Kind::Symbol {
            continue;
        }
        match token.raw {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    ensure!(start < at, "empty projected table entry");
                    entries.push(start..at);
                    close = Some(at);
                    break;
                }
            }
            "," if depth == 1 => {
                ensure!(start < at, "empty projected table entry");
                entries.push(start..at);
                start = at + 1;
            }
            _ => {}
        }
    }
    let close = close.ok_or_else(|| anyhow::anyhow!("projected table body unclosed"))?;
    let mut options = &tokens[close + 1..];
    if options
        .last()
        .is_some_and(|token| token.kind == Kind::Symbol && token.raw == ";")
    {
        options = &options[..options.len() - 1];
    }
    let storage = match options {
        [] => RowStorage::Rowid,
        [without, rowid] if keyword(without, "WITHOUT") && keyword(rowid, "ROWID") => {
            RowStorage::WithoutRowid
        }
        _ => bail!("unsupported projected table options, including STRICT"),
    };
    Ok(TableParts {
        tokens,
        entries,
        storage,
    })
}

#[cfg(test)]
#[path = "state_projection_table_tests.rs"]
mod tests;
