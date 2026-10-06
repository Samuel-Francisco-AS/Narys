//! Narrow copy of safe LR-8 facts. No manager, reservations, reset logic or accounting.
use super::*;
use crate::cognition::{
    rate::{RateConstraintSnapshot, RateSnapshot},
    telemetry::{
        Fact, ProviderTelemetrySnapshot, QuotaDimension, QuotaScope, QuotaSnapshot, Timing,
        UsageCounter, UsageDimension,
    },
};
use serde::Serialize;
use std::collections::BTreeMap;

type Quotas = BTreeMap<QuotaDimension, QuotaSnapshot>;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryFacts {
    pub captured_at_unix_ms: Option<u64>,
    pub context_generation: u64,
    /// Provider usage stays provider-scoped; never distributed among models.
    pub usage: BTreeMap<UsageDimension, UsageCounter>,
    pub provider_quotas: Quotas,
    /// Only exact locally described model IDs, without provider quota inheritance.
    pub model_quotas: BTreeMap<ModelId, Quotas>,
    pub retry_hint: Fact<Timing>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateFacts {
    pub captured_at_unix_ms: Option<u64>,
    pub context_generation: u64,
    /// External facts retained by rate can differ from the latest telemetry fact.
    pub provider_constraints: Vec<RateConstraintSnapshot>,
    pub model_constraints: BTreeMap<ModelId, Vec<RateConstraintSnapshot>>,
    pub pending_reservations: usize,
    pub local_blocks: u64,
    pub saturated: bool,
    pub persistence_failed: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lr8Facts {
    // Captures remain separately timestamped; no aggregate atomicity is claimed.
    pub telemetry: Option<TelemetryFacts>,
    pub rate: Option<RateFacts>,
}
pub(super) fn project_lr8(
    resource: &CognitiveResource,
    telemetry: &[ProviderTelemetrySnapshot],
    rate: &[RateSnapshot],
) -> Result<Option<Lr8Facts>, CatalogError> {
    let ResourceOrigin::Provider(id) = &resource.origin else {
        return Ok(None);
    };
    let telemetry = telemetry.iter().find(|s| s.provider_id == id.as_str());
    let rate = rate.iter().find(|s| s.provider_id == id.as_str());
    if telemetry.is_none() && rate.is_none() {
        return Ok(None);
    }
    if telemetry
        .zip(rate)
        .is_some_and(|(t, r)| t.context_generation != r.context_generation)
    {
        return Err(CatalogError::ContextMismatch);
    }
    let described_model = |name: &str| {
        resource
            .models
            .value()
            .and_then(|models| models.iter().find(|model| model.id.as_str() == name))
            .map(|model| model.id.clone())
    };
    let telemetry = telemetry
        .map(|snapshot| {
            let mut provider_quotas = BTreeMap::new();
            let mut model_quotas = BTreeMap::new();
            let mut seen = std::collections::BTreeSet::new();
            for scoped in &snapshot.quotas {
                // Reject ambiguity instead of silently overwriting a repeated scope.
                let scope_key = match &scoped.scope {
                    QuotaScope::Provider => None,
                    QuotaScope::Model { model } => Some(model.as_str()),
                };
                if !seen.insert(scope_key) {
                    return Err(CatalogError::DuplicateSnapshot);
                }
                match &scoped.scope {
                    QuotaScope::Provider => provider_quotas = scoped.dimensions.clone(),
                    QuotaScope::Model { model } => {
                        if let Some(id) = described_model(model) {
                            model_quotas.insert(id, scoped.dimensions.clone());
                        }
                    }
                }
            }
            Ok(TelemetryFacts {
                captured_at_unix_ms: snapshot.captured_at_unix_ms,
                context_generation: snapshot.context_generation,
                usage: snapshot.usage.clone(),
                provider_quotas,
                model_quotas,
                retry_hint: snapshot.retry_hint.clone(),
            })
        })
        .transpose()?;
    let rate = rate.map(|snapshot| {
        let mut provider_constraints = Vec::new();
        let mut model_constraints: BTreeMap<ModelId, Vec<RateConstraintSnapshot>> = BTreeMap::new();
        for constraint in &snapshot.constraints {
            match &constraint.scope {
                QuotaScope::Provider => provider_constraints.push(constraint.clone()),
                QuotaScope::Model { model } => {
                    if let Some(id) = described_model(model) {
                        model_constraints
                            .entry(id)
                            .or_default()
                            .push(constraint.clone());
                    }
                }
            }
        }
        RateFacts {
            captured_at_unix_ms: snapshot.captured_at_unix_ms,
            context_generation: snapshot.context_generation,
            provider_constraints,
            model_constraints,
            pending_reservations: snapshot.pending_reservations,
            local_blocks: snapshot.local_blocks,
            saturated: snapshot.saturated,
            persistence_failed: snapshot.persistence_failed,
        }
    });
    Ok(Some(Lr8Facts { telemetry, rate }))
}
