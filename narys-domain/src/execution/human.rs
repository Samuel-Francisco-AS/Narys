//! Narrow native product boundary. No caller-selected program, argv, cwd, env,
//! PID, origin or authority. Session lifetime belongs to this process registry.
use super::*;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

pub struct HumanTerminal {
    broker: Arc<ExecutionBroker>,
    session: Mutex<Option<PtySession>>,
}
#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub session_id: String,
    pub state: &'static str,
    pub shell: String,
    pub starting_directory: String,
    pub rows: u16,
    pub cols: u16,
    pub exit_code: Option<i32>,
    pub reaped: bool,
}
pub fn state_name(state: ExecutionState) -> &'static str {
    match state {
        ExecutionState::Starting => "starting",
        ExecutionState::Running => "running",
        ExecutionState::Completed => "completed",
        ExecutionState::Failed => "failed",
        ExecutionState::Cancelled => "cancelled",
        ExecutionState::TimedOut => "timed_out",
    }
}
pub fn starting_directory() -> PathBuf {
    // HOME is native user configuration, never a WebView argument. Canonical
    // existing directory only. Root is a stable safe fallback (not a sandbox).
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .and_then(|p| p.canonicalize().ok())
        .filter(|p| p.is_dir() && p.as_os_str().as_encoded_bytes().len() <= MAX_PATH_BYTES)
        .unwrap_or_else(|| PathBuf::from("/"))
}
impl HumanTerminal {
    pub fn process_wide() -> Arc<Self> {
        static HUB: OnceLock<Arc<HumanTerminal>> = OnceLock::new();
        HUB.get_or_init(|| Arc::new(Self::new(ExecutionBroker::process_wide())))
            .clone()
    }
    pub fn new(broker: Arc<ExecutionBroker>) -> Self {
        Self {
            broker,
            session: Mutex::new(None),
        }
    }
    pub fn status(&self) -> Option<SessionDto> {
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(Self::dto)
    }
    pub fn open(&self) -> Result<SessionDto, ExecutionError> {
        let mut current = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(session) = current.as_ref() {
            if session.result().is_none() || session.result().is_some_and(|r| r.cleanup_pending) {
                return Ok(Self::dto(session));
            }
        }
        // Replacing a terminal reaped handle prunes its bounded output history.
        let session = self.broker.open_human_shell(
            &starting_directory(),
            PtyDimensions { rows: 24, cols: 80 },
            &ExecutionAuthority::human_local(),
        )?;
        let dto = Self::dto(&session);
        *current = Some(session);
        Ok(dto)
    }
    pub fn find(&self, id: &str) -> Result<PtySession, ExecutionError> {
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|s| s.id().get().to_string() == id)
            .cloned()
            .ok_or(ExecutionError::NotRunning)
    }
    pub fn input(&self, id: &str, bytes: &[u8]) -> Result<(), ExecutionError> {
        self.find(id)?.send_input(
            &ExecutionOrigin::Human,
            &ExecutionAuthority::human_local(),
            bytes,
        )
    }
    pub fn resize(&self, id: &str, rows: u16, cols: u16) -> Result<(), ExecutionError> {
        self.find(id)?.resize(
            PtyDimensions { rows, cols },
            &ExecutionAuthority::human_local(),
        )
    }
    pub fn close(&self, id: &str) -> Result<bool, ExecutionError> {
        self.find(id)?.close(&ExecutionAuthority::human_local())
    }
    pub fn dto(s: &PtySession) -> SessionDto {
        let b = s
            .replay(0, 1, READ_CHUNK_BYTES)
            .expect("fixed valid snapshot limits");
        SessionDto {
            session_id: s.id().get().to_string(),
            state: state_name(s.state()),
            shell: s
                .request()
                .program
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            starting_directory: s.request().cwd.to_string_lossy().into(),
            rows: b.dimensions.rows,
            cols: b.dimensions.cols,
            exit_code: s.result().and_then(|r| r.exit_code),
            reaped: s.result().is_some_and(|r| r.reaped),
        }
    }
}
