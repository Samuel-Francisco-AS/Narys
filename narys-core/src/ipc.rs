//! Local protocol v1. Wire intents never contain ExecutionAuthority/HumanLocal.
use crate::policy::TaskInput;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const VERSION: u16 = 1;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
pub const MAX_CONNECTIONS: usize = 32;
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u16,
    pub request_id: String,
    pub command: Command,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Command {
    Status {},
    Credentials {},
    Stronghold {},
    Copilot {},
    SessionCheck {},
    ResumeCheck {
        task_id: u64,
    },
    Events {
        #[serde(default)]
        after: u64,
        #[serde(default = "event_limit")]
        limit: u16,
    },
    Prepare {
        task: TaskInput,
        expected: String,
    },
    Submit {
        task_id: u64,
    },
    Cancel {
        task_id: u64,
    },
    Result {
        task_id: u64,
    },
    Capabilities {},
    Conversation {
        session_id: i64,
        text: String,
    },
    Sessions {
        #[serde(default)]
        after: i64,
        #[serde(default = "session_limit")]
        limit: u16,
    },
    SessionCreate {},
    SessionGet {
        session_id: i64,
        #[serde(default)]
        after_message: i64,
        #[serde(default = "session_limit")]
        limit: u16,
    },
    SessionResume {
        session_id: i64,
    },
    SessionClose {
        session_id: i64,
    },
    ConversationPolicy {
        policy: narys_domain::cognition::policy::CognitiveRolePolicy,
    },
    Tasks {
        namespace: TaskNamespace,
        #[serde(default)]
        after: u64,
        #[serde(default = "session_limit")]
        limit: u16,
    },
    Models {},
    TaskGet {
        task: TaskRef,
    },
    TaskCancel {
        task: TaskRef,
    },
    Providers {},
    ProviderConfigure {
        provider_id: String,
        enabled: bool,
        #[serde(default)]
        free_tier_confirmed: bool,
    },
    Approval {
        approval_id: String,
        decision: ApprovalDecision,
    },
    ToolRequest {
        task: TaskRef,
        invocation: ToolInvocation,
    },
    ToolResult {
        task: TaskRef,
        invocation_id: String,
    },
}
fn session_limit() -> u16 {
    50
}
fn event_limit() -> u16 {
    128
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskNamespace {
    Lr10a,
    Product,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRef {
    pub namespace: TaskNamespace,
    pub id: u64,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    ApproveOnce,
    Deny,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolInvocation {
    ListFiles {
        workspace_id: String,
        relative_path: String,
    },
    ReadFile {
        workspace_id: String,
        relative_path: String,
    },
    EditFile {
        workspace_id: String,
        relative_path: String,
        content: String,
    },
    Build {
        workspace_id: String,
        target: String,
    },
    Test {
        workspace_id: String,
        target: String,
    },
    Diff {
        workspace_id: String,
    },
}
impl Request {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != VERSION {
            return Err("unsupported_protocol_version");
        }
        if self.request_id.is_empty()
            || self.request_id.len() > 64
            || !self
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err("invalid_request_id");
        }
        match &self.command {
            Command::ResumeCheck { task_id }
            | Command::Submit { task_id }
            | Command::Cancel { task_id }
            | Command::Result { task_id } => {
                narys_domain::security::validation::task_id(*task_id)?;
            }
            Command::TaskGet { task }
            | Command::TaskCancel { task }
            | Command::ToolRequest { task, .. }
            | Command::ToolResult { task, .. } => {
                narys_domain::security::validation::task_id(task.id)?;
            }
            Command::Tasks { after, limit, .. }
                if *after > i64::MAX as u64 || *limit == 0 || *limit > 100 =>
            {
                return Err("invalid_task_page");
            }
            Command::Events { after, .. } if *after > i64::MAX as u64 => {
                return Err("invalid_event_cursor");
            }
            Command::Events { limit, .. } if *limit == 0 || *limit > 128 => {
                return Err("invalid_event_limit")
            }
            Command::Conversation { session_id, text }
                if *session_id <= 0 || text.trim().is_empty() || text.len() > 4096 =>
            {
                return Err("invalid_conversation")
            }
            Command::Sessions { after, limit } if *after < 0 || *limit == 0 || *limit > 100 => {
                return Err("invalid_session_page")
            }
            Command::SessionGet {
                session_id,
                after_message,
                limit,
            } if *session_id <= 0 || *after_message < 0 || *limit == 0 || *limit > 100 => {
                return Err("invalid_session_page")
            }
            Command::SessionResume { session_id } | Command::SessionClose { session_id }
                if *session_id <= 0 =>
            {
                return Err("session_invalid")
            }
            Command::ProviderConfigure {
                provider_id,
                enabled,
                free_tier_confirmed,
            } => {
                if narys_domain::cognition::catalog::integration(provider_id).is_none() {
                    return Err("provider_unavailable");
                }
                if *enabled && !*free_tier_confirmed {
                    return Err("free_provider_authorization_required");
                }
            }
            Command::ConversationPolicy { policy } => {
                use narys_domain::cognition::policy::CognitiveRole;
                policy.validate()?;
                if policy.role != CognitiveRole::Conversation
                    || policy.max_provider_calls > 8
                    || policy.max_retries > 2
                    || policy.retry_backoff_ms > 60000
                    || policy.max_output_tokens.is_some_and(|n| n > 8192)
                    || policy.history_max_messages > 32
                    || policy.history_max_bytes > 65536
                    || policy.context_max_bytes > 65536
                {
                    return Err("conversation_policy_limit");
                }
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Protocol,
    Unavailable,
    Admission,
    Domain,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Success {
        version: u16,
        request_id: String,
        ok: bool,
        data: Value,
    },
    Failure {
        version: u16,
        request_id: String,
        ok: bool,
        error_code: String,
        category: ErrorCategory,
    },
}
impl Response {
    pub fn new(id: &str, result: Result<Value, &str>) -> Self {
        match result {
            Ok(data) => Self::Success {
                version: VERSION,
                request_id: id.into(),
                ok: true,
                data,
            },
            Err(code) => Self::Failure {
                version: VERSION,
                request_id: id.into(),
                ok: false,
                error_code: code.into(),
                category: match code {
                    "invalid_request"
                    | "invalid_request_id"
                    | "unsupported_protocol_version"
                    | "request_limit_or_timeout"
                    | "response_limit" => ErrorCategory::Protocol,
                    "capability_not_integrated"
                    | "provider_not_configured"
                    | "unlock_store_unavailable" => ErrorCategory::Unavailable,
                    "free_provider_authorization_required"
                    | "conversation_busy"
                    | "session_busy"
                    | "runtime_busy"
                    | "server_stopping" => ErrorCategory::Admission,
                    _ => ErrorCategory::Domain,
                },
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_protocol_rejects_unknown_fields_and_authority() {
        for value in [
            r#"{"version":1,"request_id":"a","command":{"operation":"status","authority":"HumanLocal"}}"#,
            r#"{"version":1,"request_id":"a","command":{"operation":"cancel","task_id":0},"extra":1}"#,
            r#"{"operation":"status"}"#,
        ] {
            assert!(
                serde_json::from_str::<Request>(value).is_err(),
                "accepted {value}"
            );
        }
        let r: Request = serde_json::from_str(
            r#"{"version":2,"request_id":"a","command":{"operation":"status"}}"#,
        )
        .unwrap();
        assert_eq!(r.validate(), Err("unsupported_protocol_version"));
    }
    #[test]
    fn task_namespaces_are_explicit_and_agent_intent_is_not_authority() {
        let r:Request=serde_json::from_str(r#"{"version":1,"request_id":"a","command":{"operation":"tool-request","task":{"namespace":"product","id":2},"invocation":{"tool":"read_file","workspace_id":"approved","relative_path":"src/main.rs"}}}"#).unwrap();
        assert!(r.validate().is_ok());
    }
}

#[cfg(test)]
mod conversation_validation_tests {
    use super::*;
    #[test]
    fn conversation_pages_and_financial_confirmation_have_strict_limits() {
        for command in [
            serde_json::json!({"operation":"conversation","session_id":1,"text":"x".repeat(4097)}),
            serde_json::json!({"operation":"session-get","session_id":1,"limit":101}),
            serde_json::json!({"operation":"sessions","after":-1}),
            serde_json::json!({"operation":"provider-configure","provider_id":"groq","enabled":true}),
            serde_json::json!({"operation":"provider-configure","provider_id":"invented","enabled":false}),
        ] {
            let r: Request = serde_json::from_value(
                serde_json::json!({"version":1,"request_id":"test","command":command}),
            )
            .unwrap();
            assert!(r.validate().is_err());
        }
        assert!(serde_json::from_value::<Request>(serde_json::json!({"version":1,"request_id":"test","command":{"operation":"conversation","session_id":1,"text":"hi","tools":["shell"]}})).is_err());
    }
}
