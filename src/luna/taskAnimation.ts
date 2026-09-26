import type { AnimationIntent } from '../avatar/runtime/types'
import type { TaskEvent } from './types'

// The current GLB only supports idle and greeting. Completion uses greeting as
// a provisional acknowledgement; working, cancellation and failure stay neutral.
export function taskEventToAnimationIntent(event: TaskEvent): AnimationIntent | null {
  switch (event.type) {
    case 'task_started':
    case 'step_started':
    case 'task_cancelled':
    case 'task_failed':
      return { type: 'idle' }
    case 'task_completed':
      return { type: 'greeting' }
    case 'step_completed':
      return null
  }
}
