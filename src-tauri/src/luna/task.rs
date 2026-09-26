use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
  Pending,
  Running,
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

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEventKind {
  TaskStarted,
  StepStarted { step: TaskStep },
  StepCompleted { step: TaskStep },
  TaskCompleted,
  TaskCancelled,
  TaskFailed { detail: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
  pub task_id: TaskId,
  pub sequence: u32,
  pub state: TaskState,
  #[serde(flatten)]
  pub kind: TaskEventKind,
}
