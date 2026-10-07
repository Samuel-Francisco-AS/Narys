import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef, useState } from 'react'
import type { PresentationController } from '../presentation/PresentationController'
export type ManualPresentationMode = 'economy' | 'presence'
export type PresentationPolicy = ManualPresentationMode | 'headless' | 'auto'
export type AdaptiveSnapshot = { policy: PresentationPolicy; state: ManualPresentationMode | 'headless'; epoch: number; revision: number; pendingToken: number | null; transitioning: boolean; attention: 'approval_required' | 'task_failed' | 'user_input_required' | null }
export type ShellLayout = { leftOpen: boolean; leftWidth: number; rightOpen: boolean; rightWidth: number }
export type ShellSettings = { presentationMode: ManualPresentationMode; presentationPolicy: PresentationPolicy; layout: ShellLayout }
export const defaultShellLayout: ShellLayout = { leftOpen: true, leftWidth: 208, rightOpen: true, rightWidth: 272 }
export function clampLayout(layout: ShellLayout): ShellLayout {
  const width = (value: number, fallback: number, min: number, max: number) => Number.isFinite(value) ? Math.round(Math.min(max, Math.max(min, value))) : fallback
  return { leftOpen: layout.leftOpen === true, rightOpen: layout.rightOpen === true,
    leftWidth: width(layout.leftWidth, 208, 160, 320), rightWidth: width(layout.rightWidth, 272, 220, 360) }
}
export function useShellPreferences(controller: PresentationController) {
  const [layout, setLayout] = useState(defaultShellLayout)
  const [adaptive, setAdaptive] = useState<AdaptiveSnapshot | null>(null)
  const [loaded, setLoaded] = useState(false)
  const [error, setError] = useState('')
  const writeQueue = useRef(Promise.resolve())
  const modeRevision = useRef(0)
  const nativeObserved = useRef(false)
  useEffect(() => {
    let live = true, unlisten: (() => void) | undefined
    const revision = modeRevision.current
    let unlistenAdaptive: (() => void) | undefined
    const apply = (snapshot: AdaptiveSnapshot) => {
      if (!live || !snapshot || !['economy', 'presence', 'headless'].includes(snapshot.state)) return
      modeRevision.current++
      nativeObserved.current = true
      setAdaptive(snapshot)
      const devOverride = import.meta.env.DEV ? new URLSearchParams(window.location.search).get('presentation') : null
      if (snapshot.state !== 'headless' && !devOverride) controller.setMode(snapshot.state)
    }
    void listen<AdaptiveSnapshot>('adaptive-presentation-changed', event => apply(event.payload)).then(async fn => {
      if (!live) { fn(); return }
      unlistenAdaptive = fn
      const runtimeRevision = modeRevision.current
      const snapshot = await invoke<AdaptiveSnapshot>('get_presentation_snapshot')
      if (modeRevision.current === runtimeRevision) apply(snapshot)
    }).catch(() => {})
    // Register first so a settings action during hydration cannot be overwritten.
    void listen<ManualPresentationMode>('presentation-mode-changed', event => {
      if (!live) return
      modeRevision.current++
      if (event.payload === 'economy' || event.payload === 'presence') controller.setMode(event.payload)
    }).then(async fn => {
      if (!live) { fn(); return }
      unlisten = fn
      const settings = await invoke<ShellSettings>('get_shell_settings')
      if (!live) return
      setLayout(clampLayout(settings.layout))
      const devOverride = import.meta.env.DEV ? new URLSearchParams(window.location.search).get('presentation') : null
      if (modeRevision.current === revision && !devOverride) controller.setMode(settings.presentationMode === 'presence' ? 'presence' : 'economy')
    }).catch(() => { if (live) setError('Preferências indisponíveis; Economy usa layout padrão nesta execução.') })
      .finally(() => { if (live) setLoaded(true) })
    return () => { live = false; unlisten?.(); unlistenAdaptive?.() }
  }, [controller])
  const saveLayout = (value: ShellLayout) => {
    const safe = clampLayout(value)
    setLayout(safe)
    // Serialize committed gestures, never write each pointermove.
    writeQueue.current = writeQueue.current.then(() => invoke('update_shell_layout', { layout: safe })).then(() => setError('')).catch(() => setError('Não foi possível persistir o layout.'))
  }
  const chooseMode = async (mode: ManualPresentationMode) => {
    try {
      await invoke('update_presentation_mode', { mode })
      modeRevision.current++
      // The native snapshot is authoritative; legacy fixture fallback only.
      if (!nativeObserved.current) controller.setMode(mode)
      setError('')
    } catch { setError('Não foi possível salvar o modo de apresentação. Tente novamente nas configurações.') }
  }
  return { layout, loaded, error, saveLayout, chooseMode, adaptive }
}
