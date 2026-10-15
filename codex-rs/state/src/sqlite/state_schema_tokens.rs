//! Deliberately narrow, byte-preserving lexer for pinned SQLite schema SQL.
//! Unsupported unquoted characters refuse; Unicode whitespace is never erased.

use anyhow::bail;
use anyhow::ensure;

pub(in crate::sqlite) fn tokens(sql: &str) -> anyhow::Result<Vec<String>> {
    let bytes = sql.as_bytes();
    ensure!(!bytes.contains(&0), "NUL in schema SQL");
    let mut at = 0;
    let mut result = Vec::new();
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        at += 1;
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 12) {
            continue;
        }
        if byte == b'-' && bytes.get(at) == Some(&b'-') {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(at) == Some(&b'*') {
            at += 1;
            while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                at += 1;
            }
            ensure!(at + 1 < bytes.len(), "unterminated schema comment");
            at += 2;
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`' | b'[') {
            let end = if byte == b'[' { b']' } else { byte };
            loop {
                ensure!(at < bytes.len(), "unterminated quoted schema token");
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
        } else if matches!(byte, b'x' | b'X') && bytes.get(at) == Some(&b'\'') {
            at += 1;
            let digits = at;
            while bytes.get(at).is_some_and(u8::is_ascii_hexdigit) {
                at += 1;
            }
            ensure!(
                (at - digits) % 2 == 0 && bytes.get(at) == Some(&b'\''),
                "unsupported schema blob literal"
            );
            at += 1;
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            while bytes
                .get(at)
                .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, b'_' | b'$'))
            {
                at += 1;
            }
        } else if byte.is_ascii_digit()
            || (byte == b'.' && bytes.get(at).is_some_and(u8::is_ascii_digit))
        {
            if byte == b'0'
                && bytes
                    .get(at)
                    .is_some_and(|next| matches!(next, b'x' | b'X'))
            {
                at += 1;
                let digits = at;
                while bytes.get(at).is_some_and(u8::is_ascii_hexdigit) {
                    at += 1;
                }
                ensure!(at > digits, "unsupported schema hexadecimal literal");
            } else {
                while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    at += 1;
                }
                if byte != b'.' && bytes.get(at) == Some(&b'.') {
                    at += 1;
                    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                        at += 1;
                    }
                }
                if bytes
                    .get(at)
                    .is_some_and(|next| matches!(next, b'e' | b'E'))
                {
                    at += 1;
                    if bytes
                        .get(at)
                        .is_some_and(|next| matches!(next, b'+' | b'-'))
                    {
                        at += 1;
                    }
                    let digits = at;
                    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                        at += 1;
                    }
                    ensure!(at > digits, "unsupported schema exponent");
                }
            }
            // SQLite permits separators inside numeric tokens. Stopping before
            // one would conflate a numeric suffix with a distinct alias token.
            ensure!(
                bytes.get(at) != Some(&b'_'),
                "unsupported numeric separator in schema SQL"
            );
        } else if matches!(
            byte,
            b'(' | b')'
                | b','
                | b';'
                | b'.'
                | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b'%'
                | b'&'
                | b'|'
                | b'~'
                | b'='
                | b'<'
                | b'>'
                | b'!'
        ) {
            let pair = bytes.get(start..at + 1);
            if matches!(
                pair,
                Some(b"||" | b"->" | b"<<" | b">>" | b"<=" | b">=" | b"==" | b"!=" | b"<>")
            ) {
                at += 1;
                if pair == Some(b"->") && bytes.get(at) == Some(&b'>') {
                    at += 1;
                }
            } else {
                ensure!(byte != b'!', "unsupported schema operator");
            }
        } else {
            bail!("unsupported unquoted schema byte at offset {start}");
        }
        result.push(sql[start..at].to_owned());
    }
    Ok(result)
}

#[cfg(test)]
#[path = "state_schema_tests.rs"]
mod tests;
