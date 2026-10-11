//! Defense in depth only. These callbacks are not an OS sandbox or Broker path.
use async_trait::async_trait;
use github_copilot_sdk::{
    handler::{PermissionHandler, PermissionResult},
    hooks::{HookContext, PreToolUseInput, PreToolUseOutput, SessionHooks},
    PermissionRequestData, RequestId, SessionId,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

// Names and fields observed through tools.list on the pinned CLI (offline).
// Parsing is NOT authorization. Every valid/invalid/unknown native call is denied.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewArgs {
    path: String,
    view_range: Option<Vec<i64>>,
    #[serde(rename = "forceReadLargeFiles")]
    force_read_large_files: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateArgs {
    path: String,
    file_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    old_str: Option<String>,
    new_str: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BashArgs {
    command: String,
    description: String,
    #[serde(rename = "shellId")]
    shell_id: Option<String>,
    mode: Option<BashMode>,
    detach: Option<bool>,
    initial_wait: Option<f64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum BashMode {
    Sync,
    Async,
}
fn native_reason(name: &str, arguments: &Value) -> &'static str {
    if arguments.to_string().len() > 128 * 1024 {
        return "tool_arguments_limit";
    }
    let valid = match name {
        "view" => serde_json::from_value::<ViewArgs>(arguments.clone())
            .map(|a| {
                !a.path.is_empty()
                    && a.view_range.is_none_or(|v| v.len() == 2)
                    && a.force_read_large_files != Some(true)
            })
            .unwrap_or(false),
        "create" => serde_json::from_value::<CreateArgs>(arguments.clone())
            .map(|a| !a.path.is_empty() && a.file_text.len() <= 64 * 1024)
            .unwrap_or(false),
        "edit" => serde_json::from_value::<EditArgs>(arguments.clone())
            .map(|a| !a.path.is_empty() && a.old_str.is_some() && a.new_str.is_some())
            .unwrap_or(false),
        "bash" => serde_json::from_value::<BashArgs>(arguments.clone())
            .map(|a| {
                !a.command.is_empty()
                    && a.description.len() <= 100
                    && a.shell_id.is_none_or(|s| s.len() <= 80)
                    && a.detach != Some(true)
                    && a.initial_wait.is_none_or(|v| v.is_finite() && v >= 0.)
                    && matches!(a.mode, None | Some(BashMode::Sync) | Some(BashMode::Async))
            })
            .unwrap_or(false),
        "read_bash" | "stop_bash" | "list_bash" | "glob" | "grep" | "skill" | "web_fetch"
        | "task" | "read_agent" | "list_agents" | "write_agent" => {
            return "native_resource_blocked"
        }
        _ => return "unknown_resource_denied",
    };
    if valid {
        "native_boundary_unavailable"
    } else {
        "invalid_tool_arguments"
    }
}

pub fn permissions() -> Arc<dyn PermissionHandler> {
    Arc::new(ClosedBoundary)
}
pub fn hooks() -> Arc<dyn SessionHooks> {
    Arc::new(ClosedBoundary)
}
struct ClosedBoundary;
fn observe(code: &'static str) {
    use crate::operational_trace::*;
    let draft = EventDraft::new(
        Provenance {
            source: TraceSource {
                source_type: SourceType::SpecialistAgent,
                id: TraceId::new("copilot").unwrap(),
                instance: None,
            },
            task_id: None,
            subtask_id: None,
            correlation_id: None,
            coalescing_key: None,
        },
        OperationalKind::State {
            kind: StateKind::SubtaskLifecycle,
            code: TraceId::new(code).unwrap(),
            detail: TraceText::new("native tool boundary unavailable; denied").unwrap(),
        },
    );
    if let Ok(draft) = draft {
        let _ = OperationalTraceBus::process_wide().publish(draft);
    }
}
#[async_trait]
impl PermissionHandler for ClosedBoundary {
    async fn handle(
        &self,
        _session_id: SessionId,
        _request_id: RequestId,
        _data: PermissionRequestData,
    ) -> PermissionResult {
        observe("permission_denied");
        PermissionResult::reject(Some("Core execution boundary unavailable".into()))
    }
}
#[async_trait]
impl SessionHooks for ClosedBoundary {
    async fn on_pre_tool_use(
        &self,
        input: PreToolUseInput,
        _ctx: HookContext,
    ) -> Option<PreToolUseOutput> {
        let reason = native_reason(&input.tool_name, &input.tool_args);
        observe(reason);
        Some(PreToolUseOutput {
            permission_decision: Some("deny".into()),
            permission_decision_reason: Some(reason.into()),
            suppress_output: Some(true),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn observed_native_tools_and_unknown_extensions_never_gain_permission() {
        let inventory: Value = serde_json::from_str(include_str!(
            "../../../docs/evidence/lr10c/native-tools.json"
        ))
        .unwrap();
        let mut names: Vec<_> = inventory["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names.len(), 15);
        names.extend(["mcp.server.execute", "plugin.shell", "unknown-tool"].map(str::to_owned));
        for name in names {
            let output = ClosedBoundary
                .on_pre_tool_use(
                    PreToolUseInput {
                        session_id: "s".into(),
                        timestamp: 1.,
                        working_directory: "/tmp".into(),
                        tool_name: name,
                        tool_args: json!({"authority":"HumanLocal"}),
                    },
                    HookContext {
                        session_id: SessionId::new("s"),
                    },
                )
                .await
                .unwrap();
            assert_eq!(output.permission_decision.as_deref(), Some("deny"));
            assert!(!serde_json::to_string(&output)
                .unwrap()
                .contains("HumanLocal"));
        }
        assert_eq!(
            native_reason("create", &json!({"path":"file","file_text":"fixture"})),
            "native_boundary_unavailable"
        );
        assert_eq!(
            native_reason(
                "bash",
                &json!({"command":"sh -c 'env'","description":"nested","detach":true})
            ),
            "invalid_tool_arguments"
        );
    }
}
