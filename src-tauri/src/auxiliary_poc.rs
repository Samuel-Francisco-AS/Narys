//! Temporary UIP-4-FIX-2A geometry probe. No product conversation state lives here.
#[cfg(debug_assertions)]
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

#[cfg(not(debug_assertions))]
#[tauri::command]
pub fn set_auxiliary_poc_visible() -> Result<(), &'static str> {
  Err("auxiliary POC is only available in development builds")
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn set_auxiliary_poc_visible(
  app: tauri::AppHandle,
  surface: String,
  visible: bool,
) -> Result<(), String> {
  let (width, height) = match surface.as_str() {
    "composer" => (300.0, 80.0),
    "conversation" => (300.0, 460.0),
    _ => return Err("unknown auxiliary surface".into()),
  };
  if let Some(window) = app.get_webview_window(&surface) {
    if visible {
      window.show().map_err(|error| error.to_string())?;
      eprintln!("[UIP-4-FIX-2A] {surface} show size={:?}", window.inner_size());
    } else {
      window.hide().map_err(|error| error.to_string())?;
      eprintln!("[UIP-4-FIX-2A] {surface} hide size={:?}", window.inner_size());
    }
    return Ok(());
  }
  if !visible { return Ok(()); }

  let main = app.get_webview_window("main").ok_or("main window unavailable")?;
  let builder = WebviewWindowBuilder::new(
    &app,
    &surface,
    WebviewUrl::App(format!("index.html?aux={surface}").into()),
  )
  .title(format!("Luna {surface} POC"))
  .inner_size(width, height)
  .resizable(false)
  .decorations(false)
  .transparent(true)
  .shadow(false)
  .skip_taskbar(true)
  .focused(false)
  .visible(false)
  .parent(&main).map_err(|error| error.to_string())?;
  let window = builder.build().map_err(|error| error.to_string())?;
  let log_label = surface.clone();
  let close_window = window.clone();
  window.on_window_event(move |event| match event {
    WindowEvent::CloseRequested { api, .. } => {
      api.prevent_close();
      if let Err(error) = close_window.hide() {
        eprintln!("[UIP-4-FIX-2A] {log_label} close→hide failed: {error}");
      } else {
        eprintln!("[UIP-4-FIX-2A] {log_label} close→hide");
      }
    }
    WindowEvent::Focused(focused) => eprintln!("[UIP-4-FIX-2A] {log_label} focus={focused}"),
    WindowEvent::Moved(position) => eprintln!("[UIP-4-FIX-2A] {log_label} move={position:?}"),
    WindowEvent::Resized(size) => eprintln!("[UIP-4-FIX-2A] {log_label} size={size:?}"),
    WindowEvent::Destroyed => eprintln!("[UIP-4-FIX-2A] {log_label} destroyed"),
    _ => {}
  });
  eprintln!("[UIP-4-FIX-2A] {surface} created size={:?} relation=parent/transient_for", window.inner_size());
  window.show().map_err(|error| error.to_string())?;
  eprintln!("[UIP-4-FIX-2A] {surface} show");
  Ok(())
}
