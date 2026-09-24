import { useEffect, useRef } from 'react'
import * as THREE from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'

type Props = {
  waveSignal: number
  onStatusChange: (message: string) => void
  onReadyChange: (ready: boolean) => void
}

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

export default function CharacterScene({ waveSignal, onStatusChange, onReadyChange }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const playWaveRef = useRef<() => void>(() => {})

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    let disposed = false
    let model: THREE.Object3D | undefined
    let mixer: THREE.AnimationMixer | undefined
    let idleAction: THREE.AnimationAction | undefined
    let waveAction: THREE.AnimationAction | undefined
    let waveEndsAt = 0
    let lastFrame = performance.now()
    const scene = new THREE.Scene()
    const camera = new THREE.PerspectiveCamera(38, 1, 0.1, 100)
    camera.position.set(0, 1.55, 4.7)
    camera.lookAt(0, 1.45, 0)

    scene.add(new THREE.HemisphereLight(0xe9f3ff, 0x38435e, 2.1))
    const keyLight = new THREE.DirectionalLight(0xffffff, 2.4)
    keyLight.position.set(2.5, 5, 4)
    scene.add(keyLight)
    const fillLight = new THREE.DirectionalLight(0x83a8ff, 1.1)
    fillLight.position.set(-3, 2, -2)
    scene.add(fillLight)

    const canvas = document.createElement('canvas')
    canvas.setAttribute('aria-label', 'Modelo 3D da assistente Luna')
    canvas.addEventListener('webglcontextlost', onContextLost)
    canvas.addEventListener('webglcontextrestored', onContextRestored)

    function onContextLost(event: Event) {
      event.preventDefault()
      onReadyChange(false)
      onStatusChange('Contexto WebGL perdido. Recarregue a janela.')
      console.error('[M0-B] Contexto WebGL perdido')
    }

    function onContextRestored() {
      onStatusChange('WebGL restaurado. Recarregue para refazer a cena.')
      console.warn('[M0-B] Contexto WebGL restaurado; recarregamento necessário')
    }

    let renderer: THREE.WebGLRenderer
    try {
      renderer = new THREE.WebGLRenderer({ canvas, antialias: false, alpha: true, powerPreference: 'low-power' })
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.5))
      renderer.outputColorSpace = THREE.SRGBColorSpace
      container.appendChild(canvas)
      const gl = renderer.getContext()
      console.info('[M0-B] WebGL:', gl.getParameter(gl.VERSION), '| Renderer:', gl.getParameter(gl.RENDERER))
    } catch (error) {
      console.error('[M0-B] Falha ao iniciar WebGL:', error)
      onStatusChange('WebGL indisponível neste ambiente. Veja o console.')
      return () => canvas.remove()
    }

    const gl = renderer.getContext()
    let frames = 0
    let reportedGlError = false

    const resizeObserver = new ResizeObserver(() => {
      const width = Math.max(container.clientWidth, 1)
      const height = Math.max(container.clientHeight, 1)
      camera.aspect = width / height
      camera.updateProjectionMatrix()
      renderer.setSize(width, height, false)
    })
    resizeObserver.observe(container)

    const raycaster = new THREE.Raycaster()
    const pointer = new THREE.Vector2()
    function onPointerDown(event: PointerEvent) {
      if (!model) return
      const bounds = canvas.getBoundingClientRect()
      pointer.set(
        ((event.clientX - bounds.left) / bounds.width) * 2 - 1,
        -((event.clientY - bounds.top) / bounds.height) * 2 + 1,
      )
      raycaster.setFromCamera(pointer, camera)
      if (raycaster.intersectObject(model, true).length > 0) playWaveRef.current()
    }
    canvas.addEventListener('pointerdown', onPointerDown)

    playWaveRef.current = () => {
      if (!idleAction || !waveAction || !mixer) return
      waveAction.reset()
      waveAction.setLoop(THREE.LoopOnce, 1)
      waveAction.clampWhenFinished = true
      idleAction.fadeOut(0.18)
      waveAction.fadeIn(0.18).play()
      waveEndsAt = mixer.time + waveAction.getClip().duration
      onStatusChange('Luna está acenando.')
    }

    const loader = new GLTFLoader()
    loader.load(
      '/models/Luna.glb',
      (gltf) => {
        if (disposed) {
          disposeCharacter(gltf.scene)
          return
        }
        model = gltf.scene
        const box = new THREE.Box3().setFromObject(model)
        const size = box.getSize(new THREE.Vector3())
        const center = box.getCenter(new THREE.Vector3())
        const scale = 2.9 / size.y
        model.scale.setScalar(scale)
        model.position.set(-center.x * scale, -box.min.y * scale, -center.z * scale)
        scene.add(model)

        const idleClip = THREE.AnimationClip.findByName(gltf.animations, 'Idle')
        const waveClip = THREE.AnimationClip.findByName(gltf.animations, 'Wave')
        if (!idleClip || !waveClip) {
          onStatusChange('Modelo carregado, mas faltam clipes Idle/Wave.')
          console.error('[M0-B] Clipes Idle/Wave ausentes:', gltf.animations.map((clip) => clip.name))
          return
        }
        mixer = new THREE.AnimationMixer(model)
        idleAction = mixer.clipAction(idleClip)
        waveAction = mixer.clipAction(waveClip)
        idleAction.play()
        onReadyChange(true)
        onStatusChange('Luna · WebGL ativo · animação de repouso')
        console.info('[M0-B] GLB carregado; clipes:', gltf.animations.map((clip) => clip.name).join(', '))
      },
      undefined,
      (error) => {
        if (disposed) return
        console.error('[M0-B] Falha ao carregar GLB:', error)
        onStatusChange('Falha ao carregar a personagem. Veja o console.')
      },
    )

    renderer.setAnimationLoop(() => {
      const now = performance.now()
      const delta = Math.min((now - lastFrame) / 1000, 0.05)
      lastFrame = now
      mixer?.update(delta)
      if (mixer && waveEndsAt > 0 && mixer.time >= waveEndsAt) {
        waveAction?.fadeOut(0.25)
        idleAction?.reset().fadeIn(0.25).play()
        waveEndsAt = 0
        onStatusChange('Luna · WebGL ativo · animação de repouso')
      }
      try {
        renderer.render(scene, camera)
        if (++frames % 60 === 0 && !reportedGlError) {
          const errorCode = gl.getError()
          if (errorCode !== gl.NO_ERROR) {
            reportedGlError = true
            onStatusChange(`Erro WebGL ${errorCode}. Veja o console.`)
            console.error('[M0-B] Erro WebGL:', errorCode)
          }
        }
      } catch (error) {
        renderer.setAnimationLoop(null)
        onReadyChange(false)
        onStatusChange('Erro de renderização. Veja o console.')
        console.error('[M0-B] Erro de renderização:', error)
      }
    })

    return () => {
      disposed = true
      playWaveRef.current = () => {}
      renderer.setAnimationLoop(null)
      resizeObserver.disconnect()
      canvas.removeEventListener('pointerdown', onPointerDown)
      canvas.removeEventListener('webglcontextlost', onContextLost)
      canvas.removeEventListener('webglcontextrestored', onContextRestored)
      if (model) scene.remove(model)
      mixer?.stopAllAction()
      if (model) {
        mixer?.uncacheRoot(model)
        disposeCharacter(model)
      }
      renderer.dispose()
      canvas.remove()
    }
  }, [onReadyChange, onStatusChange])

  useEffect(() => {
    if (waveSignal > 0) playWaveRef.current()
  }, [waveSignal])

  return <div className="scene-canvas" ref={containerRef} />
}
