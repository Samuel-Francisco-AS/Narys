use super::{CatalogError, CatalogFact, CatalogProvenance};
use crate::{agents::types::AgentCapabilities, cognition::types::ProviderCapabilities};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitiveCapability {
    TextGeneration,
    Streaming,
    Vision,
    ToolCalling,
    StructuredOutput,
    Planning,
    RepositoryRead,
    FileWrite,
    CommandExecution,
    ToolUse,
}
/// Missing entries are unknown. Known(false) is explicit unsupported capability.
/// Adapter descriptors are never copied into a ModelProfile by these bridges.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CapabilitySet(pub BTreeMap<CognitiveCapability, CatalogFact<bool>>);
impl CapabilitySet {
    pub fn from_provider(capabilities: ProviderCapabilities) -> Self {
        use CognitiveCapability::*;
        Self::runtime_contract([
            (TextGeneration, capabilities.text_generation),
            (Streaming, capabilities.streaming),
            (Vision, capabilities.vision),
            (ToolCalling, capabilities.tool_calling),
            (StructuredOutput, capabilities.structured_output),
        ])
    }
    pub fn from_agent(capabilities: AgentCapabilities) -> Self {
        use CognitiveCapability::*;
        Self::runtime_contract([
            (Planning, capabilities.planning),
            (RepositoryRead, capabilities.repository_read),
            (FileWrite, capabilities.file_write),
            (CommandExecution, capabilities.command_execution),
            (ToolUse, capabilities.tool_use),
            (StructuredOutput, capabilities.structured_output),
        ])
    }
    fn runtime_contract<const N: usize>(values: [(CognitiveCapability, bool); N]) -> Self {
        Self(
            values
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        CatalogFact::Known {
                            value,
                            provenance: CatalogProvenance::RuntimeContract,
                            observed_at_unix_ms: None,
                        },
                    )
                })
                .collect(),
        )
    }
    pub fn get(&self, capability: CognitiveCapability) -> CatalogFact<bool> {
        self.0.get(&capability).cloned().unwrap_or_default()
    }
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        for fact in self.0.values() {
            fact.validate()?;
        }
        Ok(())
    }
}
