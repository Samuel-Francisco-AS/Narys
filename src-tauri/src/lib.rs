mod luna;
mod security;

use tauri::Manager;

pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      let directory = app.path().app_local_data_dir()?;
      app.manage(security::secrets::SecretStore::new(directory));
      Ok(())
    })
    .manage(std::sync::Arc::new(luna::runtime::TaskRegistry::default()))
    .invoke_handler(tauri::generate_handler![
      luna::start_mock_task,
      luna::cancel_task,
      security::security_status,
      security::security_test_store_secret,
      security::security_test_delete_secret,
    ])
    .run(tauri::generate_context!())
    .expect("erro ao executar a janela Tauri");
}
