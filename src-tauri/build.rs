fn main() {
  #[cfg(debug_assertions)]
  const COMMANDS: &[&str] = &[
    "start_mock_task", "cancel_task", "security_status",
    "security_test_store_secret", "security_test_delete_secret", "lr4_status",
    "lr4_import_private_bootstrap", "lr4_create_diagnostic_conversation",
    "lr4_get_recent_conversation", "start_mock_cognition_task", "cognition_provider_status",
    "open_general_settings_window", "open_ai_settings_window", "get_ai_settings", "get_provider_operational_snapshot", "update_cognitive_role_policy", "start_orchestrator_planning", "update_provider_timeouts", "update_provider_rate_policy", "get_general_settings", "update_general_settings", "gemini_status", "gemini_set_api_key", "gemini_delete_api_key", "gemini_conversation", "groq_status", "groq_set_api_key", "groq_delete_api_key", "groq_probe", "mistral_status", "mistral_set_api_key", "mistral_delete_api_key", "cloudflare_status", "cloudflare_set_credentials", "cloudflare_delete_credentials", "get_codex_runtime_status", "probe_codex_app_server", "create_conversation_session", "get_conversation_session", "close_conversation_session", "resume_conversation_session", "list_conversation_history", "get_conversation_history_session", "conversation_routing_status", "start_conversation_task",
  ];
  #[cfg(not(debug_assertions))]
  const COMMANDS: &[&str] = &["start_mock_task", "cancel_task", "security_status",
    "open_general_settings_window", "open_ai_settings_window", "get_ai_settings", "get_provider_operational_snapshot", "update_cognitive_role_policy", "start_orchestrator_planning", "update_provider_timeouts", "update_provider_rate_policy", "get_general_settings", "update_general_settings", "gemini_status", "gemini_set_api_key", "gemini_delete_api_key", "gemini_conversation", "groq_status", "groq_set_api_key", "groq_delete_api_key", "groq_probe", "mistral_status", "mistral_set_api_key", "mistral_delete_api_key", "cloudflare_status", "cloudflare_set_credentials", "cloudflare_delete_credentials", "get_codex_runtime_status", "probe_codex_app_server", "create_conversation_session", "get_conversation_session", "close_conversation_session", "resume_conversation_session", "list_conversation_history", "get_conversation_history_session", "conversation_routing_status", "start_conversation_task"];
  tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
    tauri_build::AppManifest::new().commands(COMMANDS),
  )).expect("erro ao gerar permissoes Tauri")
}
