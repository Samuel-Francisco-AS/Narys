use super::*;
use crate::operational_trace::{
    CriticalKind, EventDraft, OperationalKind, OperationalTraceBus, Provenance, SourceType,
    StateKind, TraceId, TraceSource, TraceText,
};
use std::{
    collections::HashMap,
    os::{
        fd::AsRawFd,
        unix::process::{CommandExt, ExitStatusExt},
    },
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub(super) struct RunData {
    pub state: ExecutionState,
    pub cancelled: bool,
    pub stop_cause: Option<ExecutionState>,
    pub process_id: Option<u32>,
    pub result: Option<Arc<ExecutionResult>>,
}
pub(super) struct RunControl {
    pub data: Mutex<RunData>,
}
impl RunControl {
    fn new() -> Self {
        Self {
            data: Mutex::new(RunData {
                state: ExecutionState::Starting,
                cancelled: false,
                stop_cause: None,
                process_id: None,
                result: None,
            }),
        }
    }
    pub fn cancel(&self) -> bool {
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        if data.result.is_some() || data.stop_cause.is_some() {
            return false;
        }
        data.cancelled = true;
        data.stop_cause = Some(ExecutionState::Cancelled);
        true
    }
    pub fn cancelled(&self) -> bool {
        self.data
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .cancelled
    }
    /// First accepted timeout/cancellation wins, serialized with cancellation.
    pub fn stop_cause(&self, proposed: Option<ExecutionState>) -> Option<ExecutionState> {
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        if data.stop_cause.is_none() {
            data.stop_cause = proposed;
        }
        data.stop_cause
    }
    pub fn running(&self, pid: u32) {
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        data.state = ExecutionState::Running;
        data.process_id = Some(pid);
    }
    pub fn finish(&self, mut result: ExecutionResult) {
        let mut data = self.data.lock().unwrap_or_else(|p| p.into_inner());
        // Serialize accepted cancellation with terminal publication.
        if let Some(cause) = data.stop_cause {
            result.state = cause;
        }
        data.state = result.state;
        data.result = Some(Arc::new(result));
    }
}
struct Entry {
    control: Arc<RunControl>,
    worker: JoinHandle<()>,
    mode: ExecutionMode,
}
#[derive(Default)]
struct Registry {
    shutting_down: bool,
    entries: HashMap<ExecutionId, Entry>,
}

/// Sole production instance in Tauri managed state; no UI owns a process.
pub struct ExecutionBroker {
    ids: IdAllocator,
    registry: Mutex<Registry>,
    pub(super) workers: Arc<AtomicUsize>,
    trace: Arc<OperationalTraceBus>,
    #[cfg(test)]
    pub(super) reject_trace: AtomicBool,
}
impl ExecutionBroker {
    pub fn process_wide() -> Arc<Self> {
        static BROKER: OnceLock<Arc<ExecutionBroker>> = OnceLock::new();
        BROKER.get_or_init(|| Arc::new(Self::new())).clone()
    }
    fn new() -> Self {
        Self {
            ids: IdAllocator::default(),
            registry: Mutex::new(Registry::default()),
            workers: Arc::new(AtomicUsize::new(0)),
            trace: OperationalTraceBus::process_wide(),
            #[cfg(test)]
            reject_trace: AtomicBool::new(false),
        }
    }
    #[cfg(test)]
    pub(crate) fn isolated() -> Arc<Self> {
        Arc::new(Self::new())
    }
    #[cfg(test)]
    pub(crate) fn seed_test_id(&self, id: u64) {
        self.ids.0.store(id, Ordering::Relaxed);
    }
    fn prune(registry: &mut Registry) {
        registry.entries.retain(|_, e| !e.worker.is_finished());
        // Dropping an already-finished JoinHandle releases OS thread resources;
        // all internal reader/writer handles have been joined by that worker.
    }
    pub(super) fn launch<F>(
        self: &Arc<Self>,
        mode: ExecutionMode,
        run: F,
    ) -> Result<ExecutionHandle, ExecutionError>
    where
        F: FnOnce(ExecutionHandle, Arc<Self>) + Send + 'static,
    {
        let mut registry = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        Self::prune(&mut registry);
        if registry.shutting_down {
            return Err(ExecutionError::ShuttingDown);
        }
        if registry.entries.len() >= MAX_ACTIVE_EXECUTIONS {
            return Err(ExecutionError::ActiveLimit);
        }
        if mode == ExecutionMode::Pty
            && registry
                .entries
                .values()
                .filter(|e| e.mode == ExecutionMode::Pty)
                .count()
                >= MAX_PTY_SESSIONS
        {
            return Err(ExecutionError::SessionLimit);
        }
        let id = self.ids.next()?;
        let handle = ExecutionHandle {
            id,
            control: Arc::new(RunControl::new()),
        };
        let worker_handle = handle.clone();
        let broker = self.clone();
        let guard = WorkerGuard::new(self.workers.clone());
        let worker = thread::Builder::new()
            .name(format!("narys-exec-{}", id.get()))
            .spawn(move || {
                let _guard = guard;
                run(worker_handle, broker);
            })
            .map_err(|_| ExecutionError::WorkerSpawn)?;
        registry.entries.insert(
            id,
            Entry {
                control: handle.control.clone(),
                worker,
                mode,
            },
        );
        Ok(handle)
    }
    pub fn submit(
        self: &Arc<Self>,
        request: &ExecutionRequest,
        authority: &ExecutionAuthority,
    ) -> Result<ExecutionHandle, ExecutionError> {
        authority.authorize(request, ExecutionMode::Structured)?;
        let request = request.validate()?;
        self.launch(ExecutionMode::Structured, move |handle, broker| {
            run_structured(handle, broker, request)
        })
    }
    pub fn request_shutdown(&self) {
        let mut registry = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        registry.shutting_down = true;
        for entry in registry.entries.values() {
            entry.control.cancel();
        }
    }
    pub fn active_count(&self) -> usize {
        let mut r = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        Self::prune(&mut r);
        r.entries.len()
    }
    pub fn active_sessions(&self) -> usize {
        let mut r = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        Self::prune(&mut r);
        r.entries
            .values()
            .filter(|e| e.mode == ExecutionMode::Pty)
            .count()
    }
    pub fn worker_count(&self) -> usize {
        self.workers.load(Ordering::Acquire)
    }
    pub fn stopped(&self) -> bool {
        self.active_count() == 0 && self.worker_count() == 0
    }
    pub fn wait_shutdown(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout.min(SHUTDOWN_DEADLINE);
        while !self.stopped() && Instant::now() < deadline {
            os::poll_pause();
        }
        self.stopped()
    }
    pub(super) fn trace(
        &self,
        handle: &ExecutionHandle,
        request: &ExecutionRequest,
        mode: ExecutionMode,
        state: ExecutionState,
    ) {
        // Lifecycle metadata only: no argv, path, environment, input or output.
        // Errors/exhaustion/full subscribers never affect execution control.
        let code = match state {
            ExecutionState::Starting => "starting",
            ExecutionState::Running => "running",
            ExecutionState::Completed => "completed",
            ExecutionState::Failed => "failed",
            ExecutionState::Cancelled => "cancelled",
            ExecutionState::TimedOut => "timed_out",
        };
        let code = format!(
            "{}_{}",
            if mode == ExecutionMode::Pty {
                "pty"
            } else {
                "exec"
            },
            code
        );
        let text = TraceText::new(&code).expect("static bounded lifecycle");
        let kind = match state {
            ExecutionState::Starting | ExecutionState::Running => OperationalKind::State {
                kind: StateKind::CommandLifecycle,
                code: TraceId::new(&code).unwrap(),
                detail: text,
            },
            _ => OperationalKind::Critical {
                kind: match state {
                    ExecutionState::Completed => CriticalKind::Completed,
                    ExecutionState::Cancelled => CriticalKind::Cancelled,
                    _ => CriticalKind::Failed,
                },
                code: TraceId::new(&code).unwrap(),
                message: text,
            },
        };
        let provenance = Provenance {
            source: TraceSource {
                source_type: SourceType::ExecutionBroker,
                id: TraceId::new("execution-broker").unwrap(),
                instance: Some(TraceId::new(&format!("execution:{}", handle.id.get())).unwrap()),
            },
            task_id: request.task_id,
            subtask_id: None,
            correlation_id: request.correlation.clone(),
            coalescing_key: None,
        };
        if let Ok(draft) = EventDraft::new(provenance, kind) {
            let _ = self.publish_lifecycle(draft);
        }
    }
    fn publish_lifecycle(
        &self,
        draft: EventDraft,
    ) -> Result<crate::operational_trace::PublishReceipt, crate::operational_trace::TraceError>
    {
        #[cfg(test)]
        if self.reject_trace.load(Ordering::Acquire) {
            return Err(crate::operational_trace::TraceError::SequenceExhausted);
        }
        self.trace.publish(draft)
    }
}
#[derive(Clone)]
pub struct ExecutionHandle {
    pub(super) id: ExecutionId,
    pub(super) control: Arc<RunControl>,
}
impl ExecutionHandle {
    pub fn id(&self) -> ExecutionId {
        self.id
    }
    pub fn process_id(&self) -> Option<u32> {
        self.control
            .data
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .process_id
    }
    pub fn state(&self) -> ExecutionState {
        self.control
            .data
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .state
    }
    pub fn cancel(&self) -> bool {
        self.control.cancel()
    }
    pub fn result(&self) -> Option<Arc<ExecutionResult>> {
        self.control
            .data
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .result
            .clone()
    }
    pub fn wait(&self, timeout: Duration) -> Option<Arc<ExecutionResult>> {
        let deadline = Instant::now() + timeout.min(MAX_TIMEOUT);
        loop {
            if let Some(result) = self.result() {
                return Some(result);
            }
            if Instant::now() >= deadline {
                return None;
            }
            os::poll_pause();
        }
    }
}
pub(super) struct WorkerGuard(Arc<AtomicUsize>);
impl WorkerGuard {
    pub fn new(count: Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        Self(count)
    }
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(super) fn initial_result(
    handle: &ExecutionHandle,
    request: ExecutionRequest,
    mode: ExecutionMode,
) -> ExecutionResult {
    ExecutionResult {
        id: handle.id,
        mode,
        request,
        state: ExecutionState::Starting,
        created_at: SystemTime::now(),
        finished_at: SystemTime::now(),
        exit_code: None,
        signal: None,
        spawn_failed: false,
        runtime_error: false,
        reaped: false,
        cleanup_pending: false,
        process_id: None,
        stdout: CapturedOutput::default(),
        stderr: CapturedOutput::default(),
    }
}
pub(super) fn finish(
    handle: &ExecutionHandle,
    broker: &ExecutionBroker,
    mut result: ExecutionResult,
) {
    result.finished_at = SystemTime::now();
    handle.control.finish(result);
    let result = handle.result().expect("published terminal");
    broker.trace(handle, &result.request, result.mode, result.state);
}
fn run_structured(
    handle: ExecutionHandle,
    broker: Arc<ExecutionBroker>,
    request: ExecutionRequest,
) {
    let mut result = initial_result(&handle, request, ExecutionMode::Structured);
    broker.trace(
        &handle,
        &result.request,
        result.mode,
        ExecutionState::Starting,
    );
    let r = &result.request;
    let mut command = Command::new(&r.program);
    command
        .args(&r.args)
        .current_dir(&r.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let EnvironmentPolicy::Controlled(env) = &r.environment {
        command.env_clear().envs(env.iter().cloned());
    }
    if handle.control.cancelled() {
        result.state = ExecutionState::Cancelled;
        finish(&handle, &broker, result);
        return;
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            result.spawn_failed = true;
            result.state = ExecutionState::Failed;
            finish(&handle, &broker, result);
            return;
        }
    };
    let pid = child.id();
    let out = child.stdout.take().expect("piped stdout");
    let err = child.stderr.take().expect("piped stderr");
    let stop = Arc::new(AtomicBool::new(false));
    let readers = (|| {
        os::nonblocking(out.as_raw_fd()).map_err(|_| ())?;
        os::nonblocking(err.as_raw_fd()).map_err(|_| ())?;
        let cap = result.request.capture.stdout_bytes;
        let reader_stop = stop.clone();
        let guard = WorkerGuard::new(broker.workers.clone());
        let stdout = thread::Builder::new()
            .name("narys-stdout".into())
            .spawn(move || {
                let _guard = guard;
                os::drain(out, cap, &reader_stop)
            })
            .map_err(|_| ())?;
        let cap = result.request.capture.stderr_bytes;
        let reader_stop = stop.clone();
        let guard = WorkerGuard::new(broker.workers.clone());
        let stderr = thread::Builder::new()
            .name("narys-stderr".into())
            .spawn(move || {
                let _guard = guard;
                os::drain(err, cap, &reader_stop)
            });
        Ok::<_, ()>((stdout, stderr))
    })();
    if readers.as_ref().map_or(true, |(_, stderr)| stderr.is_err()) {
        result.runtime_error = true;
    }
    handle.control.running(pid);
    result.process_id = Some(pid);
    broker.trace(
        &handle,
        &result.request,
        result.mode,
        ExecutionState::Running,
    );
    let deadline = Instant::now() + result.request.timeout;
    let mut terminal = None;
    let mut term_at = None;
    let mut exit_observed = false;
    let mut child_owned = true;
    let mut kill_sent = false;
    loop {
        if terminal.is_none() {
            terminal = handle.control.stop_cause(if result.runtime_error {
                Some(ExecutionState::Failed)
            } else if Instant::now() >= deadline {
                Some(ExecutionState::TimedOut)
            } else {
                None
            });
            if terminal.is_some() {
                os::signal_group(pid as i32, libc::SIGTERM);
                term_at = Some(Instant::now());
            }
        }
        let exited = match os::exited(pid) {
            Ok(exited) => exited,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            // Do not signal an ID we can no longer prove belongs to our child.
            Err(_) => {
                result.runtime_error = true;
                child_owned = false;
                break;
            }
        };
        exit_observed = exited;
        if exited && terminal.is_none() {
            break;
        }
        if let Some(at) = term_at {
            if at.elapsed() >= TERMINATION_GRACE {
                if !kill_sent {
                    os::signal_group(pid as i32, libc::SIGKILL);
                    kill_sent = true;
                }
                if exited {
                    break;
                }
            }
            if at.elapsed() >= TERMINATION_GRACE + REAP_GRACE {
                result.cleanup_pending = true;
                break;
            }
        }
        os::poll_pause();
    }
    // Always kill surviving managed group members, even on ordinary leader exit.
    if child_owned {
        os::signal_group(pid as i32, libc::SIGKILL);
    }
    // wait is called only after WNOWAIT observed exit, never as an unbounded
    // cancellation wait. A kernel-stuck child keeps its counted slot below.
    if exit_observed {
        match child.wait() {
            Ok(status) => {
                result.reaped = true;
                result.exit_code = status.code();
                result.signal = status.signal().map(|n| n.to_string());
                result.state = terminal.unwrap_or(if status.success() {
                    ExecutionState::Completed
                } else {
                    ExecutionState::Failed
                });
            }
            Err(_) => {
                result.runtime_error = true;
                result.state = terminal.unwrap_or(ExecutionState::Failed);
            }
        }
    } else {
        result.state = terminal.unwrap_or(ExecutionState::Failed);
    }
    if let Ok((stdout, stderr)) = readers {
        let drain_deadline = Instant::now() + DRAIN_GRACE;
        while (!stdout.is_finished() || stderr.as_ref().is_ok_and(|r| !r.is_finished()))
            && Instant::now() < drain_deadline
        {
            os::poll_pause();
        }
        stop.store(true, Ordering::Release);
        result.stdout = stdout.join().unwrap_or_else(|_| CapturedOutput {
            read_error: true,
            ..Default::default()
        });
        match stderr {
            Ok(reader) => {
                result.stderr = reader.join().unwrap_or_else(|_| CapturedOutput {
                    read_error: true,
                    ..Default::default()
                })
            }
            Err(_) => result.runtime_error = true,
        }
    }
    if result.runtime_error || result.stdout.read_error || result.stderr.read_error {
        if result.state == ExecutionState::Completed {
            result.state = ExecutionState::Failed;
        }
    }
    let cleanup_pending = result.cleanup_pending;
    finish(&handle, &broker, result);
    if cleanup_pending {
        os::deferred_reap(
            pid,
            || {
                child
                    .wait()
                    .map(|s| (s.code(), s.signal().map(|n| n.to_string())))
            },
            &handle,
        );
    }
}
