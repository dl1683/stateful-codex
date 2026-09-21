/// Durable Stateful mutation that clients should reread from the app-server API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatefulEvent {
    RunUpdated {
        project_id: String,
        run_id: String,
        revision: u64,
    },
    ObligationUpdated {
        project_id: String,
        run_id: String,
        obligation_id: String,
        revision: u64,
    },
    SteeringUpdated {
        project_id: String,
        run_id: String,
        steering_id: String,
        revision: u64,
    },
}

/// Receives committed Stateful mutations and forwards revision hints to product clients.
///
/// Implementations must treat the durable stores as authoritative. Delivery may be best effort,
/// and clients recover missed events through the read APIs.
pub trait StatefulEventSink: Send + Sync {
    fn emit(&self, event: StatefulEvent);
}
