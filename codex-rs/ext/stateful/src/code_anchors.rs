//! Literal values a commit changed, read from its patch, and whether remembered prose may
//! state one of the old values.
//!
//! Only a narrow kind of change is recognized: a removed line and its added replacement that
//! are identical except for one quoted literal bound to a key (`"YEARS": "y"` becoming
//! `"YEARS": "yr"`). The change's subject is the words of its file name and of the mapping
//! that contains it (`_compact.py`, `_COMPACT_UNITS`), and its siblings are the other values
//! of the hunk. Prose that states the old value with the file, the key, or a subject word and
//! a sibling may be about the change; anything else, such as a lone token, is left alone.
//! Words cannot prove which code a sentence is about, so a match only ever asks for a check.

/// Literal changes kept per patch.
const MAX_CHANGES: usize = 32;
/// Sibling literals kept per change.
const MAX_SIBLINGS: usize = 32;
/// Longest literal considered a value (longer strings are messages, not symbols).
const MAX_LITERAL_BYTES: usize = 80;

/// One quoted literal one commit rebound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiteralChange {
    /// Relative to the Git worktree root.
    pub(crate) path: String,
    /// What the value is bound to (`YEARS` in `"YEARS": "y"`).
    pub(crate) key: String,
    pub(crate) old: String,
    pub(crate) new: String,
    /// The other literals of the same hunk (keys excluded).
    pub(crate) siblings: Vec<String>,
    /// Lowercase words naming what the value belongs to: the file stem and the enclosing
    /// mapping's name.
    pub(crate) subject: Vec<String>,
}

/// The literal changes in `patch` (whole, at most `MAX_CHANGES`), and whether every one was
/// kept.
pub(crate) fn literal_changes(patch: &str) -> (Vec<LiteralChange>, bool) {
    let mut changes = Vec::new();
    let mut path: Option<String> = None;
    let mut in_header = false;
    let mut hunk: Vec<(char, &str)> = Vec::new();
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            collect_hunk(path.as_deref(), &hunk, &mut changes);
            hunk.clear();
            path = None;
            in_header = true;
        } else if line.starts_with("@@") {
            collect_hunk(path.as_deref(), &hunk, &mut changes);
            hunk.clear();
            in_header = false;
        } else if in_header {
            if let Some(target) = line.strip_prefix("+++ ") {
                path = patch_path(target);
            }
        } else if let Some(tag) = line
            .chars()
            .next()
            .filter(|tag| matches!(tag, ' ' | '-' | '+'))
        {
            hunk.push((tag, &line[1..]));
        }
    }
    collect_hunk(path.as_deref(), &hunk, &mut changes);
    let complete = changes.len() <= MAX_CHANGES;
    changes.truncate(MAX_CHANGES);
    (changes, complete)
}

/// The new-side path of a `+++` header: `b/<path>`, possibly C-quoted, possibly followed by
/// a tab. `/dev/null` (a deletion) has none.
fn patch_path(target: &str) -> Option<String> {
    let target = target.strip_suffix('\t').unwrap_or(target);
    let unquoted = match target.strip_prefix('"') {
        Some(quoted) => unquote(quoted.strip_suffix('"')?)?,
        None => target.to_string(),
    };
    unquoted.strip_prefix("b/").map(str::to_string)
}

/// Undoes Git's C-style path quoting (`\t`, `\"`, `\\`, octal bytes).
fn unquote(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut input = text.bytes();
    while let Some(byte) = input.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        match input.next()? {
            b'n' => bytes.push(b'\n'),
            b't' => bytes.push(b'\t'),
            b'"' => bytes.push(b'"'),
            b'\\' => bytes.push(b'\\'),
            digit @ b'0'..=b'7' => {
                let mut value = u32::from(digit - b'0');
                for _ in 0..2 {
                    value = value * 8 + u32::from(input.next()?.checked_sub(b'0')?);
                }
                bytes.push(u8::try_from(value).ok()?);
            }
            _ => return None,
        }
    }
    String::from_utf8(bytes).ok()
}

