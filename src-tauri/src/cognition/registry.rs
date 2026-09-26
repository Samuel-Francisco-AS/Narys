use std::{collections::HashMap, sync::Arc};
use super::{provider::Provider, types::{ProviderCapabilities, ProviderConfig, ProviderId}};

pub struct ProviderEntry { pub config: ProviderConfig, pub provider: Arc<dyn Provider> }
#[derive(Default)]
pub struct ProviderRegistry { entries: HashMap<ProviderId, ProviderEntry> }
impl ProviderRegistry {
  pub fn register(&mut self, config: ProviderConfig, provider: Arc<dyn Provider>) -> Result<(), &'static str> {
    if self.entries.contains_key(&config.id) { return Err("duplicate_provider_id"); }
    self.entries.insert(config.id.clone(), ProviderEntry { config, provider }); Ok(())
  }
  pub fn get(&self, id: &str) -> Option<&ProviderEntry> { self.entries.get(id) }
  pub fn eligible(&self, required: &ProviderCapabilities) -> Vec<&ProviderEntry> {
    let mut entries: Vec<_> = self.entries.values().filter(|entry| entry.config.enabled && entry.config.capabilities.supports(required)).collect();
    // Smaller priority number wins; ID breaks ties deterministically.
    entries.sort_by(|a,b| (a.config.priority, &a.config.id).cmp(&(b.config.priority, &b.config.id)));
    entries
  }
  pub fn configs(&self) -> Vec<&ProviderConfig> { let mut configs: Vec<_> = self.entries.values().map(|entry| &entry.config).collect(); configs.sort_by(|a,b| a.id.cmp(&b.id)); configs }
}
