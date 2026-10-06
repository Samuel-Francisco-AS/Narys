mod agents;
mod cognition;
pub mod cognitive_resources;
mod luna;
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
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_always_on_top(settings.always_on_top);
                    }
                }
                // Before any CurrentRunSessions ID can be registered in this process.
                persistence::conversation::close_orphaned_product_sessions(&conn)
                    .map_err(|_| "orphan_session_normalization_failed")?;
                persistence::conversation::reset_interrupted_summaries(&conn)
                    .map_err(|_| "summary_recovery_failed")?;
                if let Ok(max_id) = persistence::task_history::max_id(&conn) {
                    app.state::<std::sync::Arc<luna::runtime::TaskRegistry>>()
                        .seed_next_id(max_id);
                }
            }
            app.manage(db.clone());
            let secrets = std::sync::Arc::new(security::secrets::SecretStore::new(directory));
            let conn = db.open().map_err(|_| "provider_settings_unavailable")?;
            let mut providers = cognition::registry::ProviderRegistry::default();
            let gemini_adapter = std::sync::Arc::new(
                cognition::gemini::GeminiProvider::new(
                    cognition::gemini::GeminiConfig::default(),
                    secrets.clone(),
                )
                .map_err(|_| "gemini_http_client_unavailable")?,
            );
            let gemini_timeouts = gemini_adapter.timeout_handle();
            *gemini_timeouts.write().unwrap_or_else(|p| p.into_inner()) =
                persistence::provider_timeouts::load(&conn, "gemini")
                    .map_err(|_| "provider_settings_unavailable")?;
            providers
                .register(
                    cognition::types::ProviderConfig {
                        id: "gemini".into(),
                        enabled: true,
                        priority: 1,
                        capabilities: cognition::types::ProviderCapabilities::text_stream(),
                    },
                    gemini_adapter,
                )
                .expect("unique Gemini ID");
            let groq_adapter = std::sync::Arc::new(
                cognition::groq::GroqProvider::new(
                    cognition::groq::GroqConfig::default(),
                    secrets.clone(),
                )
                .map_err(|_| "groq_http_client_unavailable")?,
            );
            let groq_timeouts = groq_adapter.timeout_handle();
            *groq_timeouts.write().unwrap_or_else(|p| p.into_inner()) =
                persistence::provider_timeouts::load(&conn, "groq")
                    .map_err(|_| "provider_settings_unavailable")?;
            providers
                .register(
                    cognition::types::ProviderConfig {
                        id: "groq".into(),
                        enabled: true,
                        priority: 2,
                        capabilities: cognition::types::ProviderCapabilities::with_structured_output(),
                    },
                    groq_adapter,
                )
                .expect("unique Groq ID");
            let mistral_adapter = std::sync::Arc::new(
                cognition::mistral::MistralProvider::new(
                    cognition::mistral::MistralConfig::default(),
                    secrets.clone(),
                )
                .map_err(|_| "mistral_http_client_unavailable")?,
            );
            let mistral_timeouts = mistral_adapter.timeout_handle();
            *mistral_timeouts.write().unwrap_or_else(|p| p.into_inner()) =
                persistence::provider_timeouts::load(&conn, "mistral")
                    .map_err(|_| "provider_settings_unavailable")?;
            providers
                .register(
                    cognition::types::ProviderConfig {
                        id: "mistral".into(),
                        enabled: true,
                        priority: 3,
                        capabilities: cognition::types::ProviderCapabilities::text_stream(),
                    },
                    mistral_adapter,
                )
                .expect("unique Mistral ID");
            let cloudflare_adapter = std::sync::Arc::new(
                cognition::cloudflare::CloudflareProvider::new(
                    cognition::cloudflare::CloudflareConfig::default(),
                    secrets.clone(),
                )
                .map_err(|_| "cloudflare_http_client_unavailable")?,
            );
            let cloudflare_timeouts = cloudflare_adapter.timeout_handle();
            *cloudflare_timeouts.write().unwrap_or_else(|p| p.into_inner()) =
                persistence::provider_timeouts::load(&conn, "cloudflare")
                    .map_err(|_| "provider_settings_unavailable")?;
            providers
                .register(
                    cognition::types::ProviderConfig {
                        id: "cloudflare".into(),
                        enabled: true,
                        priority: 4,
                        capabilities: cognition::types::ProviderCapabilities::text_stream(),
                    },
                    cloudflare_adapter,
                )
                .expect("unique Cloudflare ID");
            let runtime = cognition::ProviderRuntime::with_database(providers, db.clone())
                .map_err(|_| "rate_state_unavailable")?;
            runtime.connect_credentials(&secrets);
            let scheduler = runtime.scheduler.clone();
            let available_secrets = secrets.clone();
            let available_scheduler = scheduler.clone();
            let available =
                std::sync::Arc::new(move |policy: &cognition::policy::CognitiveRolePolicy| {
                    cognition::catalog::validate_policy(
                        policy,
                        &available_scheduler.status(),
                        &available_secrets,
                    )
                    .is_ok()
                });
            let worker = cognition::summary::SummaryWorker::start(
                db.clone(),
                scheduler,
                app.state::<std::sync::Arc<luna::runtime::TaskRegistry>>()
                    .inner()
                    .clone(),
                available,
            );
            app.manage(secrets);
            app.manage(std::sync::Arc::new(runtime));
            app.manage(std::sync::Arc::new(cognition::ProviderTimeoutHandles(
                std::collections::HashMap::from([
                    ("gemini".to_string(), gemini_timeouts.clone()),
                    ("groq".to_string(), groq_timeouts.clone()),
                    ("mistral".to_string(), mistral_timeouts.clone()),
                    ("cloudflare".to_string(), cloudflare_timeouts.clone()),
                ]),
            )));
            app.manage(worker);
            Ok(())
        })
        .manage(std::sync::Arc::new(luna::runtime::TaskRegistry::default()));
    let builder = builder.manage(std::sync::Arc::new(cognition::CognitionRuntime::new()));
    let builder = builder.manage(std::sync::Arc::new(
        agents::registry::AgentRegistry::production(),
    ));
    let builder = builder.manage(cognition::gemini_commands::CurrentRunSessions::default());
    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        luna::start_mock_task,
        luna::cancel_task,
        security::security_status,
        security::security_test_store_secret,
        security::security_test_delete_secret,
        persistence::lr4_status,
        persistence::lr4_import_private_bootstrap,
        persistence::lr4_create_diagnostic_conversation,
        persistence::lr4_get_recent_conversation,
        luna::start_mock_cognition_task,
        luna::cognition_provider_status,
        cognition::settings::open_general_settings_window,
        cognition::settings::open_ai_settings_window,
        cognition::settings::get_ai_settings,
        cognition::settings::get_provider_operational_snapshot,
        cognition::settings::update_cognitive_role_policy,
        cognition::settings::update_provider_timeouts,
        cognition::settings::update_provider_rate_policy,
        cognition::settings::get_general_settings,
        cognition::settings::update_general_settings,
        cognition::gemini_commands::gemini_status,
        cognition::gemini_commands::gemini_set_api_key,
        cognition::gemini_commands::gemini_delete_api_key,
        cognition::gemini_commands::gemini_conversation,
        cognition::groq_commands::groq_status,
        cognition::groq_commands::groq_set_api_key,
        cognition::groq_commands::groq_delete_api_key,
        cognition::groq_commands::groq_probe,
        cognition::credentials_commands::mistral_status,
        cognition::credentials_commands::mistral_set_api_key,
        cognition::credentials_commands::mistral_delete_api_key,
        cognition::credentials_commands::cloudflare_status,
        cognition::credentials_commands::cloudflare_set_credentials,
        cognition::credentials_commands::cloudflare_delete_credentials,
        agents::codex::get_codex_runtime_status,
        agents::codex::probe_codex_app_server,
        agents::codex::probe_codex_planner,
        agents::codex::probe_codex_planner_preflight,
        cognition::gemini_commands::create_conversation_session,
        cognition::gemini_commands::get_conversation_session,
        cognition::gemini_commands::close_conversation_session,
        cognition::gemini_commands::resume_conversation_session,
        cognition::gemini_commands::list_conversation_history,
        cognition::gemini_commands::get_conversation_history_session,
        luna::conversation_routing_status,
        luna::start_conversation_task,
        luna::start_orchestrator_planning,
        luna::start_task_graph,
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        luna::start_mock_task,
        luna::cancel_task,
        security::security_status,
        cognition::settings::open_general_settings_window,
        cognition::settings::open_ai_settings_window,
        cognition::settings::get_ai_settings,
        cognition::settings::get_provider_operational_snapshot,
        cognition::settings::update_cognitive_role_policy,
        cognition::settings::update_provider_timeouts,
        cognition::settings::update_provider_rate_policy,
        cognition::settings::get_general_settings,
        cognition::settings::update_general_settings,
        cognition::gemini_commands::gemini_status,
        cognition::gemini_commands::gemini_set_api_key,
        cognition::gemini_commands::gemini_delete_api_key,
        cognition::gemini_commands::gemini_conversation,
        cognition::groq_commands::groq_status,
        cognition::groq_commands::groq_set_api_key,
        cognition::groq_commands::groq_delete_api_key,
        cognition::groq_commands::groq_probe,
        cognition::credentials_commands::mistral_status,
        cognition::credentials_commands::mistral_set_api_key,
        cognition::credentials_commands::mistral_delete_api_key,
        cognition::credentials_commands::cloudflare_status,
        cognition::credentials_commands::cloudflare_set_credentials,
        cognition::credentials_commands::cloudflare_delete_credentials,
        agents::codex::get_codex_runtime_status,
        agents::codex::probe_codex_app_server,
        agents::codex::probe_codex_planner,
        agents::codex::probe_codex_planner_preflight,
        cognition::gemini_commands::create_conversation_session,
        cognition::gemini_commands::get_conversation_session,
        cognition::gemini_commands::close_conversation_session,
        cognition::gemini_commands::resume_conversation_session,
        cognition::gemini_commands::list_conversation_history,
        cognition::gemini_commands::get_conversation_history_session,
        luna::conversation_routing_status,
        luna::start_conversation_task,
        luna::start_orchestrator_planning,
        luna::start_task_graph,
    ]);
    builder
        .run(tauri::generate_context!())
        .expect("erro ao executar a janela Tauri");
}
