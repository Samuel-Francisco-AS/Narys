// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  // O WebKitGTK desta máquina cria WebGL, mas falha ao desenhar com a Intel HD 4000.
  // Mesa em software mantém o canvas funcional; uma variável já definida permite testar outro driver.
  #[cfg(target_os = "linux")]
  if std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_none() {
    std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
  }

  assistente_3d_lib::run();
}
