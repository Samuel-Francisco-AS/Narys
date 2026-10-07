import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef, useState } from 'react'
import type { PresentationController } from '../presentation/PresentationController'
export type ManualPresentationMode = 'economy' | 'presence'
export type ShellLayout = { leftOpen: boolean; leftWidth: number; rightOpen: boolean; rightWidth: number }
export type ShellSettings = { presentationMode: ManualPresentationMode; layout: ShellLayout }
export const defaultShellLayout: ShellLayout = { leftOpen: true, leftWidth: 208, rightOpen: true, rightWidth: 272 }
export function clampLayout(layout: ShellLayout): ShellLayout {
  const width = (value: number, fallback: number, min: number, max: number) => Number.isFinite(value) ? Math.round(Math.min(max, Math.max(min, value))) : fallback
  return { leftOpen: layout.leftOpen === true, rightOpen: layout.rightOpen === true,
    leftWidth: width(layout.leftWidth, 208, 160, 320), rightWidth: width(layout.rightWidth, 272, 220, 360) }
}
export function useShellPreferences(controller: PresentationController) {
  const [layout, setLayout] = useState(defaultShellLayout)
  const [loaded, setLoaded] = useState(false)
  const [error, setError] = useState('')
  const writeQueue = useRef(Promise.resolve())
  const modeRevision = useRef(0)
  useEffect(() => {
    let live = true, unlisten: (() => void) | undefined
    const revision = modeRevision.current
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
    return () => { live = false; unlisten?.() }
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
      controller.setMode(mode)
      setError('')
    } catch { setError('Não foi possível salvar o modo de apresentação. Tente novamente nas configurações.') }
  }
  return { layout, loaded, error, saveLayout, chooseMode }
}
