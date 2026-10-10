use serde::Serialize;

pub use super::task_id::TaskId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Running,
    Paused,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStep {
    Prepare,
    Verify,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEventKind {
    TaskStarted,
    StepStarted {
        step: TaskStep,
    },
    StepCompleted {
        step: TaskStep,
    },
    TaskPaused {
        reason: crate::persistence::continuations::PauseReason,
    },
    TaskCompleted,
    TaskCancelled,
    TaskFailed {
        detail: String,
    },
    ContextBuilt {
        memory_count: usize,
        recent_message_count: usize,
    },
    ProviderQueued {
        provider_id: String,
        traffic_class: crate::cognition::admission::TrafficClass,
        queue_depth: usize,
    },
    ProviderAdmitted {
        provider_id: String,
        traffic_class: crate::cognition::admission::TrafficClass,
        queue_delay_ms: u64,
    },
    ProviderSelected {
        provider_id: String,
        model: String,
        attempt: u32,
        routing_reason: String,
        score: Option<i64>,
    },
    ProviderChunk {
        provider_id: String,
        chunk: String,
    },
    ProviderRetry {
        provider_id: String,
        reason_code: String,
    },
    ProviderFallback {
        from_provider_id: String,
        to_provider_id: String,
        reason_code: String,
    },
    ProviderOutputObserved {
        provider_id: String,
    },
    TaskResultReady {
        result: crate::cognition::types::TaskResult,
    },
    OrchestratorPlanReady {
        result: crate::cognition::orchestrator::OrchestratorResult,
    },
    TaskPlanned {
        step_count: usize,
    },
    SubtaskWaiting {
        subtask_id: String,
        depends_on: Vec<String>,
    },
    SubtaskStarted {
        subtask_id: String,
        provider_id: String,
        unit_id: crate::cognitive_resources::ExecutionUnitId,
        allocation: crate::cognitive_resources::AllocationVariant,
        selection: crate::cognition::scheduler::AllocationSelection,
        handoff_reason: crate::cognitive_resources::HandoffReason,
        transitions: Vec<crate::cognition::task_graph_handoff::AllocationTransition>,
    },
    SubtaskCompleted {
        subtask_id: String,
        provider_id: String,
        checkpoint_id: crate::cognitive_resources::CheckpointId,
    },
    SubtaskRetry {
        subtask_id: String,
        provider_id: String,
        reason_code: String,
    },
    SubtaskOutputObserved {
        subtask_id: String,
        provider_id: String,
    },
    SubtaskFailed {
        subtask_id: String,
        provider_id: Option<String>,
        error_code: String,
    },
    TaskGraphResultReady {
        result: crate::cognition::task_graph::TaskGraphResult,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub task_id: TaskId,
    pub sequence: u32,
    pub state: TaskState,
    #[serde(flatten)]
    pub kind: TaskEventKind,
}
