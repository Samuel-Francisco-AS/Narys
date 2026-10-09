//! LR-10A experiments only. No Narys authority, registry or IPC integration.
use github_copilot_sdk::{
    CliProgram, Client, ClientOptions, Error, ErrorKind, LogLevel, ResumeSessionConfig,
    SessionConfig, Transport,
};
use serde_json::{json, Value};
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

pub mod persistence;

pub const DEADLINE: Duration = Duration::from_secs(15);

/// Codes only: never expose SDK error messages, RPC payloads, tokens or stderr.
pub fn error_code(error: &Error) -> &'static str {
    match error.kind() {
        ErrorKind::BinaryNotFound { .. } => "runtime_unavailable",
        ErrorKind::InvalidConfig => "invalid_configuration",
        ErrorKind::Protocol(_) | ErrorKind::Json => "protocol_error",
        ErrorKind::Io => "transport_unavailable",
        ErrorKind::Rpc { code: -32601 } => "method_unavailable",
        ErrorKind::Rpc { .. } => "rpc_error_unknown", // Do not guess auth/quota from prose.
        ErrorKind::Session(github_copilot_sdk::SessionErrorKind::SessionIdMismatch { .. }) => {
            "session_id_mismatch"
        }
        ErrorKind::Session(github_copilot_sdk::SessionErrorKind::NotFound(_)) => {
            "session_not_found"
        }
        ErrorKind::Session(github_copilot_sdk::SessionErrorKind::Timeout(_)) => "session_timeout",
        ErrorKind::Session(_) => "session_error",
        _ => "sdk_error_unknown",
    }
}

pub async fn bounded<T>(future: impl Future<Output = Result<T, Error>>) -> Result<T, &'static str> {
    tokio::time::timeout(DEADLINE, future)
        .await
        .map_err(|_| "timeout")?
        .map_err(|e| error_code(&e))
}

pub fn explicit_program(path: &Path) -> Result<PathBuf, &'static str> {
    if !path.is_absolute() {
        return Err("absolute_cli_path_required");
    }
    let resolved = path.canonicalize().map_err(|_| "runtime_unavailable")?;
    if !resolved.is_file() {
        return Err("runtime_unavailable");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if resolved
            .metadata()
            .map_err(|_| "runtime_unavailable")?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err("runtime_not_executable");
        }
    }
    Ok(resolved)
}

/// Separate process-local state/logs from the user's Copilot configuration.
/// Existing authentication is resolved by CLI/credential store, never extracted.
/// CopilotCli mode retains system keychain access; Empty mode disables keytar.
pub fn options(cli: PathBuf, workspace: &Path, state: &Path) -> ClientOptions {
    ClientOptions::new()
        .with_program(CliProgram::Path(cli))
        .with_transport(Transport::Stdio)
        .with_cwd(workspace)
        .with_base_directory(state)
        .with_use_logged_in_user(true)
        .with_log_level(LogLevel::None)
        .with_env_remove([
            "COPILOT_CLI_PATH",
            "COPILOT_SDK_AUTH_TOKEN",
            "GH_TOKEN",
            "GITHUB_TOKEN",
        ])
        .with_extra_args([
            "--disable-builtin-mcps",
            "--log-dir",
            state.to_str().unwrap(),
        ])
}

/// Deny every permission and expose zero tools. This is NOT a filesystem sandbox.
pub fn session_config(workspace: &Path) -> SessionConfig {
    let mut config = SessionConfig::default()
        .with_model("auto")
        .with_working_directory(workspace)
        .with_available_tools(Vec::<String>::new())
        .deny_all_permissions();
    config.enable_file_hooks = Some(false);
    config.enable_host_git_operations = Some(false);
    config.enable_skills = Some(false);
    config.skip_custom_instructions = Some(true);
    config.enable_on_demand_instruction_discovery = Some(false);
    config.enable_session_telemetry = Some(false);
    // Cross-session search/index integration, not a transcript flush guarantee.
    config.enable_session_store = Some(true);
    config
}

pub fn resume_config(id: github_copilot_sdk::SessionId, workspace: &Path) -> ResumeSessionConfig {
    let mut config = ResumeSessionConfig::new(id)
        .with_model("auto")
        .with_working_directory(workspace)
        .with_available_tools(Vec::<String>::new())
        .deny_all_permissions();
    config.allow_transcript_recovery = Some(false);
    config.enable_file_hooks = Some(false);
    config.enable_host_git_operations = Some(false);
    config.enable_skills = Some(false);
    config.skip_custom_instructions = Some(true);
    config.enable_on_demand_instruction_discovery = Some(false);
    config.enable_session_telemetry = Some(false);
    config
}

/// Quota absent/malformed remains unknown. No inference admission in this POC.
pub fn quota_state(snapshot: Option<&Value>) -> &'static str {
    let Some(s) = snapshot else {
        return "quota_unknown";
    };
    let Some(unlimited) = s.get("isUnlimitedEntitlement").and_then(Value::as_bool) else {
        return "quota_unknown";
    };
    let Some(entitlement) = s.get("entitlementRequests").and_then(Value::as_i64) else {
        return "quota_unknown";
    };
    if unlimited {
        return "unlimited_reported";
    }
    if entitlement < 0 {
        return "quota_unknown";
    }
    let Some(remaining) = s.get("remainingPercentage").and_then(Value::as_f64) else {
        return "quota_unknown";
    };
    if !(0.0..=100.0).contains(&remaining) {
        return "quota_unknown";
    }
    if entitlement == 0 || remaining == 0.0 {
        "limit_reached"
    } else {
        "quota_available"
    }
}

