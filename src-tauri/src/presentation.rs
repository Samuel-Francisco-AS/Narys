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
    #[cfg(target_os = "linux")]
    app.state::<crate::terminal_surface::SurfaceHub>().detach_main();
    app.state::<Arc<TaskRegistry>>().detach_ui_bound();
    // Auxiliary settings hosts are also disposable. They cannot sustain a fake headless state.
    for window in app.webview_windows().values() {
        destroy_window(window)?;
    }
    Ok(())
}
/// Also used when an auxiliary host closes independently, before Auto can run.
/// A destroyed settings WebView must not leave a renderer that main teardown cannot reach.
pub fn destroy_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    window.with_webview(|webview| {
        use webkit2gtk::WebViewExt;
        webview.inner().terminate_web_process();
    }).map_err(|e| e.to_string())?;
    window.destroy().map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn close_presentation(app: AppHandle) -> Result<(), String> { crate::adaptive::on_main(app, |app| crate::adaptive::close(app, crate::adaptive::Reason::ManualClose)).await }
#[tauri::command]
pub fn quit_narys(app: AppHandle) { request_quit(&app); }

pub fn request_quit(app: &AppHandle) {
    let host = app.state::<PresentationHost>();
    if host.quitting.swap(true, Ordering::AcqRel) { return; }
    crate::adaptive::quit(app);
    #[cfg(target_os = "linux")]
    app.state::<crate::terminal_surface::SurfaceHub>().detach_main();
    app.state::<Arc<TaskRegistry>>().shutdown();
    app.state::<Arc<SummaryWorker>>().shutdown();
    #[cfg(target_os = "linux")]
    app.state::<Arc<crate::execution::ExecutionBroker>>().request_shutdown();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let cooperative cancellation persist task outcomes; bounded shutdown cannot hang.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while (handle.state::<Arc<TaskRegistry>>().active_count() > 0 || handle.state::<Arc<TaskRegistry>>().worker_count() > 0 || handle.state::<Arc<TaskRegistry>>().has_foreground_provider_work() || !handle.state::<Arc<SummaryWorker>>().stopped() || execution_pending(&handle))
            && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        if handle.state::<Arc<TaskRegistry>>().active_count() > 0
            || handle.state::<Arc<TaskRegistry>>().worker_count() > 0
            || !handle.state::<Arc<SummaryWorker>>().stopped()
            || execution_pending(&handle) {
            eprintln!("[Quit] cooperative deadline exceeded: tasks={} task_workers={} summary_stopped={} execution_pending={}", handle.state::<Arc<TaskRegistry>>().active_count(), handle.state::<Arc<TaskRegistry>>().worker_count(), handle.state::<Arc<SummaryWorker>>().stopped(), execution_pending(&handle));
        }
        #[cfg(all(feature = "lr9e-probe", target_os = "linux"))]
        crate::perf1c_probe::record("lr9e_shutdown", crate::lr9e_probe::snapshot(&handle));
        handle.exit(0);
    });
}
fn execution_pending(_app: &AppHandle) -> bool {
    #[cfg(target_os = "linux")]
    { return !_app.state::<Arc<crate::execution::ExecutionBroker>>().stopped(); }
    #[cfg(not(target_os = "linux"))]
    { false }
}
pub fn should_keep_alive(_app: &AppHandle, code: Option<i32>) -> bool {
    // Also keep the loop alive if windows close during cooperative Quit.
    // Only the final programmatic exit may end that wait.
    code.is_none()
}
