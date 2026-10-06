//! B1 only answers whether evidence proves eligibility. It never chooses a winner.
//! Core/policy supplies the authorized universe; this module cannot discover or
//! authorize resources, call adapters, or replace operational runtime gates.
use super::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Local structural bounds, without commercial meaning.
pub const MAX_ALLOCATION_CANDIDATES: usize = 256;
pub const MAX_CAPABILITY_REQUIREMENTS: usize = 20;
/// Reuse the bounded LR-8.5A ordinal; no separate quality scale.
pub type QualityFloor = CognitiveTier;
/// `micros` is an authorized ceiling, not spend or remaining balance. No FX.
pub type PaidBudget = MonetaryAmount;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationError {
    TooManyRequirements,
    DuplicateRequirement,
    InvalidReserveThresholds,
    TooManyCandidates,
    ConflictingResourceSnapshot,
    ConflictingRuntimeBinding,
    DuplicateCandidate,
    InvalidCandidate,
}
impl std::fmt::Display for AllocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}") // Never echo rejected IDs, facts or task content.
    }
}
impl std::error::Error for AllocationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationProfile {
    Economy,
    Balanced,
    Fast,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VariantSelectionMode {
    Explicit,
    Auto,
}
/// Configuration only. Even Allow does not authorize execution/spend in B1.
/// Carrying the validated budget in the variant makes missing currency/budget
/// pairs and Deny-with-budget configurations unrepresentable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaidUsePolicy {
    Deny,
    AllowKnownCostWithinBudget { budget: PaidBudget },
}
impl Default for PaidUsePolicy {
    fn default() -> Self {
        Self::Deny
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScarcityState {
    Comfortable,
    Reduced,
    Reserve,
    Exhausted,
    Unknown,
}
/// Local thresholds only; B1 does not derive scarcity or impose reserve gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservePolicy {
    reduced_below_percent: u8,
    reserve_below_percent: u8,
}
impl ReservePolicy {
    pub fn new(reduced: u8, reserve: u8) -> Result<Self, AllocationError> {
        if reserve > reduced || reduced > 100 {
            return Err(AllocationError::InvalidReserveThresholds);
        }
        Ok(Self {
            reduced_below_percent: reduced,
            reserve_below_percent: reserve,
        })
    }
    pub fn reduced_below_percent(self) -> u8 {
        self.reduced_below_percent
    }
    pub fn reserve_below_percent(self) -> u8 {
        self.reserve_below_percent
    }
}
/// All components are validated by construction. No weights or formulas in B1.
/// Aggregate deserialization/persistence is deliberately deferred to B4.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationPolicy {
    pub profile: AllocationProfile,
    pub variant_selection_mode: VariantSelectionMode,
    pub paid_use: PaidUsePolicy,
    pub reserve: Option<ReservePolicy>,
}
impl Default for AllocationPolicy {
    fn default() -> Self {
        Self {
            profile: AllocationProfile::Balanced,
            variant_selection_mode: VariantSelectionMode::Explicit,
            paid_use: PaidUsePolicy::Deny,
            reserve: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityScope {
    /// Structural/runtime proof only; model facts do not define this layer.
    Runtime,
    /// Model true is required. Resource true never substitutes for model proof.
    /// Explicit resource false vetoes execution even if the model says true.
    /// Resource Unknown does not negate model proof; require Runtime as well
    /// when positive evidence at both layers is necessary.
    Model,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRequirement {
    pub capability: CognitiveCapability,
    pub scope: CapabilityScope,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRequirements {
    required_capabilities: Vec<CapabilityRequirement>,
    minimum_cognitive_tier: Option<QualityFloor>,
}
impl CandidateRequirements {
    pub fn new(
        mut capabilities: Vec<CapabilityRequirement>,
        minimum_cognitive_tier: Option<QualityFloor>,
    ) -> Result<Self, AllocationError> {
        if capabilities.len() > MAX_CAPABILITY_REQUIREMENTS {
            return Err(AllocationError::TooManyRequirements);
        }
        capabilities.sort(); // Canonical enum order, never ranking candidates.
        if capabilities.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(AllocationError::DuplicateRequirement);
        }
        Ok(Self {
            required_capabilities: capabilities,
            minimum_cognitive_tier,
        })
    }
    pub fn required_capabilities(&self) -> &[CapabilityRequirement] {
        &self.required_capabilities
    }
    pub fn minimum_cognitive_tier(&self) -> Option<QualityFloor> {
        self.minimum_cognitive_tier
    }
}

/// Complete variant identity. Labels are public/local, never account IDs/secrets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationVariant {
    pub resource_id: ResourceId,
    pub access_path: AccessPath,
    pub billing_domain_id: BillingDomainId,
    pub model_id: ModelId,
    pub effort: Option<EffortId>,
}
/// Borrows the LR-8.5A descriptor instead of duplicating CognitiveResource or
/// accepting a detached ModelProfile whose membership could be fabricated.
/// Unknown catalogs and absent selections remain representable for explainability.
/// No global catalog/registry is accepted, and construction does not authorize use.
#[derive(Clone, Debug)]
pub struct AllocationCandidate<'a> {
    resource: &'a CognitiveResource,
    model_id: ModelId,
    effort: Option<EffortId>,
}
impl<'a> AllocationCandidate<'a> {
    pub fn new(
        resource: &'a CognitiveResource,
        model_id: ModelId,
        effort: Option<EffortId>,
    ) -> Result<Self, AllocationError> {
        resource
            .validate()
            .map_err(|_| AllocationError::InvalidCandidate)?;
        Ok(Self {
            resource,
            model_id,
            effort,
        })
    }
    pub fn resource(&self) -> &'a CognitiveResource {
        self.resource
    }
    pub fn variant(&self) -> AllocationVariant {
        AllocationVariant {
            resource_id: self.resource.identity.id.clone(),
            access_path: self.resource.identity.access_path.clone(),
            billing_domain_id: self.resource.identity.billing_domain.id.clone(),
            model_id: self.model_id.clone(),
            effort: self.effort.clone(),
        }
    }
}

/// ONLY candidates already authorized by Core/policy may be supplied. The type
/// bounds/validates input, but cannot prove the caller's authorization. B3 owns
/// that builder. Empty authorized universes yield empty reports, never discovery.
#[derive(Clone, Debug)]
pub struct AllocationRequest<'a> {
    requirements: CandidateRequirements,
    policy: AllocationPolicy,
    candidates: Vec<AllocationCandidate<'a>>,
}
impl<'a> AllocationRequest<'a> {
    /// Immutable bridges for pure B2. The request owns the validated universe;
    /// B2 must join by full variant identity and require a B1 Eligible result.
    pub fn candidates(&self) -> &[AllocationCandidate<'a>] {
        &self.candidates
    }
    pub fn requirements(&self) -> &CandidateRequirements {
        &self.requirements
    }
    pub fn policy(&self) -> &AllocationPolicy {
        &self.policy
    }
    pub fn new(
        requirements: CandidateRequirements,
        policy: AllocationPolicy,
        candidates: Vec<AllocationCandidate<'a>>,
    ) -> Result<Self, AllocationError> {
        if candidates.len() > MAX_ALLOCATION_CANDIDATES {
            return Err(AllocationError::TooManyCandidates);
        }
        // Validate the whole authorized universe in fixed phases: bounds,
        // complete snapshot coherence, runtime bindings, duplicate variants.
        // Equality includes identity, all facts, provenance and timestamps;
        // cloned equal descriptors are accepted without pointer identity.
        let mut resources: BTreeMap<&ResourceId, &CognitiveResource> = BTreeMap::new();
        for candidate in &candidates {
            let resource = candidate.resource;
            if let Some(existing) = resources.get(&resource.identity.id) {
                if *existing != resource {
                    return Err(AllocationError::ConflictingResourceSnapshot);
                }
            } else {
                resources.insert(&resource.identity.id, resource);
            }
        }
        // One non-local runtime is one credential/quota context, exactly as in
        // ResourceCatalog. Provider and Agent IDs are separate namespaces.
        // Local origins and shared BillingDomains impose no uniqueness here.
        let mut providers = BTreeSet::new();
        let mut agents = BTreeSet::new();
        for resource in resources.values() {
            let (bindings, id) = match &resource.origin {
                ResourceOrigin::Provider(id) => (&mut providers, id),
                ResourceOrigin::Agent(id) => (&mut agents, id),
                ResourceOrigin::Local => continue,
            };
            if !bindings.insert(id) {
                return Err(AllocationError::ConflictingRuntimeBinding);
            }
        }
        let mut seen = BTreeSet::new();
        for candidate in &candidates {
            if !seen.insert(candidate.variant()) {
                return Err(AllocationError::DuplicateCandidate);
            }
        }
        Ok(Self {
            requirements,
            policy,
            candidates,
        })
    }
    /// Pure, synchronous and clock-free. Preserves caller order; no winner,
    /// ranking, filtering of unresolved entries or operational/spend permission.
    pub fn evaluate(&self) -> EligibilityReport {
        EligibilityReport {
            requirements: self.requirements.clone(),
            policy: self.policy.clone(),
            candidates: self
                .candidates
                .iter()
                .map(|candidate| evaluate_candidate(candidate, &self.requirements))
                .collect(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EligibilityStatus {
    Eligible,
    Ineligible,
    Unresolved,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLayer {
    Resource,
    Model,
    Effort,
}
/// Bounded enum payloads only; never prompt, raw error or free-form rationale.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EligibilityReason {
    ResourceDisabled,
    ResourceEnabledUnknown,
    AvailabilityUnavailable {
        layer: EvidenceLayer,
    },
    AvailabilityUnknown {
        layer: EvidenceLayer,
    },
    ModelNotSupported,
    ModelSupportUnknown,
    EffortNotSupported,
    EffortSupportUnknown,
    CapabilityUnsupported {
        requirement: CapabilityRequirement,
        layer: EvidenceLayer,
    },
    CapabilityUnknown {
        requirement: CapabilityRequirement,
        layer: EvidenceLayer,
    },
    CognitiveTierBelowFloor,
    CognitiveTierUnknown,
}
impl EligibilityReason {
    fn is_contradiction(&self) -> bool {
        matches!(
            self,
            Self::ResourceDisabled
                | Self::AvailabilityUnavailable { .. }
                | Self::ModelNotSupported
                | Self::EffortNotSupported
                | Self::CapabilityUnsupported { .. }
                | Self::CognitiveTierBelowFloor
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEvidence {
    pub requirement: CapabilityRequirement,
    pub resource: CatalogFact<bool>,
    pub model: CatalogFact<bool>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityEvidence {
    pub resource: CatalogFact<Availability>,
    pub model: CatalogFact<Availability>,
    /// None means no effort requested, not an invented default effort.
    pub effort: Option<CatalogFact<Availability>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveTier {
    pub fact: CatalogFact<CognitiveTier>,
    /// None iff no tier was proven. Source facts remain untouched.
    pub layer: Option<EvidenceLayer>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateEligibility {
    variant: AllocationVariant,
    resource_class: ResourceClass,
    status: EligibilityStatus,
    reasons: Vec<EligibilityReason>,
    enabled_evidence: CatalogFact<bool>,
    model_support: CatalogFact<bool>,
    effort_support: Option<CatalogFact<bool>>,
    effective_tier: EffectiveTier,
    capability_evidence: Vec<CapabilityEvidence>,
    availability_evidence: AvailabilityEvidence,
}
impl CandidateEligibility {
    pub fn variant(&self) -> &AllocationVariant {
        &self.variant
    }
    pub fn status(&self) -> EligibilityStatus {
        self.status
    }
    pub fn reasons(&self) -> &[EligibilityReason] {
        &self.reasons
    }
    pub fn effective_tier(&self) -> &EffectiveTier {
        &self.effective_tier
    }
    pub fn capability_evidence(&self) -> &[CapabilityEvidence] {
        &self.capability_evidence
    }
    pub fn availability_evidence(&self) -> &AvailabilityEvidence {
        &self.availability_evidence
    }
}
/// Output collections are private and produced only from bounded inputs. This
/// is the B1 decision/exclusion surface; unresolved is not permission to invoke.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EligibilityReport {
    requirements: CandidateRequirements,
    policy: AllocationPolicy,
    candidates: Vec<CandidateEligibility>,
}
impl EligibilityReport {
    pub fn candidates(&self) -> &[CandidateEligibility] {
        &self.candidates
    }
}

fn membership<T>(catalog: &CatalogFact<Vec<T>>, found: bool) -> CatalogFact<bool> {
    match catalog {
        CatalogFact::Unknown => CatalogFact::Unknown,
        CatalogFact::Known {
            provenance,
            observed_at_unix_ms,
            ..
        } => CatalogFact::Known {
            value: found,
            provenance: *provenance,
            observed_at_unix_ms: *observed_at_unix_ms,
        },
    }
}
fn availability_gate(
    fact: &CatalogFact<Availability>,
    layer: EvidenceLayer,
    reasons: &mut Vec<EligibilityReason>,
) {
    match fact.value() {
        Some(Availability::Available) => {}
        Some(Availability::Unavailable) => {
            reasons.push(EligibilityReason::AvailabilityUnavailable { layer });
        }
        None => reasons.push(EligibilityReason::AvailabilityUnknown { layer }),
    }
}
fn evaluate_candidate(
    candidate: &AllocationCandidate<'_>,
    requirements: &CandidateRequirements,
) -> CandidateEligibility {
    let resource = candidate.resource;
    let model = resource.model(&candidate.model_id).ok();
    let effort = candidate
        .effort
        .as_ref()
        .and_then(|id| model.and_then(|model| model.effort(id).ok()));
    let mut reasons = Vec::new();
    // Stable order: enabled, resource availability, model support/availability,
    // effort support/availability, canonical capabilities (resource then model),
    // quality. Contradictions dominate missing proof without discarding reasons.
    match resource.enabled.value() {
        Some(true) => {}
        Some(false) => reasons.push(EligibilityReason::ResourceDisabled),
        None => reasons.push(EligibilityReason::ResourceEnabledUnknown),
    }
    let availability_evidence = AvailabilityEvidence {
        resource: resource.availability.clone(),
        model: model.map_or(CatalogFact::Unknown, |model| model.availability.clone()),
        effort: candidate
            .effort
            .as_ref()
            .map(|_| effort.map_or(CatalogFact::Unknown, |effort| effort.availability.clone())),
    };
    availability_gate(
        &availability_evidence.resource,
        EvidenceLayer::Resource,
        &mut reasons,
    );
    let model_support = membership(&resource.models, model.is_some());
    match model_support.value() {
        Some(true) => availability_gate(
            &availability_evidence.model,
            EvidenceLayer::Model,
            &mut reasons,
        ),
        Some(false) => reasons.push(EligibilityReason::ModelNotSupported),
        None => reasons.push(EligibilityReason::ModelSupportUnknown),
    }
    let effort_support = candidate.effort.as_ref().map(|_| {
        model.map_or(CatalogFact::Unknown, |model| {
            membership(&model.supported_efforts, effort.is_some())
        })
    });
    if let Some(support) = &effort_support {
        match support.value() {
            Some(true) => availability_gate(
                availability_evidence
                    .effort
                    .as_ref()
                    .expect("selected effort"),
                EvidenceLayer::Effort,
                &mut reasons,
            ),
            Some(false) => reasons.push(EligibilityReason::EffortNotSupported),
            None => reasons.push(EligibilityReason::EffortSupportUnknown),
        }
    }
    let capability_evidence = requirements
        .required_capabilities
        .iter()
        .map(|required| {
            let resource_fact = resource.capabilities.get(required.capability);
            let model_fact = model.map_or(CatalogFact::Unknown, |model| {
                model.capabilities.get(required.capability)
            });
            let mut check = |fact: &CatalogFact<bool>, layer| match fact.value() {
                Some(true) => {}
                Some(false) => reasons.push(EligibilityReason::CapabilityUnsupported {
                    requirement: *required,
                    layer,
                }),
                None => reasons.push(EligibilityReason::CapabilityUnknown {
                    requirement: *required,
                    layer,
                }),
            };
            match required.scope {
                CapabilityScope::Runtime => check(&resource_fact, EvidenceLayer::Resource),
                CapabilityScope::Model => {
                    if resource_fact.value() == Some(&false) {
                        check(&resource_fact, EvidenceLayer::Resource);
                    }
                    check(&model_fact, EvidenceLayer::Model);
                }
            }
            CapabilityEvidence {
                requirement: *required,
                resource: resource_fact,
                model: model_fact,
            }
        })
        .collect();
    // Effort assessment is more specific, not additive. Unknown effort tier
    // permits descriptive fallback to model tier, without rewriting either fact.
    let effective_tier = if let Some(effort) =
        effort.filter(|e| e.facts.cognitive_tier.value().is_some())
    {
        EffectiveTier {
            fact: effort.facts.cognitive_tier.clone(),
            layer: Some(EvidenceLayer::Effort),
        }
    } else if let Some(model) = model.filter(|m| m.facts.execution.cognitive_tier.value().is_some())
    {
        EffectiveTier {
            fact: model.facts.execution.cognitive_tier.clone(),
            layer: Some(EvidenceLayer::Model),
        }
    } else {
        EffectiveTier {
            fact: CatalogFact::Unknown,
            layer: None,
        }
    };
    if let Some(floor) = requirements.minimum_cognitive_tier {
        match effective_tier.fact.value() {
            Some(tier) if *tier >= floor => {}
            Some(_) => reasons.push(EligibilityReason::CognitiveTierBelowFloor),
            None => reasons.push(EligibilityReason::CognitiveTierUnknown),
        }
    }
    let status = if reasons.iter().any(EligibilityReason::is_contradiction) {
        EligibilityStatus::Ineligible
    } else if reasons.is_empty() {
        EligibilityStatus::Eligible
    } else {
        EligibilityStatus::Unresolved
    };
    CandidateEligibility {
        variant: candidate.variant(),
        resource_class: resource.identity.class,
        status,
        reasons,
        enabled_evidence: resource.enabled.clone(),
        model_support,
        effort_support,
        effective_tier,
        capability_evidence,
        availability_evidence,
    }
}
