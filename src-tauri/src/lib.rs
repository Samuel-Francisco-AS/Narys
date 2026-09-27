mod luna;
mod cognition;
mod persistence;
mod security;
mod auxiliary_poc;

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
      let secrets = std::sync::Arc::new(security::secrets::SecretStore::new(directory));
      let gemini = cognition::GeminiRuntime::new(secrets.clone()).map_err(|_| "gemini_http_client_unavailable")?;
      app.manage(secrets);
      app.manage(std::sync::Arc::new(gemini));
      #[cfg(debug_assertions)]
      if let Some(main) = app.get_webview_window("main") {
        eprintln!("[UIP-4-FIX-2A] main startup inner={:?} outer={:?} scale={:?}", main.inner_size(), main.outer_size(), main.scale_factor());
        let app_handle = app.handle().clone();
        main.on_window_event(move |event| match event {
          tauri::WindowEvent::Resized(size) => eprintln!("[UIP-4-FIX-2A] main size={size:?}"),
          tauri::WindowEvent::Moved(position) => eprintln!("[UIP-4-FIX-2A] main move={position:?}"),
          tauri::WindowEvent::Focused(focused) => eprintln!("[UIP-4-FIX-2A] main focus={focused}"),
          tauri::WindowEvent::Destroyed => { eprintln!("[UIP-4-FIX-2A] main destroyed; exiting POC"); app_handle.exit(0); },
          _ => {}
        });
      }
      Ok(())
    })
    .manage(std::sync::Arc::new(luna::runtime::TaskRegistry::default()));
  let builder = builder.manage(std::sync::Arc::new(cognition::CognitionRuntime::new()));
  let builder = builder.manage(cognition::gemini_commands::CurrentRunSessions::default());
  #[cfg(debug_assertions)]
  let builder = builder.invoke_handler(tauri::generate_handler![
    auxiliary_poc::set_auxiliary_poc_visible,
    luna::start_mock_task, luna::cancel_task,
    security::security_status, security::security_test_store_secret, security::security_test_delete_secret,
    persistence::lr4_status, persistence::lr4_import_private_bootstrap,
    persistence::lr4_create_diagnostic_conversation, persistence::lr4_get_recent_conversation,
    luna::start_mock_cognition_task, luna::cognition_provider_status,
    cognition::gemini_commands::gemini_status, cognition::gemini_commands::gemini_set_api_key,
    cognition::gemini_commands::gemini_delete_api_key, cognition::gemini_commands::gemini_conversation,
    cognition::gemini_commands::create_conversation_session, cognition::gemini_commands::get_conversation_session,
    cognition::gemini_commands::close_conversation_session, luna::start_gemini_task,
  ]);
  #[cfg(not(debug_assertions))]
  let builder = builder.invoke_handler(tauri::generate_handler![
    auxiliary_poc::set_auxiliary_poc_visible,
    luna::start_mock_task, luna::cancel_task,
    security::security_status,
    cognition::gemini_commands::gemini_status, cognition::gemini_commands::gemini_set_api_key,
    cognition::gemini_commands::gemini_delete_api_key, cognition::gemini_commands::gemini_conversation,
    cognition::gemini_commands::create_conversation_session, cognition::gemini_commands::get_conversation_session,
    cognition::gemini_commands::close_conversation_session, luna::start_gemini_task,
  ]);
  builder
    .run(tauri::generate_context!())
    .expect("erro ao executar a janela Tauri");
}
