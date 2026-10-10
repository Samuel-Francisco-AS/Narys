use super::broker::{finish, initial_result, WorkerGuard};
use super::*;
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex, Weak,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Debug)]
pub struct PtyChunk {
    pub sequence: u64,
    pub bytes: Box<[u8]>,
}
#[derive(Default)]
struct Output {
    chunks: VecDeque<PtyChunk>,
    bytes: usize,
    latest: u64,
    total_bytes: u64,
    dropped_bytes: u64,
    dropped_chunks: u64,
    read_error: bool,
    incomplete: bool,
    observers: Vec<(u64, SyncSender<u64>, Arc<AtomicU64>)>,
    next_observer: u64,
}
impl Output {
    fn append(&mut self, bytes: &[u8]) -> bool {
        let Some(sequence) = self.latest.checked_add(1) else {
            self.read_error = true;
            return false;
        };
        self.latest = sequence;
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        while self.chunks.len() >= PTY_RETAIN_CHUNKS || self.bytes + bytes.len() > PTY_RETAIN_BYTES
        {
            if let Some(old) = self.chunks.pop_front() {
                self.bytes -= old.bytes.len();
                self.dropped_bytes = self.dropped_bytes.saturating_add(old.bytes.len() as u64);
                self.dropped_chunks = self.dropped_chunks.saturating_add(1);
            } else {
                break;
            }
        }
        self.bytes += bytes.len();
        self.chunks.push_back(PtyChunk {
            sequence,
            bytes: bytes.into(),
        });
        // Notifications carry only a cursor, never another copy of output.
        // A full queue loses a wakeup; replay remains the recovery authority.
        self.observers
            .retain(|(_, tx, loss)| match tx.try_send(sequence) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    let _ = loss.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                        Some(n.saturating_add(1))
                    });
                    true
                }
                Err(TrySendError::Disconnected(_)) => false,
            });
        true
    }
}
pub const PTY_LIVE_QUEUE: usize = 1;
pub const PTY_MAX_SUBSCRIBERS: usize = 2;
pub struct PtyLiveSubscriber {
    id: u64,
    data: Weak<PtyData>,
    receiver: Receiver<u64>,
    pub cursor: u64,
    loss: Arc<AtomicU64>,
}
impl PtyLiveSubscriber {
    pub fn lost_notifications(&self) -> u64 {
        self.loss.load(Ordering::Acquire)
    }
    /// Consumer-only blocking wait. Producer uses try_send under bounded storage lock.
    pub fn wait(&self, timeout: Duration) -> Option<u64> {
        self.receiver.recv_timeout(timeout).ok()
    }
}
impl Drop for PtyLiveSubscriber {
    fn drop(&mut self) {
        if let Some(data) = self.data.upgrade() {
            data.output
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observers
                .retain(|(id, _, _)| *id != self.id);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PtyLifecycle {
    Bounded,
    HumanInteractive,
}
impl PtyLifecycle {
    pub fn expired(self, elapsed: Duration, timeout: Duration) -> bool {
        self == Self::Bounded && elapsed >= timeout
    }
}
struct PtyData {
    output: Mutex<Output>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    dimensions: Mutex<PtyDimensions>,
    input: SyncSender<Vec<u8>>,
    request: ExecutionRequest,
    created_at: SystemTime,
    lifecycle: PtyLifecycle,
}
#[derive(Clone)]
pub struct PtySession {
    execution: ExecutionHandle,
    data: Arc<PtyData>,
}
#[derive(Debug)]
pub struct PtyReplay {
    pub id: PtySessionId,
    pub origin: ExecutionOrigin,
    pub state: ExecutionState,
    pub created_at: SystemTime,
    pub dimensions: PtyDimensions,
    pub chunks: Vec<PtyChunk>,
    pub latest_sequence: u64,
    pub retained_range: Option<(u64, u64)>,
    pub retained_bytes: usize,
    pub retained_chunks: usize,
    pub total_bytes: u64,
    pub dropped_bytes: u64,
    pub dropped_chunks: u64,
    pub gap: bool,
    pub next_after: u64,
    pub has_more: bool,
    pub read_error: bool,
    pub incomplete: bool,
}
impl PtySession {
    pub fn subscribe(&self) -> Result<PtyLiveSubscriber, ExecutionError> {
        let mut output = self.data.output.lock().unwrap_or_else(|p| p.into_inner());
        if output.observers.len() >= PTY_MAX_SUBSCRIBERS {
            return Err(ExecutionError::SessionLimit);
        }
        let id = output
            .next_observer
            .checked_add(1)
            .ok_or(ExecutionError::IdExhausted)?;
        output.next_observer = id;
        let (tx, receiver) = mpsc::sync_channel(PTY_LIVE_QUEUE);
        let loss = Arc::new(AtomicU64::new(0));
        output.observers.push((id, tx, loss.clone()));
        Ok(PtyLiveSubscriber {
            id,
            data: Arc::downgrade(&self.data),
            receiver,
            cursor: output.latest,
            loss,
        })
    }
    #[cfg(any(test, feature = "desktop-tests"))]
    pub fn subscriber_count(&self) -> usize {
        self.data.output.lock().unwrap().observers.len()
    }

    pub fn id(&self) -> PtySessionId {
        PtySessionId(self.execution.id())
    }
    pub fn request(&self) -> &ExecutionRequest {
        &self.data.request
    }
    pub fn process_id(&self) -> Option<u32> {
        self.execution.process_id()
    }
    pub fn state(&self) -> ExecutionState {
        self.execution.state()
    }
    pub fn origin(&self) -> &ExecutionOrigin {
        &self.data.request.origin
    }
    pub fn result(&self) -> Option<Arc<ExecutionResult>> {
        self.execution.result()
    }
    pub fn wait(&self, timeout: Duration) -> Option<Arc<ExecutionResult>> {
        self.execution.wait(timeout)
    }
    pub fn close(&self, authority: &ExecutionAuthority) -> Result<bool, ExecutionError> {
        authority.authorize_input(self.origin())?;
        Ok(self.execution.cancel())
    }
    /// Bounded nonblocking enqueue, not an acknowledgment that the shell consumed
    /// bytes. One writer worker serializes input; no ownership handoff exists.
    pub fn send_input(
        &self,
        origin: &ExecutionOrigin,
        authority: &ExecutionAuthority,
        bytes: &[u8],
    ) -> Result<(), ExecutionError> {
        authority.authorize_input(origin)?;
        if origin != self.origin() {
            return Err(ExecutionError::AuthorityDenied);
        }
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(ExecutionError::InputTooLarge);
        }
        let data = self
            .execution
            .control
            .data
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if data.state != ExecutionState::Running || data.cancelled {
            return Err(ExecutionError::NotRunning);
        }
        match self.data.input.try_send(bytes.to_vec()) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(ExecutionError::InputQueueFull),
            Err(TrySendError::Disconnected(_)) => Err(ExecutionError::NotRunning),
        }
    }
    pub fn resize(
        &self,
        dimensions: PtyDimensions,
        authority: &ExecutionAuthority,
    ) -> Result<(), ExecutionError> {
        authority.authorize_input(self.origin())?;
        let dimensions = dimensions.validate()?;
        let master = self.data.master.lock().unwrap_or_else(|p| p.into_inner());
        master
            .as_ref()
            .ok_or(ExecutionError::NotRunning)?
            .resize(size(dimensions))
            .map_err(|_| ExecutionError::Io)?;
        *self
            .data
            .dimensions
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = dimensions;
        Ok(())
    }
    pub fn replay(
        &self,
        after: u64,
        max_chunks: usize,
        max_bytes: usize,
    ) -> Result<PtyReplay, ExecutionError> {
        if max_chunks == 0
            || max_chunks > MAX_BATCH_CHUNKS
            || !(READ_CHUNK_BYTES..=MAX_BATCH_BYTES).contains(&max_bytes)
        {
            return Err(ExecutionError::InvalidBatch);
        }
        let output = self.data.output.lock().unwrap_or_else(|p| p.into_inner());
        if after > output.latest {
            return Err(ExecutionError::FutureCursor);
        }
        let mut chunks = Vec::new();
        let mut bytes = 0;
        let mut has_more = false;
        for chunk in output.chunks.iter().filter(|c| c.sequence > after) {
            if chunks.len() == max_chunks || bytes + chunk.bytes.len() > max_bytes {
                has_more = true;
                break;
            }
            bytes += chunk.bytes.len();
            chunks.push(chunk.clone());
        }
        let oldest = output.chunks.front().map(|c| c.sequence);
        let gap = oldest.is_some_and(|first| after < first - 1);
        let next_after = if has_more {
            chunks.last().map_or(after, |c| c.sequence)
        } else {
            output.latest
        };
        Ok(PtyReplay {
            id: self.id(),
            origin: self.origin().clone(),
            state: self.state(),
            created_at: self.data.created_at,
            dimensions: *self
                .data
                .dimensions
                .lock()
                .unwrap_or_else(|p| p.into_inner()),
            chunks,
            latest_sequence: output.latest,
            retained_range: oldest.map(|first| (first, output.latest)),
            retained_bytes: output.bytes,
            retained_chunks: output.chunks.len(),
            total_bytes: output.total_bytes,
            dropped_bytes: output.dropped_bytes,
            dropped_chunks: output.dropped_chunks,
            gap,
            next_after,
            has_more,
            read_error: output.read_error,
            incomplete: output.incomplete,
        })
    }
}
impl ExecutionBroker {
    pub fn open_pty(
        self: &Arc<Self>,
        request: &ExecutionRequest,
        dimensions: PtyDimensions,
        authority: &ExecutionAuthority,
    ) -> Result<PtySession, ExecutionError> {
        self.open_pty_with_lifecycle(request, dimensions, authority, PtyLifecycle::Bounded)
    }
    fn open_pty_with_lifecycle(
        self: &Arc<Self>,
        request: &ExecutionRequest,
        dimensions: PtyDimensions,
        authority: &ExecutionAuthority,
        lifecycle: PtyLifecycle,
    ) -> Result<PtySession, ExecutionError> {
        authority.authorize(request, ExecutionMode::Pty)?;
        // No fixture authority and no agent origin can open/write a human PTY.
        authority.authorize_input(&request.origin)?;
        let request = request.validate()?;
        let dimensions = dimensions.validate()?;
        let (input, receiver) = mpsc::sync_channel(INPUT_QUEUE_CHUNKS);
        let data = Arc::new(PtyData {
            output: Mutex::new(Output::default()),
            master: Mutex::new(None),
            dimensions: Mutex::new(dimensions),
            input,
            request,
            created_at: SystemTime::now(),
            lifecycle,
        });
        let worker_data = data.clone();
        let execution = self.launch(ExecutionMode::Pty, move |handle, broker| {
            run_pty(handle, broker, worker_data, receiver)
        })?;
        Ok(PtySession { execution, data })
    }
    pub fn open_human_shell(
        self: &Arc<Self>,
        cwd: &Path,
        dimensions: PtyDimensions,
        authority: &ExecutionAuthority,
    ) -> Result<PtySession, ExecutionError> {
        authority.authorize_input(&ExecutionOrigin::Human)?;
        let program = resolve_human_shell()?;
        let request = ExecutionRequest {
            program,
            args: Vec::new(),
            cwd: cwd.into(),
            origin: ExecutionOrigin::Human,
            task_id: None,
            correlation: None,
            workspace: None,
            environment: EnvironmentPolicy::HumanInherited,
            timeout: MAX_TIMEOUT,
            capture: CapturePolicy::default(),
        };
        self.open_pty_with_lifecycle(
            &request,
            dimensions,
            authority,
            PtyLifecycle::HumanInteractive,
        )
    }
}
/// Absolute executable $SHELL, then /bin/bash, then /bin/sh; no command string,
/// concatenated initialization or login-shell requirement. Native human only.
pub fn resolve_human_shell() -> Result<PathBuf, ExecutionError> {
    let candidates = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .into_iter()
        .chain([PathBuf::from("/bin/bash"), PathBuf::from("/bin/sh")]);
    for candidate in candidates {
        use std::os::unix::fs::PermissionsExt;
        let bytes = candidate.as_os_str().as_encoded_bytes();
        if candidate.is_absolute()
            && !bytes.contains(&0)
            && bytes.len() <= MAX_PATH_BYTES
            && std::fs::metadata(&candidate)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        {
            return Ok(candidate);
        }
    }
    Err(ExecutionError::InvalidRequest)
}
fn size(d: PtyDimensions) -> PtySize {
    PtySize {
        rows: d.rows,
        cols: d.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}
fn run_pty(
    handle: ExecutionHandle,
    broker: Arc<ExecutionBroker>,
    data: Arc<PtyData>,
    receiver: mpsc::Receiver<Vec<u8>>,
) {
    let mut result = initial_result(&handle, data.request.clone(), ExecutionMode::Pty);
    result.created_at = data.created_at;
    broker.trace(
        &handle,
        &result.request,
        result.mode,
        ExecutionState::Starting,
    );
    if handle.control.cancelled() {
        result.state = ExecutionState::Cancelled;
        finish(&handle, &broker, result);
        return;
    }
    // Prepare reader/writer before spawning so setup failure cannot orphan a child.
    let prepared = (|| {
        os::check_session_cleanup().map_err(|_| ())?;
        let pair = portable_pty::native_pty_system()
            .openpty(size(
                *data.dimensions.lock().unwrap_or_else(|p| p.into_inner()),
            ))
            .map_err(|_| ())?;
        let fd = pair.master.as_raw_fd().ok_or(())?;
        os::nonblocking(fd).map_err(|_| ())?;
        let reader = pair.master.try_clone_reader().map_err(|_| ())?;
        let writer = pair.master.take_writer().map_err(|_| ())?;
        Ok::<_, ()>((pair, reader, writer))
    })();
    let (pair, mut reader, mut writer) = match prepared {
        Ok(p) => p,
        Err(_) => {
            result.state = ExecutionState::Failed;
            result.runtime_error = true;
            finish(&handle, &broker, result);
            return;
        }
    };
    let mut command = CommandBuilder::new(&result.request.program);
    command.args(&result.request.args);
    command.cwd(&result.request.cwd);
    if let EnvironmentPolicy::Controlled(env) = &result.request.environment {
        command.env_clear();
        for (key, value) in env {
            command.env(key, value);
        }
    }
    let mut child = match pair.slave.spawn_command(command) {
        Ok(child) => child,
        Err(_) => {
            result.spawn_failed = true;
            result.state = ExecutionState::Failed;
            finish(&handle, &broker, result);
            return;
        }
    };
    drop(pair.slave);
    *data.master.lock().unwrap_or_else(|p| p.into_inner()) = Some(pair.master);
    let pid = child.process_id().expect("native Linux child PID");
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = stop.clone();
    let reader_data = data.clone();
    let guard = WorkerGuard::new(broker.workers.clone());
    let reader_thread = thread::Builder::new()
        .name("narys-pty-reader".into())
        .spawn(move || {
            let _guard = guard;
            let mut bytes = [0u8; READ_CHUNK_BYTES];
            loop {
                if reader_stop.load(Ordering::Acquire) {
                    reader_data
                        .output
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .incomplete = true;
                    break;
                }
                match reader.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => {
                        if !reader_data
                            .output
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .append(&bytes[..n])
                        {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => os::poll_pause(),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        reader_data
                            .output
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .read_error = true;
                        break;
                    }
                }
            }
        });
    if reader_thread.is_err() {
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
    let started = Instant::now();
    let mut terminal = None;
    let mut term_at = None;
    let mut exit_observed = false;
    let mut child_owned = true;
    let mut kill_sent = false;
    let mut pending: Option<(Vec<u8>, usize)> = None;
    loop {
        if terminal.is_none() {
            terminal = handle.control.stop_cause(
                if result.runtime_error
                    || data
                        .output
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .read_error
                {
                    Some(ExecutionState::Failed)
                } else if data
                    .lifecycle
                    .expired(started.elapsed(), result.request.timeout)
                {
                    Some(ExecutionState::TimedOut)
                } else {
                    None
                },
            );
            if terminal.is_some() {
                result.runtime_error |= !os::signal_session(pid, libc::SIGTERM);
                term_at = Some(Instant::now());
            }
        }
        if terminal.is_none() {
            if pending.is_none() {
                pending = receiver.try_recv().ok().map(|bytes| (bytes, 0));
            }
            if let Some((bytes, offset)) = &mut pending {
                if *offset == bytes.len() {
                    pending = None;
                } else {
                    match writer.write(&bytes[*offset..bytes.len().min(*offset + READ_CHUNK_BYTES)])
                    {
                        Ok(0) => result.runtime_error = true,
                        Ok(n) => *offset += n,
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                            ) => {}
                        Err(_) => result.runtime_error = true,
                    }
                }
            }
        }
        let exited = match os::exited(pid) {
            Ok(exited) => exited,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
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
                    result.runtime_error |= !os::signal_session(pid, libc::SIGKILL);
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
    if child_owned {
        result.runtime_error |= !os::signal_session(pid, libc::SIGKILL);
    }
    if exit_observed {
        match child.wait() {
            Ok(status) => {
                result.reaped = true;
                result.exit_code = if status.signal().is_none() {
                    Some(status.exit_code() as i32)
                } else {
                    None
                };
                result.signal = status.signal().map(str::to_owned);
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
    // Stop accepting queued input and drop writable/resize handles. Reader stays
    // alive to consume the final kernel bytes, then bounded stop even on escape.
    drop(receiver);
    drop(writer);
    data.master.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Ok(reader) = reader_thread {
        let drain_deadline = Instant::now() + DRAIN_GRACE;
        while !reader.is_finished() && Instant::now() < drain_deadline {
            os::poll_pause();
        }
        stop.store(true, Ordering::Release);
        if reader.join().is_err() {
            result.runtime_error = true;
        }
    }
    let output = data.output.lock().unwrap_or_else(|p| p.into_inner());
    result.runtime_error |= output.read_error;
    drop(output);
    if result.runtime_error && result.state == ExecutionState::Completed {
        result.state = ExecutionState::Failed;
    }
    let cleanup_pending = result.cleanup_pending;
    finish(&handle, &broker, result);
    if cleanup_pending {
        os::deferred_reap(
            pid,
            || {
                child.wait().map(|s| {
                    (
                        if s.signal().is_none() {
                            Some(s.exit_code() as i32)
                        } else {
                            None
                        },
                        s.signal().map(str::to_owned),
                    )
                })
            },
            &handle,
        );
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn output_history_caps_tiny_chunks_and_sequence_exhaustion_is_explicit() {
        let mut output = Output::default();
        for _ in 0..PTY_RETAIN_CHUNKS + 100 {
            assert!(output.append(b"x"));
        }
        assert_eq!(output.chunks.len(), PTY_RETAIN_CHUNKS);
        assert_eq!(output.bytes, PTY_RETAIN_CHUNKS);
        assert_eq!(output.dropped_chunks, 100);
        assert_eq!(output.dropped_bytes, 100);
        output.latest = u64::MAX;
        assert!(!output.append(b"later"));
        assert!(output.read_error);
        assert_eq!(output.latest, u64::MAX);
    }
    #[test]
    fn input_queue_full_is_nonblocking_and_bounded() {
        let (sender, receiver) = mpsc::sync_channel(INPUT_QUEUE_CHUNKS);
        for _ in 0..INPUT_QUEUE_CHUNKS {
            sender.try_send(vec![0; MAX_INPUT_BYTES]).unwrap();
        }
        assert!(matches!(
            sender.try_send(vec![0; MAX_INPUT_BYTES]),
            Err(TrySendError::Full(_))
        ));
        assert_eq!(
            receiver.try_iter().map(|c| c.len()).sum::<usize>(),
            INPUT_QUEUE_CHUNKS * MAX_INPUT_BYTES
        );
    }
}
