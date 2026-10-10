//! Pinned SDK policy for LR-10B. No inference operation is admitted here.
use github_copilot_sdk::{Error, ErrorKind, ResumeSessionConfig, SessionConfig};
use std::future::Future;
use std::path::Path;
use std::time::Duration;
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
    tokio::time::timeout(Duration::from_secs(15), future)
        .await
        .map_err(|_| "timeout")?
        .map_err(|e| error_code(&e))
}

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
    config.mcp_servers = Some(Default::default());
    config.enable_config_discovery = Some(false);
    config.request_extensions = Some(false);
    config.enable_mcp_apps = Some(false);
    config.plugin_directories = Some(vec![]);
    config.skill_directories = Some(vec![]);
    config.instruction_directories = Some(vec![]);
    config.custom_agents = Some(vec![]);
    config.additional_directories = Some(vec![]);
    config.hooks = Some(false);
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
    config.mcp_servers = Some(Default::default());
    config.enable_config_discovery = Some(false);
    config.request_extensions = Some(false);
    config.enable_mcp_apps = Some(false);
    config.plugin_directories = Some(vec![]);
    config.skill_directories = Some(vec![]);
    config.instruction_directories = Some(vec![]);
    config.custom_agents = Some(vec![]);
    config.additional_directories = Some(vec![]);
    config.hooks = Some(false);
    config
}
