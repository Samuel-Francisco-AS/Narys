use super::*;
use crate::luna::task::{TaskEventKind, TaskId, TaskStep};

pub(crate) struct TaskTraceAdapter {
    publisher: PassiveTracePublisher,
    source_type: SourceType,
    source_id: &'static str,
}
impl TaskTraceAdapter {
    pub fn new(
        publisher: PassiveTracePublisher,
        source_type: SourceType,
        source_id: &'static str,
    ) -> Self {
        Self {
            publisher,
            source_type,
            source_id,
        }
    }
    pub fn production(source_type: SourceType, source_id: &'static str) -> Self {
        Self::new(PassiveTracePublisher::production(), source_type, source_id)
    }
    pub fn observe(&self, task_id: TaskId, event: &TaskEventKind) {
        use TaskEventKind::*;
        // Discard duplicated projections before allocating any provenance.
        if matches!(
            event,
            ProviderQueued { .. }
                | ProviderAdmitted { .. }
                | ProviderSelected { .. }
                | ProviderChunk { .. }
                | ProviderRetry { .. }
                | ProviderFallback { .. }
                | ProviderOutputObserved { .. }
                | SubtaskRetry { .. }
                | SubtaskOutputObserved { .. }
        ) {
            return;
        }
        let subtask = match event {
            SubtaskWaiting { subtask_id, .. }
            | SubtaskStarted { subtask_id, .. }
            | SubtaskCompleted { subtask_id, .. }
            | SubtaskFailed { subtask_id, .. } => Some(subtask_id.as_str()),
            _ => None,
        };
        let p = provenance(
            self.source_type,
            self.source_id,
            Some(task_id),
            subtask,
            None,
        );
        let step = |s: &TaskStep| match s {
            TaskStep::Prepare => "prepare",
            TaskStep::Verify => "verify",
        };
        let (kind, code, detail) = match event {
            TaskStarted => (StateKind::Started, "task_started", String::new()),
            StepStarted { step: s } => (StateKind::Checkpoint, "step_started", step(s).into()),
            StepCompleted { step: s } => (StateKind::Checkpoint, "step_completed", step(s).into()),
            TaskPaused { .. } => (StateKind::Checkpoint, "task_paused", String::new()),
            TaskCompleted => {
                self.publisher
                    .critical(&p, CriticalKind::Completed, "task_completed");
                return;
            }
            TaskCancelled => {
                self.publisher
                    .critical(&p, CriticalKind::Cancelled, "task_cancelled");
                return;
            }
            TaskFailed { .. } => {
                self.publisher
                    .critical(&p, CriticalKind::Failed, "task_failed");
                return;
            }
            ContextBuilt {
                memory_count,
                recent_message_count,
            } => (
                StateKind::Checkpoint,
                "context_built",
                format!("memories={memory_count} recent_messages={recent_message_count}"),
            ),
            TaskResultReady { .. } | OrchestratorPlanReady { .. } | TaskGraphResultReady { .. } => {
                (StateKind::Checkpoint, "result_ready", String::new())
            }
            TaskPlanned { step_count } => (
                StateKind::Planning,
                "task_planned",
                format!("steps={step_count}"),
            ),
            SubtaskWaiting { .. } => (
                StateKind::SubtaskLifecycle,
                "subtask_waiting",
                String::new(),
            ),
            SubtaskStarted { .. } => (
                StateKind::SubtaskLifecycle,
                "subtask_started",
                String::new(),
            ),
            SubtaskCompleted { .. } => (
                StateKind::SubtaskLifecycle,
                "subtask_completed",
                String::new(),
            ),
            SubtaskFailed { .. } => (StateKind::SubtaskLifecycle, "subtask_failed", String::new()),
            // Scheduler is the sole authority for routing and output, even when
            // those facts have been projected to functional TaskEvents.
            ProviderQueued { .. }
            | ProviderAdmitted { .. }
            | ProviderSelected { .. }
            | ProviderChunk { .. }
            | ProviderRetry { .. }
            | ProviderFallback { .. }
            | ProviderOutputObserved { .. }
            | SubtaskRetry { .. }
            | SubtaskOutputObserved { .. } => return,
        };
        self.publisher.state(&p, kind, code, &detail);
    }
}

/// Summary does not reserve a TaskId early, nor copy persisted session metadata.
pub(crate) struct SummaryTraceAdapter {
    publisher: PassiveTracePublisher,
    provenance: Result<Provenance, TraceError>,
}
#[derive(Clone, Copy)]
pub(crate) enum SummaryTraceState {
    Started,
    Completed,
    Failed,
    Deferred,
    Disabled,
    PersistenceFailed,
    Unchanged,
}
impl SummaryTraceAdapter {
    pub fn new(publisher: PassiveTracePublisher) -> Self {
        static CALLS: AtomicU64 = AtomicU64::new(0);
        let correlation = next_call(&CALLS, "summary-call");
        let p = correlation
            .ok_or(TraceError::SequenceExhausted)
            .and_then(|id| provenance(SourceType::Worker, "summary", None, None, Some(id)));
        Self {
            publisher,
            provenance: p,
        }
    }
    pub fn observe(&self, state: SummaryTraceState) {
        use SummaryTraceState::*;
        match state {
            Completed => self.publisher.critical(
                &self.provenance,
                CriticalKind::Completed,
                "summary_completed",
            ),
            Failed => {
                self.publisher
                    .critical(&self.provenance, CriticalKind::Failed, "summary_failed")
            }
            state => {
                let (kind, code) = match state {
                    Started => (StateKind::Started, "summary_started"),
                    Deferred => (StateKind::Checkpoint, "summary_deferred"),
                    Disabled => (StateKind::Checkpoint, "summary_disabled"),
                    PersistenceFailed => (StateKind::Checkpoint, "summary_persistence_failed"),
                    Unchanged => (StateKind::Checkpoint, "summary_unchanged"),
                    Completed | Failed => unreachable!(),
                };
                self.publisher.state(&self.provenance, kind, code, "");
            }
        }
    }
    pub fn correlation(&self) -> Option<TraceId> {
        self.provenance
            .as_ref()
            .ok()
            .and_then(|p| p.correlation_id.clone())
    }
}
