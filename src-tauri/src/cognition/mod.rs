pub mod context;
pub mod gemini;
pub mod gemini_commands;
pub mod mock;
pub mod provider;
pub mod policy;
pub mod registry;
pub mod scheduler;
pub mod settings;
pub mod summary;
pub mod types;

use std::{collections::HashMap, sync::Arc};
use serde::Deserialize;
use mock::{MockProvider, MockScenario};
use registry::ProviderRegistry;
use scheduler::{ProviderStatus, Scheduler};
use types::{ProviderCapabilities, ProviderConfig};
use crate::security::secrets::SecretStore;

pub struct GeminiRuntime { pub scheduler: Arc<Scheduler>, pub timeouts: Arc<std::sync::RwLock<crate::persistence::gemini_settings::GeminiTimeouts>> }
impl GeminiRuntime {
  pub fn new(secrets: Arc<SecretStore>, initial_timeouts: crate::persistence::gemini_settings::GeminiTimeouts) -> Result<Self, types::ProviderError> {
    let mut registry = ProviderRegistry::default();
    let provider = Arc::new(gemini::GeminiProvider::new(gemini::GeminiConfig::default(), secrets)?);
    let timeouts = provider.timeout_handle();
    *timeouts.write().unwrap_or_else(|p| p.into_inner()) = initial_timeouts;
    registry.register(ProviderConfig { id: "gemini".into(), enabled: true, priority: 1,
      capabilities: ProviderCapabilities::text_stream() },
      provider).expect("unique Gemini ID");
    Ok(Self { scheduler: Arc::new(Scheduler::new(registry)), timeouts })
  }
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
