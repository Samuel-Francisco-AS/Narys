//! Official SDK transport behind a private Linux ownership wrapper. No prompts.
use super::{sdk_policy::*, startup::StartupJournal, supervisor::*};
use crate::{agents::lifecycle::AgentLifecycleOperation, policy};
use github_copilot_sdk::{CliProgram, Client, ClientOptions, LogLevel, Transport};
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub struct SdkRuntimeFactory {
    cli: PathBuf,
    root: PathBuf,
    database: crate::persistence::database::Database,
    #[cfg(test)]
    fault: Option<StartupHook>,
}
struct SdkRuntime {
    client: Client,
    directory: PathBuf,
    process: Option<OwnedFd>,
    database: crate::persistence::database::Database,
}
fn alive(fd: &OwnedFd) -> bool {
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, 0) == 0 }
}
fn record_alive(directory: &Path) -> Result<bool, &'static str> {
    for (file, fields) in [
        (
            "primary-owner.json",
            vec![("primary_pid", "primary_start_ticks")],
        ),
        (
            "owner.json",
            vec![
                ("guardian_pid", "guardian_start_ticks"),
                ("runtime_pid", "runtime_start_ticks"),
            ],
        ),
    ] {
        let path = directory.join(file);
        if file == "owner.json" && !path.exists() {
            continue;
        }
        crate::policy::private_file(&path).map_err(|_| "runtime_identity_not_verified")?;
        let value: Value =
            serde_json::from_slice(&fs::read(path).map_err(|_| "runtime_identity_not_verified")?)
                .map_err(|_| "runtime_identity_not_verified")?;
        for (pid, ticks) in fields {
            let pid = value[pid]
                .as_u64()
                .filter(|p| *p > 0 && *p <= u32::MAX as u64)
                .ok_or("runtime_identity_not_verified")?;
            let ticks = value[ticks]
                .as_u64()
                .ok_or("runtime_identity_not_verified")?;
            match fs::read_to_string(format!("/proc/{pid}/stat")) {
                Ok(stat) => {
                    let actual = stat
                        .rsplit_once(')')
                        .and_then(|(_, s)| s.split_whitespace().nth(19))
                        .and_then(|s| s.parse::<u64>().ok())
                        .ok_or("runtime_identity_not_verified")?;
                    if actual == ticks {
                        return Ok(true);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("runtime_identity_not_verified"),
            }
        }
    }
    Ok(false)
}
async fn cleanup(directory: &Path) -> Result<(), &'static str> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    loop {
        if let Ok(bytes) = fs::read(directory.join("cleanup.json")) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if v["cleanup_complete"] == true && v["kernel_children_exhausted"] == true {
                    if directory.join("primary-cleanup.json").exists() {
                        return Err("runtime_cleanup_incomplete");
                    }
                    if !record_alive(directory)? {
                        return Ok(());
                    }
                }
                if v["cleanup_complete"] != true {
                    return Err("runtime_cleanup_incomplete");
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("runtime_cleanup_incomplete");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
impl RuntimeFactory for SdkRuntimeFactory {
    fn start(&self) -> StartupFuture<'_> {
        Box::pin(async move {
            self.check_previous()?;
            crate::worker::validate_cli(&self.cli).map_err(StartupFailure::no_process)?;
            self.start_owned().await
        })
    }
}
impl SdkRuntimeFactory {
    pub fn new(
        cli: PathBuf,
        root: PathBuf,
        database: crate::persistence::database::Database,
    ) -> Self {
        Self {
            cli,
            root,
            database,
            #[cfg(test)]
            fault: None,
        }
    }
    fn check_previous(&self) -> Result<(), StartupFailure> {
        if super::startup::unresolved(&self.database).map_err(StartupFailure::persistence)? {
            return Err(StartupFailure::uncertain(
                "runtime_previous_ownership_unresolved",
            ));
        }
        super::startup::check_artifacts(&self.database, &self.root)
            .map_err(StartupFailure::persistence)
    }
    #[cfg(test)]
    fn inject(&self, point: StartupPoint, path: &Path) -> Result<(), &'static str> {
        self.fault
            .as_ref()
            .map_or(Ok(()), |hook| hook(point, path, &self.database))
    }
    async fn start_owned(&self) -> Result<Arc<dyn ManagedRuntime>, StartupFailure> {
        self.check_previous()?;
        crate::server::mkdir(&self.root).map_err(StartupFailure::no_process)?;
        prune(&self.database, &self.root).map_err(StartupFailure::persistence)?;
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(|_| StartupFailure::no_process("boot_identity_unavailable"))?;
        // RAII removes only this not-yet-registered directory if begin fails.
        // No launch is possible without the write-ahead ownership record.
        let artifact = tempfile::Builder::new()
            .prefix("runtime-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&self.root)
            .map_err(|_| StartupFailure::no_process("runtime_directory_failed"))?;
        #[cfg(test)]
        self.inject(StartupPoint::PersistPreparing, artifact.path())
            .map_err(StartupFailure::persistence)?;
        let mut journal =
            StartupJournal::begin(self.database.clone(), artifact.path(), boot.trim())?;
        let dir = artifact.keep();
        // All preparation errors after the INSERT use the same terminal path.
        let prepared = (|| -> Result<ClientOptions, &'static str> {
            #[cfg(test)]
            self.inject(StartupPoint::Workspace, &dir)?;
            crate::server::mkdir(&dir.join("workspace"))?;
            #[cfg(test)]
            self.inject(StartupPoint::Logs, &dir)?;
            crate::server::mkdir(&dir.join("logs"))?;
            let sdk_state = self
                .root
                .parent()
                .ok_or("runtime_path_invalid")?
                .join("sdk-state");
            #[cfg(test)]
            self.inject(StartupPoint::SdkState, &dir)?;
            crate::server::mkdir(&sdk_state)?;
            let options = ClientOptions::new()
                .with_program(CliProgram::Path(PathBuf::from("/usr/bin/python3")))
                .with_prefix_args([
                    "-I".into(),
                    "-c".into(),
                    include_str!("../../ops/agent_runtime.py").into(),
                    self.cli.as_os_str().to_owned(),
                    dir.as_os_str().to_owned(),
                ])
                .with_transport(Transport::Stdio)
                .with_cwd(dir.join("workspace"))
                .with_base_directory(&sdk_state)
                .with_use_logged_in_user(true)
                .with_log_level(LogLevel::None)
                .with_env([
                    ("PATH", "/usr/bin:/bin"),
                    ("LANG", "C.UTF-8"),
                    ("RES_OPTIONS", "no-aaaa"),
                    ("COPILOT_AUTO_UPDATE", "false"),
                ])
                .with_env_remove(std::env::vars_os().map(|(key, _)| key).filter(|key| {
                    ![
                        "HOME",
                        "PATH",
                        "LANG",
                        "XDG_RUNTIME_DIR",
                        "DBUS_SESSION_BUS_ADDRESS",
                        "RES_OPTIONS",
                        "COPILOT_AUTO_UPDATE",
                    ]
                    .iter()
                    .any(|allowed| key == allowed)
                }))
                .with_extra_args([
                    "--disable-builtin-mcps",
                    "--no-custom-instructions",
                    "--log-dir",
                    dir.join("logs").to_str().ok_or("runtime_path_invalid")?,
                ]);
            Ok(options)
        })();
        let options = match prepared {
            Ok(options) => options,
            Err(code) => return Err(journal.fail_before_launch(code, false)),
        };
        let intent = (|| {
            #[cfg(test)]
            self.inject(StartupPoint::PersistLaunchIntent, &dir)?;
            journal.launch_intent()
        })();
        if let Err(code) = intent {
            return Err(journal.fail_before_launch(code, true));
        }
        #[cfg(test)]
        if let Err(code) = self.inject(StartupPoint::BeforeLaunch, &dir) {
            return Err(journal.fail_before_launch(code, false));
        }
        // This invocation is the single effects boundary. Every exit beyond it
        // requires a positive kernel cleanup receipt and durable terminal state.
        let client = match bounded(Client::start(options)).await {
            Ok(client) => client,
            Err(code) => {
                let cleaned = cleanup(&dir).await;
                let owner = cleanup_ownership_json(&dir)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());
                let proof = fs::read(dir.join("cleanup.json"))
                    .ok()
                    .and_then(|s| serde_json::from_slice(&s).ok());
                return Err(journal.fail_after_launch(code, cleaned, false, owner, proof, None));
            }
        };
        let fd = client
            .pid()
            .map(|pid| unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) as i32 });
        let process = fd
            .filter(|fd| *fd >= 0)
            .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) });
        let runtime = Arc::new(SdkRuntime {
            client,
            directory: dir,
            process,
            database: self.database.clone(),
        });
        let validation = async {
            #[cfg(test)]
            self.inject(StartupPoint::AfterLaunch, &runtime.directory)?;
            if runtime.process.is_none() {
                return Err("runtime_identity_unavailable");
            }
            let status = bounded(runtime.client.get_status()).await?;
            if status.version != "1.0.95" || status.protocol_version != 3 {
                return Err("runtime_protocol_mismatch");
            }
            Ok(())
        }
        .await;
        if let Err(code) = validation {
            return Err(self
                .finish_launched_failure(&mut journal, &runtime, code, false)
                .await);
        }
        let ready = (|| {
            #[cfg(test)]
            self.inject(StartupPoint::PersistReady, &runtime.directory)?;
            let owner = ownership_json(&runtime.directory)?;
            let owner: Value =
                serde_json::from_str(&owner).map_err(|_| "runtime_identity_unavailable")?;
            if owner["primary_pid"].as_u64() != runtime.client.pid().map(u64::from) {
                return Err("runtime_identity_unavailable");
            }
            journal.ready(owner)
        })();
        if let Err(code) = ready {
            return Err(self
                .finish_launched_failure(&mut journal, &runtime, code, true)
                .await);
        }
        Ok(runtime as Arc<dyn ManagedRuntime>)
    }
    async fn finish_launched_failure(
        &self,
        journal: &mut StartupJournal,
        runtime: &SdkRuntime,
        code: &'static str,
        persistence_fault: bool,
    ) -> StartupFailure {
        let stopped = runtime.shutdown_owned().await;
        let cleaned = cleanup(&runtime.directory).await;
        let owner = cleanup_ownership_json(&runtime.directory)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        let proof = fs::read(runtime.directory.join("cleanup.json"))
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok());
        journal.fail_after_launch(
            code,
            cleaned,
            persistence_fault
                || stopped
                    .as_ref()
                    .err()
                    .is_some_and(|f| f.persistence_uncertain),
            owner,
            proof,
            stopped.err().map(|f| f.code),
        )
    }
}
impl ManagedRuntime for SdkRuntime {
    fn force_stop(&self) {
        self.client.force_stop();
    }
    fn ownership_ref(&self) -> Option<String> {
        self.directory
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
    }
    fn process_id(&self) -> Option<u32> {
        self.client.pid()
    }
    fn healthy(&self) -> bool {
        self.process.as_ref().is_some_and(alive)
    }
    fn session<'a>(
        &'a self,
        invocation: &'a SessionInvocation,
        cancel: &'a Cancellation,
        progress: Arc<dyn Fn(&'static str) + Send + Sync>,
    ) -> RuntimeFuture<'a, SessionReceipt> {
        Box::pin(async move {
            policy::private_directory(&invocation.directory)?;
            for n in ["workspace", "session-state"] {
                crate::server::mkdir(&invocation.directory.join(n))?;
            }
            let workspace = invocation.directory.join("workspace");
            // Configuration paths belong to Core private storage, never /tmp.
            let prepared = match invocation.operation {
                AgentLifecycleOperation::Create => {
                    if invocation.provider_session_id.is_some() {
                        return Err("invalid_session_invocation");
                    }
                    let mut cfg = session_config(&workspace);
                    cfg.config_directory = Some(invocation.directory.join("session-state"));
                    cfg.enable_session_store = Some(false);
                    cfg.infinite_sessions = Some(
                        github_copilot_sdk::types::InfiniteSessionConfig::new().with_enabled(false),
                    );
                    self.client
                        .prepare_session(cfg)
                        .map_err(|e| error_code(&e))?
                }
                AgentLifecycleOperation::Resume => {
                    let id = invocation
                        .provider_session_id
                        .clone()
                        .ok_or("session_not_resumable")?;
                    let mut cfg = resume_config(id.into(), &workspace);
                    cfg.config_directory = Some(invocation.directory.join("session-state"));
                    cfg.enable_session_store = Some(false);
                    cfg.infinite_sessions = Some(
                        github_copilot_sdk::types::InfiniteSessionConfig::new().with_enabled(false),
                    );
                    self.client
                        .prepare_resume_session(cfg)
                        .map_err(|e| error_code(&e))?
                }
            };
            // Subscribe before start to avoid the SDK's unbounded bootstrap path.
            let mut events = prepared.subscribe();
            if cancel.is_cancelled() {
                return Err("cancelled");
            }
            progress("session_starting");
            // Do not abandon session.create at cancellation: resolve it within a
            // deadline, then abort/detach the created session even if cancelled.
            let session = bounded(prepared.start()).await?;
            let provider_id = session.id().to_string();
            let mut history_anchor = None;
            let mut failure = None;
            if invocation
                .provider_session_id
                .as_ref()
                .is_some_and(|expected| expected != &provider_id)
            {
                failure = Some("session_id_mismatch");
            }
            if session
                .workspace_path()
                .is_some_and(|p| !p.starts_with(&invocation.directory))
            {
                failure = Some("session_storage_outside_private_root");
            }
            if !cancel.is_cancelled() && failure.is_none() {
                match bounded(session.get_events()).await {
                    Ok(history) => {
                        if let Some(first) = history
                            .first()
                            .filter(|event| event.event_type == "session.start")
                        {
                            use sha2::{Digest, Sha256};
                            history_anchor = Some(format!(
                                "{:x}",
                                Sha256::digest(format!("{provider_id}:{}", first.id).as_bytes())
                            ));
                        }
                        if invocation.operation == AgentLifecycleOperation::Resume
                            && (history_anchor.is_none()
                                || history_anchor != invocation.expected_history_anchor)
                        {
                            failure = Some("session_history_not_verified");
                        }
                    }
                    Err(_) => {
                        failure = Some("session_history_not_verified");
                    }
                }
            }
            progress("session_ready");
            let mut gaps = 0u64;
            let mut observed = 0usize;
            let mut observe = |event: github_copilot_sdk::SessionEvent| {
                observed = observed.saturating_add(1);
                match event.event_type.to_string().as_str() {
                    "tool.execution_start" => failure = Some("unexpected_tool_execution"),
                    "session.error" => failure = Some("session_error_observed"),
                    "session.start" | "session.resume" if observed <= 32 => {
                        progress("session_progress")
                    }
                    _ => {}
                }
                if observed > 32 {
                    gaps = gaps.saturating_add(1);
                }
            };
            tokio::task::yield_now().await;
            for _ in 0..128 {
                match tokio::time::timeout(Duration::ZERO, events.recv()).await {
                    Ok(Ok(e)) => observe(e),
                    _ => break,
                }
            }
            drop(observe);
            // Resume is accepted only when SDK reports that exact registered ID.
            // No transcript recovery, create fallback, send, retries or replay.
            if cancel.is_cancelled() || failure.is_some() {
                progress("session_cancelling");
                if bounded(session.abort()).await.is_err() {
                    failure = Some("session_abort_failed");
                }
            }
            let detached = {
                let detach = bounded(session.disconnect());
                tokio::pin!(detach);
                let detached = loop {
                    tokio::select! {
                        result=&mut detach=>break result,
                        event=events.recv()=>match event {
                            Ok(e)=>{
                                observed=observed.saturating_add(1);
                                if observed>32 {gaps=gaps.saturating_add(1);}
                                match e.event_type.to_string().as_str() {
                                    "tool.execution_start"=>failure=Some("unexpected_tool_execution"),
                                    "session.error"=>failure=Some("session_error_observed"),
                                    _=>{},
                                }
                            },
                            Err(e)=>{
                                gaps=gaps.saturating_add(match e.kind() {github_copilot_sdk::subscription::RecvErrorKind::Lagged(l)=>l.skipped(),_=>1});
                                // Closed observers are passive; finish the RPC once.
                                if matches!(e.kind(),github_copilot_sdk::subscription::RecvErrorKind::Closed) {break detach.as_mut().await;}
                            }
                        }
                    }
                };
                // The detach response may race already-queued terminal facts.
                for _ in 0..128 {
                    match tokio::time::timeout(Duration::ZERO, events.recv()).await {
                        Ok(Ok(e)) => {
                            observed = observed.saturating_add(1);
                            if observed > 32 {
                                gaps = gaps.saturating_add(1);
                            }
                            match e.event_type.to_string().as_str() {
                                "tool.execution_start" => {
                                    failure = Some("unexpected_tool_execution")
                                }
                                "session.error" => failure = Some("session_error_observed"),
                                _ => {}
                            }
                        }
                        Ok(Err(e)) => {
                            gaps = gaps.saturating_add(match e.kind() {
                                github_copilot_sdk::subscription::RecvErrorKind::Lagged(l) => {
                                    l.skipped()
                                }
                                _ => 1,
                            });
                            break;
                        }
                        Err(_) => break,
                    }
                }
                detached
            };
            drop(events);
            drop(session);
            detached.map_err(|_| "session_detach_failed")?;
            progress("session_detached");
            if let Some(code) = failure {
                return Err(code);
            }
            if cancel.is_cancelled() {
                return Err("cancelled");
            }
            Ok(SessionReceipt {
                provider_session_id: provider_id,
                observation_gaps: gaps,
                history_anchor,
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async move { self.shutdown_owned().await.map_err(|failure| failure.code) })
    }
}
struct StopFailure {
    code: &'static str,
    persistence_uncertain: bool,
}
impl SdkRuntime {
    async fn shutdown_owned(&self) -> Result<(), StopFailure> {
        let stopping = self.database.open().map_err(|e| e.code()).and_then(|conn| {
            conn.execute(
                "UPDATE agent_runtime_owners SET state='stopping' WHERE runtime_ref=?1",
                [self.ownership_ref()],
            )
            .map_err(|_| "runtime_ownership_persist_failed")
            .and_then(|n| {
                if n == 1 {
                    Ok(())
                } else {
                    Err("runtime_ownership_missing")
                }
            })
        });
        // A write failure never skips the independent process cleanup path.
        let graceful = matches!(
            tokio::time::timeout(Duration::from_secs(5), self.client.stop()).await,
            Ok(Ok(()))
        );
        if !graceful {
            self.client.force_stop();
        }
        let verified = cleanup(&self.directory).await.is_ok();
        persist_cleanup(
            &self.database,
            &self.directory,
            verified,
            if graceful {
                None
            } else {
                Some("sdk_shutdown_recovered")
            },
        )
        .map_err(|code| StopFailure {
            code,
            persistence_uncertain: true,
        })?;
        if let Err(code) = stopping {
            return Err(StopFailure {
                code,
                persistence_uncertain: true,
            });
        }
        if !verified {
            return Err(StopFailure {
                code: "runtime_cleanup_incomplete",
                persistence_uncertain: false,
            });
        }
        if !graceful {
            return Err(StopFailure {
                code: "sdk_shutdown_recovered",
                persistence_uncertain: false,
            });
        }
        Ok(())
    }
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupPoint {
    PersistPreparing,
    Workspace,
    Logs,
    SdkState,
    PersistLaunchIntent,
    BeforeLaunch,
    AfterLaunch,
    PersistReady,
}
#[cfg(test)]
type StartupHook = Arc<
    dyn Fn(StartupPoint, &Path, &crate::persistence::database::Database) -> Result<(), &'static str>
        + Send
        + Sync,
>;

fn persist_cleanup(
    db: &crate::persistence::database::Database,
    directory: &Path,
    verified: bool,
    error: Option<&str>,
) -> Result<(), &'static str> {
    let reference = directory
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("runtime_path_invalid")?;
    let mut conn = db.open().map_err(|e| e.code())?;
    let tx = conn
        .transaction()
        .map_err(|_| "runtime_ownership_persist_failed")?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT owner_json FROM agent_runtime_owners WHERE runtime_ref=?1",
            [reference],
            |r| r.get(0),
        )
        .map_err(|_| "runtime_ownership_missing")?;
    let mut owner: Value = match previous {
        Some(json) => serde_json::from_str(&json).map_err(|_| "runtime_ownership_invalid")?,
        None => serde_json::json!({}),
    };
    if !owner.is_object() {
        return Err("runtime_ownership_invalid");
    }
    if let Ok(actual) = cleanup_ownership_json(directory)
        .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|_| "runtime_ownership_invalid"))
    {
        if let Some(fields) = actual.as_object() {
            for (key, value) in fields {
                owner[key] = value.clone();
            }
        }
    }
    let proof = fs::read_to_string(directory.join("cleanup.json")).ok();
    if tx.execute("UPDATE agent_runtime_owners SET state=?2,cleanup_verified=?3,error_code=coalesce(error_code,?4),owner_json=?5,cleanup_json=?6,finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE runtime_ref=?1",
        rusqlite::params![reference,if verified {"stopped"}else{"faulted"},verified,error,owner.to_string(),proof]).map_err(|_|"runtime_ownership_persist_failed")? != 1 {
        return Err("runtime_ownership_missing");
    }
    if verified {
        tx.execute(
            "UPDATE agent_runs SET cleanup_verified=1 WHERE runtime_ref=?1",
            [reference],
        )
        .map_err(|_| "runtime_ownership_persist_failed")?;
    }
    tx.commit().map_err(|_| "runtime_ownership_persist_failed")
}

