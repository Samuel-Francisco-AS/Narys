mod luna;
mod cognition;
mod persistence;
mod security;

use tauri::Manager;

pub fn run() {
  let builder = tauri::Builder::default()
    .setup(|app| {
      let directory = app.path().app_local_data_dir()?;
      let db = persistence::database::Database::new(directory.clone());
      if let Ok(conn) = db.open() {
        if let Ok(max_id) = persistence::task_history::max_id(&conn) {
          app.state::<std::sync::Arc<luna::runtime::TaskRegistry>>().seed_next_id(max_id);
        }
      }
      app.manage(db);
      app.manage(security::secrets::SecretStore::new(directory));
      Ok(())
    })
    .manage(std::sync::Arc::new(luna::runtime::TaskRegistry::default()));
  let builder = builder.manage(std::sync::Arc::new(cognition::CognitionRuntime::new()));
  let builder = builder;
  #[cfg(debug_assertions)]
  let builder = builder.invoke_handler(tauri::generate_handler![
    luna::start_mock_task, luna::cancel_task,
    security::security_status, security::security_test_store_secret, security::security_test_delete_secret,
    persistence::lr4_status, persistence::lr4_import_private_bootstrap,
    persistence::lr4_create_diagnostic_conversation, persistence::lr4_get_recent_conversation,
    luna::start_mock_cognition_task, luna::cognition_provider_status,
  ]);
  #[cfg(not(debug_assertions))]
  let builder = builder.invoke_handler(tauri::generate_handler![
    luna::start_mock_task, luna::cancel_task,
    security::security_status, security::security_test_store_secret, security::security_test_delete_secret,
    persistence::lr4_status,
  ]);
  builder
    .run(tauri::generate_context!())
    .expect("erro ao executar a janela Tauri");
}
