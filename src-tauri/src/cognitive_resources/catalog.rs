use super::*;
use crate::cognition::{rate::RateSnapshot, telemetry::ProviderTelemetrySnapshot};
use serde::Serialize;
use std::collections::BTreeMap;

/// Owned descriptions only. No singleton, locks, backend handles or live ledgers.
#[derive(Default)]
pub struct ResourceCatalog {
    resources: BTreeMap<ResourceId, CognitiveResource>,
}
impl ResourceCatalog {
    /// Registration validates atomically; duplicates never replace a descriptor.
    pub fn register(&mut self, resource: CognitiveResource) -> Result<(), CatalogError> {
        resource.validate()?;
        if self.resources.contains_key(&resource.identity.id) {
            return Err(CatalogError::DuplicateResource);
        }
        if self.resources.len() >= MAX_RESOURCES {
            return Err(CatalogError::CapacityExceeded);
        }
        for existing in self.resources.values() {
            // One registered runtime ID is one credential/quota context. Different
            // access paths require independent bindings, never family inference.
            if !matches!(resource.origin, ResourceOrigin::Local)
                && existing.origin == resource.origin
            {
                return Err(CatalogError::DuplicateRuntimeBinding);
            }
        }
        self.resources
            .insert(resource.identity.id.clone(), resource);
        Ok(())
    }
    /// Domain-level reads fail closed unless all associated observations agree,
    /// including provenance/timestamps. Registration and identity never depend on
    /// observations matching; snapshots preserve each resource's evidence separately.
    pub fn domain_economics(&self, id: &BillingDomainId) -> Result<&EconomicFacts, CatalogError> {
        let mut observations = self
            .resources
            .values()
            .filter(|resource| &resource.identity.billing_domain.id == id)
            .map(|resource| &resource.economics);
        let first = observations
            .next()
            .ok_or(CatalogError::BillingDomainNotFound)?;
        if observations.any(|other| other != first) {
            return Err(CatalogError::ConflictingEconomicFacts);
        }
        Ok(first)
    }
    pub fn resource(&self, id: &ResourceId) -> Option<&CognitiveResource> {
        self.resources.get(id)
    }
    /// ID order is serialization determinism, with no economic/quality ranking.
    pub fn resources(&self) -> impl Iterator<Item = &CognitiveResource> {
        self.resources.values()
    }
    pub fn describe_variant(
        &self,
        resource: &ResourceId,
        model: &ModelId,
        effort: Option<&EffortId>,
    ) -> Result<ExecutionVariant, CatalogError> {
        let resource = self
            .resource(resource)
            .ok_or(CatalogError::ResourceNotFound)?;
        let model = resource.model(model)?;
        let effort_profile = effort.map(|id| model.effort(id)).transpose()?;
        let effort_availability =
            effort_profile.map_or(CatalogFact::Unknown, |profile| profile.availability.clone());
        Ok(ExecutionVariant {
            resource_id: resource.identity.id.clone(),
            access_path: resource.identity.access_path.clone(),
            billing_domain_id: resource.identity.billing_domain.id.clone(),
            model_id: model.id.clone(),
            effort: effort.cloned(),
            resource_availability: resource.availability.clone(),
            model_availability: model.availability.clone(),
            effort_availability,
            model_facts: model.facts.clone(),
            effort_facts: effort_profile.map(|profile| profile.facts.clone()),
        })
    }
    /// Snapshots are already captured by LR-8. This method cannot refresh/reset
    /// its authorities. Call rate.read_only_snapshots(), not the mutating reader.
    pub fn snapshot(
        &self,
        telemetry: &[ProviderTelemetrySnapshot],
        rate: &[RateSnapshot],
    ) -> Result<ResourceCatalogSnapshot, CatalogError> {
        unique_snapshot_ids(telemetry.iter().map(|s| s.provider_id.as_str()))?;
        unique_snapshot_ids(rate.iter().map(|s| s.provider_id.as_str()))?;
        let resources = self
            .resources()
            .map(|resource| {
                Ok(ResourceSnapshot {
                    descriptor: resource.clone(),
                    lr8: project_lr8(resource, telemetry, rate)?,
                })
            })
            .collect::<Result<_, CatalogError>>()?;
        Ok(ResourceCatalogSnapshot { resources })
    }
}
fn unique_snapshot_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), CatalogError> {
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(CatalogError::DuplicateSnapshot);
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCatalogSnapshot {
    pub resources: Vec<ResourceSnapshot>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshot {
    pub descriptor: CognitiveResource,
    /// None for Agent/Local or no captured LR-8 facts; never interpreted as free.
    pub lr8: Option<Lr8Facts>,
}
