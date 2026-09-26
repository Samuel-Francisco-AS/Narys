import { useEffect, useState } from 'react'
import { invoke, isTauri } from '@tauri-apps/api/core'

type SecurityStatus = {
  storeAvailable: boolean
  testSecretConfigured: boolean
  errorCode: string | null
  unlockProtection: string
  legacyKeyPresent: boolean
}

export default function SecurityPanel() {
  const [status, setStatus] = useState<SecurityStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const available = isTauri()

  useEffect(() => {
    if (!available) return
    let active = true
    invoke<SecurityStatus>('security_status').then(
      (result) => { if (active) setStatus(result) },
      () => { if (active) setError('Status de segurança indisponível') },
    )
    return () => { active = false }
  }, [available])

  async function run(command: 'security_test_store_secret' | 'security_test_delete_secret') {
    setBusy(true)
    setError(null)
    try {
      setStatus(await invoke<SecurityStatus>(command))
    } catch {
      setError('Falha no diagnóstico do SecretStore')
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="security-panel" aria-label="Diagnóstico de segurança LR-3">
      <span className="section-label">SECURITY · LR-3</span>
      <p>CSP: configurada · SecretStore: {!available ? 'indisponível no navegador' : !status ? 'verificando' : status.storeAvailable ? 'disponível' : 'indisponível'}</p>
      {available && <p>Unlock protection: {status?.unlockProtection === 'system_credential_store' ? 'cofre do sistema' : status?.unlockProtection === 'legacy_pending_migration' ? 'migração pendente' : 'indisponível'} · Legacy key: {status?.legacyKeyPresent ? 'presente' : 'ausente'}</p>}
      {available && <p>Segredo artificial: {status?.testSecretConfigured ? 'armazenado' : 'ausente'}</p>}
      {status?.errorCode && <p className="luna-core-error" role="alert">Falha de segurança: {status.errorCode}</p>}
      {error && <p className="luna-core-error" role="alert">{error}</p>}
      {available && import.meta.env.DEV && (
        <div className="luna-core-controls">
          <button type="button" className="secondary-button" disabled={busy || !status?.storeAvailable} onClick={() => void run('security_test_store_secret')}>Testar storage</button>
          <button type="button" className="secondary-button" disabled={busy || !status?.testSecretConfigured} onClick={() => void run('security_test_delete_secret')}>Remover teste</button>
        </div>
      )}
    </section>
  )
}
