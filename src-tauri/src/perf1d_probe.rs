//! Controlled clock/focus/attention gates, compiled only with perf1d-probe.
use crate::adaptive::{self, AdaptivePresentationManager, AttentionReason, PresentationPolicy};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use std::sync::{Arc, Mutex};
static UI_TASK: Mutex<Option<crate::luna::task::TaskId>> = Mutex::new(None);
pub fn prepare(app: &AppHandle) -> Result<(), String> {
    if crate::perf1c_probe::directory().is_none() { return Ok(()); }
    if let Ok(value) = std::env::var("NARYS_PERF1D_INITIAL_POLICY") {
        let policy = serde_json::from_value(Value::String(value)).map_err(|_| "probe_policy_invalid")?;
        let conn = app.state::<crate::persistence::database::Database>().open().map_err(|e| e.code())?;
        crate::persistence::shell_settings::save_policy(&conn, policy).map_err(|e| e.code())?;
    }
    Ok(())
}
pub fn action(app: &AppHandle, request: &Value) -> Option<Result<(), String>> {
    let action = request["action"].as_str()?;
    let registry = app.state::<Arc<crate::luna::runtime::TaskRegistry>>();
    Some(match action {
        "auto" => adaptive::set_policy(app, PresentationPolicy::Auto),
        "headless" => adaptive::set_policy(app, PresentationPolicy::Headless),
        "background" | "focused" => { let epoch = app.state::<AdaptivePresentationManager>().snapshot().epoch; adaptive::focus(app, epoch, action == "focused"); Ok(()) },
        "expire" => adaptive::expire_for_probe(app),
        "attention" => adaptive::require_attention(app, AttentionReason::UserInputRequired),
        "approval_attention" => adaptive::require_attention(app, AttentionReason::ApprovalRequired),
        "ack_attention" => { // Same native state mutation as the real explicit acknowledgment.
            // Use frontend command when the surface is present.
            app.get_webview_window("main").ok_or("no_main".to_owned()).and_then(|w| w.eval("window.__TAURI_INTERNALS__.invoke('acknowledge_presentation_attention')").map_err(|e| e.to_string()))
        },
        "draft" | "clear_draft" => app.get_webview_window("main").ok_or("no_main".to_owned()).and_then(|w| {
            w.eval(if action == "draft" { "(()=>{const d=document.querySelector('textarea');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(d,'Unsent local draft');d.dispatchEvent(new Event('input',{bubbles:true}));})()" } else { "(()=>{const d=document.querySelector('textarea');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(d,'');d.dispatchEvent(new Event('input',{bubbles:true}));})()" }).map_err(|e| e.to_string())
        }),
        "settings" => crate::cognition::settings::open_general_settings_window(app.clone()),
        "close_settings" => app.get_webview_window("settings-general").ok_or("no_settings".to_owned()).and_then(|w| w.close().map_err(|e| e.to_string())),
        "ui_bound_on" => { registry.register().map(|(id, _)| { *UI_TASK.lock().unwrap() = Some(id); }) },
        "ui_bound_off" => { if let Some(id) = UI_TASK.lock().unwrap().take() { registry.finish(id, crate::luna::task::TaskState::Completed); } Ok(()) },
        _ => return None,
    })
}
