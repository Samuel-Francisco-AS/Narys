import { Channel, invoke, isTauri } from '@tauri-apps/api/core'
import type { TaskEvent, TaskId } from './types'

export const lunaCoreAvailable = isTauri()

export async function startMockTask(onEvent: (event: TaskEvent) => void): Promise<TaskId> {
  const channel = new Channel<TaskEvent>(onEvent)
  return invoke<TaskId>('start_mock_task', { channel })
}

export function cancelTask(taskId: TaskId): Promise<boolean> {
  return invoke<boolean>('cancel_task', { taskId })
}
