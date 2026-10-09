//! Opt-in native evidence harness. Not compiled into ordinary builds.
//! Uses real Tauri WebViews, real Conversation/Scheduler and local HTTP Groq SSE.
use crate::{cognition::{policy::{self, CognitiveRole, CognitiveTargetPolicy, RoutingMode}, ProviderRuntime}, luna::{runtime::TaskRegistry, task::TaskId}, persistence::{database::Database, identity}, security::secrets::{SecretStore, SecretKey, SecretError, UnlockKeyStore}};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::{Arc, Mutex}, time::Duration};
use tauri::{AppHandle, Manager};

pub fn directory() -> Option<PathBuf> { std::env::var_os("NARYS_PERF1C_PROBE").map(PathBuf::from) }
#[derive(Default)]
struct Keys(Mutex<Option<Vec<u8>>>);
impl UnlockKeyStore for Keys {
    fn load(&self) -> Result<Option<Vec<u8>>, SecretError> { Ok(self.0.lock().unwrap().clone()) }
    fn store(&self, key: &[u8]) -> Result<(), SecretError> { *self.0.lock().unwrap() = Some(key.to_vec()); Ok(()) }
    fn delete(&self) -> Result<(), SecretError> { *self.0.lock().unwrap() = None; Ok(()) }
}
pub fn secrets(path: PathBuf) -> SecretStore {
    if directory().is_some() { SecretStore::with_key_store(path, Arc::new(Keys::default())) }
    else { SecretStore::new(path) }
}
pub fn groq_config() -> crate::cognition::groq::GroqConfig {
    let mut config = crate::cognition::groq::GroqConfig::default();
    if directory().is_some() {
        let endpoint = std::env::var("NARYS_PERF1C_ENDPOINT").expect("probe endpoint required");
        assert!(endpoint.starts_with("http://127.0.0.1:"), "probe only permits loopback HTTP");
        config.endpoint = endpoint;
    }
    config
}
pub fn prepare(db: &Database, secrets: &SecretStore) -> Result<(), Box<dyn std::error::Error>> {
    let Some(dir) = directory() else { return Ok(()); };
    let isolated = PathBuf::from(std::env::var_os("XDG_DATA_HOME").ok_or("isolated XDG_DATA_HOME required")?);
    if !dir.is_absolute() || !isolated.starts_with(&dir) { return Err("probe requires isolated data under probe directory".into()); }
    let mut conn = db.open().map_err(|_| "db")?;
    let input = serde_json::from_slice(&std::fs::read(dir.join("identity.json"))?)?;
    let tx = conn.transaction()?; identity::insert_version(&tx, &input).map_err(|_| "identity")?; tx.commit()?;
    let mut role = policy::load(&conn, CognitiveRole::Conversation).map_err(|_| "policy")?;
    role.routing_mode = RoutingMode::Fixed;
    role.targets = vec![CognitiveTargetPolicy { provider_id: "groq".into(), model: crate::cognition::groq::MODEL.into(), thinking_level: Some(crate::cognition::policy::ThinkingLevel::Low) }];
    let tx = conn.transaction()?; policy::write_in_transaction(&tx, &role).map_err(|_| "policy")?; tx.commit()?;
    let mut summary = policy::load(&conn, CognitiveRole::Summary).map_err(|_| "policy")?;
    summary.routing_mode = RoutingMode::Fixed;
    summary.targets = role.targets.clone();
    summary.summary_input_max_bytes = 32768;
    let tx = conn.transaction()?; policy::write_in_transaction(&tx, &summary).map_err(|_| "policy")?; tx.commit()?;
    secrets.set_secret(SecretKey::GroqApiKey, b"synthetic-local-probe").map_err(|_| "probe secrets")?;
    Ok(())
}
static RECORD_LOCK: Mutex<()> = Mutex::new(());
pub fn record(event: &str, data: Value) {
    let _guard = RECORD_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(dir) = directory() {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("events.jsonl")) {
            let _ = writeln!(file, "{}", json!({"event":event,"data":data}));
        }
    }
}
pub fn start(app: &AppHandle) {
    let Some(dir) = directory() else { return; };
    app.add_capability(r#"{"identifier":"perf1c-probe-only","windows":["main"],"permissions":["allow-perf1c-ui-report"]}"#).expect("probe capability");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut handled = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let request: Value = match std::fs::read(dir.join("request.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()) { Some(r) => r, None => continue };
            let sequence = request["sequence"].as_u64().unwrap_or(0);
            if sequence <= handled { continue; } handled = sequence;
            let handle = app.clone(); let output = dir.join("response.json");
            let _ = app.run_on_main_thread(move || {
                #[cfg(feature = "perf1d-probe")]
                let adaptive_action = crate::perf1d_probe::action(&handle, &request);
                #[cfg(not(feature = "perf1d-probe"))]
                let adaptive_action: Option<Result<(), String>> = None;
                #[cfg(all(feature = "lr9b-probe", target_os = "linux"))]
                let execution_action = crate::execution::probe::action(&handle, request["action"].as_str().unwrap_or(""));
                #[cfg(not(all(feature = "lr9b-probe", target_os = "linux")))]
                let execution_action: Option<Result<(), String>> = None;
                #[cfg(all(feature = "lr9c-probe", target_os = "linux"))]
                let surface_action = crate::terminal_surface::probe::action(&handle, request["action"].as_str().unwrap_or(""));
                #[cfg(not(all(feature = "lr9c-probe", target_os = "linux")))]
                let surface_action: Option<Result<(), String>> = None;
                #[cfg(all(feature = "lr9e-probe", target_os = "linux"))]
                let final_gate_action = crate::lr9e_probe::action(&handle, request["action"].as_str().unwrap_or(""));
                #[cfg(not(all(feature = "lr9e-probe", target_os = "linux")))]
                let final_gate_action: Option<Result<(), String>> = None;
                let result = final_gate_action.or(surface_action).or(execution_action).or(adaptive_action).unwrap_or_else(|| match request["action"].as_str().unwrap_or("") {
                    "close" => crate::adaptive::close(&handle, crate::adaptive::Reason::ManualClose),
                    "send" => handle.get_webview_window("main").ok_or("no main".to_owned()).and_then(|w| w.eval("{const t=document.querySelector('textarea'); Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(t,'Native headless fixture'); t.dispatchEvent(new Event('input',{bubbles:true})); setTimeout(()=>{[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Enviar').click()},100)}").map_err(|e| e.to_string())),
                    "new" => handle.get_webview_window("main").ok_or("no main".to_owned()).and_then(|w| w.eval("[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Nova conversa').click()").map_err(|e| e.to_string())),
                    "cancel" => { let session = handle.state::<crate::cognition::gemini_commands::CurrentRunSessions>().selected().unwrap().unwrap(); let task = handle.state::<Arc<TaskRegistry>>().events.snapshot(session).unwrap(); handle.state::<Arc<TaskRegistry>>().cancel(TaskId(task.task_id.0)); Ok(()) },
                    "presence" | "economy" => { crate::adaptive::set_policy(&handle, if request["action"] == "presence" { crate::adaptive::PresentationPolicy::Presence } else { crate::adaptive::PresentationPolicy::Economy }) },
                    "quit" => { crate::presentation::request_quit(&handle); Ok(()) },
                    "frontend" => handle.get_webview_window("main").ok_or("no main".to_owned()).and_then(|w| w.eval("window.__TAURI_INTERNALS__.invoke('perf1c_ui_report',{report:{mode:document.querySelector('[data-presentation-mode]')?.dataset.presentationMode,phase:document.querySelector('[data-presentation-mode]')?.dataset.presentationPhase,canvases:document.querySelectorAll('canvas').length,glb:performance.getEntriesByType('resource').filter(r=>r.name.includes('Luna.glb')).length,body:document.body.innerText,bootstrapMs:performance.now()}})").map_err(|e| e.to_string())),
                    "snapshot" => Ok(()),
                    _ => Err("unknown probe action".into()),
                });
                let registry = handle.state::<Arc<TaskRegistry>>();
                let session = handle.state::<crate::cognition::gemini_commands::CurrentRunSessions>().selected().unwrap();
                let mut response = json!({"sequence":sequence,"error":result.err(),"pid":std::process::id(),"windows":handle.webview_windows().keys().collect::<Vec<_>>(),"mainPresent":handle.get_webview_window("main").is_some(),"registryAddress":Arc::as_ptr(registry.inner()) as usize,"providerRuntimeAddress":Arc::as_ptr(handle.state::<Arc<ProviderRuntime>>().inner()) as usize,"sessionId":session,"task":session.and_then(|id| registry.events.snapshot(id)),"activeCount":registry.active_count(),"adaptive":handle.state::<crate::adaptive::AdaptivePresentationManager>().snapshot(),"scheduler":handle.state::<Arc<ProviderRuntime>>().scheduler.operational_snapshot()});
                #[cfg(all(feature = "lr9b-probe", target_os = "linux"))]
                { response["execution"] = crate::execution::probe::snapshot(&handle); }
                #[cfg(all(feature = "lr9c-probe", target_os = "linux"))]
                { response["terminal"] = crate::terminal_surface::probe::snapshot(&handle); }
                #[cfg(all(feature = "lr9e-probe", target_os = "linux"))]
                { response["finalGate"] = crate::lr9e_probe::snapshot(&handle); }
                // Mutability is needed only by the opt-in execution probe.
                #[cfg(not(all(feature = "lr9b-probe", target_os = "linux")))]
                let response = { let _ = &mut response; response };
                std::fs::write(output, serde_json::to_vec(&response).unwrap()).unwrap();
            });
        }
    });
}

#[tauri::command]
pub fn perf1c_ui_report(report: Value) { record("ui_report", report); }
