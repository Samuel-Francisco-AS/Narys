//! Agent-neutral lifecycle intents. None of these values convey execution authority.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentSessionRef(pub String);
impl AgentSessionRef {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.0.len() != 35
            || !self.0.starts_with("cs-")
            || !self.0[3..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid_agent_session_ref");
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAutonomyProfile {
    Assisted,
    Isolated,
    ExplicitYolo,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeState {
    Dormant,
    Starting,
    Ready,
    Busy,
    Stopping,
    Faulted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionState {
    Creating,
    Detached,
    Resuming,
    Cancelled,
    Failed,
    Interrupted,
    Closed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycleOperation {
    Create,
    Resume,
}
#[derive(Clone, Debug, Serialize)]
pub struct SpecialistCapabilities {
    pub textual: bool,
    pub tools: bool,
    pub permissions_integrated: bool,
    pub inference_admission_integrated: bool,
}
