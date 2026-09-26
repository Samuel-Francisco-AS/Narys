pub mod audit;
pub mod secrets;
pub mod validation;

use serde::Serialize;
use tauri::State;

use audit::{Action, AuditEvent, Outcome};
use secrets::{SecretKey, SecretStore};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityStatus {
  store_available: bool,
  test_secret_configured: bool,
  error_code: Option<&'static str>,
}

#[tauri::command]
pub fn security_status(store: State<'_, SecretStore>) -> SecurityStatus {
  AuditEvent::new(Action::CommandInvoked, Outcome::Allowed).with_detail("security_status").emit();
  match store.get_secret(SecretKey::Lr3Test) {
    Ok(secret) => SecurityStatus {
      store_available: true,
      test_secret_configured: secret.is_some(),
      error_code: None,
    },
    Err(error) => {
      AuditEvent::new(Action::SecurityError, Outcome::Failed).with_detail(error.code()).emit();
      SecurityStatus {
        store_available: false,
        test_secret_configured: false,
        error_code: Some(error.code()),
      }
    }
  }
}

// LR-3 diagnostic only. Release builds reject these commands before storage access.
#[tauri::command]
pub fn security_test_store_secret(store: State<'_, SecretStore>) -> Result<SecurityStatus, String> {
  AuditEvent::new(Action::CommandInvoked, Outcome::Allowed).with_detail("security_test_store_secret").emit();
  if !cfg!(debug_assertions) {
    AuditEvent::new(Action::SecurityError, Outcome::Denied).with_detail("diagnostic_disabled").emit();
    return Err("Diagnóstico indisponível nesta compilação".into());
  }
  const TEST_VALUE: &[u8] = b"lr3_test_secret";
  store.set_secret(SecretKey::Lr3Test, TEST_VALUE).map_err(|error| {
    AuditEvent::new(Action::SecurityError, Outcome::Failed).with_detail(error.code()).emit();
    "Falha ao gravar segredo de teste".to_string()
  })?;
  let recovered = store.get_secret(SecretKey::Lr3Test).map_err(|error| {
    AuditEvent::new(Action::SecurityError, Outcome::Failed).with_detail(error.code()).emit();
    "Falha ao ler segredo de teste".to_string()
  })?;
  if recovered.as_deref() != Some(TEST_VALUE) {
    AuditEvent::new(Action::SecurityError, Outcome::Failed).with_detail("roundtrip_mismatch").emit();
    return Err("Falha na verificação do segredo de teste".into());
  }
  AuditEvent::new(Action::SecretTestWritten, Outcome::Succeeded).emit();
  Ok(SecurityStatus { store_available: true, test_secret_configured: true, error_code: None })
}

#[tauri::command]
pub fn security_test_delete_secret(store: State<'_, SecretStore>) -> Result<SecurityStatus, String> {
  AuditEvent::new(Action::CommandInvoked, Outcome::Allowed).with_detail("security_test_delete_secret").emit();
  if !cfg!(debug_assertions) {
    AuditEvent::new(Action::SecurityError, Outcome::Denied).with_detail("diagnostic_disabled").emit();
    return Err("Diagnóstico indisponível nesta compilação".into());
  }
  store.delete_secret(SecretKey::Lr3Test).map_err(|error| {
    AuditEvent::new(Action::SecurityError, Outcome::Failed).with_detail(error.code()).emit();
    "Falha ao remover segredo de teste".to_string()
  })?;
  AuditEvent::new(Action::SecretTestDeleted, Outcome::Succeeded).emit();
  Ok(SecurityStatus { store_available: true, test_secret_configured: false, error_code: None })
}