/// Pairs each run of removed lines with the added run that follows it, line by line.
fn collect_hunk(path: Option<&str>, hunk: &[(char, &str)], changes: &mut Vec<LiteralChange>) {
    let Some(path) = path else {
        return;
    };
    let mut values: Vec<String> = Vec::new();
    for (_, text) in hunk {
        for (_, literal) in literals(text) {
            if !values.contains(&literal) {
                values.push(literal);
            }
        }
    }
    let keys = hunk
        .iter()
        .flat_map(|(_, line)| bound_pairs(line).into_iter().map(|(key, _)| key))
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < hunk.len() {
        let removed_start = index;
        while index < hunk.len() && hunk[index].0 == '-' {
            index += 1;
        }
        let added_start = index;
        while index < hunk.len() && hunk[index].0 == '+' {
            index += 1;
        }
        let removed = &hunk[removed_start..added_start];
        let added = &hunk[added_start..index];
        if removed.len() == added.len() {
            for ((_, old_line), (_, new_line)) in removed.iter().zip(added) {
                let Some((key, old, new)) = replaced_literal(old_line, new_line) else {
                    continue;
                };
                let siblings = values
                    .iter()
                    .filter(|value| **value != old && **value != new && !keys.contains(value))
                    .take(MAX_SIBLINGS)
                    .cloned()
                    .collect();
                let file_stem = path
                    .rsplit('/')
                    .next()
                    .and_then(|name| name.split('.').next())
                    .unwrap_or_default();
                let mut subject = words_of(file_stem);
                if let Some(container) = hunk[..removed_start]
                    .iter()
                    .rev()
                    .map(|(_, line)| code_part(line).trim_end())
                    .find(|line| line.ends_with(['{', '[', '(']))
                {
                    let name = container.split(['=', ':', '(']).next().unwrap_or_default();
                    subject.extend(words_of(name));
                }
                subject.sort();
                subject.dedup();
                changes.push(LiteralChange {
                    path: path.to_string(),
                    key,
                    old,
                    new,
                    siblings,
                    subject,
                });
            }
        }
        if index == removed_start {
            index += 1;
        }
    }
}

/// Lowercase words of identifiers (`_COMPACT_UNITS` gives `compact` and `units`), at least
/// three letters each.
fn words_of(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| word.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect()
}

/// The key, old and new value when the two lines bind the same key and differ only in that
/// value.
fn replaced_literal(old: &str, new: &str) -> Option<(String, String, String)> {
    let (old_key, old_value) = bound_pair(old)?;
    let (new_key, new_value) = bound_pair(new)?;
    if old_key != new_key || old_value == new_value || skeleton(old) != skeleton(new) {
        return None;
    }
    Some((old_key, old_value, new_value))
}

/// The line, outside any comment, with every literal's content removed.
fn skeleton(line: &str) -> String {
    let line = code_part(line);
    let mut out = String::new();
    let mut last = 0;
    for (start, end) in literal_spans(line) {
        out.push_str(&line[last..=start]);
        last = end;
    }
    out.push_str(&line[last..]);
    out.trim().to_string()
}

/// The line up to a `#` or `//` comment that is not inside a literal.
fn code_part(line: &str) -> &str {
    let spans = literal_spans(line);
    let inside = |position: usize| {
        spans
            .iter()
            .any(|(start, end)| *start < position && position < *end)
    };
    let comment = line
        .match_indices('#')
        .chain(line.match_indices("//"))
        .map(|(position, _)| position)
        .filter(|position| !inside(*position))
        .min();
    comment.map_or(line, |position| &line[..position])
}

/// Words that may precede a bound name in a supported declaration (`const YEARS = "yr"`).
const DECLARATION_WORDS: &[&str] = &[
    "const", "let", "var", "export", "static", "final", "pub", "public", "private", "readonly",
];

