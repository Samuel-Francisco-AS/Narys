//! B3: policy-authorized provider variants -> B1 + exact runtime proof -> B2.
//! No execution, reservation, live manager reads, remote discovery or catalog mutation.
use super::*;
use crate::cognition::{
    policy::MAX_TARGETS,
    rate::RateSnapshot,
    registry::{ProviderEntry, ProviderRegistry},
    telemetry::ProviderTelemetrySnapshot,
    types::{InvocationMode, ProviderCapabilities, ProviderTarget, SchedulerError},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// B3 baseline for regression fixtures only; production has no fallback.
#[cfg(test)]
pub fn provider_allocation_default() -> AllocationPolicy {
    AllocationPolicy {
        profile: AllocationProfile::Balanced,
        variant_selection_mode: VariantSelectionMode::Auto,
        paid_use: PaidUsePolicy::Deny,
        reserve: None,
    }
}

/// Local relative context reconstruction scale, not tokens, money or latency.
/// ceil(bytes / KiB), capped at 100: 0 is neutral; one byte is only 1/100.
/// Division precedes addition, so even usize::MAX cannot overflow. Monotonic.
pub fn context_switch_signal(bytes: usize) -> u16 {
    let kib = bytes / 1024 + usize::from(bytes % 1024 != 0);
    kib.min(100) as u16
}

pub fn provider_candidate_signals(
    ordinal: usize,
    priority: u16,
    provider_id: &str,
    affinity: Option<&str>,
    bytes: usize,
) -> Result<CandidateSignals, ProviderBridgeError> {
    if ordinal >= MAX_TARGETS {
        return Err(ProviderBridgeError::InvalidTargets);
    }
    let signal = context_switch_signal(bytes);
    let (continuity, switching) = match affinity.filter(|_| signal > 0) {
        Some(id) if id == provider_id => (Some(signal), Some(0)),
        Some(_) => (Some(0), Some(signal)),
        None => (None, None),
    };
    // Registry priorities are u16. Explicit clamp, never reject >32 or wrap.
    CandidateSignals::new(
        Some(ordinal as u16),
        Some(priority.min(REGISTRY_PRIORITY_CAP)),
        continuity,
        switching,
    )
    .map_err(|_| ProviderBridgeError::InvalidTargets)
}

/// Provider capabilities describe the runtime contract, never ModelProfile.
pub fn provider_requirements(
    required: ProviderCapabilities,
    floor: Option<QualityFloor>,
) -> CandidateRequirements {
    use CognitiveCapability::*;
    let capabilities = [
        (TextGeneration, required.text_generation),
        (Streaming, required.streaming),
        (Vision, required.vision),
        (ToolCalling, required.tool_calling),
        (StructuredOutput, required.structured_output),
    ]
    .into_iter()
    .filter(|(_, needed)| *needed)
    .map(|(capability, _)| CapabilityRequirement {
        capability,
        scope: CapabilityScope::Runtime,
    })
    .collect();
    CandidateRequirements::new(capabilities, floor)
        .expect("five distinct bounded runtime requirements")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBridgeError {
    InvalidTargets,
    CatalogRuntimeMismatch,
    ExpansionOverflow,
    InvalidOperationalProof,
    InvalidAllocationUniverse,
    InvalidEconomicEvidence,
    Lr8ContextMismatch,
    NoEligibleCandidates,
}
impl ProviderBridgeError {
    pub fn scheduler_error(self) -> SchedulerError {
        match self {
            Self::NoEligibleCandidates => SchedulerError::NoProvider,
            Self::Lr8ContextMismatch => SchedulerError::RateContextChanged,
            _ => SchedulerError::InvalidTargetConfig,
        }
    }
}
impl std::fmt::Display for ProviderBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ProviderBridgeError {}

/// Created only after registered/enabled/runtime requirements/supports_invocation.
/// Bound to one complete variant, created and consumed in the same planning call
/// with the exact invocation and mode. No sibling/other effort proof, no remote
/// availability guarantee, price, quota, quality or backend handle.
#[derive(Clone, PartialEq)]
pub struct OperationalVariantProof {
    variant: AllocationVariant,
    target: ProviderTarget,
    mode: InvocationMode,
    capabilities: ProviderCapabilities,
}
// Even diagnostic formatting omits the mode's schema. Proofs are not serialized
// and never escape into the plan/events or contain adapter/backend handles.
impl std::fmt::Debug for OperationalVariantProof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationalVariantProof")
            .field("variant", &self.variant)
            .finish_non_exhaustive()
    }
}
impl OperationalVariantProof {
    fn prove(
        entry: &ProviderEntry,
        candidate: &AllocationCandidate<'_>,
        target: &ProviderTarget,
        required: ProviderCapabilities,
        mode: &InvocationMode,
    ) -> Option<Self> {
        let variant = candidate.variant();
        if !entry.config.enabled
            || !entry.config.capabilities.supports(&required)
            || entry.config.id != target.provider_id
            || variant.model_id.as_str() != target.invocation.model
            || variant.effort
                != target
                    .invocation
                    .thinking_level
                    .map(EffortId::from_thinking_level)
            || !target.invocation.valid()
            || !mode.valid()
            || !entry.provider.supports_invocation(&target.invocation, mode)
        {
            return None;
        }
        Some(Self {
            variant,
            target: target.clone(),
            mode: mode.clone(),
            capabilities: entry.config.capabilities,
        })
    }
    fn covers(&self, reason: &EligibilityReason) -> bool {
        match reason {
            EligibilityReason::ModelSupportUnknown
            | EligibilityReason::EffortSupportUnknown
            | EligibilityReason::ResourceEnabledUnknown => true,
            EligibilityReason::AvailabilityUnknown {
                layer: EvidenceLayer::Resource | EvidenceLayer::Model,
            } => true,
            EligibilityReason::AvailabilityUnknown {
                layer: EvidenceLayer::Effort,
            } => self.variant.effort.is_some(),
            EligibilityReason::CapabilityUnknown {
                requirement,
                layer: EvidenceLayer::Resource,
            } if requirement.scope == CapabilityScope::Runtime => {
                CapabilitySet::from_provider(self.capabilities)
                    .get(requirement.capability)
                    .value()
                    == Some(&true)
            }
            // Every contradiction, model capability Unknown and quality Unknown
            // remains inviolable. Operational compatibility cannot prove quality.
            _ => false,
        }
    }
}

