use std::sync::Arc;
use serde::Serialize;
use tauri::State;
use crate::security::{audit::{Action, AuditEvent, Outcome}, secrets::{SecretKey, SecretStore}};
use super::gemini::MODEL;
use crate::persistence::{conversation::{self, ConversationSession}, database::Database};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiStatus { pub configured: bool, pub enabled: bool, pub model: &'static str, pub credential_store_available: bool }

#[tauri::command]
pub async fn gemini_status(store: State<'_, Arc<SecretStore>>) -> Result<GeminiStatus, String> {
  let store = store.inner().clone();
  let result = tauri::async_runtime::spawn_blocking(move || store.get_secret(SecretKey::GeminiApiKey).map(|v| v.is_some()))
    .await.map_err(|_| "gemini_status_failed")?;
  Ok(GeminiStatus { configured: result.as_ref().copied().unwrap_or(false), enabled: true, model: MODEL, credential_store_available: result.is_ok() })
}

#[tauri::command]
pub async fn gemini_set_api_key(store: State<'_, Arc<SecretStore>>, api_key: String) -> Result<GeminiStatus, String> {
  let key = api_key.trim();
  if key.is_empty() || key.len() > 512 || key.bytes().any(|b| b.is_ascii_control()) { return Err("gemini_key_invalid".into()); }
  let key = key.as_bytes().to_vec();
  let store = store.inner().clone();
  tauri::async_runtime::spawn_blocking(move || store.set_secret(SecretKey::GeminiApiKey, &key))
    .await.map_err(|_| "gemini_key_store_failed")?.map_err(|e| e.code())?;
  AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded).with_detail("gemini_key_configured").emit();
  Ok(GeminiStatus { configured: true, enabled: true, model: MODEL, credential_store_available: true })
}

#[tauri::command]
pub async fn gemini_delete_api_key(store: State<'_, Arc<SecretStore>>) -> Result<GeminiStatus, String> {
  let store = store.inner().clone();
  tauri::async_runtime::spawn_blocking(move || store.delete_secret(SecretKey::GeminiApiKey))
    .await.map_err(|_| "gemini_key_delete_failed")?.map_err(|e| e.code())?;
  AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded).with_detail("gemini_key_deleted").emit();
  Ok(GeminiStatus { configured: false, enabled: true, model: MODEL, credential_store_available: true })
}

#[tauri::command]
pub async fn gemini_conversation(db: State<'_, Database>) -> Result<Option<ConversationSession>, String> {
  let db = db.inner().clone();
  tauri::async_runtime::spawn_blocking(move || { let conn = db.open().map_err(|e| e.code())?;
    conversation::gemini_session(&conn).map_err(|e| e.code()) })
    .await.map_err(|_| "worker_failed".to_string())?.map_err(str::to_owned)
}
