//! One capsule from the structural floor, current direction and validated semantic records,
//! packed as whole records under one total cap of 512 tokens and 2,048 bytes.
//!
//! Status (active, suppressed, withdrawn) is decided before packing. Admission order is fixed
//! by authority and safety, never by model output: current direction and the primary next
//! action; rejections and unsafe routes; changed content; diagnoses; choices; measurements;
//! then locators and secondary facts. A record that does not fit is omitted whole and
//! counted; mandatory direction that cannot fit becomes an unavailable marker, never older
//! wording. The complete escaped fragment is measured after every record.

use super::Candidate;
use super::CandidateKind;
use super::Exclusions;
use super::Projection;
use super::SourceRef;
use super::Status;
use super::UserStatement;
use super::directives::CurrentDirection;
use super::directives::RouteWithdrawal;
use super::directives::RuleEntry;
use super::directives::current_direction;
use super::directives::route_withdrawal;
use super::render::CLOSE;
use super::render::CapsuleError;
use super::render::CapsuleHeader;
use super::render::MAX_CAPSULE_BYTES;
use super::render::MAX_CAPSULE_TOKENS;
use super::render::OPEN;
use super::render::QUALIFICATION;
use super::render::TokenCounter;
use super::render::escape;
use super::render::id_line;
use super::render::record_line;
use super::render::slot_records;
use super::semantic::DebtReason;
use super::semantic::Quote;
use super::semantic::SemanticKind;
use super::semantic::SemanticLedger;
use super::semantic::SemanticRecord;

/// Space held back for the coverage line until the final fit.
const COVERAGE: (usize, usize) = (96, 24);

pub(crate) struct ComposeInput<'a> {
    pub projection: &'a Projection,
    pub header: &'a CapsuleHeader<'a>,
    pub exclusions: &'a Exclusions,
    /// The store's scoped rule view (active successors and supersession links).
    pub rules: &'a [RuleEntry],
    pub ledger: &'a SemanticLedger,
    pub counter: TokenCounter<'a>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ComposedCapsule {
    pub text: String,
    pub tokens: usize,
    pub omitted: usize,
    pub aliases: Vec<(String, SourceRef)>,
}

