import type * as THREE from 'three'
import type { AnimationRegistry } from './AnimationRegistry'

export type AnimationIntent = { type: 'idle' } | { type: 'greeting' }

export type AnimationRequest = {
  id: number
  intent: AnimationIntent
}

export type LoadedAvatar = {
  root: THREE.Object3D
  animations: AnimationRegistry
  dispose: () => void
}

export type AvatarAdapter = {
  load: (onLoaded: (avatar: LoadedAvatar) => void, onError: (error: unknown) => void) => void
}
