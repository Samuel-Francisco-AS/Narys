import { Channel, invoke } from '@tauri-apps/api/core'
import type { TaskEvent, TaskId } from './types'

export function startConversationTask(sessionId: number, message: string, onEvent: (event: TaskEvent) => void): Promise<TaskId> {
  const channel = new Channel<TaskEvent>(onEvent)
  return invoke<TaskId>('start_conversation_task', { sessionId, message, channel })
}

export type TaskObservation = { taskId: TaskId; sessionId: number; state: TaskEvent['state']; sequence: number; terminal: TaskEvent | null; replayComplete: boolean }
export type InteractionSnapshot = { sessionId: number | null; task: TaskObservation | null }
export function getCurrentInteraction(): Promise<InteractionSnapshot> { return invoke('get_current_interaction') }
export function attachConversationEvents(taskId: TaskId, sessionId: number, afterSequence: number, onEvent: (event: TaskEvent) => void): Promise<TaskObservation> {
  return invoke('attach_conversation_events', { taskId, sessionId, afterSequence, channel: new Channel<TaskEvent>(onEvent) })
}
