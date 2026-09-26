import * as THREE from 'three'
import type { AnimationIntent } from './types'

export class AnimationDirector {
  private readonly mixer: THREE.AnimationMixer
  private readonly idleAction: THREE.AnimationAction
  private readonly greetingAction: THREE.AnimationAction
  private greetingEndsAt = 0
  private greetingReadyAt = 0

  constructor(
    private readonly root: THREE.Object3D,
    idleClip: THREE.AnimationClip,
    greetingClip: THREE.AnimationClip,
    private readonly onStatusChange: (message: string) => void,
  ) {
    this.mixer = new THREE.AnimationMixer(root)
    this.idleAction = this.mixer.clipAction(idleClip)
    this.greetingAction = this.mixer.clipAction(greetingClip)
    this.idleAction.play()
  }

  request(intent: AnimationIntent): void {
    if (intent.type === 'idle') return // Idle already plays or resumes after the current greeting.
    if (this.greetingEndsAt > 0 || this.mixer.time < this.greetingReadyAt) return
    this.greetingAction.reset()
    this.greetingAction.setLoop(THREE.LoopOnce, 1)
    this.greetingAction.clampWhenFinished = true
    this.idleAction.fadeOut(0.26)
    this.greetingAction.fadeIn(0.26).play()
    this.greetingEndsAt = this.mixer.time + this.greetingAction.getClip().duration - 0.32
    this.onStatusChange('Luna está acenando.')
  }

  update(delta: number): void {
    this.mixer.update(delta)
    if (this.greetingEndsAt > 0 && this.mixer.time >= this.greetingEndsAt) {
      this.greetingAction.fadeOut(0.32)
      this.idleAction.enabled = true
      this.idleAction.fadeIn(0.32).play()
      this.greetingReadyAt = this.mixer.time + 0.32
      this.greetingEndsAt = 0
      this.onStatusChange('Luna · WebGL ativo · animação de repouso')
    }
  }

  dispose(): void {
    this.mixer.stopAllAction()
    this.mixer.uncacheRoot(this.root)
  }
}
