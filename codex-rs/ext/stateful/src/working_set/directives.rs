//! Current direction and route status for the capsule, decided by host policy only.
//!
//! Adopted rules come from the store's supersession view: only active successors are shown,
//! and a broken or ineligible successor chain is unresolved authority, never a reason to show
//! its predecessor. Unlinked user statements are conservative: the newest one is shown and is
//! a barrier to older wording, which is reported only as `older direction unresolved`. A
//! validated model link is advisory: it suppresses the old wording and carries the new exact
//! user wording, without mutating stored rules. Routes lose their working label on a
//! validated rejection that names them or a later user statement that names them.

use super::Candidate;
use super::Exclusions;
use super::Projection;
use super::UserStatement;
use super::semantic::Quote;
use super::semantic::SemanticKind;
use super::semantic::SemanticRecord;
use super::semantic::SupersessionLink;

/// Successor chains longer than this are unresolved authority.
const MAX_CHAIN: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuleStatus {
    Active,
    Superseded,
    Retired,
}

/// One adopted rule revision as the store's scoped successor view reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuleEntry {
    pub id: String,
    pub text: String,
    pub status: RuleStatus,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CurrentDirection<'a> {
    /// Active rules with no successor, in store order.
    pub rules: Vec<&'a RuleEntry>,
    /// Superseded rules whose successor chain does not end in an active rule.
    pub unresolved_rules: u32,
    /// The newest advisory successor wording, when the newest statement does not hold it.
    pub successor: Option<&'a Quote>,
    pub newest: Option<&'a UserStatement>,
    /// Older user statements exist and are not shown as active direction.
    pub older_unresolved: bool,
}

pub(crate) fn current_direction<'a>(
    rules: &'a [RuleEntry],
    projection: &'a Projection,
    links: &'a [SupersessionLink],
    exclusions: &Exclusions,
) -> CurrentDirection<'a> {
    let retired = |text: &str| {
        exclusions
            .retired_texts
            .iter()
            .any(|retired| !retired.is_empty() && text.contains(retired.as_str()))
    };
    let find = |id: &str| rules.iter().find(|rule| rule.id == id);
    let mut active = Vec::new();
    let mut unresolved_rules = 0;
    for rule in rules {
        match (rule.status, rule.superseded_by.as_deref()) {
            (RuleStatus::Active, None) if !retired(&rule.text) => active.push(rule),
            (RuleStatus::Retired, _) | (RuleStatus::Active, None) => {}
            (_, successor) => {
                let mut next = successor.and_then(find);
                let mut resolved = false;
                for _ in 0..MAX_CHAIN {
                    let Some(rule) = next else { break };
                    if rule.status == RuleStatus::Active && rule.superseded_by.is_none() {
                        resolved = !retired(&rule.text);
                        break;
                    }
                    if rule.status == RuleStatus::Retired {
                        break;
                    }
                    next = rule.superseded_by.as_deref().and_then(find);
                }
                unresolved_rules += u32::from(!resolved);
            }
        }
    }
    let statement_excluded = |statement: &UserStatement| {
        exclusions.turns.contains(&statement.source.turn_id)
            || statement.text.as_deref().is_some_and(retired)
    };
    let newest = projection
        .statements
        .iter()
        .rev()
        .find(|statement| !statement_excluded(statement));
    // A link whose new wording is itself the old side of a later link is not current.
    let superseded = |quote: &Quote| {
        links.iter().any(|later| {
            later.old.sequence >= quote.sequence && later.old.text.contains(quote.text.as_str())
        })
    };
    let successor = links
        .iter()
        .map(|link| &link.new)
        .filter(|quote| {
            quote.user
                && !exclusions.turns.contains(&quote.turn_id)
                && !retired(&quote.text)
                && !superseded(quote)
        })
        .max_by_key(|quote| quote.sequence)
        .filter(|quote| {
            newest
                .and_then(|statement| statement.text.as_deref())
                .is_none_or(|text| !text.contains(quote.text.as_str()))
        });
    CurrentDirection {
        rules: active,
        unresolved_rules,
        successor,
        newest,
        older_unresolved: projection.statements_seen > 1,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RouteWithdrawal {
    /// A validated (reported) rejection names the replacement.
    Rejected,
    /// A later user statement names it: availability is unknown until resolved.
    Unknown,
}

/// Whether a working replacement route must lose its working label.
pub(crate) fn route_withdrawal(
    replacement: &Candidate,
    projection: &Projection,
    semantic: &[&SemanticRecord],
) -> Option<RouteWithdrawal> {
    let names = route_names(replacement);
    let named = |text: &str| names.iter().any(|name| names_token(text, name));
    if semantic.iter().any(|record| {
        record.kind == SemanticKind::Rejection
            && (named(&record.main.text) || named(&record.subject.text))
    }) {
        return Some(RouteWithdrawal::Rejected);
    }
    projection
        .statements
        .iter()
        .filter(|statement| statement.after_candidate >= replacement.id)
        .any(|statement| statement.text.as_deref().is_some_and(named))
        .then_some(RouteWithdrawal::Unknown)
}

/// Identifying names of a route: script-like path arguments when present, else the program.
fn route_names(route: &Candidate) -> Vec<String> {
    let head = route.text.as_deref().unwrap_or_default();
    let paths: Vec<String> = head
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| "\"'`".contains(c)))
        .filter(|token| !token.starts_with('-'))
        .filter_map(|token| {
            let name = token.rsplit(['/', '\\']).next()?;
            let (stem, extension) = name.rsplit_once('.')?;
            (!stem.is_empty() && extension.chars().all(char::is_alphanumeric))
                .then(|| name.to_string())
        })
        .collect();
    if paths.is_empty() {
        route.subject.iter().cloned().collect()
    } else {
        paths
    }
}

/// `name` occurs in `text` as a whole token (bounded by non-identifier characters).
fn names_token(text: &str, name: &str) -> bool {
    let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
    text.match_indices(name).any(|(at, _)| {
        boundary(text[..at].chars().next_back()) && boundary(text[at + name.len()..].chars().next())
    })
}
