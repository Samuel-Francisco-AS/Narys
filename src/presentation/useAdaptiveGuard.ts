import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef } from 'react'
import type { AdaptiveSnapshot } from '../shell/shellPreferences'
import type { PresentationMode } from './PresentationController'

/** Event-driven, boolean only. Draft stays in React; no keystrokes/text cross IPC. */
export function useAdaptiveGuard(snapshot: AdaptiveSnapshot | null, mode: PresentationMode, hasDraft: () => boolean) {
  const activeCheck = useRef<number | null>(null)
  const current = useRef({ snapshot, mode, hasDraft })
  current.current = { snapshot, mode, hasDraft }
  const guarded = hasDraft()
  useEffect(() => {
    if (!snapshot || mode === 'detached' || snapshot.state !== mode) return
    void invoke('report_presentation_ui', { epoch: snapshot.epoch, revision: snapshot.revision, mode, guarded }).catch(() => {})
  }, [snapshot?.epoch, snapshot?.revision, mode, guarded])
  useEffect(() => {
    let live = true, unlisten: (() => void) | undefined
    void listen<number>('adaptive-close-check', event => {
      const value = current.current
      if (!live || !value.snapshot || value.mode !== 'economy') return
      // Freeze input during the final native check. A queued edit cannot follow a safe ack.
      activeCheck.current = event.payload
      document.body.inert = true
      const guarded = value.hasDraft()
      void invoke<boolean>('confirm_auto_close', { epoch: value.snapshot.epoch, token: event.payload, guarded }).then(closing => {
        if (!closing && live && activeCheck.current === event.payload) { activeCheck.current = null; document.body.inert = false }
      }).catch(() => { if (live && activeCheck.current === event.payload) { activeCheck.current = null; document.body.inert = false } })
    }).then(fn => { if (live) unlisten = fn; else fn() }).catch(() => {})
    return () => { live = false; unlisten?.(); activeCheck.current = null; document.body.inert = false }
  }, [])
  // Focus/policy invalidation can arrive while a close handshake is awaiting IPC.
  useEffect(() => { if (snapshot && !snapshot.transitioning && snapshot.pendingToken === null) { activeCheck.current = null; document.body.inert = false } }, [snapshot])
}