/// The keys a line binds and the literals it binds them to: `KEY = "v"`, `"KEY": "v"`,
/// `KEY: "v"`, `Enum.KEY: "v"` or `const KEY = "v"`, ignoring comments. Each value literal
/// must follow its separator directly, and the key must start the binding (after `{`, `,`,
/// `(` or declaration words); anything else, such as a typed declaration, is not a binding.
fn bound_pairs(line: &str) -> Vec<(String, String)> {
    let line = code_part(line);
    let mut pairs = Vec::new();
    for (start, value) in literals(line) {
        let before = line[..start].trim_end();
        let Some(before) = before.strip_suffix(':').or_else(|| {
            before
                .strip_suffix('=')
                .filter(|rest| !rest.ends_with(['=', '!', '<', '>']))
        }) else {
            continue;
        };
        let before = before.trim_end();
        let (key, prefix) = match before.chars().last() {
            Some(quote @ ('"' | '\'')) => {
                let inner = &before[..before.len() - 1];
                match inner.rfind(quote) {
                    Some(open) => (inner[open + 1..].to_string(), &inner[..open]),
                    None => continue,
                }
            }
            Some(_) => {
                let key = before
                    .rsplit(|character: char| !(character.is_alphanumeric() || character == '_'))
                    .next()
                    .unwrap_or_default();
                let mut prefix = &before[..before.len() - key.len()];
                // `Enum.KEY` and `self.KEY` name the same key.
                while let Some(qualified) = prefix.strip_suffix('.') {
                    prefix = qualified.trim_end_matches(|character: char| {
                        character.is_alphanumeric() || character == '_'
                    });
                }
                (key.to_string(), prefix)
            }
            None => continue,
        };
        let prefix = prefix.trim_end();
        let starts_binding = prefix.is_empty()
            || prefix.ends_with(['{', ',', '('])
            || prefix
                .split_whitespace()
                .all(|word| DECLARATION_WORDS.contains(&word));
        if !key.is_empty() && starts_binding {
            pairs.push((key, value));
        }
    }
    pairs
}

/// The one key a line binds, when it binds exactly one.
fn bound_pair(line: &str) -> Option<(String, String)> {
    let mut pairs = bound_pairs(line);
    (pairs.len() == 1).then(|| pairs.remove(0))
}

/// Quoted literals of a line with the byte offset of their opening quote.
fn literals(line: &str) -> Vec<(usize, String)> {
    literal_spans(line)
        .into_iter()
        .filter_map(|(start, end)| {
            let content = &line[start + 1..end];
            (!content.is_empty() && content.len() <= MAX_LITERAL_BYTES && !content.contains('\\'))
                .then(|| (start, content.to_string()))
        })
        .collect()
}

/// Byte offsets of each quoted literal's opening and closing quote. Escaped quotes end
/// nothing; an unterminated quote (an apostrophe in a comment) ends the scan.
fn literal_spans(line: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut characters = line.char_indices();
    while let Some((start, quote)) = characters.next() {
        if !matches!(quote, '"' | '\'' | '`') {
            continue;
        }
        let mut escaped = false;
        let mut end = None;
        for (position, character) in characters.by_ref() {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                end = Some(position);
                break;
            }
        }
        let Some(end) = end else {
            break;
        };
        found.push((start, end));
    }
    found
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Whether `text` states `literal` as a whole word (case-sensitive). An apostrophe (straight
/// or typographic) between letters joins a word, so `s` is not found in "user's".
pub(crate) fn contains_literal(text: &str, literal: &str) -> bool {
    literal_count(text, literal) > 0
}

/// How many times `text` states `literal` as a whole word.
fn literal_count(text: &str, literal: &str) -> usize {
    if literal.is_empty() {
        return 0;
    }
    text.match_indices(literal)
        .filter(|(start, _)| {
            let start = *start;
            let mut before = text[..start].chars().rev();
            let mut after = text[start + literal.len()..].chars();
            let joins = |next: Option<char>, beyond: Option<char>| match next {
                None => false,
                Some('\'' | '\u{2019}') => beyond.is_some_and(char::is_alphabetic),
                Some(character) => is_word(character),
            };
            !joins(before.next(), before.next()) && !joins(after.next(), after.next())
        })
        .count()
}

/// Whether `content`, prose not anchored to code, may state the value `change` replaced:
/// it states the old value (and not the new one) together with either the file or key, or
/// a subject word and a sibling value. Such prose is never retired on this evidence alone,
/// only marked as needing a check: words cannot prove which code a sentence is about.
pub(crate) fn may_state_old_value(content: &str, change: &LiteralChange) -> bool {
    if !contains_literal(content, &change.old) || contains_literal(content, &change.new) {
        return false;
    }
    let lowered = content.to_lowercase();
    let names_subject = change
        .subject
        .iter()
        .any(|word| contains_literal(&lowered, word));
    let names_sibling = change
        .siblings
        .iter()
        .any(|sibling| contains_literal(content, sibling));
    let file_name = change.path.rsplit('/').next().unwrap_or(&change.path);
    content.contains(file_name)
        || contains_literal(content, &change.key)
        || (names_subject && names_sibling)
}

#[cfg(test)]
#[path = "code_anchors_tests.rs"]
mod tests;
