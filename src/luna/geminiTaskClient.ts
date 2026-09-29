import { Channel, invoke } from '@tauri-apps/api/core'
import type { TaskEvent, TaskId } from './types'

export function startConversationTask(sessionId: number, message: string, onEvent: (event: TaskEvent) => void): Promise<TaskId> {
  const channel = new Channel<TaskEvent>(onEvent)
  return invoke<TaskId>('start_conversation_task', { sessionId, message, channel })
}
