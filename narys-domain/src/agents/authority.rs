//! Agent-neutral intent contracts. Deserialization never creates authority.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalPolicy {
    #[default]
    Assisted,
    Isolated,
    ExplicitYolo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentOperation {
    Read {
        relative_path: PathBuf,
    },
    Write {
        relative_path: PathBuf,
        content: String,
    },
    Delete {
        relative_path: PathBuf,
    },
    Command {
        program: PathBuf,
        arguments: Vec<String>,
    },
    Git {
        arguments: Vec<String>,
    },
    Network {
        destination: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOperationContext {
    pub task_id: u64,
    pub session_id: String,
    pub specialist_id: String,
    pub profile: AgentApprovalPolicy,
    pub workspace: PathBuf,
    pub tool: String,
    pub operation: AgentOperation,
    pub policy_version: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalState {
    Pending,
    Approved,
    Denied,
    Expired,
    Cancelled,
    Consumed,
    Interrupted,
}
