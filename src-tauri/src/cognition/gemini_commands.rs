use std::sync::Arc;
use serde::Serialize;
use tauri::State;
use crate::security::{audit::{Action, AuditEvent, Outcome}, secrets::{SecretKey, SecretStore}};
use crate::persistence::{conversation::{self, ConversationHistoryItem, ConversationSession}, database::Database};
use std::collections::HashSet;
use std::sync::Mutex;
use crate::luna::runtime::TaskRegistry;
use super::{summary::SummaryWorker, GeminiRuntime};

#[derive(Default)]
pub struct CurrentRunSessions(pub Mutex<HashSet<i64>>);

pub fn resume_registered_session(db: &Database, sessions: &CurrentRunSessions, registry: &TaskRegistry,
  target_session_id: i64, current_session_id: Option<i64>) -> Result<ConversationSession, String> {
  if target_session_id <= 0 || current_session_id.is_some_and(|id| id <= 0 || id == target_session_id) { return Err("session_invalid".into()); }
  let mut current_run = sessions.0.lock().map_err(|_| "session_registry_failed")?;
  if current_session_id.is_none() && !current_run.is_empty() { return Err("session_invalid".into()); }
  if current_session_id.is_some_and(|id| !current_run.contains(&id)) { return Err("session_invalid".into()); }
  if current_session_id.is_some_and(|id| registry.has_foreground_provider_work_for_session(id)) { return Err("session_busy".into()); }
  let mut conn = db.open().map_err(|e| e.code())?;
  let resumed = conversation::resume_session(&mut conn, target_session_id, current_session_id).map_err(str::to_owned)?;
  if let Some(id) = current_session_id { current_run.remove(&id); }
  current_run.insert(target_session_id);
  Ok(resumed)
}

#[tauri::command]
pub fn resume_conversation_session(db: State<'_, Database>, sessions: State<'_, CurrentRunSessions>,
  registry: State<'_, Arc<TaskRegistry>>, worker: State<'_, Arc<SummaryWorker>>, target_session_id: i64, current_session_id: Option<i64>) -> Result<ConversationSession, String> {
  let resumed = resume_registered_session(&db, &sessions, &registry, target_session_id, current_session_id)?;
  if current_session_id.is_some() { worker.kick(); }
  Ok(resumed)
}

#[tauri::command]
pub async fn list_conversation_history(db: State<'_, Database>) -> Result<Vec<ConversationHistoryItem>, String> {
  let db = db.inner().clone();
  tauri::async_runtime::spawn_blocking(move || {
    let conn = db.open().map_err(|e| e.code())?;
    conversation::list_history(&conn, 50).map_err(|e| e.code())
  }).await.map_err(|_| "worker_failed".to_string())?.map_err(str::to_owned)
}

#[tauri::command]
pub async fn get_conversation_history_session(db: State<'_, Database>, session_id: i64) -> Result<ConversationSession, String> {
  if session_id <= 0 { return Err("session_invalid".into()); }
  let db = db.inner().clone();
  tauri::async_runtime::spawn_blocking(move || {
    let conn = db.open().map_err(|e| e.code())?;
    conversation::history_session(&conn, session_id).map_err(|e| e.code())?.ok_or("session_invalid")
  }).await.map_err(|_| "worker_failed".to_string())?.map_err(str::to_owned)
}

#[tauri::command]
pub async fn create_conversation_session(db: State<'_, Database>, sessions: State<'_, CurrentRunSessions>) -> Result<i64, String> {
  let db = db.inner().clone();
  let id = tauri::async_runtime::spawn_blocking(move || {
    let conn = db.open().map_err(|e| e.code())?;
    conversation::create_session(&conn).map_err(|e| e.code())
  }).await.map_err(|_| "worker_failed")?.map_err(str::to_owned)?;
  sessions.0.lock().map_err(|_| "session_registry_failed")?.insert(id);
  Ok(id)
}

#[tauri::command]
pub async fn get_conversation_session(db: State<'_, Database>, sessions: State<'_, CurrentRunSessions>, session_id: i64) -> Result<ConversationSession, String> {
  if session_id <= 0 || !sessions.0.lock().map_err(|_| "session_registry_failed")?.contains(&session_id) { return Err("session_invalid".into()); }
  let db = db.inner().clone();
  tauri::async_runtime::spawn_blocking(move || {
    let conn = db.open().map_err(|e| e.code())?;
    conversation::session(&conn, session_id).map_err(|e| e.code())?.ok_or("session_invalid")
  }).await.map_err(|_| "worker_failed".to_string())?.map_err(str::to_owned)
}

