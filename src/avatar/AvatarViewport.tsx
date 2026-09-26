import { useEffect, useRef } from 'react'
import * as THREE from 'three'
import { LegacyGlbAdapter } from './adapters/LegacyGlbAdapter'
import { AnimationDirector } from './runtime/AnimationDirector'
import { AvatarManager } from './runtime/AvatarManager'
import { SceneRuntime } from './runtime/SceneRuntime'
import type { AnimationRequest } from './runtime/types'

type Props = {
  animationRequest: AnimationRequest | null
  onStatusChange: (message: string) => void
  onReadyChange: (ready: boolean) => void
}

export default function AvatarViewport({ animationRequest, onStatusChange, onReadyChange }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const directorRef = useRef<AnimationDirector | null>(null)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    let runtime: SceneRuntime
    try {
      runtime = new SceneRuntime(container, { onStatusChange, onReadyChange })
    } catch (error) {
      console.error('[M0-B] Falha ao iniciar WebGL:', error)
      onStatusChange('WebGL indisponível neste ambiente. Veja o console.')
      return
    }

    const avatar = new AvatarManager(runtime.scene, new LegacyGlbAdapter())
    const raycaster = new THREE.Raycaster()
    const pointer = new THREE.Vector2()
    const onPointerDown = (event: PointerEvent) => {
      const bounds = runtime.canvas.getBoundingClientRect()
      pointer.set(
        ((event.clientX - bounds.left) / bounds.width) * 2 - 1,
        -((event.clientY - bounds.top) / bounds.height) * 2 + 1,
      )
      raycaster.setFromCamera(pointer, runtime.camera)
      if (avatar.hitTest(raycaster)) directorRef.current?.request({ type: 'greeting' })
    }
    runtime.canvas.addEventListener('pointerdown', onPointerDown)

    avatar.load(
      (loaded) => {
        const idleClip = loaded.animations.get('idle')
        const greetingClip = loaded.animations.get('greeting')
        if (!idleClip || !greetingClip) {
          onStatusChange('Modelo carregado, mas faltam clipes Idle/Wave.')
          return
        }
        directorRef.current = new AnimationDirector(loaded.root, idleClip, greetingClip, onStatusChange)
        onReadyChange(true)
        onStatusChange('Luna · WebGL ativo · animação de repouso')
      },
      (error) => {
        console.error('[M0-B] Falha ao carregar GLB:', error)
        onStatusChange('Falha ao carregar a personagem. Veja o console.')
      },
    )

    runtime.start((delta) => directorRef.current?.update(delta))

    return () => {
      runtime.stop()
      runtime.canvas.removeEventListener('pointerdown', onPointerDown)
      directorRef.current?.dispose()
      directorRef.current = null
      avatar.dispose()
      runtime.dispose()
    }
  }, [onReadyChange, onStatusChange])

  useEffect(() => {
    if (animationRequest) directorRef.current?.request(animationRequest.intent)
  }, [animationRequest])

  return <div className="scene-canvas" ref={containerRef} />
}
