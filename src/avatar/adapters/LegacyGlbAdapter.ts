import * as THREE from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { AnimationRegistry } from '../runtime/AnimationRegistry'
import type { AvatarAdapter, LoadedAvatar } from '../runtime/types'

const MODEL_URL = '/models/Luna.glb'

// The candidate has embedded textures: release them on unmount and late loads.
function disposeCharacter(root: THREE.Object3D) {
  const textures = new Set<THREE.Texture>()
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return
    object.geometry.dispose()
    const materials = Array.isArray(object.material) ? object.material : [object.material]
    for (const material of materials) {
      for (const value of Object.values(material)) {
        if (value instanceof THREE.Texture) textures.add(value)
      }
      material.dispose()
    }
  })
  textures.forEach((texture) => texture.dispose())
}

export class LegacyGlbAdapter implements AvatarAdapter {
  load(onLoaded: (avatar: LoadedAvatar) => void, onError: (error: unknown) => void): void {
    new GLTFLoader().load(
      MODEL_URL,
      (gltf) => {
        const root = gltf.scene
        const box = new THREE.Box3().setFromObject(root)
        const size = box.getSize(new THREE.Vector3())
        const center = box.getCenter(new THREE.Vector3())
        const scale = 2.55 / size.y
        root.scale.setScalar(scale)
        root.position.set(-center.x * scale, -box.min.y * scale, -center.z * scale)

        const animations = new AnimationRegistry({
          idle: THREE.AnimationClip.findByName(gltf.animations, 'Idle') ?? undefined,
          greeting: THREE.AnimationClip.findByName(gltf.animations, 'Wave') ?? undefined,
        })
        console.info('[M0-B] GLB carregado; clipes:', gltf.animations.map((clip) => clip.name).join(', '))
        if (!animations.get('idle') || !animations.get('greeting')) {
          console.error('[M0-B] Clipes Idle/Wave ausentes:', gltf.animations.map((clip) => clip.name))
        }
        onLoaded({ root, animations, dispose: () => disposeCharacter(root) })
      },
      undefined,
      onError,
    )
  }
}
