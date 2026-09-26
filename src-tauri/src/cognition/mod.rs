pub mod context;
pub mod mock;
pub mod provider;
pub mod registry;
pub mod scheduler;
pub mod types;

use std::{collections::HashMap, sync::Arc};
use serde::Deserialize;
use mock::{MockProvider, MockScenario};
use registry::ProviderRegistry;
use scheduler::{ProviderStatus, Scheduler};
use types::{ProviderCapabilities, ProviderConfig};

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