enum Unit<'a> {
    Rule(&'a RuleEntry),
    Successor(&'a Quote),
    Newest(&'a UserStatement),
    Floor(Vec<&'a Candidate>, Option<RouteWithdrawal>),
    Semantic(&'a SemanticRecord),
}

struct Placed {
    order: (u8, usize),
    mandatory: bool,
    text: String,
}

pub(crate) fn compose_capsule(input: &ComposeInput<'_>) -> Result<ComposedCapsule, CapsuleError> {
    let count = |text: &str| input.counter.map_or(text.len(), |count| count(text));
    let projection = input.projection;
    let visible: Vec<&Candidate> = projection
        .candidates
        .iter()
        .filter(|c| !input.exclusions.excludes(c))
        .collect();
    let semantic = input.ledger.eligible(input.exclusions);
    let direction = current_direction(
        input.rules,
        projection,
        &input.ledger.links,
        input.exclusions,
    );
    let units = units(&direction, &visible, &semantic, projection);

    let id_line = id_line(input.header);
    let assemble = |placed: &[Placed], coverage: Option<&str>| {
        let mut ordered: Vec<&Placed> = placed.iter().collect();
        ordered.sort_by_key(|p| p.order);
        let body: Vec<&str> = ordered
            .iter()
            .map(|p| p.text.as_str())
            .chain(coverage)
            .collect();
        let tail = if body.is_empty() { "" } else { "\n" };
        format!(
            "{OPEN}\n{id_line}\n{QUALIFICATION}\n{}{tail}{CLOSE}",
            body.join("\n")
        )
    };
    let fits = |text: &str, reserve: (usize, usize)| {
        text.len() + reserve.0 <= MAX_CAPSULE_BYTES && count(text) + reserve.1 <= MAX_CAPSULE_TOKENS
    };
    if !fits(&assemble(&[], None), COVERAGE) {
        return Err(CapsuleError::Capacity);
    }
    let mut placed: Vec<Placed> = Vec::new();
    let mut aliases: Vec<(String, SourceRef)> = Vec::new();
    let mut omitted = 0;
    let mut direction_unavailable = false;
    for (rank, (tier, mandatory, unit)) in units.iter().enumerate() {
        let aliases_before = aliases.len();
        let mut accepted = false;
        for text in unit_texts(unit, &visible, &mut aliases) {
            placed.push(Placed {
                order: (*tier, rank),
                mandatory: *mandatory,
                text,
            });
            if fits(&assemble(&placed, None), COVERAGE) {
                accepted = true;
                break;
            }
            placed.pop();
        }
        if !accepted {
            aliases.truncate(aliases_before);
            omitted += 1;
            direction_unavailable |= !matches!(unit, Unit::Floor(..));
        }
    }
    // Real tokenizers are not additive: drop the lowest-priority optional records until the
    // final fragment, with its actual coverage line, fits.
    loop {
        let coverage = coverage_line(input, &direction, omitted, direction_unavailable);
        let text = assemble(&placed, coverage.as_deref());
        if fits(&text, (0, 0)) {
            return Ok(ComposedCapsule {
                tokens: count(&text),
                text,
                omitted,
                aliases,
            });
        }
        let Some(drop) = placed
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.mandatory)
            .max_by_key(|(_, p)| p.order)
            .map(|(index, _)| index)
        else {
            return Err(CapsuleError::Capacity);
        };
        placed.remove(drop);
        omitted += 1;
    }
}

/// Every unit in admission order: (tier, mandatory, unit).
fn units<'a>(
    direction: &CurrentDirection<'a>,
    visible: &[&'a Candidate],
    semantic: &[&'a SemanticRecord],
    projection: &'a Projection,
) -> Vec<(u8, bool, Unit<'a>)> {
    let mut units = Vec::new();
    units.extend(
        direction
            .rules
            .iter()
            .map(|rule| (0, true, Unit::Rule(rule))),
    );
    units.extend(
        direction
            .successor
            .map(|quote| (0, true, Unit::Successor(quote))),
    );
    units.extend(
        direction
            .newest
            .map(|statement| (0, true, Unit::Newest(statement))),
    );
    let floor = |kind: CandidateKind| slot_records(kind, visible);
    let mut nexts = floor(CandidateKind::Next).into_iter();
    units.extend(
        nexts
            .next()
            .map(|record| (0, true, Unit::Floor(record, None))),
    );
    let semantic_of = |kind: SemanticKind| {
        semantic
            .iter()
            .filter(move |record| record.kind == kind)
            .map(|record| Unit::Semantic(record))
    };
    units.extend(semantic_of(SemanticKind::Rejection).map(|unit| (1, false, unit)));
    for record in floor(CandidateKind::Route) {
        let withdrawal = record
            .get(1)
            .and_then(|fix| route_withdrawal(fix, projection, semantic));
        units.push((1, false, Unit::Floor(record, withdrawal)));
    }
    for record in floor(CandidateKind::Change) {
        units.push((2, false, Unit::Floor(record, None)));
    }
    units.extend(semantic_of(SemanticKind::Diagnosis).map(|unit| (3, false, unit)));
    units.extend(semantic_of(SemanticKind::Choice).map(|unit| (4, false, unit)));
    let (baselines, values): (Vec<_>, Vec<_>) = floor(CandidateKind::Value)
        .into_iter()
        .partition(|record| record[0].status == Status::Baseline);
    for record in baselines {
        units.push((5, false, Unit::Floor(record, None)));
    }
    units.extend(semantic_of(SemanticKind::Measurement).map(|unit| (5, false, unit)));
    for record in values.into_iter().chain(nexts) {
        units.push((6, false, Unit::Floor(record, None)));
    }
    units
}

/// The unit's text, then its fallback for mandatory direction that does not fit.
fn unit_texts(
    unit: &Unit<'_>,
    visible: &[&Candidate],
    aliases: &mut Vec<(String, SourceRef)>,
) -> Vec<String> {
    match unit {
        Unit::Rule(rule) => vec![
            format!("rule(user, store): \"{}\"", escape(&rule.text)),
            "rule(user, store): unavailable here (oversize); see the project store".to_string(),
        ],
        Unit::Successor(quote) => {
            let at = alias(aliases, quote_source(quote));
            vec![
                format!(
                    "direction(user; proposed successor, link unconfirmed): \"{}\" {at}",
                    escape(&quote.text)
                ),
                format!("direction(user): current wording unavailable here (oversize) {at}"),
            ]
        }
        Unit::Newest(statement) => {
            let at = alias(aliases, statement.source.clone());
            let marker = format!("user(newest): too long to carry; see history {at}");
            match &statement.text {
                Some(text) => vec![format!("user(newest): \"{}\" {at}", escape(text)), marker],
                None => vec![marker],
            }
        }
        Unit::Floor(record, None) => vec![record_line(record, visible, false, aliases)],
        Unit::Floor(record, Some(withdrawal)) => {
            let line = record_line(&record[..1], visible, false, aliases);
            let status = match withdrawal {
                RouteWithdrawal::Rejected => "replacement rejected (reported)",
                RouteWithdrawal::Unknown => "replacement availability unknown",
            };
            vec![format!("{line}; {status}")]
        }
        Unit::Semantic(record) => {
            let sources: Vec<String> = [Some(&record.main), Some(&record.subject)]
                .into_iter()
                .chain([record.qualifier.as_ref(), record.detail.as_ref()])
                .flatten()
                .map(|quote| alias(aliases, quote_source(quote)))
                .map(|at| at.trim_start_matches('@').to_string())
                .fold(Vec::new(), |mut names, name| {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                    names
                });
            vec![record.render(&format!("@{}", sources.join(",")))]
        }
    }
}

fn alias(aliases: &mut Vec<(String, SourceRef)>, source: SourceRef) -> String {
    if let Some((name, _)) = aliases.iter().find(|(_, known)| *known == source) {
        return format!("@{name}");
    }
    let name = format!("s{}", aliases.len() + 1);
    aliases.push((name.clone(), source));
    format!("@{name}")
}

/// Alias target of a semantic quotation: its original item, part and byte range.
fn quote_source(quote: &Quote) -> SourceRef {
    SourceRef {
        turn_id: quote.turn_id.clone(),
        item_id: quote.item_id.clone(),
        part: quote.part,
        start: quote.start,
        end: quote.end,
        digest: quote.part_sha256.clone(),
        ..SourceRef::default()
    }
}

fn coverage_line(
    input: &ComposeInput<'_>,
    direction: &CurrentDirection<'_>,
    omitted: usize,
    direction_unavailable: bool,
) -> Option<String> {
    let ledger = input.ledger;
    let debt =
        |reason: DebtReason| ledger.debt.0.get(&reason).copied().unwrap_or_default() as usize;
    let mut parts = Vec::new();
    for (label, n) in [
        ("omitted", omitted),
        ("debt", input.projection.capacity_debt as usize),
        ("unscanned", input.projection.uninspected_items as usize),
        (
            "semantic unavailable",
            ledger.unavailable_boundaries as usize,
        ),
        ("unindexed", debt(DebtReason::Unindexed)),
        ("invalid", debt(DebtReason::InvalidSpan)),
        ("unknown run", debt(DebtReason::UnknownRun)),
        (
            "unresolved authority",
            debt(DebtReason::UnresolvedAuthority) + direction.unresolved_rules as usize,
        ),
    ] {
        if n > 0 {
            parts.push(format!("{label} {n}"));
        }
    }
    if direction.older_unresolved {
        parts.push("older direction unresolved".to_string());
    }
    if direction_unavailable {
        parts.push("current direction unavailable".to_string());
    }
    if input.counter.is_none() {
        parts.push("tokens=bytes".to_string());
    }
    (!parts.is_empty()).then(|| format!("coverage: {}", parts.join("; ")))
}
