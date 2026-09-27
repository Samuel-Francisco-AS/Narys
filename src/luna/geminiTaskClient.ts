import { Channel, invoke } from '@tauri-apps/api/core'
import type { TaskEvent, TaskId } from './types'

export function startGeminiTask(sessionId: number, message: string, onEvent: (event: TaskEvent) => void): Promise<TaskId> {
  const channel = new Channel<TaskEvent>(onEvent)
  return invoke<TaskId>('start_gemini_task', { sessionId, message, channel })
}
