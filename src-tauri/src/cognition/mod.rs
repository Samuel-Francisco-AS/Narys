pub mod context;
pub mod catalog;
pub mod gemini;
pub mod gemini_commands;
pub mod groq;
pub mod groq_commands;
pub mod mock;
pub mod provider;
pub mod policy;
pub mod registry;
pub mod scheduler;
pub mod settings;
pub mod summary;
pub mod types;

use std::{collections::HashMap, sync::{Arc, RwLock}};
use serde::Deserialize;
use mock::{MockProvider, MockScenario};
use registry::ProviderRegistry;
use scheduler::{ProviderStatus, Scheduler};
use types::{ProviderCapabilities, ProviderConfig};

pub struct ProviderRuntime { pub scheduler: Arc<Scheduler> }
pub struct ProviderTimeoutHandles(pub HashMap<String, Arc<RwLock<types::ProviderTimeouts>>>);
impl ProviderRuntime {
  pub fn new(registry: ProviderRegistry) -> Self { Self { scheduler: Arc::new(Scheduler::new(registry)) } }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticScenario { Normal, Streaming, RateLimitFallback, TimeoutRetry, BudgetExhausted, Cancel }

pub struct CognitionRuntime { schedulers: HashMap<DiagnosticScenario, Arc<Scheduler>> }
impl CognitionRuntime {
  pub fn new() -> Self {
    let mut schedulers = HashMap::new();
    for (scenario, primary) in [
      (DiagnosticScenario::Normal, MockScenario::Normal),
      (DiagnosticScenario::Streaming, MockScenario::Streaming),
      (DiagnosticScenario::RateLimitFallback, MockScenario::RateLimited),
      (DiagnosticScenario::TimeoutRetry, MockScenario::Timeout),
      (DiagnosticScenario::BudgetExhausted, MockScenario::RateLimited),
      (DiagnosticScenario::Cancel, MockScenario::Streaming),
    ] {
      let mut registry = ProviderRegistry::default();
      for (id, priority, behavior) in [("mock-primary", 1, primary), ("mock-fallback", 2, MockScenario::Normal)] {
        registry.register(ProviderConfig { id: id.into(), enabled: true, priority,
          capabilities: ProviderCapabilities::text_stream() }, Arc::new(MockProvider::new(behavior))).expect("unique mock IDs");
      }
      schedulers.insert(scenario, Arc::new(Scheduler::new(registry)));
    }
    Self { schedulers }
  }
  pub fn scheduler(&self, scenario: DiagnosticScenario) -> Arc<Scheduler> { self.schedulers[&scenario].clone() }
  pub fn status(&self) -> Vec<ProviderStatus> { self.scheduler(DiagnosticScenario::RateLimitFallback).status() }
}

#[cfg(test)]
mod tests;

impl From<crate::persistence::gemini_settings::GeminiTimeouts> for types::ProviderTimeouts {
  fn from(value: crate::persistence::gemini_settings::GeminiTimeouts) -> Self {
    Self { request_timeout_ms: value.request_timeout_ms, stream_idle_timeout_ms: value.stream_idle_timeout_ms }
  }
}
