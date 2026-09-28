import * as THREE from 'three'
import { RenderBudget, type RenderBudgetConfig } from './RenderBudget'
import { SceneDiagnostics } from './SceneDiagnostics'

type SceneCallbacks = {
  onStatusChange: (message: string) => void
  onReadyChange: (ready: boolean) => void
}

export class SceneRuntime {
  readonly scene = new THREE.Scene()
  readonly camera = new THREE.PerspectiveCamera(38, 1, 0.1, 100)
  readonly canvas = document.createElement('canvas')
  private readonly renderer: THREE.WebGLRenderer
  private readonly resizeObserver: ResizeObserver
  private readonly diagnostics: SceneDiagnostics | null
  private readonly renderBudget: RenderBudget
  private frames = 0
  private reportedGlError = false
  updateRenderConfig(config: RenderBudgetConfig): void { this.renderBudget.updateConfig(config) }

  private readonly onContextLost = (event: Event) => {
    event.preventDefault()
    this.callbacks.onReadyChange(false)
    this.callbacks.onStatusChange('Contexto WebGL perdido. Recarregue a janela.')
    console.error('[M0-B] Contexto WebGL perdido')
  }

  private readonly onContextRestored = () => {
    this.callbacks.onStatusChange('WebGL restaurado. Recarregue para refazer a cena.')
    console.warn('[M0-B] Contexto WebGL restaurado; recarregamento necessário')
  }

  constructor(private readonly container: HTMLDivElement, private readonly callbacks: SceneCallbacks) {
    this.camera.position.set(0, 1.50, 4.65)
    this.camera.lookAt(0, 1.40, 0)

    this.scene.add(new THREE.HemisphereLight(0xe9f3ff, 0x38435e, 2.1))
    const keyLight = new THREE.DirectionalLight(0xffffff, 2.4)
    keyLight.position.set(2.5, 5, 4)
    this.scene.add(keyLight)
    const fillLight = new THREE.DirectionalLight(0x83a8ff, 1.1)
    fillLight.position.set(-3, 2, -2)
    this.scene.add(fillLight)

    this.canvas.setAttribute('aria-label', 'Modelo 3D da assistente Luna')
    this.canvas.addEventListener('webglcontextlost', this.onContextLost)
    this.canvas.addEventListener('webglcontextrestored', this.onContextRestored)
    try {
      this.renderer = new THREE.WebGLRenderer({ canvas: this.canvas, antialias: false, alpha: true, powerPreference: 'low-power' })
      this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.5))
      this.renderer.outputColorSpace = THREE.SRGBColorSpace
      this.container.appendChild(this.canvas)
      const gl = this.renderer.getContext()
      const webglVersion = gl.getParameter(gl.VERSION) as string
      const webglRenderer = gl.getParameter(gl.RENDERER) as string
      console.info('[M0-B] WebGL:', webglVersion, '| Renderer:', webglRenderer)
      this.renderBudget = new RenderBudget(() => this.diagnostics?.reportTransition())
      this.diagnostics = import.meta.env.DEV
        ? new SceneDiagnostics(this.container, this.renderer, webglVersion, webglRenderer, () => this.renderBudget.current)
        : null
    } catch (error) {
      this.canvas.removeEventListener('webglcontextlost', this.onContextLost)
      this.canvas.removeEventListener('webglcontextrestored', this.onContextRestored)
      this.canvas.remove()
      throw error
    }

    this.resizeObserver = new ResizeObserver(() => {
      const width = Math.max(this.container.clientWidth, 1)
      const height = Math.max(this.container.clientHeight, 1)
      this.camera.aspect = width / height
      this.camera.updateProjectionMatrix()
      this.renderer.setSize(width, height, false)
      this.diagnostics?.recordResize()
    })
    this.resizeObserver.observe(this.container)
  }

  start(onFrame: (delta: number) => void): void {
    const gl = this.renderer.getContext()
    this.renderBudget.resetClock()
    this.renderer.setAnimationLoop(() => {
      const now = performance.now()
      const delta = this.renderBudget.sample(now)
      this.diagnostics?.recordCallback(delta === null, this.renderBudget.current.mode === 'suspended')
      if (delta === null) return
      const workStartedAt = this.diagnostics ? now : 0
      try {
        onFrame(delta)
        this.renderer.render(this.scene, this.camera)
        this.diagnostics?.recordFrame(workStartedAt, performance.now())
        if (++this.frames % 60 === 0 && !this.reportedGlError) {
          const errorCode = gl.getError()
          if (errorCode !== gl.NO_ERROR) {
            this.reportedGlError = true
            this.callbacks.onStatusChange(`Erro WebGL ${errorCode}. Veja o console.`)
            console.error('[M0-B] Erro WebGL:', errorCode)
          }
        }
      } catch (error) {
        this.renderer.setAnimationLoop(null)
        this.callbacks.onReadyChange(false)
        this.callbacks.onStatusChange('Erro de renderização. Veja o console.')
        console.error('[M0-B] Erro de renderização:', error)
      }
    })
  }

  stop(): void {
    this.renderer.setAnimationLoop(null)
  }

  dispose(): void {
    this.stop()
    this.renderBudget.dispose()
    this.diagnostics?.dispose()
    this.resizeObserver.disconnect()
    this.canvas.removeEventListener('webglcontextlost', this.onContextLost)
    this.canvas.removeEventListener('webglcontextrestored', this.onContextRestored)
    this.renderer.dispose()
    this.canvas.remove()
    this.scene.clear()
  }
}
