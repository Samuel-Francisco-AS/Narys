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
        if let Ok(settings) = persistence::general_settings::load(&conn) {
          if let Some(window) = app.get_webview_window("main") { let _ = window.set_always_on_top(settings.always_on_top); }
        }
        // Before any CurrentRunSessions ID can be registered in this process.
        persistence::conversation::close_orphaned_product_sessions(&conn)
          .map_err(|_| "orphan_session_normalization_failed")?;
        persistence::conversation::reset_interrupted_summaries(&conn)
          .map_err(|_| "summary_recovery_failed")?;
        if let Ok(max_id) = persistence::task_history::max_id(&conn) {
          app.state::<std::sync::Arc<luna::runtime::TaskRegistry>>().seed_next_id(max_id);
        }
      }
      app.manage(db.clone());
      let secrets = std::sync::Arc::new(security::secrets::SecretStore::new(directory));
      let timeouts = db.open().ok().and_then(|conn| persistence::gemini_settings::load(&conn).ok()).unwrap_or_default();
      let mut providers = cognition::registry::ProviderRegistry::default();
      let gemini_adapter = std::sync::Arc::new(cognition::gemini::GeminiProvider::new(
        cognition::gemini::GeminiConfig::default(), secrets.clone()
      ).map_err(|_| "gemini_http_client_unavailable")?);
      let gemini_timeouts = gemini_adapter.timeout_handle();
      *gemini_timeouts.write().unwrap_or_else(|p| p.into_inner()) = timeouts.into();
      providers.register(cognition::types::ProviderConfig { id: "gemini".into(), enabled: true, priority: 1,
        capabilities: cognition::types::ProviderCapabilities::text_stream() }, gemini_adapter)
        .expect("unique Gemini ID");
      let runtime = cognition::ProviderRuntime::new(providers);
      let scheduler = runtime.scheduler.clone();
      let available_secrets = secrets.clone();
      let available = std::sync::Arc::new(move || available_secrets.get_secret(security::secrets::SecretKey::GeminiApiKey)
        .ok().flatten().is_some());
      // UIP-6 will configure provider/model by cognitive role. Summary currently
      // routes through the available scheduler; the worker itself is provider agnostic.
      let worker = cognition::summary::SummaryWorker::start(db.clone(), scheduler,
        app.state::<std::sync::Arc<luna::runtime::TaskRegistry>>().inner().clone(), available);
      app.manage(secrets);
      app.manage(std::sync::Arc::new(runtime));
      app.manage(std::sync::Arc::new(cognition::gemini::GeminiTimeoutState { timeouts: gemini_timeouts }));
      app.manage(worker);
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
    cognition::settings::open_general_settings_window, cognition::settings::open_ai_settings_window,
    cognition::settings::get_ai_settings, cognition::settings::update_cognitive_role_policy,
    cognition::settings::update_gemini_timeouts,
    cognition::settings::get_general_settings, cognition::settings::update_general_settings,
    cognition::gemini_commands::gemini_status, cognition::gemini_commands::gemini_set_api_key,
    cognition::gemini_commands::gemini_delete_api_key, cognition::gemini_commands::gemini_conversation,
    cognition::gemini_commands::create_conversation_session, cognition::gemini_commands::get_conversation_session,
    cognition::gemini_commands::close_conversation_session, cognition::gemini_commands::resume_conversation_session,
    cognition::gemini_commands::list_conversation_history, cognition::gemini_commands::get_conversation_history_session,
    luna::start_gemini_task,
  ]);
  #[cfg(not(debug_assertions))]
  let builder = builder.invoke_handler(tauri::generate_handler![
    luna::start_mock_task, luna::cancel_task,
    security::security_status,
    cognition::settings::open_general_settings_window, cognition::settings::open_ai_settings_window,
    cognition::settings::get_ai_settings, cognition::settings::update_cognitive_role_policy,
    cognition::settings::update_gemini_timeouts,
    cognition::settings::get_general_settings, cognition::settings::update_general_settings,
    cognition::gemini_commands::gemini_status, cognition::gemini_commands::gemini_set_api_key,
    cognition::gemini_commands::gemini_delete_api_key, cognition::gemini_commands::gemini_conversation,
    cognition::gemini_commands::create_conversation_session, cognition::gemini_commands::get_conversation_session,
    cognition::gemini_commands::close_conversation_session, cognition::gemini_commands::resume_conversation_session,
    cognition::gemini_commands::list_conversation_history, cognition::gemini_commands::get_conversation_history_session,
    luna::start_gemini_task,
  ]);
  builder
    .run(tauri::generate_context!())
    .expect("erro ao executar a janela Tauri");
}
