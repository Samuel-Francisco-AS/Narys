use std::collections::HashMap;
use std::sync::Arc;

use super::{
    backend::AgentBackend,
    types::{AgentBackendId, AgentCapabilities, AgentConfig},
};

pub struct AgentEntry {
    pub config: AgentConfig,
    pub backend: Arc<dyn AgentBackend>,
}

#[derive(Default)]
pub struct AgentRegistry {
    entries: HashMap<AgentBackendId, AgentEntry>,
}

impl AgentRegistry {
    pub fn register(
        &mut self,
        config: AgentConfig,
        backend: Arc<dyn AgentBackend>,
    ) -> Result<(), &'static str> {
        if self.entries.contains_key(&config.id) {
            return Err("duplicate_agent_backend_id");
        }
        self.entries
            .insert(config.id.clone(), AgentEntry { config, backend });
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&AgentEntry> {
        self.entries.get(id)
    }

    pub fn configs(&self) -> Vec<&AgentConfig> {
        let mut configs: Vec<_> = self.entries.values().map(|entry| &entry.config).collect();
        configs.sort_by(|a, b| a.id.cmp(&b.id));
        configs
    }

    pub fn eligible(&self, required: &AgentCapabilities) -> Vec<&AgentEntry> {
        let mut entries: Vec<_> = self
            .entries
            .values()
            .filter(|entry| entry.config.enabled && entry.config.capabilities.supports(required))
            .collect();
        entries.sort_by(|a, b| {
            (a.config.priority, &a.config.id).cmp(&(b.config.priority, &b.config.id))
        });
        entries
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicBool, Arc};

    use super::*;
    use crate::agents::{
        backend::{AgentBackend, MockAgentBackend},
        types::{AgentError, AgentEvent, AgentRequest},
    };

    fn config(
        id: &str,
        enabled: bool,
        priority: u16,
        capabilities: AgentCapabilities,
    ) -> AgentConfig {
        AgentConfig {
            id: id.into(),
            enabled,
            priority,
            capabilities,
        }
    }

    fn request(required_capabilities: AgentCapabilities) -> AgentRequest {
        AgentRequest {
            objective: "inspect the project".into(),
            required_capabilities,
        }
    }

    fn backend() -> Arc<MockAgentBackend> {
        MockAgentBackend::new("mock result")
    }

    #[test]
    fn registers_and_looks_up_by_id() {
        let mut registry = AgentRegistry::default();
        registry
            .register(
                config("mock", true, 1, AgentCapabilities::default()),
                backend(),
            )
            .unwrap();
        assert!(registry.get("mock").is_some());
        assert_eq!(registry.configs().len(), 1);
    }

    #[test]
    fn rejects_duplicate_id() {
        let mut registry = AgentRegistry::default();
        registry
            .register(
                config("mock", true, 1, AgentCapabilities::default()),
                backend(),
            )
            .unwrap();
        assert_eq!(
            registry.register(
                config("mock", true, 2, AgentCapabilities::default()),
                backend()
            ),
            Err("duplicate_agent_backend_id")
        );
    }

    #[test]
    fn excludes_disabled_backends() {
        let mut registry = AgentRegistry::default();
        registry
            .register(
                config("disabled", false, 1, AgentCapabilities::default()),
                backend(),
            )
            .unwrap();
        assert!(registry.eligible(&AgentCapabilities::default()).is_empty());
    }

    #[test]
    fn excludes_incompatible_capabilities() {
        let mut registry = AgentRegistry::default();
        registry
            .register(
                config(
                    "reader",
                    true,
                    1,
                    AgentCapabilities {
                        repository_read: true,
                        ..Default::default()
                    },
                ),
                backend(),
            )
            .unwrap();
        assert!(registry
            .eligible(&AgentCapabilities {
                file_write: true,
                ..Default::default()
            })
            .is_empty());
    }

    #[test]
    fn requires_all_capabilities() {
        let mut registry = AgentRegistry::default();
        registry
            .register(
                config(
                    "partial",
                    true,
                    1,
                    AgentCapabilities {
                        repository_read: true,
                        ..Default::default()
                    },
                ),
                backend(),
            )
            .unwrap();
        registry
            .register(
                config(
                    "complete",
                    true,
                    2,
                    AgentCapabilities {
                        repository_read: true,
                        tool_use: true,
                        ..Default::default()
                    },
                ),
                backend(),
            )
            .unwrap();
        let required = AgentCapabilities {
            repository_read: true,
            tool_use: true,
            ..Default::default()
        };
        assert_eq!(
            registry
                .eligible(&required)
                .iter()
                .map(|entry| entry.config.id.as_str())
                .collect::<Vec<_>>(),
            vec!["complete"]
        );
    }

    #[test]
    fn orders_by_priority() {
        let mut registry = AgentRegistry::default();
        registry
            .register(
                config("later", true, 2, AgentCapabilities::default()),
                backend(),
            )
            .unwrap();
        registry
            .register(
                config("first", true, 1, AgentCapabilities::default()),
                backend(),
            )
            .unwrap();
        assert_eq!(
            registry
                .eligible(&AgentCapabilities::default())
                .iter()
                .map(|entry| entry.config.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "later"]
        );
    }

    #[test]
    fn breaks_priority_ties_by_id() {
        let mut registry = AgentRegistry::default();
        registry
            .register(
                config("zeta", true, 1, AgentCapabilities::default()),
                backend(),
            )
            .unwrap();
        registry
            .register(
                config("alpha", true, 1, AgentCapabilities::default()),
                backend(),
            )
            .unwrap();
        assert_eq!(
            registry
                .eligible(&AgentCapabilities::default())
                .iter()
                .map(|entry| entry.config.id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
    }

    #[tokio::test]
    async fn executes_through_object_safe_trait_with_mock() {
        let backend: Arc<dyn AgentBackend> = backend();
        let cancelled = AtomicBool::new(false);
        let mut events = Vec::new();
        let mut sink = |event| {
            events.push(event);
            Ok(())
        };
        let result = backend
            .execute(
                &request(AgentCapabilities::default()),
                &cancelled,
                &mut sink,
            )
            .await
            .unwrap();
        assert_eq!(result.output, "mock result");
        assert_eq!(
            events,
            vec![AgentEvent::Output {
                text: "mock result".into()
            }]
        );
    }

    #[tokio::test]
    async fn marked_cancellation_is_returned_by_mock() {
        let backend: Arc<dyn AgentBackend> = backend();
        let cancelled = AtomicBool::new(true);
        let mut sink = |_| Ok(());
        assert_eq!(
            backend
                .execute(
                    &request(AgentCapabilities::default()),
                    &cancelled,
                    &mut sink
                )
                .await,
            Err(AgentError::Cancelled)
        );
    }

    #[tokio::test]
    async fn event_sink_failure_is_propagated_by_mock() {
        let backend: Arc<dyn AgentBackend> = backend();
        let cancelled = AtomicBool::new(false);
        let mut sink = |_| Err(AgentError::EventSinkClosed);
        assert_eq!(
            backend
                .execute(
                    &request(AgentCapabilities::default()),
                    &cancelled,
                    &mut sink
                )
                .await,
            Err(AgentError::EventSinkClosed)
        );
    }
}
