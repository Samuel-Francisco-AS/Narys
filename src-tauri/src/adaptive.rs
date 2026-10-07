//! Native Presentation policy. All host mutations run on the Tauri event loop.
//! No cognition settings, activity score, polling, or automatic 3D selection.
use crate::{persistence::{database::Database, shell_settings::{self, PresentationMode}}, luna::runtime::TaskRegistry};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager};

pub const AUTO_HEADLESS_DELAY: Duration = Duration::from_secs(30);
const HISTORY_LIMIT: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresentationPolicy { Economy, Presence, Headless, Auto }
impl PresentationPolicy {
    pub fn control_surface(self) -> RuntimeState {
        if self == Self::Presence { RuntimeState::Presence } else { RuntimeState::Economy }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeState { Economy, Presence, Headless }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason { Startup, UserPolicy, ExplicitActivation, AutoBackgroundTimeout, AttentionRequired, ManualClose, Quit }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason { ApprovalRequired, TaskFailed, UserInputRequired }
#[derive(Clone, Debug, Serialize)]
pub struct Transition { from: RuntimeState, to: RuntimeState, reason: Reason, policy: PresentationPolicy, timestamp: i64 }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub policy: PresentationPolicy,
    pub state: RuntimeState,
    pub epoch: u64,
    pub revision: u64,
    pub focused: bool,
    pub ui_guard: bool,
    pub pending_token: Option<u64>,
    pub timer_active: bool,
    pub transitioning: bool,
    pub quitting: bool,
    pub attention: Option<AttentionReason>,
    pub history: VecDeque<Transition>,
}
struct Pending { token: u64, deadline: Instant, awaiting_guard: bool }
pub(crate) struct Machine {
    snapshot: Snapshot,
    pending: Option<Pending>,
    sequence: u64,
    closing: Option<Reason>,
    recovery: Option<Reason>,
    timer: Option<tauri::async_runtime::JoinHandle<()>>,
}
pub struct AdaptivePresentationManager(pub(crate) Mutex<Machine>);
impl AdaptivePresentationManager {
    pub fn new(policy: PresentationPolicy) -> Self { Self(Mutex::new(Machine::new(policy))) }
    pub fn snapshot(&self) -> Snapshot {
        let m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let mut snapshot = m.snapshot.clone(); snapshot.timer_active = m.timer.is_some(); snapshot
    }
}
impl Machine {
    fn new(policy: PresentationPolicy) -> Self {
        Self { snapshot: Snapshot { policy, state: RuntimeState::Headless, epoch: 0, revision: 0, focused: true, ui_guard: true, pending_token: None, timer_active: false, transitioning: false, quitting: false, attention: None, history: VecDeque::new() }, pending: None, sequence: 0, closing: None, recovery: None, timer: None }
    }
    fn invalidate(&mut self) {
        self.sequence += 1;
        self.pending = None;
        self.snapshot.pending_token = None;
        if let Some(timer) = self.timer.take() { timer.abort(); }
    }
    fn record(&mut self, to: RuntimeState, reason: Reason) {
        let from = self.snapshot.state;
        self.snapshot.state = to;
        if self.snapshot.history.len() == HISTORY_LIMIT { self.snapshot.history.pop_front(); }
        self.snapshot.history.push_back(Transition { from, to, reason, policy: self.snapshot.policy, timestamp: chrono::Utc::now().timestamp_millis() });
    }
    fn focus(&mut self, epoch: u64, focused: bool, now: Instant) -> Option<u64> {
        if epoch != self.snapshot.epoch || self.snapshot.quitting || self.snapshot.transitioning { return None; }
        if self.snapshot.focused == focused { return None; }
        self.snapshot.focused = focused;
        self.invalidate();
        if focused || self.snapshot.policy != PresentationPolicy::Auto || self.snapshot.state != RuntimeState::Economy { return None; }
        let token = self.sequence;
        self.pending = Some(Pending { token, deadline: now + AUTO_HEADLESS_DELAY, awaiting_guard: false });
        self.snapshot.pending_token = Some(token);
        Some(token)
    }
    fn eligible(&self, token: u64, now: Instant, ui_bound: bool, window_count: usize) -> bool {
        self.pending.as_ref().is_some_and(|p| p.token == token && now >= p.deadline)
            && self.snapshot.policy == PresentationPolicy::Auto && self.snapshot.state == RuntimeState::Economy
            && !self.snapshot.focused && !self.snapshot.ui_guard && !self.snapshot.quitting && !self.snapshot.transitioning
            && self.snapshot.attention.is_none() && !ui_bound && window_count == 1
    }
}
fn publish(app: &AppHandle) {
    let snapshot = app.state::<AdaptivePresentationManager>().snapshot();
    let _ = app.emit_to("main", "adaptive-presentation-changed", &snapshot);
    let _ = app.emit_to("settings-general", "adaptive-presentation-changed", &snapshot);
}
/// Dispatch, await result, and never hold a mutex across a native operation/await.
pub async fn on_main<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || { let _ = tx.send(f(&handle)); }).map_err(|_| "presentation_dispatch_failed".to_owned())?;
    rx.await.map_err(|_| "presentation_dispatch_failed".to_owned())?
}
pub fn startup(app: &AppHandle) -> Result<(), String> {
    let policy = shell_settings::load(&app.state::<Database>().open().map_err(|e| e.code())?).map_err(|e| e.code())?.presentation_policy;
    app.manage(AdaptivePresentationManager::new(policy));
    if policy == PresentationPolicy::Headless {
        app.state::<Arc<TaskRegistry>>().suspend_ui_if_safe();
        app.state::<AdaptivePresentationManager>().0.lock().unwrap().record(RuntimeState::Headless, Reason::Startup);
        Ok(())
    } else { reopen(app, Reason::Startup) }
}
/// Same PresentationHost primitive for activation and attention. Policy is never written here.
pub fn reopen(app: &AppHandle, reason: Reason) -> Result<(), String> {
    let manager = app.state::<AdaptivePresentationManager>();
    let (target, creating) = {
        let mut m = manager.0.lock().unwrap();
        if m.snapshot.quitting { return Err("runtime_shutting_down".into()); }
        m.invalidate();
        if m.closing.is_some() { m.recovery = Some(reason); return Ok(()); }
        let target = if reason == Reason::AttentionRequired { RuntimeState::Economy } else { m.snapshot.policy.control_surface() };
        let creating = app.get_webview_window("main").is_none();
        m.snapshot.transitioning = true;
        m.snapshot.focused = true; // A fresh full blur is required after every recovery.
        m.snapshot.revision += 1;
        if creating { m.snapshot.epoch += 1; }
        m.snapshot.ui_guard = true; // Fail closed until this incarnation acknowledges its surface/guard.
        (target, creating)
    };
    let result = crate::presentation::reopen(app);
    {
        let mut m = manager.0.lock().unwrap();
        m.snapshot.transitioning = false;
        if result.is_ok() { m.record(target, reason); app.state::<Arc<TaskRegistry>>().resume_ui(); }
    }
    if result.is_ok() && creating {
        let epoch = manager.snapshot().epoch;
        let handle = app.clone();
        app.get_webview_window("main").ok_or("main_missing")?.on_window_event(move |event| {
            match event {
                tauri::WindowEvent::Focused(focused) => focus(&handle, epoch, *focused),
                tauri::WindowEvent::Destroyed => {
                    let app = handle.clone();
                    let _ = handle.run_on_main_thread(move || destroyed(&app, epoch));
                }
                _ => {}
            }
        });
    }
    publish(app);
    result
}
pub fn close(app: &AppHandle, reason: Reason) -> Result<(), String> {
    let manager = app.state::<AdaptivePresentationManager>();
    {
        let mut m = manager.0.lock().unwrap();
        if m.snapshot.quitting { return Err("runtime_shutting_down".into()); }
        m.invalidate();
        if m.closing.is_some() { return Ok(()); }
        m.closing = Some(reason);
        m.snapshot.transitioning = true;
    }
    let result = crate::presentation::close(app);
    if app.get_webview_window("main").is_none() { destroyed(app, manager.snapshot().epoch); }
    if result.is_err() {
        let mut m = manager.0.lock().unwrap();
        m.closing = None; m.snapshot.transitioning = false;
    }
    publish(app);
    result
}
fn destroyed(app: &AppHandle, epoch: u64) {
    let manager = app.state::<AdaptivePresentationManager>();
    let recovery = {
        let mut m = manager.0.lock().unwrap();
        if m.snapshot.epoch != epoch || app.get_webview_window("main").is_some() || (m.snapshot.state == RuntimeState::Headless && m.closing.is_none()) { return; }
        m.invalidate();
        let reason = m.closing.take().unwrap_or(Reason::ManualClose);
        m.snapshot.transitioning = false;
        m.snapshot.ui_guard = true;
        m.record(RuntimeState::Headless, reason);
        if m.snapshot.quitting { None } else { m.recovery.take() }
    };
    publish(app);
    if let Some(reason) = recovery { if let Err(e) = reopen(app, reason) { eprintln!("[Presentation] recovery failed: {e}"); } }
}
pub fn focus(app: &AppHandle, epoch: u64, focused: bool) {
    let manager = app.state::<AdaptivePresentationManager>();
    let token = manager.0.lock().unwrap().focus(epoch, focused, Instant::now());
    if let Some(token) = token {
        let handle = app.clone();
        let timer = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(AUTO_HEADLESS_DELAY).await;
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || timeout(&app, token));
        });
        manager.0.lock().unwrap().timer = Some(timer);
    }
    publish(app);
}
fn facts(app: &AppHandle) -> (bool, usize) { (app.state::<Arc<TaskRegistry>>().has_ui_bound_work(), if app.get_webview_window("main").is_some() { app.webview_windows().len() } else { 0 }) }
pub fn timeout(app: &AppHandle, token: u64) {
    let (bound, count) = facts(app);
    let manager = app.state::<AdaptivePresentationManager>();
    let request = {
        let mut m = manager.0.lock().unwrap();
        if !m.pending.as_ref().is_some_and(|p| p.token == token) { return; }
        m.timer = None;
        if m.eligible(token, Instant::now(), bound, count) {
            m.pending.as_mut().unwrap().awaiting_guard = true;
            true
        } else { m.invalidate(); false }
    };
    if request {
        if app.emit_to("main", "adaptive-close-check", token).is_err() { manager.0.lock().unwrap().invalidate(); }
    }
    publish(app);
}
pub fn set_policy(app: &AppHandle, policy: PresentationPolicy) -> Result<(), String> {
    let manager = app.state::<AdaptivePresentationManager>();
    let old = manager.snapshot().policy;
    if manager.snapshot().quitting { return Err("runtime_shutting_down".into()); }
    // Serialized on the event loop: persistence precedes close and cannot reorder writes.
    let conn = app.state::<Database>().open().map_err(|e| e.code())?;
    shell_settings::save_policy(&conn, policy).map_err(|e| e.code())?;
    {
        let mut m = manager.0.lock().unwrap();
        m.invalidate(); m.snapshot.policy = policy;
        if policy == PresentationPolicy::Headless { m.recovery = None; }
    }
    let result = if policy == PresentationPolicy::Headless { close(app, Reason::UserPolicy) } else { reopen(app, Reason::UserPolicy) };
    if result.is_err() {
        // Report failure honestly; restore the persisted/runtime preference if host enqueue failed.
        shell_settings::save_policy(&conn, old).map_err(|_| "presentation_rollback_failed".to_owned())?;
        manager.0.lock().unwrap().snapshot.policy = old;
        publish(app);
    }
    result
}
pub async fn request_attention(app: AppHandle, reason: AttentionReason) -> Result<(), String> {
    on_main(app, move |app| require_attention(app, reason)).await
}
/// Small native hook for future real callers; no text, prompts, secrets or fake approval workflow.
pub fn require_attention(app: &AppHandle, reason: AttentionReason) -> Result<(), String> {
    let manager = app.state::<AdaptivePresentationManager>();
    let recover = {
        let mut m = manager.0.lock().unwrap();
        if m.snapshot.quitting { return Err("runtime_shutting_down".into()); }
        m.invalidate(); m.snapshot.attention = Some(reason);
        m.snapshot.state == RuntimeState::Headless || m.closing.is_some()
    };
    if recover { reopen(app, Reason::AttentionRequired)?; }
    publish(app);
    Ok(())
}
pub fn quit(app: &AppHandle) {
    let manager = app.state::<AdaptivePresentationManager>();
    let mut m = manager.0.lock().unwrap();
    m.invalidate(); m.recovery = None; m.snapshot.quitting = true;
    m.snapshot.transitioning = false;
    let state = m.snapshot.state;
    m.record(state, Reason::Quit);
}
#[tauri::command]
pub fn get_presentation_snapshot(app: AppHandle) -> Snapshot { app.state::<AdaptivePresentationManager>().snapshot() }
#[tauri::command]
pub async fn update_presentation_policy(app: AppHandle, policy: PresentationPolicy) -> Result<(), String> { on_main(app, move |app| set_policy(app, policy)).await }
#[tauri::command]
pub async fn report_presentation_ui(app: AppHandle, epoch: u64, revision: u64, mode: PresentationMode, guarded: bool) -> Result<(), String> {
    on_main(app, move |app| {
        let manager = app.state::<AdaptivePresentationManager>();
        let mut m = manager.0.lock().unwrap();
        let surface = if mode == PresentationMode::Presence { RuntimeState::Presence } else { RuntimeState::Economy };
        if m.snapshot.epoch != epoch || m.snapshot.revision != revision || m.snapshot.state != surface { return Err("stale_presentation_ui".into()); }
        m.snapshot.ui_guard = guarded;
        if guarded { m.invalidate(); }
        Ok(())
    }).await
}
#[tauri::command]
pub async fn confirm_auto_close(app: AppHandle, epoch: u64, token: u64, guarded: bool) -> Result<bool, String> {
    on_main(app, move |app| {
        let (bound, count) = facts(app);
        let manager = app.state::<AdaptivePresentationManager>();
        let eligible = {
            let mut m = manager.0.lock().unwrap();
            if epoch != m.snapshot.epoch || !m.pending.as_ref().is_some_and(|p| p.token == token && p.awaiting_guard) { return Ok(false); }
            m.snapshot.ui_guard = guarded;
            m.eligible(token, Instant::now(), bound, count)
        };
        let eligible = eligible && app.state::<Arc<TaskRegistry>>().suspend_ui_if_safe();
        if eligible {
            if let Err(e) = close(app, Reason::AutoBackgroundTimeout) { app.state::<Arc<TaskRegistry>>().resume_ui(); return Err(e); }
        }
        else { manager.0.lock().unwrap().invalidate(); publish(app); }
        Ok(eligible)
    }).await
}
#[tauri::command]
pub async fn acknowledge_presentation_attention(app: AppHandle) -> Result<(), String> {
    on_main(app, |app| { app.state::<AdaptivePresentationManager>().0.lock().unwrap().snapshot.attention = None; publish(app); Ok(()) }).await
}
#[cfg(feature = "perf1d-probe")]
pub fn expire_for_probe(app: &AppHandle) -> Result<(), String> {
    let manager = app.state::<AdaptivePresentationManager>();
    let token = {
        let mut m = manager.0.lock().unwrap();
        let pending = m.pending.as_mut().ok_or("no_pending_auto_timer")?;
        pending.deadline = Instant::now(); let token = pending.token;
        if let Some(timer) = m.timer.take() { timer.abort(); }
        token
    };
    timeout(app, token);
    Ok(())
}
#[cfg(test)]
#[path = "adaptive_tests.rs"]
mod tests;