/// Sealed B3 input: original B1 evidence is retained, including Unknown facts.
/// No public constructor; generic B2 still rejects every B1 Unresolved input.
pub(super) struct ResolvedProviderCandidate<'a> {
    candidate: AllocationCandidate<'a>,
    b1: CandidateEligibility,
    proof: OperationalVariantProof,
    signals: CandidateSignals,
}
impl<'a> ResolvedProviderCandidate<'a> {
    fn resolve(
        candidate: AllocationCandidate<'a>,
        b1: CandidateEligibility,
        proof: OperationalVariantProof,
        signals: CandidateSignals,
        target: &ProviderTarget,
        mode: &InvocationMode,
    ) -> Result<Option<Self>, ProviderBridgeError> {
        if candidate.variant() != *b1.variant()
            || proof.variant != candidate.variant()
            || proof.target != *target
            || proof.mode != *mode
        {
            return Err(ProviderBridgeError::InvalidOperationalProof);
        }
        if b1.status() == EligibilityStatus::Ineligible
            || b1.reasons().iter().any(|reason| !proof.covers(reason))
        {
            return Ok(None);
        }
        Ok(Some(Self {
            candidate,
            b1,
            proof,
            signals,
        }))
    }
    pub(super) fn into_scoring_parts(
        self,
    ) -> (
        AllocationCandidate<'a>,
        CandidateEligibility,
        CandidateSignals,
        Result<ResolvedExecutionFacts, EconomicExclusion>,
    ) {
        debug_assert_eq!(self.proof.variant, self.candidate.variant());
        let facts = ResolvedExecutionFacts::for_provider_candidate(&self.candidate);
        (self.candidate, self.b1, self.signals, facts)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderVariantExclusionReason {
    RuntimeUnsupported,
    EffortBridgeUnsupported,
    HardEligibility { reasons: Vec<EligibilityReason> },
    Economic { reasons: Vec<EconomicExclusion> },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderVariantExclusion {
    pub variant: AllocationVariant,
    pub reason: ProviderVariantExclusionReason,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRouteEntry {
    pub target: ProviderTarget,
    pub variant: AllocationVariant,
    pub score: i64,
}
/// Sanitized frozen route. No prompt/context, credential, remote account,
/// schema, adapter handle or ScoreBreakdown is kept/serialized in this plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRoutePlan {
    entries: Vec<AutoRouteEntry>,
    exclusions: Vec<ProviderVariantExclusion>,
}
impl AutoRoutePlan {
    pub fn entries(&self) -> &[AutoRouteEntry] {
        &self.entries
    }
    pub fn exclusions(&self) -> &[ProviderVariantExclusion] {
        &self.exclusions
    }
}

pub struct ProviderAutoAllocator {
    catalog: ResourceCatalog,
}
impl ProviderAutoAllocator {
    pub fn production(registry: &ProviderRegistry) -> Result<Self, ProviderBridgeError> {
        let mut catalog = ResourceCatalog::default();
        for config in registry.configs() {
            let identity = ResourceIdentity {
                id: ResourceId::new(&config.id)
                    .map_err(|_| ProviderBridgeError::CatalogRuntimeMismatch)?,
                class: ResourceClass::CognitiveProvider,
                family: ProviderFamily::new(&config.id)
                    .map_err(|_| ProviderBridgeError::CatalogRuntimeMismatch)?,
                access_path: AccessPath::new("provider_runtime").expect("local identity"),
                billing_domain: BillingDomain {
                    id: BillingDomainId::new(&config.id)
                        .map_err(|_| ProviderBridgeError::CatalogRuntimeMismatch)?,
                },
            };
            catalog
                .register(
                    CognitiveResource::from_provider_config(identity, config)
                        .map_err(|_| ProviderBridgeError::CatalogRuntimeMismatch)?,
                )
                .map_err(|_| ProviderBridgeError::CatalogRuntimeMismatch)?;
        }
        Ok(Self::new(catalog))
    }
    /// Catalog injection does not carry per-role policy.
    pub fn new(catalog: ResourceCatalog) -> Self {
        Self { catalog }
    }
    pub fn catalog(&self) -> &ResourceCatalog {
        &self.catalog
    }

    pub fn plan(
        &self,
        registry: &ProviderRegistry,
        runtime_policy: &AllocationRuntimePolicy,
        targets: &[ProviderTarget],
        required: ProviderCapabilities,
        mode: &InvocationMode,
        affinity: Option<&str>,
        bytes: usize,
        telemetry: &[ProviderTelemetrySnapshot],
        rate: &[RateSnapshot],
    ) -> Result<AutoRoutePlan, ProviderBridgeError> {
        if targets.is_empty() || targets.len() > MAX_TARGETS || !mode.valid() {
            return Err(ProviderBridgeError::InvalidTargets);
        }
        // Only an authorized known affinity can carry continuity/switching.
        let affinity = affinity.filter(|id| targets.iter().any(|t| t.provider_id == *id));
        let mut ids = BTreeSet::new();
        let mut variants = BTreeMap::new();
        for (ordinal, target) in targets.iter().enumerate() {
            if !target.invocation.valid() || !ids.insert(&target.provider_id) {
                return Err(ProviderBridgeError::InvalidTargets);
            }
            let entry = registry
                .get(&target.provider_id)
                .ok_or(ProviderBridgeError::NoEligibleCandidates)?;
            let id = ResourceId::new(&target.provider_id)
                .map_err(|_| ProviderBridgeError::InvalidTargets)?;
            let resource = self
                .catalog
                .resource(&id)
                .ok_or(ProviderBridgeError::CatalogRuntimeMismatch)?;
            check_runtime_binding(resource, entry)?;
            let explicit = AllocationCandidate::new(
                resource,
                ModelId::new(&target.invocation.model)
                    .map_err(|_| ProviderBridgeError::InvalidTargets)?,
                target
                    .invocation
                    .thinking_level
                    .map(EffortId::from_thinking_level),
            )
            .map_err(|_| ProviderBridgeError::InvalidAllocationUniverse)?;
            insert_variant(&mut variants, explicit, ordinal)?;
            if runtime_policy.policy().variant_selection_mode == VariantSelectionMode::Auto {
                for model in resource.models.value().into_iter().flatten() {
                    insert_variant(
                        &mut variants,
                        AllocationCandidate::new(resource, model.id.clone(), None)
                            .map_err(|_| ProviderBridgeError::InvalidAllocationUniverse)?,
                        ordinal,
                    )?;
                    for effort in model.supported_efforts.value().into_iter().flatten() {
                        insert_variant(
                            &mut variants,
                            AllocationCandidate::new(
                                resource,
                                model.id.clone(),
                                Some(effort.id.clone()),
                            )
                            .map_err(|_| ProviderBridgeError::InvalidAllocationUniverse)?,
                            ordinal,
                        )?;
                    }
                }
            }
        }
        // Full expansion bound checked before any adapter compatibility/scoring.
        // Unsupported efforts count too; no truncation or hidden overflow.
        let requirements = provider_requirements(required, runtime_policy.minimum_cognitive_tier());
        let candidates = variants.values().map(|(c, _)| c.clone()).collect();
        let b1 = AllocationRequest::new(
            requirements.clone(),
            runtime_policy.policy().clone(),
            candidates,
        )
        .map_err(|_| ProviderBridgeError::InvalidAllocationUniverse)?;
        let report = b1.evaluate();
        let mut resolved = Vec::new();
        let mut exclusions = Vec::new();
        let mut selected_targets = BTreeMap::new();
        let mut resource_models: BTreeMap<ResourceId, BTreeSet<ModelId>> = BTreeMap::new();
        for evidence in report.candidates() {
            let variant = evidence.variant();
            let (candidate, ordinal) = &variants[variant];
            let original = &targets[*ordinal];
            let thinking = match variant
                .effort
                .as_ref()
                .map(EffortId::try_thinking_level)
                .transpose()
            {
                Ok(value) => value,
                Err(_) => {
                    exclusions.push(ProviderVariantExclusion {
                        variant: variant.clone(),
                        reason: ProviderVariantExclusionReason::EffortBridgeUnsupported,
                    });
                    continue;
                }
            };
            let mut target = original.clone();
            target.invocation.model = variant.model_id.as_str().into();
            target.invocation.thinking_level = thinking;
            let entry = registry
                .get(&original.provider_id)
                .expect("authorized registered entry");
            let Some(proof) =
                OperationalVariantProof::prove(entry, candidate, &target, required, mode)
            else {
                exclusions.push(ProviderVariantExclusion {
                    variant: variant.clone(),
                    reason: ProviderVariantExclusionReason::RuntimeUnsupported,
                });
                continue;
            };
            let signals = provider_candidate_signals(
                *ordinal,
                entry.config.priority,
                &target.provider_id,
                affinity,
                bytes,
            )?;
            let Some(value) = ResolvedProviderCandidate::resolve(
                candidate.clone(),
                evidence.clone(),
                proof,
                signals,
                &target,
                mode,
            )?
            else {
                exclusions.push(ProviderVariantExclusion {
                    variant: variant.clone(),
                    reason: ProviderVariantExclusionReason::HardEligibility {
                        reasons: evidence.reasons().to_vec(),
                    },
                });
                continue;
            };
            resource_models
                .entry(variant.resource_id.clone())
                .or_default()
                .insert(variant.model_id.clone());
            selected_targets.insert(variant.clone(), target);
            resolved.push(value);
        }
        let mut contexts = Vec::new();
        for (id, models) in resource_models {
            let resource = self.catalog.resource(&id).expect("authorized descriptor");
            let ResourceOrigin::Provider(runtime) = &resource.origin else {
                return Err(ProviderBridgeError::CatalogRuntimeMismatch);
            };
            // Reject ambiguous DTOs rather than choosing by incidental order.
            if telemetry
                .iter()
                .filter(|s| s.provider_id == runtime.as_str())
                .count()
                > 1
                || rate
                    .iter()
                    .filter(|s| s.provider_id == runtime.as_str())
                    .count()
                    > 1
            {
                return Err(ProviderBridgeError::InvalidEconomicEvidence);
            }
            contexts.push(
                EconomicContext::capture_provider_models(
                    resource,
                    &models,
                    telemetry.iter().find(|s| s.provider_id == runtime.as_str()),
                    rate.iter().find(|s| s.provider_id == runtime.as_str()),
                )
                .map_err(|e| {
                    if e == ScoringError::Lr8ContextMismatch {
                        ProviderBridgeError::Lr8ContextMismatch
                    } else {
                        ProviderBridgeError::InvalidEconomicEvidence
                    }
                })?,
            );
        }
        let decision = AllocationScoringRequest::from_provider_candidates(
            runtime_policy.policy().clone(),
            requirements,
            resolved,
            contexts,
        )
        .map_err(|_| ProviderBridgeError::InvalidEconomicEvidence)?
        .decide();
        for excluded in &decision.excluded_candidates {
            if let EconomicEligibility::Excluded(reasons) = &excluded.evidence.economic_eligibility
            {
                exclusions.push(ProviderVariantExclusion {
                    variant: excluded.variant.clone(),
                    reason: ProviderVariantExclusionReason::Economic {
                        reasons: reasons.clone(),
                    },
                });
            }
        }
        let mut seen = BTreeSet::new();
        let entries = decision
            .ranked_candidates
            .iter()
            .filter_map(|ranked| {
                let target = &selected_targets[&ranked.variant];
                seen.insert(target.provider_id.clone())
                    .then(|| AutoRouteEntry {
                        target: target.clone(),
                        variant: ranked.variant.clone(),
                        score: ranked.score_breakdown.total,
                    })
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Err(ProviderBridgeError::NoEligibleCandidates);
        }
        exclusions.sort_by(|a, b| a.variant.cmp(&b.variant));
        Ok(AutoRoutePlan {
            entries,
            exclusions,
        })
    }
}
fn insert_variant<'a>(
    variants: &mut BTreeMap<AllocationVariant, (AllocationCandidate<'a>, usize)>,
    candidate: AllocationCandidate<'a>,
    ordinal: usize,
) -> Result<(), ProviderBridgeError> {
    variants
        .entry(candidate.variant())
        .or_insert((candidate, ordinal));
    if variants.len() > MAX_ALLOCATION_CANDIDATES {
        return Err(ProviderBridgeError::ExpansionOverflow);
    }
    Ok(())
}
fn check_runtime_binding(
    resource: &CognitiveResource,
    entry: &ProviderEntry,
) -> Result<(), ProviderBridgeError> {
    if resource.identity.id.as_str() != entry.config.id
        || resource.identity.class != ResourceClass::CognitiveProvider
        || !matches!(&resource.origin, ResourceOrigin::Provider(id) if id.as_str() == entry.config.id)
        || resource
            .enabled
            .value()
            .is_some_and(|v| *v != entry.config.enabled)
    {
        return Err(ProviderBridgeError::CatalogRuntimeMismatch);
    }
    let runtime = CapabilitySet::from_provider(entry.config.capabilities);
    for capability in [
        CognitiveCapability::TextGeneration,
        CognitiveCapability::Streaming,
        CognitiveCapability::Vision,
        CognitiveCapability::ToolCalling,
        CognitiveCapability::StructuredOutput,
    ] {
        let fact = resource.capabilities.get(capability);
        if fact.value().is_some() && fact.value() != runtime.get(capability).value() {
            return Err(ProviderBridgeError::CatalogRuntimeMismatch);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "provider_bridge_tests.rs"]
mod tests;
