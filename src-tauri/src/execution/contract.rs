//! Native contracts. Provenance is caller-supplied information, never authority.
use crate::{luna::task::TaskId, operational_trace::TraceId};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

pub const MAX_ACTIVE_EXECUTIONS: usize = 8; // includes PTYs
pub const MAX_PTY_SESSIONS: usize = 4;
pub const MAX_ARGS: usize = 256;
pub const MAX_ARG_BYTES: usize = 64 * 1024;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_ENV_ENTRIES: usize = 128;
pub const MAX_ENV_BYTES: usize = 64 * 1024;
pub const MAX_SCOPE_ROOTS: usize = 16;
pub const MAX_TIMEOUT: Duration = Duration::from_secs(3600);
pub const MAX_CAPTURE_BYTES: usize = 2 * 1024 * 1024;
pub const PTY_RETAIN_BYTES: usize = 2 * 1024 * 1024;
pub const PTY_RETAIN_CHUNKS: usize = 512;
pub const READ_CHUNK_BYTES: usize = 8192;
pub const MAX_PROC_SCAN_ENTRIES: usize = 65_536;
pub const MAX_PROC_STAT_BYTES: usize = 4096;
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
pub const INPUT_QUEUE_CHUNKS: usize = 8;
pub const MAX_BATCH_CHUNKS: usize = 32;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;
pub const MAX_DIMENSION: u16 = 1000;
pub const TERMINATION_GRACE: Duration = Duration::from_millis(250);
pub const REAP_GRACE: Duration = Duration::from_secs(1);
pub const DRAIN_GRACE: Duration = Duration::from_millis(250);
pub const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ExecutionId(u64);
impl ExecutionId {
    pub fn get(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PtySessionId(pub(super) ExecutionId);
impl PtySessionId {
    pub fn get(self) -> u64 {
        self.0.get()
    }
}
#[derive(Default)]
pub(super) struct IdAllocator(pub(super) AtomicU64);
impl IdAllocator {
    pub fn next(&self) -> Result<ExecutionId, ExecutionError> {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map(|n| ExecutionId(n + 1))
            .map_err(|_| ExecutionError::IdExhausted)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionOrigin {
    Human,
    SpecialistAgent(TraceId),
    Worker(TraceId),
    CognitiveProvider(TraceId),
    #[cfg(test)]
    TestFixture,
}

/// Opaque, non-deserializable capability. No public constructor; only the native
/// execution boundary can mint HumanLocal. Neither PID nor declared origin mints it.
/// LR-9C human registry uses this natively, never forwards an IPC authority enum.
/// Serialized IDs, declared origins and trace correlations cannot mint authority.
/// ```compile_fail
/// use assistente_3d_lib::execution::ExecutionAuthority;
/// let _: ExecutionAuthority = serde_json::from_str(r#"{"origin":"Human","taskId":1,"pid":123,"correlation":"agent-call-1"}"#).unwrap();
/// ```
/// ```compile_fail
/// use assistente_3d_lib::execution::ExecutionAuthority;
/// let _ = ExecutionAuthority::human_local();
/// ```
pub struct ExecutionAuthority(AuthorityKind);
enum AuthorityKind {
    HumanLocal,
    #[cfg(test)]
    Fixture {
        program: PathBuf,
    },
}
impl ExecutionAuthority {
    pub(super) fn human_local() -> Self {
        Self(AuthorityKind::HumanLocal)
    }
    #[cfg(test)]
    pub(super) fn fixture(program: &Path) -> Self {
        Self(AuthorityKind::Fixture {
            program: program.into(),
        })
    }
    pub(super) fn authorize(
        &self,
        request: &ExecutionRequest,
        _mode: ExecutionMode,
    ) -> Result<(), ExecutionError> {
        match (&self.0, &request.origin) {
            (AuthorityKind::HumanLocal, ExecutionOrigin::Human) => Ok(()),
            #[cfg(test)]
            (AuthorityKind::Fixture { program }, ExecutionOrigin::TestFixture)
                if _mode == ExecutionMode::Structured
                    && request.program == *program
                    && matches!(request.environment, EnvironmentPolicy::Controlled(_)) =>
            {
                Ok(())
            }
            _ => Err(ExecutionError::AuthorityDenied),
        }
    }
    pub(super) fn authorize_input(&self, origin: &ExecutionOrigin) -> Result<(), ExecutionError> {
        match (&self.0, origin) {
            (AuthorityKind::HumanLocal, ExecutionOrigin::Human) => Ok(()),
            _ => Err(ExecutionError::AuthorityDenied),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionMode {
    Structured,
    Pty,
}
#[derive(Clone, Debug)]
pub enum EnvironmentPolicy {
    /// Only explicit Human + HumanLocal may inherit the user's environment.
    HumanInherited,
    /// env_clear first. No implicit PATH, HOME, tokens or credentials.
    Controlled(Vec<(OsString, OsString)>),
}
#[derive(Clone, Copy, Debug)]
pub struct CapturePolicy {
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}
impl Default for CapturePolicy {
    fn default() -> Self {
        Self {
            stdout_bytes: MAX_CAPTURE_BYTES,
            stderr_bytes: MAX_CAPTURE_BYTES,
        }
    }
}

/// Canonical cwd confinement at admission; NOT a filesystem sandbox. Executables
/// can access external files, network and fork. Renames after validation are not
/// contained; future agent integrations require a real sandbox/policy boundary.
#[derive(Clone, Debug)]
pub struct WorkspaceScope {
    roots: Vec<PathBuf>,
}
impl WorkspaceScope {
    pub fn confined(roots: &[PathBuf]) -> Result<Self, ExecutionError> {
        if roots.is_empty() || roots.len() > MAX_SCOPE_ROOTS {
            return Err(ExecutionError::InvalidRequest);
        }
        let roots = roots
            .iter()
            .map(|p| canonical_directory(p))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { roots })
    }
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
    pub(super) fn validate(&self, cwd: &Path) -> Result<PathBuf, ExecutionError> {
        let cwd = canonical_directory(cwd)?;
        // Re-canonicalize roots as well: fail closed if a root was replaced by a symlink.
        for root in &self.roots {
            if canonical_directory(root)? != *root {
                return Err(ExecutionError::CwdOutsideScope);
            }
        }
        if self.roots.iter().any(|root| cwd.starts_with(root)) {
            Ok(cwd)
        } else {
            Err(ExecutionError::CwdOutsideScope)
        }
    }
}
#[derive(Clone, Debug)]
pub struct ExecutionRequest {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub origin: ExecutionOrigin,
    pub task_id: Option<TaskId>,
    pub correlation: Option<TraceId>,
    pub workspace: Option<WorkspaceScope>,
    pub environment: EnvironmentPolicy,
    pub timeout: Duration,
    pub capture: CapturePolicy,
}
impl ExecutionRequest {
    pub(super) fn validate(&self) -> Result<Self, ExecutionError> {
        validate_path(&self.program)?;
        if self.args.len() > MAX_ARGS
            || self.args.iter().any(|a| a.as_encoded_bytes().contains(&0))
            || self
                .args
                .iter()
                .try_fold(0usize, |sum, a| sum.checked_add(a.as_encoded_bytes().len()))
                .unwrap_or(usize::MAX)
                > MAX_ARG_BYTES
            || self.timeout.is_zero()
            || self.timeout > MAX_TIMEOUT
            || self.capture.stdout_bytes > MAX_CAPTURE_BYTES
            || self.capture.stderr_bytes > MAX_CAPTURE_BYTES
            || self
                .task_id
                .is_some_and(|t| t.0 == 0 || t.0 > 9_007_199_254_740_991)
        {
            return Err(ExecutionError::InvalidRequest);
        }
        if let EnvironmentPolicy::Controlled(env) = &self.environment {
            if env.len() > MAX_ENV_ENTRIES {
                return Err(ExecutionError::InvalidRequest);
            }
            let mut bytes = 0usize;
            for (key, value) in env {
                let k = key.as_encoded_bytes();
                let v = value.as_encoded_bytes();
                if k.is_empty() || k.contains(&0) || k.contains(&b'=') || v.contains(&0) {
                    return Err(ExecutionError::InvalidRequest);
                }
                bytes = bytes.saturating_add(k.len()).saturating_add(v.len());
            }
            if bytes > MAX_ENV_BYTES {
                return Err(ExecutionError::InvalidRequest);
            }
        } else if self.origin != ExecutionOrigin::Human {
            return Err(ExecutionError::AuthorityDenied);
        }
        // Clone only after aggregate budgets have been checked.
        let cwd = match &self.workspace {
            Some(scope) => scope.validate(&self.cwd)?,
            None => canonical_directory(&self.cwd)?,
        };
        let mut validated = self.clone();
        validated.cwd = cwd;
        Ok(validated)
    }
}
fn validate_path(path: &Path) -> Result<(), ExecutionError> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.is_empty() || bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) {
        Err(ExecutionError::InvalidRequest)
    } else {
        Ok(())
    }
}
fn canonical_directory(path: &Path) -> Result<PathBuf, ExecutionError> {
    validate_path(path)?;
    let resolved = path
        .canonicalize()
        .map_err(|_| ExecutionError::InvalidCwd)?;
    validate_path(&resolved)?;
    if !resolved.is_dir() {
        return Err(ExecutionError::InvalidCwd);
    }
    Ok(resolved)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionState {
    Starting,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionError {
    InvalidRequest,
    InvalidCwd,
    CwdOutsideScope,
    AuthorityDenied,
    IdExhausted,
    ActiveLimit,
    SessionLimit,
    ShuttingDown,
    InvalidDimensions,
    InputTooLarge,
    InputQueueFull,
    NotRunning,
    FutureCursor,
    InvalidBatch,
    Io,
    WorkerSpawn,
}
#[derive(Clone, Debug, Default)]
pub struct CapturedOutput {
    pub bytes: Vec<u8>, // deterministic prefix
    pub total_bytes: u64,
    pub dropped_bytes: u64,
    pub truncated: bool,
    pub read_error: bool,
    /// EOF was not observed before bounded post-exit drain expired.
    pub incomplete: bool,
}
impl CapturedOutput {
    pub fn captured_bytes(&self) -> usize {
        self.bytes.len()
    }
    pub(super) fn append(&mut self, bytes: &[u8], cap: usize) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        let take = bytes.len().min(cap.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..take]);
        self.dropped_bytes = self
            .dropped_bytes
            .saturating_add((bytes.len() - take) as u64);
        self.truncated |= take < bytes.len();
    }
}
#[derive(Clone, Debug)]
pub struct ExecutionResult {
    pub id: ExecutionId,
    pub mode: ExecutionMode,
    pub request: ExecutionRequest,
    pub state: ExecutionState,
    pub created_at: SystemTime,
    pub finished_at: SystemTime,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub spawn_failed: bool,
    pub runtime_error: bool,
    pub reaped: bool,
    /// Kernel did not report exit within REAP_GRACE after KILL. The counted
    /// supervisor retains the child and slot until it can reap; Quit is bounded.
    pub cleanup_pending: bool,
    pub process_id: Option<u32>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
}
#[derive(Clone, Copy, Debug)]
pub struct PtyDimensions {
    pub rows: u16,
    pub cols: u16,
}
impl PtyDimensions {
    pub(super) fn validate(self) -> Result<Self, ExecutionError> {
        if self.rows == 0
            || self.cols == 0
            || self.rows > MAX_DIMENSION
            || self.cols > MAX_DIMENSION
        {
            Err(ExecutionError::InvalidDimensions)
        } else {
            Ok(self)
        }
    }
}
