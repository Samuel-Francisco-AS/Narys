pub mod admission;
#[cfg(test)]
mod admission_tests;
pub mod allocation_policy;
mod bounded_json;
pub mod catalog;
pub mod cloudflare;
pub mod context;
#[cfg(test)]
mod fix5_tests;
pub mod gemini;
pub mod groq;
#[cfg(test)]
mod lr8e_gate_tests;
pub mod mistral;
pub mod mock;
pub mod operational;
#[cfg(test)]
mod operational_tests;
pub mod orchestrator;
pub mod policy;
pub mod provider;
pub mod rate;
#[cfg(test)]
mod rate_tests;
pub mod registry;
pub mod resilience;
#[cfg(test)]
mod resilience_tests;
pub mod scheduler;
pub mod summary;
pub mod sessions;
pub mod task_graph;
pub(crate) mod task_graph_handoff;
pub mod task_graph_runtime;
#[cfg(any(test, feature = "desktop-tests"))]
pub mod task_graph_runtime_tests;
mod task_graph_worker;
pub mod telemetry;
#[cfg(test)]
mod telemetry_tests;
mod transport;
pub mod types;

use mock::{MockProvider, MockScenario};
use registry::ProviderRegistry;
use scheduler::{ProviderStatus, Scheduler};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use types::{ProviderCapabilities, ProviderConfig};

pub struct ProviderRuntime {
    pub scheduler: Arc<Scheduler>,
}
pub struct ProviderTimeoutHandles(pub HashMap<String, Arc<RwLock<types::ProviderTimeouts>>>);
impl ProviderRuntime {
    pub fn connect_credentials(&self, store: &crate::security::secrets::SecretStore) {
        let observer: Arc<dyn crate::security::secrets::CredentialContextObserver> =
            self.scheduler.clone();
        store.observe_context_changes(Arc::downgrade(&observer));
    }
    pub fn new(registry: ProviderRegistry) -> Self {
        Self {
            scheduler: Arc::new(Scheduler::new(registry)),
        }
    }
    pub fn with_database(
        registry: ProviderRegistry,
        db: crate::persistence::database::Database,
    ) -> Result<Self, types::SchedulerError> {
        Ok(Self {
            scheduler: Arc::new(Scheduler::with_rate_storage(registry, Some(db))?),
        })
    }
}
impl crate::security::secrets::CredentialContextObserver for Scheduler {
    fn credentials_changed(&self, keys: &[crate::security::secrets::SecretKey]) {
        use crate::security::secrets::SecretKey;
        // Deduplicate a combined Cloudflare token/account mutation into one era.
        let mut providers = std::collections::BTreeSet::new();
        for key in keys {
            let id = match key {
                SecretKey::GeminiApiKey => "gemini",
                SecretKey::GroqApiKey => "groq",
                SecretKey::MistralApiKey => "mistral",
                SecretKey::CloudflareApiToken | SecretKey::CloudflareAccountId => "cloudflare",
                SecretKey::Lr3Test => continue,
            };
            providers.insert(id);
        }
        for id in providers {
            self.invalidate_rate_context(id);
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticScenario {
    Normal,
    Streaming,
    RateLimitFallback,
    TimeoutRetry,
    BudgetExhausted,
    Cancel,
}

pub struct CognitionRuntime {
    schedulers: HashMap<DiagnosticScenario, Arc<Scheduler>>,
}
impl CognitionRuntime {
    pub fn new() -> Self {
        let mut schedulers = HashMap::new();
        for (scenario, primary) in [
            (DiagnosticScenario::Normal, MockScenario::Normal),
            (DiagnosticScenario::Streaming, MockScenario::Streaming),
            (
                DiagnosticScenario::RateLimitFallback,
                MockScenario::RateLimited,
            ),
            (DiagnosticScenario::TimeoutRetry, MockScenario::Timeout),
            (
                DiagnosticScenario::BudgetExhausted,
                MockScenario::RateLimited,
            ),
            (DiagnosticScenario::Cancel, MockScenario::Streaming),
        ] {
            let mut registry = ProviderRegistry::default();
            for (id, priority, behavior) in [
                ("mock-primary", 1, primary),
                ("mock-fallback", 2, MockScenario::Normal),
            ] {
                registry
                    .register(
                        ProviderConfig {
                            id: id.into(),
                            enabled: true,
                            priority,
                            capabilities: ProviderCapabilities::text_stream(),
                        },
                        Arc::new(MockProvider::new(behavior)),
                    )
                    .expect("unique mock IDs");
            }
            schedulers.insert(scenario, Arc::new(Scheduler::new(registry)));
        }
        Self { schedulers }
    }
    pub fn scheduler(&self, scenario: DiagnosticScenario) -> Arc<Scheduler> {
        self.schedulers[&scenario].clone()
    }
    pub fn status(&self) -> Vec<ProviderStatus> {
        self.scheduler(DiagnosticScenario::RateLimitFallback)
            .status()
    }
}

#[cfg(test)]
mod tests;

impl From<crate::persistence::gemini_settings::GeminiTimeouts> for types::ProviderTimeouts {
    fn from(value: crate::persistence::gemini_settings::GeminiTimeouts) -> Self {
        Self {
            request_timeout_ms: value.request_timeout_ms,
            stream_idle_timeout_ms: value.stream_idle_timeout_ms,
        }
    }
}

pub mod scheduler_usage;
