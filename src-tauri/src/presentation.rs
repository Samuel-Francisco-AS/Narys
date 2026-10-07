//! Disposable host. Core lifetime is governed only by explicit Quit.
use crate::{cognition::summary::SummaryWorker, luna::runtime::TaskRegistry};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

#[derive(Default)]
pub struct PresentationHost { quitting: AtomicBool }

/// Called on the event-loop thread only (setup or run_on_main_thread).
/// Configuration stays authoritative in tauri.conf.json with create=false.
pub fn reopen(app: &AppHandle) -> Result<(), String> {
    if app.state::<PresentationHost>().quitting.load(Ordering::Acquire) { return Err("runtime_shutting_down".into()); }
    if let Some(window) = app.get_webview_window("main") {
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let config = app.config().app.windows.iter().find(|w| w.label == "main").ok_or("main_config_missing")?;
    let builder = WebviewWindowBuilder::from_config(app, config).map_err(|e| e.to_string())?;
    #[cfg(feature = "perf1c-probe")]
    let builder = builder.on_web_resource_request(|request, response| {
        crate::perf1c_probe::record("asset", serde_json::json!({"path":request.uri().path(),"status":response.status().as_u16()}));
    });
    let window = builder.build().map_err(|e| e.to_string())?;
    let db = app.state::<crate::persistence::database::Database>();
    if let Ok(conn) = db.open() {
        if let Ok(settings) = crate::persistence::general_settings::load(&conn) {
            window.set_always_on_top(settings.always_on_top).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn close(app: &AppHandle) -> Result<(), String> {
    // Remove subscription before destroying its Channel/WebView. No keep-alive window.
    app.state::<Arc<TaskRegistry>>().events.detach_main();
    app.state::<Arc<TaskRegistry>>().detach_ui_bound();
    // Auxiliary settings hosts are also disposable. They cannot sustain a fake headless state.
    for window in app.webview_windows().values() {
        // WebKitGTK can retain a WebProcess after widget destruction. Terminate the
        // associated renderer via its public API, then destroy the actual window.
        // with_webview runs on the UI thread; its message precedes destroy's message.
        #[cfg(target_os = "linux")]
        window.with_webview(|webview| {
            use webkit2gtk::WebViewExt;
            webview.inner().terminate_web_process();
        }).map_err(|e| e.to_string())?;
        window.destroy().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub async fn close_presentation(app: AppHandle) -> Result<(), String> { close(&app) }
#[tauri::command]
pub fn quit_narys(app: AppHandle) { request_quit(&app); }

pub fn request_quit(app: &AppHandle) {
    let host = app.state::<PresentationHost>();
    if host.quitting.swap(true, Ordering::AcqRel) { return; }
    app.state::<Arc<TaskRegistry>>().shutdown();
    app.state::<Arc<SummaryWorker>>().shutdown();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let cooperative cancellation persist task outcomes; bounded shutdown cannot hang.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while (handle.state::<Arc<TaskRegistry>>().active_count() > 0 || handle.state::<Arc<TaskRegistry>>().worker_count() > 0 || handle.state::<Arc<TaskRegistry>>().has_foreground_provider_work() || !handle.state::<Arc<SummaryWorker>>().stopped())
            && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        handle.exit(0);
    });
}
pub fn should_keep_alive(_app: &AppHandle, code: Option<i32>) -> bool {
    // Also keep the loop alive if windows close during cooperative Quit.
    // Only the final programmatic exit may end that wait.
    code.is_none()
}
