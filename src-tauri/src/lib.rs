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
        // Before any CurrentRunSessions ID can be registered in this process.
        persistence::conversation::close_orphaned_product_sessions(&conn)
          .map_err(|_| "orphan_session_normalization_failed")?;
        if let Ok(max_id) = persistence::task_history::max_id(&conn) {
          app.state::<std::sync::Arc<luna::runtime::TaskRegistry>>().seed_next_id(max_id);
        }
      }
      app.manage(db);
      let secrets = std::sync::Arc::new(security::secrets::SecretStore::new(directory));
      let gemini = cognition::GeminiRuntime::new(secrets.clone()).map_err(|_| "gemini_http_client_unavailable")?;
      app.manage(secrets);
      app.manage(std::sync::Arc::new(gemini));
      Ok(())
    })
    .manage(std::sync::Arc::new(luna::runtime::TaskRegistry::default()));
  let builder = builder.manage(std::sync::Arc::new(cognition::CognitionRuntime::new()));
  let builder = builder.manage(cognition::gemini_commands::CurrentRunSessions::default());
  #[cfg(debug_assertions)]
  let builder = builder.invoke_handler(tauri::generate_handler![
    luna::start_mock_task, luna::cancel_task,
    security::security_status, security::security_test_store_secret, security::security_test_delete_secret,
    persistence::lr4_status, persistence::lr4_import_private_bootstrap,
    persistence::lr4_create_diagnostic_conversation, persistence::lr4_get_recent_conversation,
    luna::start_mock_cognition_task, luna::cognition_provider_status,
    cognition::gemini_commands::gemini_status, cognition::gemini_commands::gemini_set_api_key,
    cognition::gemini_commands::gemini_delete_api_key, cognition::gemini_commands::gemini_conversation,
    cognition::gemini_commands::create_conversation_session, cognition::gemini_commands::get_conversation_session,
    cognition::gemini_commands::close_conversation_session,
    cognition::gemini_commands::list_conversation_history, cognition::gemini_commands::get_conversation_history_session,
    luna::start_gemini_task,
  ]);
  #[cfg(not(debug_assertions))]
  let builder = builder.invoke_handler(tauri::generate_handler![
    luna::start_mock_task, luna::cancel_task,
    security::security_status,
    cognition::gemini_commands::gemini_status, cognition::gemini_commands::gemini_set_api_key,
    cognition::gemini_commands::gemini_delete_api_key, cognition::gemini_commands::gemini_conversation,
    cognition::gemini_commands::create_conversation_session, cognition::gemini_commands::get_conversation_session,
    cognition::gemini_commands::close_conversation_session,
    cognition::gemini_commands::list_conversation_history, cognition::gemini_commands::get_conversation_history_session,
    luna::start_gemini_task,
  ]);
  builder
    .run(tauri::generate_context!())
    .expect("erro ao executar a janela Tauri");
}
