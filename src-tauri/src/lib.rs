mod luna;

pub fn run() {
  tauri::Builder::default()
    .manage(std::sync::Arc::new(luna::runtime::TaskRegistry::default()))
    .invoke_handler(tauri::generate_handler![luna::start_mock_task, luna::cancel_task])
    .run(tauri::generate_context!())
    .expect("erro ao executar a janela Tauri");
}
