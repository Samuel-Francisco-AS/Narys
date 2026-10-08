//! Disposable Presentation bridge. Its workers and acknowledgments are never
//! on the Execution/Observation producer path. A single main attachment, two
//! workers, one in-flight batch per stream, consumer-only waits and deadlines.
mod dto;
#[cfg(feature = "lr9c-probe")]
pub(crate) mod probe;
#[cfg(test)]
mod tests;
use crate::{
    execution::{
        human::{HumanTerminal, SessionDto},
        *,
    },
    operational_trace::*,
};
use dto::*;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::Duration;
use tauri::{
    ipc::{Channel, InvokeBody, Request, Response},
    State, WebviewWindow,
};

pub(crate) const CONSUMER_WAIT: Duration = Duration::from_millis(250);
pub(crate) const BATCH_WINDOW: Duration = Duration::from_millis(32);
pub(crate) const ACK_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const MAX_BRIDGE_WORKERS: usize = 4; // two active + two retiring
#[derive(Default)]
struct Delivery {
    stopped: bool,
    pending: [Option<String>; 2],
}
pub(crate) struct Attachment {
    pub id: String,
    pub session_id: Option<String>,
    delivery: Mutex<Delivery>,
    wake: Condvar,
}
impl Attachment {
    fn new(id: String, session_id: Option<String>) -> Self {
        Self {
            id,
            session_id,
            delivery: Mutex::new(Delivery::default()),
            wake: Condvar::new(),
        }
    }
    fn stop(&self) {
        self.delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stopped = true;
        self.wake.notify_all();
    }
    fn stopped(&self) -> bool {
        self.delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stopped
    }
    fn pause(&self, timeout: Duration) -> bool {
        let d = self.delivery.lock().unwrap_or_else(|p| p.into_inner());
        !self
            .wake
            .wait_timeout_while(d, timeout, |d| !d.stopped)
            .unwrap_or_else(|p| p.into_inner())
            .0
            .stopped
    }
    fn prepare(&self, stream: usize, cursor: String) -> bool {
        let mut d = self.delivery.lock().unwrap_or_else(|p| p.into_inner());
        if d.stopped || d.pending[stream].is_some() {
            return false;
        }
        d.pending[stream] = Some(cursor);
        true
    }
    fn ack(&self, stream: usize, cursor: &str) -> Result<(), String> {
        let mut d = self.delivery.lock().unwrap_or_else(|p| p.into_inner());
        if d.stopped || d.pending[stream].as_deref() != Some(cursor) {
            return Err("stale_ack".into());
        }
        d.pending[stream] = None;
        self.wake.notify_all();
        Ok(())
    }
    fn delivered(&self, stream: usize) -> bool {
        let d = self.delivery.lock().unwrap_or_else(|p| p.into_inner());
        let (mut d, _) = self
            .wake
            .wait_timeout_while(d, ACK_DEADLINE, |d| {
                !d.stopped && d.pending[stream].is_some()
            })
            .unwrap_or_else(|p| p.into_inner());
        if d.pending[stream].is_some() {
            d.stopped = true;
            self.wake.notify_all();
        }
        !d.stopped
    }
}
#[derive(Default)]
pub(crate) struct SurfaceHub {
    current: Mutex<Option<Arc<Attachment>>>,
    next_id: AtomicU64,
    workers: Arc<AtomicUsize>,
}
struct Worker(Arc<AtomicUsize>);
impl Drop for Worker {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl SurfaceHub {
    pub fn detach_main(&self) {
        if let Some(a) = self
            .current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            a.stop();
        }
    }
    pub fn worker_count(&self) -> usize {
        self.workers.load(Ordering::Acquire)
    }
    fn attached(&self, id: &str) -> Result<Arc<Attachment>, String> {
        self.current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|a| a.id == id && !a.stopped())
            .cloned()
            .ok_or("attachment_closed".into())
    }
    fn create(&self, session_id: Option<String>) -> Result<Arc<Attachment>, String> {
        let mut current = self.current.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = current.take() {
            old.stop();
        }
        if self.worker_count() > MAX_BRIDGE_WORKERS - 2 {
            return Err("attachment_retiring_retry".into());
        }
        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "attachment_id_exhausted")?
            + 1;
        let a = Arc::new(Attachment::new(id.to_string(), session_id));
        *current = Some(a.clone());
        Ok(a)
    }
    fn spawn(&self, a: Arc<Attachment>, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
        self.workers
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_BRIDGE_WORKERS).then_some(n + 1)
            })
            .map_err(|_| {
                a.stop();
                "bridge_worker_limit".to_owned()
            })?;
        let guard = Worker(self.workers.clone());
        let stopped = a.clone();
        std::thread::Builder::new()
            .name("narys-terminal-bridge".into())
            .spawn(move || {
                let _guard = guard;
                f();
                stopped.stop();
            })
            .map_err(|_| {
                a.stop();
                "bridge_worker_failed".to_owned()
            })?;
        Ok(())
    }
}
fn main_only(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("human_main_only".into())
    }
}
fn error(e: ExecutionError) -> String {
    match e {
        ExecutionError::InputQueueFull => "input_queue_full",
        ExecutionError::InputTooLarge => "input_too_large",
        ExecutionError::NotRunning => "session_not_running",
        ExecutionError::InvalidDimensions => "invalid_dimensions",
        _ => "terminal_native_error",
    }
    .into()
}
#[tauri::command]
pub(crate) fn terminal_session_status(
    window: WebviewWindow,
    human: State<Arc<HumanTerminal>>,
) -> Result<Option<SessionDto>, String> {
    main_only(&window)?;
    Ok(human.status())
}
#[tauri::command]
pub(crate) fn open_human_terminal(
    window: WebviewWindow,
    human: State<Arc<HumanTerminal>>,
) -> Result<SessionDto, String> {
    main_only(&window)?;
    human.open().map_err(error)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachDto {
    attachment_id: String,
}
#[tauri::command]
pub(crate) fn attach_terminal_surface(
    window: WebviewWindow,
    hub: State<SurfaceHub>,
    human: State<Arc<HumanTerminal>>,
    trace: State<Arc<OperationalTraceBus>>,
    session_id: Option<String>,
    pty: Channel<Response>,
    status: Channel<SessionDto>,
    activity: Channel<TraceBatchDto>,
) -> Result<AttachDto, String> {
    main_only(&window)?;
    let session = session_id
        .as_deref()
        .map(|id| human.find(id).map_err(error))
        .transpose()?;
    let live = session
        .as_ref()
        .map(|s| s.subscribe().map_err(error))
        .transpose()?;
    let trace_live = trace.subscribe().map_err(|_| "trace_subscriber_limit")?;
    let a = hub.create(session_id)?;
    if let (Some(session), Some(live)) = (session, live) {
        let worker_a = a.clone();
        hub.spawn(a.clone(), move || {
            run_pty_bridge(
                &worker_a,
                session,
                live,
                |frame| pty.send(Response::new(frame)).is_ok(),
                |s| status.send(s).is_ok(),
            )
        })?;
    }
    let worker_a = a.clone();
    let bus = trace.inner().clone();
    hub.spawn(a.clone(), move || {
        run_trace_bridge(&worker_a, bus, trace_live, |b| activity.send(b).is_ok())
    })?;
    Ok(AttachDto {
        attachment_id: a.id.clone(),
    })
}
#[tauri::command]
pub(crate) fn detach_terminal_surface(
    window: WebviewWindow,
    hub: State<SurfaceHub>,
    attachment_id: String,
) -> Result<(), String> {
    main_only(&window)?;
    let mut current = hub.current.lock().unwrap_or_else(|p| p.into_inner());
    if current.as_ref().is_some_and(|a| a.id == attachment_id) {
        current.take().unwrap().stop();
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn acknowledge_terminal_batch(
    window: WebviewWindow,
    hub: State<SurfaceHub>,
    attachment_id: String,
    stream: String,
    cursor: String,
) -> Result<(), String> {
    main_only(&window)?;
    let index = match stream.as_str() {
        "pty" => 0,
        "trace" => 1,
        _ => return Err("invalid_stream".into()),
    };
    hub.attached(&attachment_id)?.ack(index, &cursor)
}
fn session_for_attachment(hub: &SurfaceHub, id: &str) -> Result<String, String> {
    hub.attached(id)?
        .session_id
        .clone()
        .ok_or("no_attached_session".into())
}
#[tauri::command]
pub(crate) fn send_terminal_input(
    window: WebviewWindow,
    hub: State<SurfaceHub>,
    human: State<Arc<HumanTerminal>>,
    request: Request<'_>,
) -> Result<(), String> {
    main_only(&window)?;
    let id = request
        .headers()
        .get("x-terminal-attachment")
        .and_then(|h| h.to_str().ok())
        .ok_or("attachment_required")?;
    let session = session_for_attachment(&hub, id)?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("raw_input_required".into());
    };
    human.input(&session, bytes).map_err(error)
}
#[tauri::command]
pub(crate) fn resize_terminal(
    window: WebviewWindow,
    hub: State<SurfaceHub>,
    human: State<Arc<HumanTerminal>>,
    attachment_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    main_only(&window)?;
    human
        .resize(&session_for_attachment(&hub, &attachment_id)?, rows, cols)
        .map_err(error)
}
#[tauri::command]
pub(crate) fn close_human_terminal(
    window: WebviewWindow,
    human: State<Arc<HumanTerminal>>,
    session_id: String,
) -> Result<bool, String> {
    main_only(&window)?;
    human.close(&session_id).map_err(error)
}

pub(crate) fn run_pty_bridge(
    a: &Attachment,
    session: PtySession,
    live: PtyLiveSubscriber,
    mut send: impl FnMut(Vec<u8>) -> bool,
    mut status: impl FnMut(SessionDto) -> bool,
) {
    let mut cursor = 0;
    let mut last_status = None;
    while !a.stopped() {
        let Ok(b) = session.replay(cursor, MAX_BATCH_CHUNKS, crate::execution::MAX_BATCH_BYTES)
        else {
            break;
        };
        let dto = HumanTerminal::dto(&session);
        let changed = last_status.as_ref() != Some(&dto);
        if b.next_after != cursor || changed {
            if !a.prepare(0, b.next_after.to_string()) {
                break;
            }
            if changed {
                if !status(dto.clone()) {
                    break;
                }
                last_status = Some(dto);
            }
            if !send(pty_frame(cursor, &b)) || !a.delivered(0) {
                break;
            }
            cursor = b.next_after;
        }
        if b.has_more {
            if !a.pause(BATCH_WINDOW) {
                break;
            }
            continue;
        }
        // Timeout checks factual process state only; no idle WebView traffic.
        if live.wait(CONSUMER_WAIT).is_some() && !a.pause(BATCH_WINDOW) {
            break;
        }
    }
}
pub(crate) fn run_trace_bridge(
    a: &Attachment,
    bus: Arc<OperationalTraceBus>,
    mut live: LiveSubscriber,
    mut send: impl FnMut(TraceBatchDto) -> bool,
) {
    let mut cursor = 0;
    let mut initial = true;
    let mut last_loss = 0;
    while !a.stopped() {
        let Ok(b) = bus.replay(cursor, BatchLimits::default()) else {
            break;
        };
        let more = b.has_more;
        let next = b.next_after;
        let loss = live.status().delivery_dropped;
        if initial || next != cursor || loss != last_loss {
            if !a.prepare(1, next.to_string())
                || !send(trace_batch(cursor, b, loss))
                || !a.delivered(1)
            {
                break;
            }
            initial = false;
            cursor = next;
            last_loss = loss;
        }
        if more {
            if !a.pause(BATCH_WINDOW) {
                break;
            }
            continue;
        }
        // Drain wake events; recovery always reads priority-aware retained facts.
        if live.wait(CONSUMER_WAIT) {
            if !a.pause(BATCH_WINDOW) {
                break;
            }
            live.drain_batch(BatchLimits::default());
        }
    }
}
