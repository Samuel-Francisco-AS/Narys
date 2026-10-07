import { Component, lazy, Suspense, useCallback, useState, type ReactNode } from 'react'
import type { AnimationRequest } from '../avatar/runtime/types'
import type { RenderBudgetConfig } from './renderConfig'
import type { PresentationController } from './PresentationController'

type Props = {
  controller: PresentationController
  generation: number
  animationRequest: AnimationRequest | null
  renderConfig: RenderBudgetConfig
  onStatusChange: (message: string) => void
  onReadyChange: (ready: boolean) => void
}

class PresenceErrorBoundary extends Component<{ onError: () => void; onRetry: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() { return { failed: true } }
  componentDidCatch() { this.props.onError() }
  render() {
    return this.state.failed
      ? <p role="alert">Não foi possível carregar a Presence. <button type="button" onClick={this.props.onRetry}>Tentar novamente</button></p>
      : this.props.children
  }
}

/** The sole runtime import boundary for the 3D surface. Interaction stays above it. */
export function PresenceSurface({ controller, generation, onReadyChange, onStatusChange, ...props }: Props) {
  // A fresh lazy wrapper also permits retry after a rejected chunk load.
  const [AvatarViewport] = useState(() => lazy(() => import('../avatar/AvatarViewport')))
  const [runtimeFailed, setRuntimeFailed] = useState(false)
  const ready = useCallback((value: boolean) => {
    if (controller.getSnapshot().generation !== generation || controller.getSnapshot().mode !== 'presence') return
    if (!value) setRuntimeFailed(true)
    onReadyChange(value)
    controller.report(generation, value ? 'ready' : 'error')
  }, [controller, generation, onReadyChange])
  const status = useCallback((message: string) => {
    if (controller.getSnapshot().generation === generation && controller.getSnapshot().mode === 'presence') onStatusChange(message)
  }, [controller, generation, onStatusChange])
  const failed = useCallback(() => {
    ready(false)
    status('Falha ao carregar a Presence.')
  }, [ready, status])
  if (runtimeFailed) return <p role="alert">Presence indisponível. <button type="button" onClick={() => controller.recreatePresence()}>Tentar novamente</button></p>
  return <PresenceErrorBoundary onError={failed} onRetry={() => controller.recreatePresence()}>
    <Suspense fallback={<p role="status">Carregando Presence…</p>}>
      <AvatarViewport {...props} onReadyChange={ready} onStatusChange={status} />
    </Suspense>
  </PresenceErrorBoundary>
}
