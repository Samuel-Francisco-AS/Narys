//! Agent-neutral passive side channel. Only backend-classified display content
//! belongs here; no raw protocol, objective, private reasoning or result contract.
use super::types::AgentEvent;

pub enum AgentTraceObservation<'a> {
    Lifecycle(&'a AgentEvent),
    AgentMessage(&'a str),
    DisplayReasoningSummary(&'a str),
}
pub trait AgentTraceSink: Send + Sync {
    /// Must return promptly. No ACK or functional error return is available.
    fn observe(&self, observation: AgentTraceObservation<'_>);
}
#[cfg(test)]
pub struct NoopAgentTrace;
#[cfg(test)]
impl AgentTraceSink for NoopAgentTrace {
    fn observe(&self, _: AgentTraceObservation<'_>) {}
}
/// Even a defective observer has no error/unwind path into the agent worker.
pub fn observe_passively(sink: &dyn AgentTraceSink, observation: AgentTraceObservation<'_>) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.observe(observation)));
}
