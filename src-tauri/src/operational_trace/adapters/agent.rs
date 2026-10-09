use super::*;
use crate::agents::{
    trace::{AgentTraceObservation, AgentTraceSink},
    types::AgentEvent,
};
use crate::luna::task::TaskId;

pub(crate) struct AgentTraceContext {
    pub source_id: String,
    pub task_id: Option<TaskId>,
    pub subtask_id: Option<String>,
}
pub(crate) struct AgentTraceAdapter {
    publisher: PassiveTracePublisher,
    provenance: Result<Provenance, TraceError>,
}
impl AgentTraceAdapter {
    pub fn production(source_id: &str) -> Self {
        Self::new(
            PassiveTracePublisher::production(),
            AgentTraceContext {
                source_id: source_id.into(),
                task_id: None,
                subtask_id: None,
            },
        )
    }
    pub fn new(publisher: PassiveTracePublisher, context: AgentTraceContext) -> Self {
        static CALLS: AtomicU64 = AtomicU64::new(0);
        let p = next_call(&CALLS, "agent-call")
            .ok_or(TraceError::SequenceExhausted)
            .and_then(|id| {
                provenance(
                    SourceType::SpecialistAgent,
                    &context.source_id,
                    context.task_id,
                    context.subtask_id.as_deref(),
                    Some(id),
                )
            });
        Self {
            publisher,
            provenance: p,
        }
    }
}
impl AgentTraceSink for AgentTraceAdapter {
    fn observe(&self, observation: AgentTraceObservation<'_>) {
        match observation {
            AgentTraceObservation::AgentMessage(text) => {
                self.publisher
                    .text(&self.provenance, TextChannel::AgentMessage, text)
            }
            AgentTraceObservation::DisplayReasoningSummary(text) => {
                self.publisher
                    .text(&self.provenance, TextChannel::DisplayReasoningSummary, text)
            }
            AgentTraceObservation::Lifecycle(event) => {
                use AgentEvent::*;
                let (kind, code) = match event {
                    SessionReady => (StateKind::Started, "session_ready"),
                    WorkStarted => (StateKind::Started, "work_started"),
                    OutputObserved => (StateKind::Checkpoint, "output_observed"),
                    CancellationRequested => (StateKind::Checkpoint, "cancellation_requested"),
                    Completed => {
                        self.publisher.critical(
                            &self.provenance,
                            CriticalKind::Completed,
                            "agent_completed",
                        );
                        return;
                    }
                    Cancelled => {
                        self.publisher.critical(
                            &self.provenance,
                            CriticalKind::Cancelled,
                            "agent_cancelled",
                        );
                        return;
                    }
                    Failed => {
                        self.publisher.critical(
                            &self.provenance,
                            CriticalKind::Failed,
                            "agent_failed",
                        );
                        return;
                    }
                    Output { text } => {
                        self.publisher
                            .text(&self.provenance, TextChannel::AgentMessage, text);
                        return;
                    }
                };
                self.publisher.state(&self.provenance, kind, code, "");
            }
        }
    }
}
