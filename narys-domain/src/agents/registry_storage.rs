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
