//! Optional temporal evidence in existing entry context. Absent payload means unknown.
use crate::SourceSpan;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalAnchor {
    pub source_id: String,
    pub source_revision: u64,
    pub digest: String,
    pub spans: Vec<SourceSpan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimePrecision {
    Second,
    Minute,
    Hour,
    Day,
    Month,
    Year,
    Approximate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TimeZone {
    Unknown,
    Utc,
    OffsetMinutes { minutes: i16 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SourceTime {
    Unknown,
    HostObserved {
        unix_ms: i64,
        precision: TimePrecision,
    },
    Attributed {
        expression: String,
        anchor: TemporalAnchor,
        precision: TimePrecision,
        timezone: TimeZone,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum TemporalDerivation {
    SourceExplicit,
    Deterministic { convention: String },
    ProposedModel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EventTime {
    Unknown,
    Point {
        expression: String,
        anchor: TemporalAnchor,
        unix_ms: Option<i64>,
        precision: TimePrecision,
        timezone: TimeZone,
        derivation: TemporalDerivation,
    },
    Interval {
        expression: String,
        anchor: TemporalAnchor,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
        inclusive_start: bool,
        inclusive_end: bool,
        precision: TimePrecision,
        timezone: TimeZone,
        derivation: TemporalDerivation,
    },
    Duration {
        expression: String,
        anchor: TemporalAnchor,
    },
    Recurrence {
        expression: String,
        anchor: TemporalAnchor,
    },
    UnresolvedRelative {
        expression: String,
        anchor: Option<TemporalAnchor>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventStatus {
    Unknown,
    SourceReportedOccurrence,
    CurrentAssertion,
    Plan,
    CancelledPlan,
    UncertainReport,
    ProposedModel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalContext {
    pub version: u32,
    pub recorded_at_ms: i64,
    pub source_time: SourceTime,
    pub event_time: EventTime,
    pub event_status: EventStatus,
}

impl TemporalContext {
    /// Validates representation and bounds; does not infer dates or certify occurrence.
    pub fn validate(&self) -> Result<(), crate::BlackboardStoreError> {
        let json =
            serde_json::to_value(self).map_err(|_| crate::BlackboardStoreError::InvalidSource)?;
        if self.version != 1 || json.to_string().len() > 8192 || !bounded(&json) {
            return Err(crate::BlackboardStoreError::InvalidSource);
        }
        if let EventTime::Interval {
            start_ms: Some(start),
            end_ms: Some(end),
            ..
        } = self.event_time
            && start > end
        {
            return Err(crate::BlackboardStoreError::InvalidSource);
        }
        Ok(())
    }
}

fn bounded(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => value.len() <= 512,
        serde_json::Value::Array(values) => values.len() <= 8 && values.iter().all(bounded),
        serde_json::Value::Object(fields) => fields.iter().all(|(key, value)| {
            if key == "digest" {
                value.as_str().is_some_and(|digest| {
                    digest.len() == 64 && digest.bytes().all(|ch| ch.is_ascii_hexdigit())
                })
            } else if key == "sourceId" {
                value
                    .as_str()
                    .is_some_and(|source| !source.is_empty() && source.len() <= 512)
            } else if key == "spans" {
                value.as_array().is_some_and(|spans| {
                    let mut end = 0;
                    !spans.is_empty()
                        && spans.len() <= 8
                        && spans.iter().all(|span| {
                            let parsed = serde_json::from_value::<SourceSpan>(span.clone());
                            match parsed {
                                Ok(span)
                                    if span.start_byte >= end
                                        && span.start_byte < span.end_byte =>
                                {
                                    end = span.end_byte;
                                    true
                                }
                                Ok(_) | Err(_) => false,
                            }
                        })
                })
            } else if key == "minutes" {
                value
                    .as_i64()
                    .is_some_and(|offset| (-1439..=1439).contains(&offset))
            } else if key == "sourceRevision" {
                value
                    .as_u64()
                    .is_some_and(|revision| revision > 0 && revision <= i64::MAX as u64)
            } else {
                bounded(value)
            }
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => true,
    }
}