#[tauri::command]
pub async fn close_conversation_session(db: State<'_, Database>, sessions: State<'_, CurrentRunSessions>, worker: State<'_, Arc<SummaryWorker>>, session_id: i64) -> Result<(), String> {
  if session_id <= 0 || !sessions.0.lock().map_err(|_| "session_registry_failed")?.contains(&session_id) { return Err("session_invalid".into()); }
  let db = db.inner().clone();
  let closed = tauri::async_runtime::spawn_blocking(move || {
    let conn = db.open().map_err(|e| e.code())?;
    conversation::close_session(&conn, session_id).map_err(|e| e.code())
  }).await.map_err(|_| "worker_failed")?.map_err(str::to_owned)?;
  if !closed { return Err("session_invalid".into()); }
  sessions.0.lock().map_err(|_| "session_registry_failed")?.remove(&session_id);
  worker.kick();
  Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiStatus { pub configured: bool, pub enabled: bool, pub credential_store_available: bool, pub cooldown_ms: u64 }

fn gemini_cooldown_ms(runtime: &GeminiRuntime) -> u64 {
  runtime.scheduler.status().into_iter().find(|status| status.id == "gemini").map(|status| status.cooldown_ms).unwrap_or(0)
}

#[tauri::command]
pub async fn gemini_status(store: State<'_, Arc<SecretStore>>, runtime: State<'_, Arc<GeminiRuntime>>) -> Result<GeminiStatus, String> {
  let store = store.inner().clone();
  let result = tauri::async_runtime::spawn_blocking(move || store.get_secret(SecretKey::GeminiApiKey).map(|v| v.is_some()))
    .await.map_err(|_| "gemini_status_failed")?;
  Ok(GeminiStatus { configured: result.as_ref().copied().unwrap_or(false), enabled: true, credential_store_available: result.is_ok(), cooldown_ms: gemini_cooldown_ms(&runtime) })
}

#[tauri::command]
pub async fn gemini_set_api_key(store: State<'_, Arc<SecretStore>>, worker: State<'_, Arc<SummaryWorker>>, runtime: State<'_, Arc<GeminiRuntime>>, api_key: String) -> Result<GeminiStatus, String> {
  let key = api_key.trim();
  if key.is_empty() || key.len() > 512 || key.bytes().any(|b| b.is_ascii_control()) { return Err("gemini_key_invalid".into()); }
  let key = key.as_bytes().to_vec();
  let store = store.inner().clone();
  tauri::async_runtime::spawn_blocking(move || store.set_secret(SecretKey::GeminiApiKey, &key))
    .await.map_err(|_| "gemini_key_store_failed")?.map_err(|e| e.code())?;
  AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded).with_detail("gemini_key_configured").emit();
  worker.kick();
  Ok(GeminiStatus { configured: true, enabled: true, credential_store_available: true, cooldown_ms: gemini_cooldown_ms(&runtime) })
}

#[tauri::command]
pub async fn gemini_delete_api_key(store: State<'_, Arc<SecretStore>>, worker: State<'_, Arc<SummaryWorker>>, runtime: State<'_, Arc<GeminiRuntime>>) -> Result<GeminiStatus, String> {
  let store = store.inner().clone();
  tauri::async_runtime::spawn_blocking(move || store.delete_secret(SecretKey::GeminiApiKey))
    .await.map_err(|_| "gemini_key_delete_failed")?.map_err(|e| e.code())?;
  AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded).with_detail("gemini_key_deleted").emit();
  worker.kick();
  Ok(GeminiStatus { configured: false, enabled: true, credential_store_available: true, cooldown_ms: gemini_cooldown_ms(&runtime) })
}

#[tauri::command]
pub async fn gemini_conversation(db: State<'_, Database>) -> Result<Option<ConversationSession>, String> {
  let db = db.inner().clone();
  tauri::async_runtime::spawn_blocking(move || { let conn = db.open().map_err(|e| e.code())?;
    conversation::gemini_session(&conn).map_err(|e| e.code()) })
    .await.map_err(|_| "worker_failed".to_string())?.map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::{SystemTime, UNIX_EPOCH};

  #[test]
  fn resume_registry_changes_only_after_db_success() {
    let n=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir=std::env::temp_dir().join(format!("uip5b-registry-{}-{n}",std::process::id()));
    let db=Database::for_test(dir.join("test.sqlite3"));
    let mut conn=db.open().unwrap();
    let a=conversation::create_session(&conn).unwrap(); let b=conversation::create_session(&conn).unwrap();
    let legacy=conversation::create_diagnostic(&mut conn).unwrap();
    conversation::append_exchange_to_session(&mut conn,b,"ORQUIDEA-71","Entendido.").unwrap();
    conversation::close_session(&conn,b).unwrap();
    let sessions=CurrentRunSessions::default(); let registry=TaskRegistry::default();
    sessions.0.lock().unwrap().insert(a);
    for id in [0,legacy,a] {
      assert!(resume_registered_session(&db,&sessions,&registry,id,Some(a)).is_err());
      assert_eq!(*sessions.0.lock().unwrap(),HashSet::from([a]));
    }
    assert!(resume_registered_session(&db,&sessions,&registry,b,Some(legacy)).is_err());
    assert_eq!(*sessions.0.lock().unwrap(),HashSet::from([a]));
    assert!(resume_registered_session(&db,&sessions,&registry,b,None).is_err());
    registry.mark_foreground_provider_work_for_test(a);
    assert_eq!(resume_registered_session(&db,&sessions,&registry,b,Some(a)).err().as_deref(),Some("session_busy"));
    assert_eq!(*sessions.0.lock().unwrap(),HashSet::from([a]));
    registry.foreground_provider_work_for_test_clear(a);
    conn.execute_batch("CREATE TRIGGER reject_resume_registry BEFORE UPDATE ON conversation_sessions WHEN NEW.status='active' AND OLD.status='closed' BEGIN SELECT RAISE(ABORT, 'synthetic'); END;").unwrap();
    assert_eq!(resume_registered_session(&db,&sessions,&registry,b,Some(a)).err().as_deref(),Some("write_failed"));
    assert_eq!(*sessions.0.lock().unwrap(),HashSet::from([a]));
    assert!(conversation::is_active_session(&conn,a).unwrap());
    conn.execute_batch("DROP TRIGGER reject_resume_registry").unwrap();
    let result=resume_registered_session(&db,&sessions,&registry,b,Some(a)).unwrap();
    assert_eq!(result.messages[0].content,"ORQUIDEA-71");
    assert_eq!(*sessions.0.lock().unwrap(),HashSet::from([b]));
    conversation::close_session(&conn,b).unwrap();
    sessions.0.lock().unwrap().clear();
    resume_registered_session(&db,&sessions,&registry,b,None).unwrap();
    assert_eq!(*sessions.0.lock().unwrap(),HashSet::from([b]));
  }
}
