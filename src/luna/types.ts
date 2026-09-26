export type TaskId = number
export type TaskState = 'pending' | 'running' | 'completed' | 'cancelled' | 'failed'
export type TaskStep = 'prepare' | 'verify'

type TaskEventBase = {
  taskId: TaskId
  sequence: number
  state: TaskState
}

export type TaskEvent = TaskEventBase & (
  | { type: 'task_started' }
  | { type: 'step_started'; step: TaskStep }
  | { type: 'step_completed'; step: TaskStep }
  | { type: 'task_completed' }
  | { type: 'task_cancelled' }
  | { type: 'task_failed'; detail: string }
)
