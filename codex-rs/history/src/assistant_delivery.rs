//! Host classification of an assistant delivery, independent of the provider-supplied phase.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// How the host resolved a delivered assistant message.
///
/// Explicit provider phases stay authoritative. A phase-less message starts `Pending` and is
/// resolved only when its response completes successfully; failed or interrupted responses leave it
/// pending. Absent classification is legacy evidence and is never promoted retroactively.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssistantDeliveryClassification {
    /// Delivered, but its response has not established whether it answered the user.
    Pending,
    /// Delivered as progress or a preamble, not as a completed answer.
    Commentary,
    /// A completed answer delivered to the user, not necessarily the turn's last message.
    Final,
}