fn prune(db: &crate::persistence::database::Database, root: &Path) -> Result<(), &'static str> {
    let conn = db.open().map_err(|e| e.code())?;
    let mut q=conn.prepare("SELECT private_directory FROM agent_runtime_owners WHERE cleanup_verified=1 ORDER BY created_at DESC,runtime_ref DESC LIMIT 64 OFFSET 32").map_err(|_|"runtime_ownership_read_failed")?;
    let rows = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|_| "runtime_ownership_read_failed")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "runtime_ownership_read_failed")?;
    for path in rows {
        let path = Path::new(&path);
        if path.parent() != Some(root) {
            return Err("runtime_directory_mismatch");
        }
        if path.exists() {
            crate::policy::private_directory(path)?;
            fs::remove_dir_all(path).map_err(|_| "runtime_artifact_cleanup_failed")?;
        }
    }
    Ok(())
}
/// Reentry never signals an external PID or starts the SDK. Same-boot missing
/// ownership proof remains faulted; a different kernel boot proves old PIDs gone.
pub async fn recover(db: &crate::persistence::database::Database) -> Result<(), &'static str> {
    let root = db
        .directory()
        .parent()
        .ok_or("agent_state_directory_missing")?
        .join("copilot/runtimes");
    super::startup::check_artifacts(db, &root)?;
    // Validate positive terminal certificates as well as pending rows. A bit
    // saying cleanup_verified is not sufficient when its evidence is missing.
    super::startup::unresolved(db)?;
    let rows = {
        let conn = db.open().map_err(|e| e.code())?;
        let mut q = conn.prepare("SELECT runtime_ref,private_directory,boot_id,owner_json FROM agent_runtime_owners WHERE cleanup_verified=0 OR state!='stopped' ORDER BY created_at LIMIT 65")
            .map_err(|_|"runtime_ownership_read_failed")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|_| "runtime_ownership_read_failed")?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "runtime_ownership_read_failed")?;
        rows
    };
    if rows.len() > 64 {
        return Err("runtime_recovery_limit");
    }
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| "boot_identity_unavailable")?;
    let mut unresolved = false;
    for (reference, directory, old_boot, owner) in rows {
        let path = Path::new(&directory);
        if path.parent() != Some(root.as_path())
            || path.file_name().and_then(|s| s.to_str()) != Some(reference.as_str())
            || !reference.starts_with("runtime-")
        {
            return Err("runtime_directory_mismatch");
        }
        if path.exists() {
            crate::policy::private_directory(path)?;
        }
        if !valid_boot_id(&old_boot) {
            return Err("runtime_boot_identity_invalid");
        }
        let owner = match owner {
            Some(s) => {
                serde_json::from_str::<Value>(&s).map_err(|_| "runtime_ownership_invalid")?
            }
            None => serde_json::json!({}), // Legacy rows have no prelaunch proof.
        };
        if !owner.is_object() {
            return Err("runtime_ownership_invalid");
        }
        if let Some(journal) = owner.get("startup") {
            if journal["version"] != 1
                || journal["runtime_ref"] != reference
                || journal["boot_id"] != old_boot
            {
                return Err("runtime_ownership_invalid");
            }
        }
        if old_boot != boot.trim() {
            persist_recovery_proof(
                db,
                &reference,
                &owner,
                serde_json::json!({"evidence":"previous_kernel_boot","recorded_boot_id":old_boot,"current_boot_id":boot.trim()}),
                "previous_kernel_boot_no_replay",
            )?;
        } else if super::startup::prelaunch_proven(&owner, &reference, &old_boot, path) {
            persist_recovery_proof(
                db,
                &reference,
                &owner,
                serde_json::json!({"evidence":"write_ahead_prelaunch_boundary","no_process_launched":true}),
                "prelaunch_recovered_no_process",
            )?;
        } else {
            let verified = cleanup(path).await.is_ok();
            persist_cleanup(
                db,
                path,
                verified,
                Some(if verified {
                    "owner_recovered_no_replay"
                } else {
                    "runtime_cleanup_incomplete"
                }),
            )?;
            unresolved |= !verified;
        }
    }
    if unresolved {
        Err("runtime_cleanup_incomplete")
    } else {
        Ok(())
    }
}
pub(super) fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
fn persist_recovery_proof(
    db: &crate::persistence::database::Database,
    reference: &str,
    owner: &Value,
    proof: Value,
    code: &str,
) -> Result<(), &'static str> {
    let mut conn = db.open().map_err(|e| e.code())?;
    let tx = conn
        .transaction()
        .map_err(|_| "runtime_ownership_persist_failed")?;
    if tx.execute("UPDATE agent_runtime_owners SET state='stopped',cleanup_verified=1,error_code=coalesce(error_code,?2),owner_json=?3,cleanup_json=?4,finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE runtime_ref=?1",rusqlite::params![reference,code,owner.to_string(),proof.to_string()]).map_err(|_|"runtime_ownership_persist_failed")?!=1 {return Err("runtime_ownership_missing");}
    tx.execute(
        "UPDATE agent_runs SET cleanup_verified=1 WHERE runtime_ref=?1",
        [reference],
    )
    .map_err(|_| "runtime_ownership_persist_failed")?;
    tx.commit().map_err(|_| "runtime_ownership_persist_failed")
}

