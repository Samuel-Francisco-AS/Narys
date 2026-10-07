import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { ManualPresentationMode, ShellSettings } from '../shell/shellPreferences'
import './settings.css'

type GeneralSettings = { alwaysOnTop: boolean; activeFps: number; backgroundFps: number }
type Update = { settings: GeneralSettings; alwaysOnTopRequested: boolean }
const numberValue = (value: string) => value === '' ? NaN : Number(value)

export default function GeneralSettingsApp() {
  const [presentationMode, setPresentationMode] = useState<ManualPresentationMode | null>(null)
  const [settings, setSettings] = useState<GeneralSettings | null>(null)
  const [active, setActive] = useState('30')
  const [background, setBackground] = useState('24')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => { void invoke<GeneralSettings>('get_general_settings').then(value => {
    setSettings(value); setActive(String(value.activeFps)); setBackground(String(value.backgroundFps))
  }).catch(() => setError('Não foi possível carregar as configurações gerais.')) }, [])
  useEffect(() => { void invoke<ShellSettings>('get_shell_settings').then(value => setPresentationMode(value.presentationMode)).catch(() => setError('Não foi possível carregar Presentation.')) }, [])
  async function savePresentation(mode: ManualPresentationMode) {
    setBusy(true); setError('')
    try { await invoke('update_presentation_mode', { mode }); setPresentationMode(mode); setNotice('Presentation salva e aplicada à janela principal.') }
    catch { setError('Não foi possível salvar Presentation.') }
    finally { setBusy(false) }
  }
  async function save() {
    if (!settings) return
    const activeFps = numberValue(active), backgroundFps = numberValue(background)
    if (![activeFps, backgroundFps].every(value => Number.isInteger(value) && value >= 1 && value <= 60)) {
      setError('FPS deve ser um inteiro de 1 a 60.'); return
    }
    setBusy(true); setError(''); setNotice('')
    try {
      const result = await invoke<Update>('update_general_settings', { settings: { ...settings, activeFps, backgroundFps } })
      setSettings(result.settings)
      setNotice(result.alwaysOnTopRequested ? 'Salvo. Always-on-top solicitado à janela principal; o compositor pode ignorar.' : 'Salvo. A janela principal aplicará o FPS; não foi possível solicitar always-on-top agora.')
    } catch (cause) { setError(`Não foi possível salvar: ${String(cause)}`) }
    finally { setBusy(false) }
  }
  return <main className="settings-page">
    <header><p className="settings-kicker">LUNA · CONFIGURAÇÕES</p><h1>Configurações da Luna</h1><p>Preferências do aplicativo em janelas independentes.</p></header>
    {settings ? <>
      <section className="settings-card"><h2>Presentation</h2>
        <p>Economy é o padrão 2D. Presence carrega Luna 3D por escolha explícita. A cognição permanece igual.</p>
        <label>Modo de apresentação <select aria-label="Modo de apresentação" disabled={busy || presentationMode === null} value={presentationMode ?? 'economy'} onChange={event => void savePresentation(event.target.value as ManualPresentationMode)}><option value="economy">Economy — padrão</option><option value="presence">Presence — opcional</option></select></label>
      </section>
      <section className="settings-card"><h2>Janela</h2>
        <label className="radio"><input type="checkbox" checked={settings.alwaysOnTop} onChange={event => setSettings({ ...settings, alwaysOnTop: event.target.checked })} />Sempre visível sobre outras janelas</label>
        <small>O compositor do sistema pode ignorar esta preferência.</small>
        <p>Click-through: indisponível. Requer mecanismo externo de recuperação seguro.</p>
      </section>
      <section className="settings-card settings-fields"><h2>Performance</h2>
        <label>FPS com janela em foco <small>Inteiro de 1 a 60.</small><input type="number" min="1" max="60" value={active} onChange={event => setActive(event.target.value)} /></label>
        <label>FPS sem foco <small>Inteiro de 1 a 60.</small><input type="number" min="1" max="60" value={background} onChange={event => setBackground(event.target.value)} /></label>
        {numberValue(background) > numberValue(active) && <p className="settings-warning">O FPS sem foco está acima do FPS em foco.</p>}
        <p>Suspenso/oculto: 0 FPS.</p>
      </section>
      <section className="settings-card"><h2>Atalhos</h2><p>Composer: Ctrl+Shift+Space. Funciona enquanto a janela recebe eventos de teclado.</p></section>
      <div className="settings-actions"><button type="button" disabled={busy} onClick={() => void save()}>Salvar configurações gerais</button>{notice && <span role="status">{notice}</span>}</div>
    </> : <p>Carregando…</p>}
    <section className="settings-card"><h2>IA e modelos</h2><p>Provider, modelo e limites de cada papel cognitivo.</p>
      <button type="button" onClick={() => void invoke('open_ai_settings_window').catch(() => setError('Não foi possível abrir IA e modelos.'))}>Abrir IA e modelos</button>
    </section>
    {error && <p role="alert" className="settings-error">{error}</p>}
  </main>
}
