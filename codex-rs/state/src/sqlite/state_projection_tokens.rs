//! Borrowed, bounded lexemes for opaque-schema projection, not SQL admission.
//! Unlike the pinned-schema lexer, unrelated identifiers may be Unicode and
//! numeric literals may contain SQLite separators. Raw spellings never merge.

use std::borrow::Cow;

use anyhow::ensure;

pub(super) struct Limits {
    pub bytes: usize,
    pub tokens: usize,
    pub depth: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Identifier,
    Literal,
    Symbol,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Token<'a> {
    pub raw: &'a str,
    pub kind: Kind,
}

impl<'a> Token<'a> {
    /// Decode lexical identifiers only, using SQLite's ASCII-only name matching.
    /// A structural parser must separately handle SQLite's context-dependent
    /// treatment of single-quoted tokens in identifier-required positions.
    pub(super) fn identifier(self) -> Option<Cow<'a, str>> {
        if self.kind != Kind::Identifier {
            return None;
        }
        match self.raw.as_bytes()[0] {
            b'"' => Some(Cow::Owned(
                self.raw[1..self.raw.len() - 1].replace("\"\"", "\""),
            )),
            b'`' => Some(Cow::Owned(
                self.raw[1..self.raw.len() - 1].replace("``", "`"),
            )),
            b'[' => Some(Cow::Borrowed(&self.raw[1..self.raw.len() - 1])),
            _ => Some(Cow::Borrowed(self.raw)),
        }
    }
}

fn identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 128
}

fn digits(bytes: &[u8], at: &mut usize, digit: fn(&u8) -> bool) -> anyhow::Result<usize> {
    let mut count = 0;
    while let Some(byte) = bytes.get(*at) {
        if digit(byte) {
            count += 1;
            *at += 1;
        } else if *byte == b'_' {
            ensure!(
                count > 0 && bytes.get(*at + 1).is_some_and(digit),
                "invalid numeric separator"
            );
            *at += 1;
        } else {
            break;
        }
    }
    Ok(count)
}

/// Byte limits must also be checked by the catalog reader before fetching SQL.
/// These caller-supplied component bounds are not a production admission policy.
pub(super) fn scan<'a>(sql: &'a str, limits: &Limits) -> anyhow::Result<Vec<Token<'a>>> {
    ensure!(
        sql.len() <= limits.bytes,
        "projection SQL byte limit exceeded"
    );
    let bytes = sql.as_bytes();
    ensure!(!bytes.contains(&0), "NUL in projection SQL");
    let mut result = Vec::with_capacity(sql.len().min(limits.tokens));
    let mut at = 0;
    let mut depth = 0_usize;
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 12) {
            at += 1;
            continue;
        }
        // SQLite treats a UTF-8 BOM at a token boundary as whitespace, but it
        // remains identifier content when reached inside an identifier token.
        if bytes.get(at..at + 3) == Some(b"\xEF\xBB\xBF") {
            at += 3;
            continue;
        }
        if bytes.get(at..at + 2) == Some(b"--") {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes.get(at..at + 2) == Some(b"/*") {
            at += 2;
            while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                at += 1;
            }
            ensure!(at + 1 < bytes.len(), "unterminated projection comment");
            at += 2;
            continue;
        }
        ensure!(
            result.len() < limits.tokens,
            "projection SQL token limit exceeded"
        );
        let blob = matches!(byte, b'x' | b'X') && bytes.get(at + 1) == Some(&b'\'');
        let kind = if blob || matches!(byte, b'\'' | b'"' | b'`' | b'[') {
            if blob {
                at += 1;
            }
            let quote = bytes[at];
            let end = if quote == b'[' { b']' } else { quote };
            at += 1;
            loop {
                ensure!(at < bytes.len(), "unterminated projection quote");
                let next = bytes[at];
                at += 1;
                if next == end {
                    if end != b']' && bytes.get(at) == Some(&end) {
                        at += 1;
                    } else {
                        break;
                    }
                }
            }
            if blob {
                let contents = &bytes[start + 2..at - 1];
                ensure!(
                    contents.len().is_multiple_of(2) && contents.iter().all(u8::is_ascii_hexdigit),
                    "invalid projection blob literal"
                );
            }
            if quote == b'\'' {
                Kind::Literal
            } else {
                Kind::Identifier
            }
        } else if identifier_start(byte) {
            at += 1;
            while bytes.get(at).is_some_and(|byte| {
                identifier_start(*byte) || byte.is_ascii_digit() || *byte == b'$'
            }) {
                at += 1;
            }
            Kind::Identifier
        } else if byte.is_ascii_digit()
            || (byte == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit))
        {
            if byte == b'0'
                && bytes
                    .get(at + 1)
                    .is_some_and(|byte| matches!(byte, b'x' | b'X'))
            {
                at += 2;
                ensure!(
                    digits(bytes, &mut at, u8::is_ascii_hexdigit)? > 0,
                    "empty projection hex literal"
                );
            } else {
                digits(bytes, &mut at, u8::is_ascii_digit)?;
                if bytes.get(at) == Some(&b'.') {
                    at += 1;
                    digits(bytes, &mut at, u8::is_ascii_digit)?;
                }
                if bytes
                    .get(at)
                    .is_some_and(|byte| matches!(byte, b'e' | b'E'))
                {
                    at += 1;
                    if bytes
                        .get(at)
                        .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                    {
                        at += 1;
                    }
                    ensure!(
                        digits(bytes, &mut at, u8::is_ascii_digit)? > 0,
                        "empty projection exponent"
                    );
                }
            }
            ensure!(
                !bytes
                    .get(at)
                    .is_some_and(|byte| identifier_start(*byte) || *byte == b'$'),
                "identifier adjacent to numeric literal"
            );
            Kind::Literal
        } else {
            ensure!(
                byte >= 32 && byte != 127,
                "unsupported projection control byte"
            );
            at += 1;
            if byte == b'(' {
                depth += 1;
                ensure!(depth <= limits.depth, "projection nesting limit exceeded");
            } else if byte == b')' {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| anyhow::anyhow!("unbalanced projection parentheses"))?;
            } else if bytes.get(start..start + 3) == Some(b"->>") {
                at += 2;
            } else if matches!(
                bytes.get(start..start + 2),
                Some(b"||" | b"->" | b"<<" | b">>" | b"<=" | b">=" | b"==" | b"!=" | b"<>")
            ) {
                at += 1;
            }
            Kind::Symbol
        };
        result.push(Token {
            raw: &sql[start..at],
            kind,
        });
    }
    ensure!(depth == 0, "unbalanced projection parentheses");
    Ok(result)
}

#[cfg(test)]
#[path = "state_projection_tokens_tests.rs"]
mod tests;
