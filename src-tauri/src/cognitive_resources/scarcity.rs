//! B2 decision views only. No descriptor mutation, allowance debit, quota
//! reservation, reset application or operational authorization.
use super::*;
use crate::cognition::{
    rate::ConstraintSource,
    telemetry::{QuotaDimension, QuotaScope, QuotaSnapshot},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFact<T> {
    pub fact: CatalogFact<T>,
    /// None means Unknown; facts preserve provenance and timestamps.
    pub layer: Option<EvidenceLayer>,
}
fn resolve<T: Clone>(model: &CatalogFact<T>, effort: Option<&CatalogFact<T>>) -> ResolvedFact<T> {
    if let Some(fact) = effort.filter(|f| f.value().is_some()) {
        ResolvedFact {
            fact: fact.clone(),
            layer: Some(EvidenceLayer::Effort),
        }
    } else if model.value().is_some() {
        ResolvedFact {
            fact: model.clone(),
            layer: Some(EvidenceLayer::Model),
        }
    } else {
        ResolvedFact {
            fact: CatalogFact::Unknown,
            layer: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedConsumption {
    pub consumption: AllowanceConsumption,
    /// Even Unknown amount retains the layer explicitly declaring relevance.
    pub layer: EvidenceLayer,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedExecutionFacts {
    pub cognitive_tier: ResolvedFact<CognitiveTier>,
    pub relative_cost: ResolvedFact<RelativeCostTier>,
    pub latency_ms: ResolvedFact<u64>,
    pub monetary_cost: ResolvedFact<MonetaryAmount>,
    pub allowance_costs: Vec<ResolvedConsumption>,
}
impl ResolvedExecutionFacts {
    pub(super) fn for_candidate(
        candidate: &AllocationCandidate<'_>,
    ) -> Result<Self, EconomicExclusion> {
        let variant = candidate.variant();
        let model = candidate
            .resource()
            .model(&variant.model_id)
            .map_err(|_| EconomicExclusion::EconomicEvidenceConflict)?;
        let effort = variant
            .effort
            .as_ref()
            .map(|id| model.effort(id))
            .transpose()
            .map_err(|_| EconomicExclusion::EconomicEvidenceConflict)?
            .map(|e| &e.facts);
        let facts = &model.facts.execution;
        let mut allowances = BTreeMap::new();
        for c in &facts.allowance_costs {
            allowances.insert(
                c.dimension_id.clone(),
                ResolvedConsumption {
                    consumption: c.clone(),
                    layer: EvidenceLayer::Model,
                },
            );
        }
        if let Some(e) = effort {
            for c in &e.allowance_costs {
                if allowances
                    .get(&c.dimension_id)
                    .is_some_and(|m| m.consumption.unit != c.unit)
                {
                    return Err(EconomicExclusion::EconomicEvidenceConflict);
                }
                // Substitute by dimension even when the effort amount is Unknown.
                // Never add model and effort amounts or convert units.
                allowances.insert(
                    c.dimension_id.clone(),
                    ResolvedConsumption {
                        consumption: c.clone(),
                        layer: EvidenceLayer::Effort,
                    },
                );
            }
        }
        Ok(Self {
            cognitive_tier: resolve(&facts.cognitive_tier, effort.map(|e| &e.cognitive_tier)),
            relative_cost: resolve(&facts.relative_cost, effort.map(|e| &e.relative_cost)),
            latency_ms: resolve(&facts.latency_ms, effort.map(|e| &e.latency_ms)),
            monetary_cost: resolve(&facts.monetary_cost, effort.map(|e| &e.monetary_cost)),
            allowance_costs: allowances.into_values().collect(),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScarcityReason {
    ExplicitZeroConsumption,
    RemainingZero,
    CannotCoverExecution,
    Percentage,
    InsufficientEvidence,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowanceScarcity {
    pub consumption: ResolvedConsumption,
    pub state_evidence: Option<AllowanceState>,
    /// Known(0) consumption stays visible but does not influence aggregate state.
    pub consumed: bool,
    pub state: ScarcityState,
    pub remaining_percent: Option<u8>,
    pub reason: ScarcityReason,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScarcitySummary {
    pub known_worst: Option<ScarcityState>,
    pub has_unknown: bool,
}
fn severity(state: ScarcityState) -> Option<u8> {
    match state {
        ScarcityState::Comfortable => Some(0),
        ScarcityState::Reduced => Some(1),
        ScarcityState::Reserve => Some(2),
        ScarcityState::Exhausted => Some(3),
        ScarcityState::Unknown => None,
    }
}
impl ScarcitySummary {
    fn observe(&mut self, state: ScarcityState) {
        if let Some(level) = severity(state) {
            if self
                .known_worst
                .and_then(severity)
                .is_none_or(|old| level > old)
            {
                self.known_worst = Some(state);
            }
        } else {
            self.has_unknown = true;
        }
    }
    pub(super) fn combined(self, other: Self) -> Self {
        let mut result = self;
        if let Some(state) = other.known_worst {
            result.observe(state);
        }
        result.has_unknown |= other.has_unknown;
        result
    }
}
/// floor(remaining * 100 / limit), widened before multiplication; clamp is a
/// numeric bound, not an invented remaining value. Inputs validated upstream.
fn percentage(remaining: Option<u64>, limit: Option<u64>) -> Option<u8> {
    remaining
        .zip(limit.filter(|l| *l > 0))
        .map(|(r, l)| ((u128::from(r) * 100) / u128::from(l)).min(100) as u8)
}
fn classify(percent: Option<u8>, policy: Option<ReservePolicy>) -> ScarcityState {
    match percent.zip(policy) {
        Some((p, thresholds)) if p < thresholds.reserve_below_percent() => ScarcityState::Reserve,
        Some((p, thresholds)) if p < thresholds.reduced_below_percent() => ScarcityState::Reduced,
        Some(_) => ScarcityState::Comfortable,
        None => ScarcityState::Unknown,
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScarcityAssessment {
    pub dimensions: Vec<AllowanceScarcity>,
    pub summary: ScarcitySummary,
}
pub(super) fn assess_allowances(
    facts: &ResolvedExecutionFacts,
    economics: &EconomicFacts,
    policy: Option<ReservePolicy>,
) -> ScarcityAssessment {
    let mut result = ScarcityAssessment::default();
    for resolved in &facts.allowance_costs {
        let c = &resolved.consumption;
        let evidence = economics
            .allowances
            .iter()
            .find(|s| s.id == c.dimension_id)
            .cloned();
        let amount = c.amount.value().copied();
        let remaining = evidence.as_ref().and_then(|s| s.remaining.value()).copied();
        let limit = evidence.as_ref().and_then(|s| s.limit.value()).copied();
        let consumed = amount != Some(0);
        let percent = if consumed {
            percentage(remaining, limit)
        } else {
            None
        };
        let (state, reason) = if !consumed {
            (
                ScarcityState::Unknown,
                ScarcityReason::ExplicitZeroConsumption,
            )
        } else if remaining == Some(0) {
            (ScarcityState::Exhausted, ScarcityReason::RemainingZero)
        } else if remaining.zip(amount).is_some_and(|(r, a)| a > 0 && r < a) {
            (
                ScarcityState::Exhausted,
                ScarcityReason::CannotCoverExecution,
            )
        } else {
            let state = classify(percent, policy);
            (
                state,
                if state == ScarcityState::Unknown {
                    ScarcityReason::InsufficientEvidence
                } else {
                    ScarcityReason::Percentage
                },
            )
        };
        if consumed {
            result.summary.observe(state);
        }
        result.dimensions.push(AllowanceScarcity {
            consumption: resolved.clone(),
            state_evidence: evidence,
            consumed,
            state,
            remaining_percent: percent,
            reason,
        });
    }
    result
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "modelId", rename_all = "snake_case")]
pub enum PressureScope {
    Provider,
    Model(ModelId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureSource {
    RateExternalFact,
    RateLocalPolicy,
    RateDailyBudget,
    TelemetryFallback,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintPressure {
    pub scope: PressureScope,
    pub dimension: QuotaDimension,
    pub source: PressureSource,
    pub capacity: Option<u64>,
    pub effective_remaining: Option<u64>,
    pub saturated: bool,
    pub unaccounted_token_calls: u64,
    pub external_evidence: Option<QuotaSnapshot>,
    pub provenance: Option<crate::cognition::telemetry::Provenance>,
    pub reset_unix_ms: Option<u64>,
    pub reset_in_ms: Option<u64>,
    pub state: ScarcityState,
    pub remaining_percent: Option<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationalPressure {
    pub constraints: Vec<ConstraintPressure>,
    pub summary: ScarcitySummary,
    pub telemetry_generation: Option<u64>,
    pub rate_generation: Option<u64>,
    pub telemetry_captured_at_unix_ms: Option<u64>,
    pub rate_captured_at_unix_ms: Option<u64>,
}
pub(super) fn assess_pressure(
    lr8: Option<&Lr8Facts>,
    model: &ModelId,
    policy: Option<ReservePolicy>,
) -> OperationalPressure {
    let mut result = OperationalPressure::default();
    let Some(lr8) = lr8 else {
        return result;
    };
    if let Some(rate) = &lr8.rate {
        result.rate_generation = Some(rate.context_generation);
        result.rate_captured_at_unix_ms = rate.captured_at_unix_ms;
        for c in rate
            .provider_constraints
            .iter()
            .chain(rate.model_constraints.get(model).into_iter().flatten())
        {
            let scope = match &c.scope {
                QuotaScope::Provider => PressureScope::Provider,
                QuotaScope::Model { model: name } if name == model.as_str() => {
                    PressureScope::Model(model.clone())
                }
                _ => continue,
            };
            let remaining_percent = percentage(c.effective_remaining, c.capacity);
            // Never use global rate.saturated: another model or counter can
            // cause it. An applicable saturated constraint is conservative B2
            // unavailability; this is not a transport/admission permission.
            let state = if c.saturated || c.effective_remaining == Some(0) {
                ScarcityState::Exhausted
            } else {
                classify(remaining_percent, policy)
            };
            result.constraints.push(ConstraintPressure {
                scope,
                dimension: c.dimension,
                source: match c.source {
                    ConstraintSource::ExternalFact => PressureSource::RateExternalFact,
                    ConstraintSource::LocalPolicy => PressureSource::RateLocalPolicy,
                    ConstraintSource::DailyBudget => PressureSource::RateDailyBudget,
                },
                capacity: c.capacity,
                effective_remaining: c.effective_remaining,
                saturated: c.saturated,
                unaccounted_token_calls: c.unaccounted_token_calls,
                external_evidence: c.external.clone(),
                provenance: c.provenance,
                reset_unix_ms: c.reset_unix_ms,
                reset_in_ms: c.reset_in_ms,
                state,
                remaining_percent,
            });
        }
    }
    if let Some(telemetry) = &lr8.telemetry {
        result.telemetry_generation = Some(telemetry.context_generation);
        result.telemetry_captured_at_unix_ms = telemetry.captured_at_unix_ms;
        for (scope, quotas) in
            std::iter::once((PressureScope::Provider, &telemetry.provider_quotas)).chain(
                telemetry
                    .model_quotas
                    .get(model)
                    .map(|q| (PressureScope::Model(model.clone()), q)),
            )
        {
            for (dimension, q) in quotas {
                // ANY normalized rate constraint for the same exact scope and
                // dimension takes precedence, even when its remaining is Unknown.
                // Telemetry must not resurrect retained conservative rate state.
                if result
                    .constraints
                    .iter()
                    .any(|c| c.scope == scope && c.dimension == *dimension)
                {
                    continue;
                }
                let capacity = CatalogFact::from(&q.limit).value().copied();
                let remaining = CatalogFact::from(&q.remaining).value().copied();
                let remaining_percent = percentage(remaining, capacity);
                let state = if remaining == Some(0) {
                    ScarcityState::Exhausted
                } else {
                    classify(remaining_percent, policy)
                };
                result.constraints.push(ConstraintPressure {
                    scope: scope.clone(),
                    dimension: *dimension,
                    source: PressureSource::TelemetryFallback,
                    capacity,
                    effective_remaining: remaining,
                    saturated: false,
                    unaccounted_token_calls: 0,
                    external_evidence: Some(q.clone()),
                    provenance: None,
                    // The historical telemetry Timing stays in external_evidence.
                    // It is not an operational deadline/countdown at capture time.
                    reset_unix_ms: None,
                    reset_in_ms: None,
                    state,
                    remaining_percent,
                });
            }
        }
    }
    result
        .constraints
        .sort_by(|a, b| (&a.scope, a.dimension, a.source).cmp(&(&b.scope, b.dimension, b.source)));
    for c in &result.constraints {
        result.summary.observe(c.state);
    }
    result
}
