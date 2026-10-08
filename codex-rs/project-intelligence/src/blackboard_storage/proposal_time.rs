//! Source association only: relative dates are never resolved against the ingest clock.
use crate::*;

pub(super) fn temporal(
    proposal: &SourceProposal,
    seal: &SourceSeal,
    exact: &str,
) -> Result<TemporalContext, BlackboardStoreError> {
    let mut result = TemporalContext {
        version: 1,
        recorded_at_ms: seal.recorded_at_ms,
        source_time: SourceTime::Unknown,
        event_time: EventTime::Unknown,
        event_status: EventStatus::ProposedModel,
    };
    let Some(time) = &proposal.temporal else {
        return Ok(result);
    };
    let span = |index: u8| {
        proposal
            .spans
            .get(usize::from(index))
            .cloned()
            .filter(|span| {
                span.role == SourceSpanRole::Temporal && span.end_byte - span.start_byte <= 512
            })
            .ok_or(BlackboardStoreError::InvalidSource)
    };
    let words = |range: &SourceSpan| {
        exact
            .get(range.start_byte as usize..range.end_byte as usize)
            .map(str::to_string)
            .ok_or(BlackboardStoreError::InvalidSource)
    };
    let anchor = |range| TemporalAnchor {
        source_id: seal.exact_source_locator.clone(),
        source_revision: seal.observation.source_revision,
        digest: seal.digest.clone(),
        spans: vec![range],
    };
    if let Some(index) = time.source_time_span {
        let range = span(index)?;
        result.source_time = SourceTime::Attributed {
            expression: words(&range)?,
            anchor: anchor(range),
            precision: time.precision.unwrap_or(TimePrecision::Approximate),
            timezone: TimeZone::Unknown,
        };
    }
    let cited_anchor = time.anchor_span.map(span).transpose()?;
    if let Some(index) = time.event_time_span {
        let range = span(index)?;
        let expression = words(&range)?;
        // With no supplied anchor a relative expression remains explicitly unresolved.
        let event_anchor = anchor(cited_anchor.clone().unwrap_or(range));
        result.event_time = match time.form {
            ProposalTimeForm::Point => EventTime::Point {
                expression,
                anchor: event_anchor,
                unix_ms: None,
                precision: time.precision.unwrap_or(TimePrecision::Approximate),
                timezone: TimeZone::Unknown,
                derivation: TemporalDerivation::ProposedModel,
            },
            ProposalTimeForm::Interval => EventTime::Interval {
                expression,
                anchor: event_anchor,
                start_ms: None,
                end_ms: None,
                inclusive_start: time.inclusive_start.unwrap_or(false),
                inclusive_end: time.inclusive_end.unwrap_or(false),
                precision: time.precision.unwrap_or(TimePrecision::Approximate),
                timezone: TimeZone::Unknown,
                derivation: TemporalDerivation::ProposedModel,
            },
            ProposalTimeForm::Duration => EventTime::Duration {
                expression,
                anchor: event_anchor,
            },
            ProposalTimeForm::Recurrence => EventTime::Recurrence {
                expression,
                anchor: event_anchor,
            },
            ProposalTimeForm::UnresolvedRelative => EventTime::UnresolvedRelative {
                expression,
                anchor: cited_anchor.map(anchor),
            },
        };
    } else if time.anchor_span.is_some() {
        return Err(BlackboardStoreError::InvalidSource);
    }
    result.validate()?;
    Ok(result)
}