#[cfg(test)]
mod tests;

fn ownership_json(directory: &Path) -> Result<String, &'static str> {
    crate::policy::private_file(&directory.join("owner.json"))
        .map_err(|_| "runtime_identity_unavailable")?;
    let json = cleanup_ownership_json(directory)?;
    let owner: Value = serde_json::from_str(&json).map_err(|_| "runtime_identity_unavailable")?;
    for (pid, ticks) in [
        ("primary_pid", "primary_start_ticks"),
        ("guardian_pid", "guardian_start_ticks"),
        ("runtime_pid", "runtime_start_ticks"),
    ] {
        if !owner[pid]
            .as_u64()
            .is_some_and(|pid| pid > 0 && pid <= u32::MAX as u64)
            || owner[ticks].as_u64().is_none()
        {
            return Err("runtime_identity_unavailable");
        }
    }
    Ok(json)
}
fn cleanup_ownership_json(directory: &Path) -> Result<String, &'static str> {
    let owner_file = directory.join("owner.json");
    let mut owner: Value = if owner_file.exists() {
        serde_json::from_slice(&fs::read(owner_file).map_err(|_| "runtime_identity_unavailable")?)
            .map_err(|_| "runtime_identity_unavailable")?
    } else {
        serde_json::json!({})
    };
    if !owner.is_object() {
        return Err("runtime_identity_unavailable");
    }
    crate::policy::private_file(&directory.join("primary-owner.json"))
        .map_err(|_| "runtime_identity_unavailable")?;
    let primary: Value = serde_json::from_slice(
        &fs::read(directory.join("primary-owner.json"))
            .map_err(|_| "runtime_identity_unavailable")?,
    )
    .map_err(|_| "runtime_identity_unavailable")?;
    for key in ["primary_pid", "primary_start_ticks"] {
        owner[key] = primary[key].clone();
    }
    Ok(owner.to_string())
}
pub fn process_snapshot(db: &crate::persistence::database::Database) -> Value {
    let read = (|| -> Result<Value, &'static str> {
        let conn = db.open().map_err(|e| e.code())?;
        let row=conn.query_row("SELECT runtime_ref,state,cleanup_verified,owner_json,cleanup_json,boot_id,private_directory FROM agent_runtime_owners ORDER BY created_at DESC,runtime_ref DESC LIMIT 1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,bool>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?))).optional().map_err(|_|"runtime_ownership_read_failed")?;
        let Some((reference, state, recorded_verified, owner, proof, boot, path)) = row else {
            return Ok(serde_json::json!({"processes":[],"source":"core_ownership_no_runtime"}));
        };
        let owner = owner
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
        let verified = recorded_verified
            && state == "stopped"
            && proof
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .is_some_and(|proof| {
                    super::startup::completed_proven(
                        &owner,
                        &proof,
                        &reference,
                        &boot,
                        Path::new(&path),
                    )
                });
        let mut processes = vec![];
        for (role, pid, ticks) in [
            ("sdk_transport_owner", "primary_pid", "primary_start_ticks"),
            ("runtime_reaper", "guardian_pid", "guardian_start_ticks"),
            ("copilot_cli", "runtime_pid", "runtime_start_ticks"),
        ] {
            if let (Some(pid), Some(expected)) = (owner[pid].as_u64(), owner[ticks].as_u64()) {
                let mut observed = "not_present";
                let mut rss = None;
                if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) {
                    if let Some((_, fields)) = stat.rsplit_once(')') {
                        let fields: Vec<_> = fields.split_whitespace().collect();
                        if fields.get(19).and_then(|s| s.parse::<u64>().ok()) == Some(expected) {
                            observed = if fields.first() == Some(&"Z") {
                                "zombie"
                            } else {
                                "alive"
                            };
                            rss = fields
                                .get(21)
                                .and_then(|s| s.parse::<u64>().ok())
                                .map(|pages| {
                                    pages
                                        .saturating_mul(
                                            unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64,
                                        )
                                });
                        } else {
                            observed = "identity_changed";
                        }
                    }
                }
                processes.push(serde_json::json!({"role":role,"pid":pid,"identity_start_ticks":expected,"observed_state":observed,"rss_bytes":rss}));
            }
        }
        Ok(
            serde_json::json!({"runtime_ref":reference,"recorded_state":state,"cleanup_verified":verified,"recorded_cleanup_verified":recorded_verified,"processes":processes,"source":"core_receipt_plus_proc_identity_read_only"}),
        )
    })();
    read.unwrap_or_else(|code| serde_json::json!({"source":"not_verified","error_code":code}))
}
