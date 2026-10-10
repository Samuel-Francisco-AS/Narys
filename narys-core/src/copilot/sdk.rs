//! Official SDK transport behind a private Linux ownership wrapper. No prompts.
use super::{sdk_policy::*, supervisor::*};
use crate::{agents::lifecycle::AgentLifecycleOperation, policy};
use github_copilot_sdk::{CliProgram, Client, ClientOptions, LogLevel, Transport};
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
fn record_alive(directory: &Path) -> bool {
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
        if let Ok(bytes) = fs::read(directory.join(file)) {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                for (pid, ticks) in fields {
                    if let (Some(pid), Some(ticks)) = (value[pid].as_u64(), value[ticks].as_u64()) {
                        if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) {
                            if stat
                                .rsplit_once(')')
                                .and_then(|(_, s)| s.split_whitespace().nth(19))
                                .and_then(|s| s.parse::<u64>().ok())
                                == Some(ticks)
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
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
                    if !record_alive(directory) {
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
    fn start(&self) -> RuntimeFuture<'_, Arc<dyn ManagedRuntime>> {
        Box::pin(async move {
            crate::worker::validate_cli(&self.cli)?;
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
        }
    }
    async fn start_owned(&self) -> Result<Arc<dyn ManagedRuntime>, &'static str> {
        crate::server::mkdir(&self.root)?;
        prune(&self.database, &self.root)?;
        let dir = tempfile::Builder::new()
            .prefix("runtime-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&self.root)
            .map_err(|_| "runtime_directory_failed")?
            .keep();
        let reference = dir
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("runtime_path_invalid")?
            .to_owned();
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(|_| "boot_identity_unavailable")?;
        self.database.open().map_err(|e|e.code())?.execute("INSERT INTO agent_runtime_owners(runtime_ref,private_directory,boot_id,state) VALUES(?1,?2,?3,'starting')",rusqlite::params![reference,dir.to_str(),boot.trim()]).map_err(|_|"runtime_ownership_persist_failed")?;
        for n in ["workspace", "logs"] {
            crate::server::mkdir(&dir.join(n))?;
        }
        let sdk_state = self
            .root
            .parent()
            .ok_or("runtime_path_invalid")?
            .join("sdk-state");
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
        let client = match bounded(Client::start(options)).await {
            Ok(client) => client,
            Err(code) => {
                // SDK Drop/failed handshake closes or kills the wrapper;
                // its independent guardian still reaps before this proof.
                let verified = cleanup(&dir).await.is_ok();
                persist_cleanup(&self.database, &dir, verified, Some(code))?;
                return Err(if verified {
                    code
                } else {
                    "runtime_cleanup_incomplete"
                });
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
            runtime.stop().await?;
            return Err(code);
        }
        let ready=ownership_json(&runtime.directory).map_err(|_|"runtime_identity_unavailable").and_then(|owner|
                runtime.database.open().map_err(|e|e.code()).and_then(|conn|conn.execute("UPDATE agent_runtime_owners SET state='ready',owner_json=?2 WHERE runtime_ref=?1",rusqlite::params![reference,owner]).map_err(|_|"runtime_ownership_persist_failed")));
        if ready.is_err() {
            runtime.stop().await?;
            return Err("runtime_ownership_persist_failed");
        }
        Ok(runtime as Arc<dyn ManagedRuntime>)
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
        Box::pin(async move {
            let _ = self.database.open().map(|conn| {
                conn.execute(
                    "UPDATE agent_runtime_owners SET state='stopping' WHERE runtime_ref=?1",
                    [self.ownership_ref()],
                )
            });
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
            )?;
            if !verified {
                return Err("runtime_cleanup_incomplete");
            }
            // SDK stop may fail after an unexpected CLI death even though kernel
            // cleanup succeeds. Record the failure, never report a graceful stop.
            if !graceful {
                return Err("sdk_shutdown_recovered");
            }
            Ok(())
        })
    }
}

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
    let owner = ownership_json(directory).ok();
    let proof = fs::read_to_string(directory.join("cleanup.json")).ok();
    let conn = db.open().map_err(|e| e.code())?;
    conn.execute("UPDATE agent_runtime_owners SET state=?2,cleanup_verified=?3,error_code=?4,owner_json=coalesce(owner_json,?5),cleanup_json=?6,finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE runtime_ref=?1",
        rusqlite::params![reference,if verified {"stopped"}else{"faulted"},verified,error,owner,proof]).map_err(|_|"runtime_ownership_persist_failed")?;
    if verified {
        conn.execute(
            "UPDATE agent_runs SET cleanup_verified=1 WHERE runtime_ref=?1",
            [reference],
        )
        .map_err(|_| "agent_write_failed")?;
    }
    Ok(())
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
    let rows = {
        let conn = db.open().map_err(|e| e.code())?;
        let mut q=conn.prepare("SELECT private_directory,boot_id FROM agent_runtime_owners WHERE cleanup_verified=0 ORDER BY created_at LIMIT 65").map_err(|_|"runtime_ownership_read_failed")?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
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
    let root = db
        .directory()
        .parent()
        .ok_or("agent_state_directory_missing")?
        .join("copilot/runtimes");
    let mut unresolved = false;
    for (directory, old_boot) in rows {
        let path = Path::new(&directory);
        if path.parent() != Some(root.as_path()) {
            return Err("runtime_directory_mismatch");
        }
        if old_boot != boot.trim() {
            persist_cleanup(db, path, true, Some("previous_kernel_boot_no_replay"))?;
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
#[cfg(test)]
mod tests;

fn ownership_json(directory: &Path) -> Result<String, &'static str> {
    let mut owner: Value = serde_json::from_slice(
        &fs::read(directory.join("owner.json")).map_err(|_| "runtime_identity_unavailable")?,
    )
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
        let row=conn.query_row("SELECT runtime_ref,state,cleanup_verified,owner_json FROM agent_runtime_owners ORDER BY created_at DESC,runtime_ref DESC LIMIT 1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,bool>(2)?,r.get::<_,Option<String>>(3)?))).ok();
        let Some((reference, state, verified, owner)) = row else {
            return Ok(serde_json::json!({"processes":[],"source":"core_ownership_no_runtime"}));
        };
        let owner = owner
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
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
            serde_json::json!({"runtime_ref":reference,"recorded_state":state,"cleanup_verified":verified,"processes":processes,"source":"core_receipt_plus_proc_identity_read_only"}),
        )
    })();
    read.unwrap_or_else(|code| serde_json::json!({"source":"not_verified","error_code":code}))
}