pub async fn quota_probe(client: &Client) -> Value {
    match bounded(client.rpc().account().get_quota()).await {
        Ok(q) => {
            let snapshots: Vec<_> = q.quota_snapshots.iter().map(|(kind, s)| {
                let data = serde_json::to_value(s).unwrap_or(Value::Null);
                json!({"kind":kind, "unit":"requests_as_reported_by_runtime", "state":quota_state(Some(&data)), "snapshot":data})
            }).collect();
            json!({"source":"account.getQuota", "state": if snapshots.is_empty() {"quota_unknown"} else {"observed"}, "snapshots":snapshots})
        }
        Err(code) => json!({"source":"account.getQuota", "state":"quota_unknown", "error":code}),
    }
}

/// Only metadata, no send/send_and_wait/fleet/prompt calls exist in this crate.
pub async fn metadata(client: &Client) -> Value {
    let auth = match bounded(client.get_auth_status()).await {
        Ok(a) => {
            json!({"state":if a.is_authenticated {"authenticated"} else {"authentication_required"},
            "entitlement":"unknown_until_service_metadata", "identity_omitted":true})
        }
        Err(code) => json!({"state":"auth_unknown", "error":code}),
    };
    let models = match bounded(client.list_models()).await {
        Ok(models) => {
            json!({"state":"observed", "auto_in_catalog":models.iter().any(|m| m.id == "auto"),
            "models": models.iter().map(|m| json!({"id":m.id,"capabilities":m.capabilities,"policy":m.policy,"billing":m.billing})).collect::<Vec<_>>()})
        }
        Err(code) => json!({"state":"models_unavailable", "error":code}),
    };
    json!({"auth":auth,"catalog":models,"quota":quota_probe(client).await})
}

/// Successful metadata/session RPCs prove operation termination only.
/// session.idle and assistant prose can never set task_completed here.
#[derive(Default)]
pub struct EventFacts {
    pub idle_seen: bool,
    pub operation_closed: bool,
    pub events: usize,
    pub gaps: usize,
    pub correlation_invalid: bool,
    seen: std::collections::HashSet<String>,
}
impl EventFacts {
    pub fn observe(&mut self, event: &github_copilot_sdk::SessionEvent) {
        self.events = self.events.saturating_add(1);
        if self.seen.len() >= 32 {
            self.gaps += 1;
            return;
        }
        if event
            .parent_id
            .as_ref()
            .is_some_and(|p| !self.seen.contains(p))
        {
            self.correlation_invalid = true;
        }
        if !self.seen.insert(event.id.clone()) {
            self.correlation_invalid = true;
        }
        self.idle_seen |= event.event_type == "session.idle";
    }
    pub fn task_completed(&self) -> bool {
        false
    }
}

/// Prepare subscription before start: avoid the SDK's unbounded bootstrap prefix.
#[derive(Debug)]
pub struct LifecycleFailure {
    pub stage: &'static str,
    pub code: &'static str,
}
fn at(stage: &'static str, code: &'static str) -> LifecycleFailure {
    LifecycleFailure { stage, code }
}

pub async fn lifecycle(client: &Client, workspace: &Path) -> Result<Value, LifecycleFailure> {
    let prepared = client
        .prepare_session(session_config(workspace))
        .map_err(|e| at("prepare", error_code(&e)))?;
    let mut events = prepared.subscribe();
    let session = bounded(prepared.start())
        .await
        .map_err(|code| at("create", code))?;
    let id = session.id().clone();
    let mut facts = EventFacts::default();
    // Observe only naturally available events, bounded duration and count. No raw payload logs.
    for _ in 0..32 {
        match tokio::time::timeout(Duration::from_millis(30), events.recv()).await {
            Ok(Ok(event)) => facts.observe(&event),
            Ok(Err(_)) => {
                facts.gaps += 1;
                break;
            }
            Err(_) => break,
        }
    }
    let abort = bounded(session.abort()).await;
    bounded(session.disconnect())
        .await
        .map_err(|code| at("detach_created", code))?;
    drop(events);
    drop(session);
    let prepared = client
        .prepare_resume_session(resume_config(id.clone(), workspace))
        .map_err(|e| at("prepare", error_code(&e)))?;
    let _resume_events = prepared.subscribe();
    let resumed = bounded(prepared.start())
        .await
        .map_err(|code| at("resume", code))?;
    let same = resumed.id() == &id;
    bounded(resumed.disconnect())
        .await
        .map_err(|code| at("detach_resumed", code))?;
    drop(resumed);
    bounded(client.delete_session(&id))
        .await
        .map_err(|code| at("delete", code))?;
    facts.operation_closed = true;
    Ok(
        json!({"created":true,"resume_same_id":same,"detached":true,"deleted":true,
        "abort_without_turn":abort.err().unwrap_or("rpc_acknowledged"),"events_observed":facts.events,
        "event_gaps":facts.gaps,"correlation_invalid":facts.correlation_invalid,"idle_seen":facts.idle_seen,"operation_closed":facts.operation_closed,
        "task_completed":facts.task_completed(),"opaque_session_id_omitted":true}),
    )
}

pub async fn shutdown(client: &Client) -> &'static str {
    match tokio::time::timeout(Duration::from_secs(5), client.stop()).await {
        Ok(Ok(())) => "graceful",
        Ok(Err(_)) => {
            client.force_stop();
            "stop_error_force_requested"
        }
        Err(_) => {
            client.force_stop();
            "stop_timeout_force_requested"
        }
    }
}
