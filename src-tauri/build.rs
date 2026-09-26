fn main() {
  #[cfg(debug_assertions)]
  const COMMANDS: &[&str] = &[
    "start_mock_task", "cancel_task", "security_status",
    "security_test_store_secret", "security_test_delete_secret", "lr4_status",
    "lr4_import_private_bootstrap", "lr4_create_diagnostic_conversation",
    "lr4_get_recent_conversation", "start_mock_cognition_task", "cognition_provider_status",
  ];
  #[cfg(not(debug_assertions))]
  const COMMANDS: &[&str] = &["start_mock_task", "cancel_task", "security_status"];
  tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
    tauri_build::AppManifest::new().commands(COMMANDS),
  )).expect("erro ao gerar permissoes Tauri")
}
