use database::Database;
pub use narys_domain::persistence::*;
use std::path::PathBuf;
use tauri::State;
#[tauri::command]
pub async fn lr4_status(db: State<'_, Database>) -> Result<Lr4Status, String> {
    let db = db.inner().clone();
    Ok(
        match tauri::async_runtime::spawn_blocking(move || status(&db)).await {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => unavailable(error.code()),
            Err(_) => unavailable("worker_failed"),
        },
    )
}
fn unavailable(code: &'static str) -> Lr4Status {
    Lr4Status {
        database_available: false,
        error_code: Some(code),
        identity_name: None,
        identity_version: None,
        memory_count: 0,
        conversation_count: 0,
        task_count: 0,
        memory_titles: vec![],
    }
}
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn lr4_import_private_bootstrap(db: State<'_, Database>) -> Result<ImportResult, String> {
    let db = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("private/luna-bootstrap.private.json");
        import_bootstrap(&db, &path)
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(|e| e.code().to_string())
}
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn lr4_create_diagnostic_conversation(db: State<'_, Database>) -> Result<i64, String> {
    let db = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = db.open()?;
        conversation::create_diagnostic(&mut conn)
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(|e| e.code().to_string())
}
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn lr4_get_recent_conversation(
    db: State<'_, Database>,
) -> Result<Option<conversation::ConversationSession>, String> {
    let db = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open()?;
        conversation::recent(&conn)
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(|e| e.code().to_string())
}
