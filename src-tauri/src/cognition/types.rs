use serde::Serialize;
use std::sync::Arc;
use crate::persistence::{conversation::ConversationMessage, identity::IdentityInput, memory::MemoryRecord};

pub type ProviderId = String;

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
    (!required.text_generation || self.text_generation) && (!required.streaming || self.streaming)
      && (!required.vision || self.vision) && (!required.tool_calling || self.tool_calling)
      && (!required.structured_output || self.structured_output)
  }
  pub fn text_stream() -> Self { Self { text_generation: true, streaming: true, ..Self::default() } }
}

#[derive(Clone, Debug)]
pub struct ProviderConfig { pub id: ProviderId, pub enabled: bool, pub priority: u16, pub capabilities: ProviderCapabilities }

#[derive(Debug)]
pub struct ContextBundle {
  pub identity: IdentityInput,
  pub relevant_memories: Vec<MemoryRecord>,
  pub recent_messages: Vec<ConversationMessage>,
  pub metadata: ContextMetadata,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMetadata { pub identity_version: String, pub memory_count: usize, pub recent_message_count: usize }

#[derive(Debug)]
pub struct ProviderRequest {
  pub input: String,
  pub context: Arc<ContextBundle>,
  pub max_output_tokens: u32,
  pub required_capabilities: ProviderCapabilities,
}
#[derive(Clone, Debug)]
pub struct ProviderChunk { pub text: String }
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage { pub calls: u32, pub input_tokens: u32, pub output_tokens: u32 }
#[derive(Clone, Debug)]
pub struct ProviderResponse { pub text: String, pub usage: ProviderUsage }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
  RateLimited { retry_after_ms: Option<u64> }, Timeout, QuotaExceeded, Fatal, Cancelled, Unavailable,
}
impl ProviderError {
  pub fn code(&self) -> &'static str { match self {
    Self::RateLimited { .. } => "rate_limited", Self::Timeout => "timeout", Self::QuotaExceeded => "quota_exceeded",
    Self::Fatal => "fatal", Self::Cancelled => "cancelled", Self::Unavailable => "unavailable",
  }}
}

#[derive(Clone, Copy, Debug)]
pub struct TaskBudget { pub max_provider_calls: u32, pub max_output_tokens: u32 }
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerUsage {
  pub provider_calls: u32, pub input_tokens: u32, pub output_tokens: u32,
  pub providers_used: Vec<ProviderId>, pub retries: u32, pub fallbacks: u32,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult { pub text: String, pub provider_id: ProviderId, pub usage: SchedulerUsage, pub context_metadata: ContextMetadata }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchedulerError { BudgetExceeded, Cancelled, NoProvider, Provider(ProviderError) }
impl SchedulerError { pub fn code(&self) -> &'static str { match self {
  Self::BudgetExceeded => "budget_exceeded", Self::Cancelled => "cancelled", Self::NoProvider => "provider_unavailable", Self::Provider(e) => e.code(),
}} }
