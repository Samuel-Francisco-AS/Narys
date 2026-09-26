import type * as THREE from 'three'
import type { AnimationIntent } from './types'

type Capability = AnimationIntent['type']

export class AnimationRegistry {
  private readonly clips = new Map<Capability, THREE.AnimationClip>()

  constructor(clips: Partial<Record<Capability, THREE.AnimationClip>>) {
    for (const capability of ['idle', 'greeting'] as const) {
      const clip = clips[capability]
      if (clip) this.clips.set(capability, clip)
    }
  }

  get(capability: Capability): THREE.AnimationClip | undefined {
    return this.clips.get(capability)
  }
}
