use super::policy::ThinkingLevel;
use crate::persistence::{
    conversation::ConversationMessage, identity::IdentityInput, memory::MemoryRecord,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type ProviderId = String;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderSelection {
    Fixed(ProviderId),
    Preferred,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderTimeouts {
    pub request_timeout_ms: u32,
    pub stream_idle_timeout_ms: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub text_generation: bool,
    pub streaming: bool,
    pub vision: bool,
    pub tool_calling: bool,
    pub structured_output: bool,
}
impl ProviderCapabilities {
    pub fn supports(&self, required: &Self) -> bool {
        (!required.text_generation || self.text_generation)
            && (!required.streaming || self.streaming)
            && (!required.vision || self.vision)
            && (!required.tool_calling || self.tool_calling)
            && (!required.structured_output || self.structured_output)
    }
    /// Union of implemented adapter modes, not a claim about every model.
    pub fn with_structured_output() -> Self {
        Self {
            structured_output: true,
            ..Self::text_stream()
        }
    }
    pub fn structured() -> Self {
        Self {
            text_generation: true,
            structured_output: true,
            ..Self::default()
        }
    }
    pub fn text_stream() -> Self {
        Self {
            text_generation: true,
            streaming: true,
            ..Self::default()
        }
    }
}

/// Core-owned output intent. JSON Schema always requests native strict generation;
/// local semantic validation remains mandatory after transport succeeds.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputContract {
    Text,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        max_bytes: usize,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportMode {
    Streaming,
    NonStreaming,
}
#[derive(Clone, Debug, PartialEq)]
pub struct InvocationMode {
    pub output: OutputContract,
    pub transport: TransportMode,
}
impl Default for InvocationMode {
    fn default() -> Self {
        Self {
            output: OutputContract::Text,
            transport: TransportMode::Streaming,
        }
    }
}
impl InvocationMode {
    pub fn text_stream(&self) -> bool {
        matches!(self.output, OutputContract::Text) && self.transport == TransportMode::Streaming
    }
    pub fn max_bytes(&self) -> Option<usize> {
        match &self.output {
            OutputContract::Text => None,
            OutputContract::JsonSchema { max_bytes, .. } => Some(*max_bytes),
        }
    }
    pub fn valid(&self) -> bool {
        match &self.output {
            OutputContract::Text => true,
            OutputContract::JsonSchema {
                name,
                schema,
                max_bytes,
            } => {
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                    && schema.is_object()
                    && *max_bytes > 0
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub id: ProviderId,
    pub enabled: bool,
    pub priority: u16,
    pub capabilities: ProviderCapabilities,
}

#[derive(Debug)]
pub struct ContextBundle {
    pub identity: IdentityInput,
    pub relevant_memories: Vec<MemoryRecord>,
    pub recent_messages: Vec<ConversationMessage>,
    pub metadata: ContextMetadata,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMetadata {
    pub identity_version: String,
    pub memory_count: usize,
    pub recent_message_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderInvocationConfig {
    pub model: String,
    pub thinking_level: Option<ThinkingLevel>,
    pub timeouts: Option<ProviderTimeouts>,
}
impl ProviderInvocationConfig {
    pub fn valid(&self) -> bool {
        !self.model.is_empty()
            && self.model.len() <= 128
            && self.model.trim() == self.model
            && !self.model.chars().any(char::is_control)
            && self
                .timeouts
                .is_none_or(|t| t.request_timeout_ms > 0 && t.stream_idle_timeout_ms > 0)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderTarget {
    pub provider_id: ProviderId,
    pub invocation: ProviderInvocationConfig,
}

#[derive(Debug)]
pub struct ProviderTaskRequest {
    pub traffic_class: super::admission::TrafficClass,
    pub mode: InvocationMode,
    pub input: String,
    /// Trusted instruction supplied by the Core. User content never populates this field.
    pub internal_system_instruction: Option<String>,
    pub history: Vec<ProviderMessage>,
    pub context: Arc<ContextBundle>,
    pub max_output_tokens: Option<u32>,
    pub selection: ProviderSelection,
    pub targets: Vec<ProviderTarget>,
    /// Runtime-only continuity key; never copied into ProviderRequest.
    pub affinity_key: Option<String>,
    pub estimated_context_bytes: usize,
    pub required_capabilities: ProviderCapabilities,
}

// Only the Scheduler constructs this for a selected target.
#[derive(Debug)]
pub struct ProviderRequest {
    pub mode: InvocationMode,
    pub input: String,
    pub internal_system_instruction: Option<String>,
    pub history: Vec<ProviderMessage>,
    pub context: Arc<ContextBundle>,
    pub max_output_tokens: Option<u32>,
    pub target: ProviderTarget,
    pub attempt: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderRole {
    User,
    Assistant,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderMessage {
    pub role: ProviderRole,
    pub content: String,
}
#[derive(Clone, Debug)]
pub struct ProviderChunk {
    pub text: String,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: Option<u32>,
    pub thought_tokens: Option<u32>,
    /// False when the adapter could not obtain provider-reported output usage.
    pub output_tokens_measured: bool,
}
#[derive(Clone, Debug)]
pub struct ProviderResponse {
    pub text: String,
    pub usage: ProviderUsage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    RateLimited { retry_after_ms: Option<u64> },
    Timeout,
    QuotaExceeded,
    Authentication,
    Fatal,
    Cancelled,
    RemoteCancelled,
    Incomplete,
    RequiresAction,
    Protocol,
    InvalidRequest,
    Unavailable { retry_after_ms: Option<u64> },
    EventSinkClosed,
    UnsupportedMode,
    OutputLimitExceeded,
}
impl ProviderError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RateLimited { .. } => "rate_limited",
            Self::Timeout => "timeout",
            Self::QuotaExceeded => "quota_exceeded",
            Self::Authentication => "provider_auth_failed",
            Self::Fatal => "fatal",
            Self::Cancelled => "cancelled",
            Self::RemoteCancelled => "provider_cancelled",
            Self::Incomplete => "provider_incomplete",
            Self::RequiresAction => "provider_requires_action",
            Self::Protocol => "provider_protocol_error",
            Self::InvalidRequest => "model_or_request_rejected",
            Self::Unavailable { .. } => "unavailable",
            Self::EventSinkClosed => "channel_closed",
            Self::UnsupportedMode => "provider_mode_unsupported",
            Self::OutputLimitExceeded => "provider_output_limit_exceeded",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TaskBudget {
    pub max_provider_calls: u32,
    pub max_output_tokens: Option<u32>,
}
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
}
impl RetryPolicy {
    pub fn backoff_ms(self, retry: u32) -> u64 {
        self.initial_backoff_ms.saturating_mul(
            1_u64
                .checked_shl(retry.saturating_sub(1))
                .unwrap_or(u64::MAX),
        )
    }
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerUsage {
    pub provider_calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Sum of provider-reported output tokens; incomplete when any attempt is unmeasured.
    pub output_tokens_measured: bool,
    /// Output budget conservatively debited, including attempts without usage.
    pub output_tokens_accounted: u32,
    pub total_tokens: Option<u32>,
    pub thought_tokens: Option<u32>,
    pub providers_used: Vec<ProviderId>,
    pub retries: u32,
    pub fallbacks: u32,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult {
    pub text: String,
    pub provider_id: ProviderId,
    pub usage: SchedulerUsage,
    pub context_metadata: ContextMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchedulerError {
    AdmissionQueueFull,
    AdmissionTimeout,
    BudgetExceeded,
    Cancelled,
    EventSinkClosed,
    NoProvider,
    InvalidTargetConfig,
    Provider(ProviderError),
}
impl SchedulerError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AdmissionQueueFull => "admission_queue_full",
            Self::AdmissionTimeout => "admission_timeout",
            Self::BudgetExceeded => "budget_exceeded",
            Self::Cancelled => "cancelled",
            Self::EventSinkClosed => "channel_closed",
            Self::NoProvider => "provider_unavailable",
            Self::InvalidTargetConfig => "provider_config_invalid",
            Self::Provider(e) => e.code(),
        }
    }
}
