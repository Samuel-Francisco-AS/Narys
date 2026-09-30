export type TaskId = number
export type TaskState = 'pending' | 'running' | 'completed' | 'cancelled' | 'failed'
export type TaskStep = 'prepare' | 'verify'
export type CognitiveResult = {
  text: string
  providerId: string
  usage: { providerCalls: number; inputTokens: number; outputTokens: number; totalTokens: number | null; thoughtTokens: number | null; providersUsed: string[]; retries: number; fallbacks: number }
  contextMetadata: { identityVersion: string; memoryCount: number; recentMessageCount: number }
}

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
  | { type: 'context_built'; memory_count: number; recent_message_count: number }
  | { type: 'provider_selected'; provider_id: string; attempt: number; routing_reason: 'fixed' | 'preferred_order' | 'auto_score' | 'auto_affinity'; score: number | null }
  | { type: 'provider_chunk'; provider_id: string; chunk: string }
  | { type: 'provider_retry'; provider_id: string; reason_code: string }
  | { type: 'provider_fallback'; from_provider_id: string; to_provider_id: string; reason_code: string }
  | { type: 'task_result_ready'; result: CognitiveResult }
)
