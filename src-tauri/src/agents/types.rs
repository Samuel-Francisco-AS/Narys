use std::fmt;

pub type AgentBackendId = String;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgentCapabilities {
    pub planning: bool,
    pub repository_read: bool,
    pub file_write: bool,
    pub command_execution: bool,
    pub tool_use: bool,
    pub structured_output: bool,
}

impl AgentCapabilities {
    pub fn supports(&self, required: &Self) -> bool {
        (!required.planning || self.planning)
            && (!required.repository_read || self.repository_read)
            && (!required.file_write || self.file_write)
            && (!required.command_execution || self.command_execution)
            && (!required.tool_use || self.tool_use)
            && (!required.structured_output || self.structured_output)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentConfig {
    pub id: AgentBackendId,
    pub enabled: bool,
    pub priority: u16,
    pub capabilities: AgentCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRequest {
    pub objective: String,
    pub required_capabilities: AgentCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentResult {
    pub output: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentEvent {
    Output { text: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentError {
    Cancelled,
    UnsupportedCapability,
    InvalidRequest,
    Unavailable,
    Protocol,
    BackendFailed,
    EventSinkClosed,
    PlannerDiagnostic(super::codex::backend::PlannerDiagnosticCode),
}

impl AgentError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::InvalidRequest => "invalid_request",
            Self::Unavailable => "unavailable",
            Self::Protocol => "protocol_error",
            Self::BackendFailed => "backend_failed",
            Self::EventSinkClosed => "event_sink_closed",
            Self::PlannerDiagnostic(code) => code.code(),
        }
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
