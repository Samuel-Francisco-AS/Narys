import * as THREE from 'three'
import type { AvatarAdapter, LoadedAvatar } from './types'

export class AvatarManager {
  private avatar: LoadedAvatar | undefined
  private disposed = false

  constructor(private readonly scene: THREE.Scene, private readonly adapter: AvatarAdapter) {}

  load(onLoaded: (avatar: LoadedAvatar) => void, onError: (error: unknown) => void): void {
    this.adapter.load(
      (avatar) => {
        if (this.disposed) {
          avatar.dispose()
          return
        }
        this.avatar = avatar
        this.scene.add(avatar.root)
        onLoaded(avatar)
      },
      (error) => {
        if (!this.disposed) onError(error)
      },
    )
  }

  hitTest(raycaster: THREE.Raycaster): boolean {
    return !!this.avatar && raycaster.intersectObject(this.avatar.root, true).length > 0
  }

  dispose(): void {
    this.disposed = true
    if (!this.avatar) return
    this.scene.remove(this.avatar.root)
    this.avatar.dispose()
    this.avatar = undefined
  }
}
