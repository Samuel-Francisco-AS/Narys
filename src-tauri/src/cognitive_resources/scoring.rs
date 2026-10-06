//! Pure B2: B1 Eligible -> economic guards -> bounded integer score -> rank.
//! This API accepts neither ProviderError nor partial-output state. A 429/retry
//! hint cannot change PaidUsePolicy. B3 must revalidate every runtime gate.
use super::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_PREFERENCE_ORDINAL: u16 = 255;
pub const REGISTRY_PRIORITY_CAP: u16 = 32;
/// Secondary utility 0..=3, below one policy ordinal step in every profile.
pub const REGISTRY_UTILITY_CAP: i64 = 3;
pub const MAX_CONTINUITY_SIGNAL: u16 = 100;
pub const MAX_SWITCHING_SIGNAL: u16 = 100;
pub const LATENCY_BUCKET_MS: u64 = 100;
pub const MAX_LATENCY_BUCKET: u64 = 100;
pub const MONETARY_NORMALIZATION: u64 = 100;

/// Core policy weights, not prices/measurements. Every numeric influence is
/// explicit here. No score for surplus cognitive tier. Scarcity dominates small
/// continuity advantages in Economy/Balanced; Fast can trade factual continuity
/// and latency for Reserve, never for exhaustion or spend-guard failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileWeights {
    pub policy_preference: i64,
    pub registry_preference: i64,
    pub continuity: i64,
    pub switching: i64,
    pub relative_cost: i64,
    pub reduced: i64,
    pub reserve: i64,
    pub monetary_cost: i64,
    pub latency: i64,
}
pub const ECONOMY_WEIGHTS: ProfileWeights = ProfileWeights {
    policy_preference: 4,
    registry_preference: 1,
    continuity: 2,
    switching: 2,
    relative_cost: 12,
    reduced: 600,
    reserve: 1800,
    monetary_cost: 10,
    latency: 2,
};
pub const BALANCED_WEIGHTS: ProfileWeights = ProfileWeights {
    policy_preference: 6,
    registry_preference: 1,
    continuity: 4,
    switching: 4,
    relative_cost: 6,
    reduced: 300,
    reserve: 900,
    monetary_cost: 5,
    latency: 6,
};
pub const FAST_WEIGHTS: ProfileWeights = ProfileWeights {
    policy_preference: 6,
    registry_preference: 1,
    continuity: 12,
    switching: 12,
    relative_cost: 2,
    reduced: 100,
    reserve: 300,
    monetary_cost: 1,
    latency: 15,
};
impl AllocationProfile {
    pub fn scoring_weights(self) -> ProfileWeights {
        match self {
            Self::Economy => ECONOMY_WEIGHTS,
            Self::Balanced => BALANCED_WEIGHTS,
            Self::Fast => FAST_WEIGHTS,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSignals {
    preference_ordinal: Option<u16>,
    registry_priority: Option<u16>,
    continuity: Option<u16>,
    switching_cost: Option<u16>,
}
impl CandidateSignals {
    /// Lower ordinal/priority is preferred. Continuity and switching are local
    /// relative signals 0..=100, never money, capability or quality evidence.
    /// None is neutral. No registry or Scheduler is read by this constructor.
    pub fn new(
        preference_ordinal: Option<u16>,
        registry_priority: Option<u16>,
        continuity: Option<u16>,
        switching_cost: Option<u16>,
    ) -> Result<Self, ScoringError> {
        if preference_ordinal.is_some_and(|v| v > MAX_PREFERENCE_ORDINAL)
            || registry_priority.is_some_and(|v| v > REGISTRY_PRIORITY_CAP)
            || continuity.is_some_and(|v| v > MAX_CONTINUITY_SIGNAL)
            || switching_cost.is_some_and(|v| v > MAX_SWITCHING_SIGNAL)
        {
            return Err(ScoringError::InvalidSignals);
        }
        Ok(Self {
            preference_ordinal,
            registry_priority,
            continuity,
            switching_cost,
        })
    }
    pub fn preference_ordinal(self) -> Option<u16> {
        self.preference_ordinal
    }
    pub fn registry_priority(self) -> Option<u16> {
        self.registry_priority
    }
    pub fn continuity(self) -> Option<u16> {
        self.continuity
    }
    pub fn switching_cost(self) -> Option<u16> {
        self.switching_cost
    }
}
#[derive(Clone, Debug)]
pub struct ScoringCandidate {
    pub variant: AllocationVariant,
    pub signals: CandidateSignals,
}
struct ScoringInput<'a> {
    candidate: AllocationCandidate<'a>,
    eligibility: CandidateEligibility,
    signals: CandidateSignals,
}
/// Select a subset of the already authorized B1 universe by explicit variant
/// identity. B1 is evaluated by this constructor; detached/fabricated reports
/// cannot be paired with different descriptors. Non-Eligible input is an error,
/// never upgraded, silently filtered or made attractive by low price.
pub struct AllocationScoringRequest<'a> {
    policy: AllocationPolicy,
    requirements: CandidateRequirements,
    inputs: Vec<ScoringInput<'a>>,
    contexts: BTreeMap<ResourceId, EconomicContext>,
}
impl<'a> AllocationScoringRequest<'a> {
    pub fn new(
        b1: &AllocationRequest<'a>,
        selected: Vec<ScoringCandidate>,
        contexts: Vec<EconomicContext>,
    ) -> Result<Self, ScoringError> {
        if selected.len() > MAX_ALLOCATION_CANDIDATES {
            return Err(ScoringError::TooManyCandidates);
        }
        if contexts.len() > MAX_RESOURCES {
            return Err(ScoringError::TooManyContexts);
        }
        let report = b1.evaluate();
        let reports: BTreeMap<_, _> = report
            .candidates()
            .iter()
            .map(|e| (e.variant(), e))
            .collect();
        let candidates: BTreeMap<_, _> = b1.candidates().iter().map(|c| (c.variant(), c)).collect();
        let mut seen = BTreeSet::new();
        let mut inputs = Vec::new();
        for selection in selected {
            if !seen.insert(selection.variant.clone()) {
                return Err(ScoringError::DuplicateCandidate);
            }
            let eligibility = reports
                .get(&selection.variant)
                .ok_or(ScoringError::CandidateNotInB1Universe)?;
            match eligibility.status() {
                EligibilityStatus::Unresolved => return Err(ScoringError::B1Unresolved),
                EligibilityStatus::Ineligible => return Err(ScoringError::B1Ineligible),
                EligibilityStatus::Eligible => {}
            }
            inputs.push(ScoringInput {
                candidate: (*candidates
                    .get(&selection.variant)
                    .ok_or(ScoringError::CandidateNotInB1Universe)?)
                .clone(),
                eligibility: (*eligibility).clone(),
                signals: selection.signals,
            });
        }
        let resources: BTreeMap<_, _> = inputs
            .iter()
            .map(|i| (&i.candidate.resource().identity.id, i.candidate.resource()))
            .collect();
        // Equal shared domain descriptors are valid. Any structural economic
        // difference (including provenance/timestamp/order) fails the request.
        let mut domains = BTreeMap::new();
        for resource in resources.values() {
            if let Some(prior) =
                domains.insert(&resource.identity.billing_domain.id, &resource.economics)
            {
                if prior != &resource.economics {
                    return Err(ScoringError::ConflictingBillingDomainFacts);
                }
            }
        }
        let mut joined = BTreeMap::new();
        for context in contexts {
            let id = &context.descriptor().identity.id;
            if joined.contains_key(id) {
                return Err(ScoringError::DuplicateEconomicContext);
            }
            let descriptor = resources
                .get(id)
                .ok_or(ScoringError::UnexpectedEconomicContext)?;
            if *descriptor != context.descriptor() {
                return Err(ScoringError::ConflictingResourceSnapshot);
            }
            joined.insert(id.clone(), context);
        }
        // Canonical iteration makes evidence/exclusion order independent of the
        // caller's incidental Vec order. Explicit policy ordinal stays a signal.
        inputs.sort_by_key(|i| i.candidate.variant());
        Ok(Self {
            policy: b1.policy().clone(),
            requirements: b1.requirements().clone(),
            inputs,
            contexts: joined,
        })
    }
    pub fn decide(&self) -> AllocationDecision {
        let mut ranked_candidates = Vec::new();
        let mut excluded_candidates = Vec::new();
        for input in &self.inputs {
            let candidate = &input.candidate;
            let variant = candidate.variant();
            let resolved = ResolvedExecutionFacts::for_candidate(candidate);
            let pressure = super::scarcity::assess_pressure(
                self.contexts
                    .get(&variant.resource_id)
                    .and_then(EconomicContext::lr8),
                &variant.model_id,
                self.policy.reserve,
            );
            let (facts, scarcity, spend, mut reasons) = match resolved {
                Ok(facts) => {
                    // Quality resolution must agree with B1, without rerunning
                    // the quality gate or rewarding excess capacity.
                    debug_assert_eq!(
                        facts.cognitive_tier.fact,
                        input.eligibility.effective_tier().fact
                    );
                    debug_assert_eq!(
                        facts.cognitive_tier.layer,
                        input.eligibility.effective_tier().layer
                    );
                    let scarcity = super::scarcity::assess_allowances(
                        &facts,
                        &candidate.resource().economics,
                        self.policy.reserve,
                    );
                    let spend = super::spend::assess_spend(
                        &candidate.resource().economics,
                        &facts.monetary_cost,
                        &self.policy.paid_use,
                    );
                    let mut reasons = Vec::new();
                    if scarcity.summary.known_worst == Some(ScarcityState::Exhausted) {
                        reasons.push(EconomicExclusion::AllowanceExhausted);
                    }
                    (Some(facts), scarcity, Some(spend), reasons)
                }
                Err(reason) => (None, ScarcityAssessment::default(), None, vec![reason]),
            };
            if pressure.summary.known_worst == Some(ScarcityState::Exhausted) {
                reasons.push(EconomicExclusion::Lr8ConstraintSaturated);
            }
            if let Some(spend) = &spend {
                reasons.extend_from_slice(&spend.exclusions);
            }
            let economic_eligibility = if reasons.is_empty() {
                EconomicEligibility::Eligible
            } else {
                EconomicEligibility::Excluded(reasons)
            };
            let evidence = CandidateAssessment {
                b1: input.eligibility.clone(),
                signals: input.signals,
                resolved_facts: facts,
                scarcity,
                operational_pressure: pressure,
                spend,
                economic_eligibility,
            };
            if evidence.economic_eligibility == EconomicEligibility::Eligible {
                let score_breakdown = score(&evidence, self.policy.profile.scoring_weights());
                ranked_candidates.push(RankedCandidate {
                    variant,
                    evidence,
                    score_breakdown,
                    order_over_next: None,
                });
            } else {
                excluded_candidates.push(ExcludedCandidate { variant, evidence });
            }
        }
        ranked_candidates.sort_by(|a, b| {
            b.score_breakdown
                .total
                .cmp(&a.score_breakdown.total)
                .then_with(|| ordinal_key(a.evidence.signals).cmp(&ordinal_key(b.evidence.signals)))
                .then_with(|| a.variant.cmp(&b.variant))
        });
        for n in 0..ranked_candidates.len().saturating_sub(1) {
            ranked_candidates[n].order_over_next = Some(order_reason(
                &ranked_candidates[n],
                &ranked_candidates[n + 1],
            ));
        }
        let tie_break = match ranked_candidates.as_slice() {
            [] => TieBreakReason::NoEconomicCandidate,
            [_] => TieBreakReason::OnlyEconomicCandidate,
            [first, second, ..] => order_reason(first, second),
        };
        AllocationDecision {
            winner: ranked_candidates.first().map(|c| c.variant.clone()),
            ranked_candidates,
            excluded_candidates,
            policy: self.policy.clone(),
            requirements: self.requirements.clone(),
            weights: self.policy.profile.scoring_weights(),
            tie_break,
        }
    }
}
fn ordinal_key(signals: CandidateSignals) -> (bool, u16) {
    // Absence has no preference proof and follows all explicit ordinals on ties.
    (
        signals.preference_ordinal.is_none(),
        signals.preference_ordinal.unwrap_or(0),
    )
}
fn order_reason(a: &RankedCandidate, b: &RankedCandidate) -> TieBreakReason {
    if a.score_breakdown.total != b.score_breakdown.total {
        TieBreakReason::HigherTotal
    } else if ordinal_key(a.evidence.signals) != ordinal_key(b.evidence.signals) {
        TieBreakReason::PolicyOrdinal
    } else {
        TieBreakReason::CanonicalVariant
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TieBreakReason {
    NoEconomicCandidate,
    OnlyEconomicCandidate,
    HigherTotal,
    PolicyOrdinal,
    CanonicalVariant,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateAssessment {
    pub b1: CandidateEligibility,
    pub signals: CandidateSignals,
    pub resolved_facts: Option<ResolvedExecutionFacts>,
    pub scarcity: ScarcityAssessment,
    pub operational_pressure: OperationalPressure,
    pub spend: Option<SpendAssessment>,
    pub economic_eligibility: EconomicEligibility,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreComponent<T> {
    pub value: i64,
    pub evidence: T,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonetaryScoreEvidence {
    pub cost: ResolvedFact<MonetaryAmount>,
    pub budget: Option<PaidBudget>,
    pub comparable: bool,
    pub budget_percent: Option<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBreakdown {
    pub policy_preference: ScoreComponent<Option<u16>>,
    pub registry_preference: ScoreComponent<Option<u16>>,
    pub continuity: ScoreComponent<Option<u16>>,
    pub switching: ScoreComponent<Option<u16>>,
    pub relative_cost: ScoreComponent<ResolvedFact<RelativeCostTier>>,
    pub scarcity: ScoreComponent<ScarcitySummary>,
    pub monetary_cost: ScoreComponent<MonetaryScoreEvidence>,
    pub latency: ScoreComponent<ResolvedFact<u64>>,
    pub total: i64,
}
impl ScoreBreakdown {
    /// Inputs and named weights bound the reachable total far below i64 limits.
    /// Saturation is explicit defense; it cannot collapse ordering for any valid
    /// B2 input. Audit tests verify exact addition at all input maxima.
    pub fn component_sum(&self) -> i64 {
        [
            self.policy_preference.value,
            self.registry_preference.value,
            self.continuity.value,
            self.switching.value,
            self.relative_cost.value,
            self.scarcity.value,
            self.monetary_cost.value,
            self.latency.value,
        ]
        .into_iter()
        .fold(0i64, i64::saturating_add)
    }
}
fn component<T>(value: i64, evidence: T) -> ScoreComponent<T> {
    ScoreComponent { value, evidence }
}
fn score(e: &CandidateAssessment, w: ProfileWeights) -> ScoreBreakdown {
    let facts = e
        .resolved_facts
        .as_ref()
        .expect("economic Eligible requires resolved facts");
    let s = e.signals;
    let scarcity = e.scarcity.summary.combined(e.operational_pressure.summary);
    let monetary = e
        .spend
        .as_ref()
        .expect("economic Eligible requires spend assessment");
    let fraction = facts
        .monetary_cost
        .fact
        .value()
        .zip(monetary.budget.as_ref())
        .filter(|(c, b)| c.currency() == b.currency() && c.micros() <= b.micros())
        .map(|(c, b)| {
            if b.micros() == 0 {
                0
            } else {
                ((u128::from(c.micros()) * u128::from(MONETARY_NORMALIZATION))
                    / u128::from(b.micros()))
                .min(100) as u8
            }
        });
    let mut result = ScoreBreakdown {
        policy_preference: component(
            // Explicit ordinals have nonnegative utility; absence stays neutral.
            // MAX ties absence at zero, then the existing ordinal tie-break wins.
            s.preference_ordinal.map_or(0, |n| {
                i64::from(MAX_PREFERENCE_ORDINAL - n).saturating_mul(w.policy_preference)
            }),
            s.preference_ordinal,
        ),
        registry_preference: component(
            s.registry_priority.map_or(0, |n| {
                (i64::from(REGISTRY_PRIORITY_CAP - n).saturating_mul(REGISTRY_UTILITY_CAP)
                    / i64::from(REGISTRY_PRIORITY_CAP))
                .saturating_mul(w.registry_preference)
            }),
            s.registry_priority,
        ),
        continuity: component(
            s.continuity
                .map_or(0, |n| i64::from(n).saturating_mul(w.continuity)),
            s.continuity,
        ),
        switching: component(
            s.switching_cost
                .map_or(0, |n| -i64::from(n).saturating_mul(w.switching)),
            s.switching_cost,
        ),
        relative_cost: component(
            facts
                .relative_cost
                .fact
                .value()
                .map_or(0, |n| -i64::from(n.value()).saturating_mul(w.relative_cost)),
            facts.relative_cost.clone(),
        ),
        // Strongest factual dimension/constraint only, never unit sums or quota
        // double counting. Unknown flag stays visible and adds no penalty.
        scarcity: component(
            match scarcity.known_worst {
                Some(ScarcityState::Reduced) => -w.reduced,
                Some(ScarcityState::Reserve) => -w.reserve,
                _ => 0,
            },
            scarcity,
        ),
        monetary_cost: component(
            -i64::from(fraction.unwrap_or(0)).saturating_mul(w.monetary_cost),
            MonetaryScoreEvidence {
                cost: facts.monetary_cost.clone(),
                budget: monetary.budget.clone(),
                comparable: fraction.is_some(),
                budget_percent: fraction,
            },
        ),
        // Centered bucket utility [-100,100]. Unknown = 0, neither best nor
        // worst. Known 0ms is an observation, not a fabricated provider default.
        latency: component(
            facts.latency_ms.fact.value().map_or(0, |ms| {
                let bucket = (ms / LATENCY_BUCKET_MS).min(MAX_LATENCY_BUCKET) as i64;
                (MAX_LATENCY_BUCKET as i64 - bucket.saturating_mul(2)).saturating_mul(w.latency)
            }),
            facts.latency_ms.clone(),
        ),
        total: 0,
    };
    result.total = result.component_sum();
    result
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedCandidate {
    pub variant: AllocationVariant,
    pub evidence: CandidateAssessment,
    pub score_breakdown: ScoreBreakdown,
    pub order_over_next: Option<TieBreakReason>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedCandidate {
    pub variant: AllocationVariant,
    pub evidence: CandidateAssessment,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationDecision {
    pub winner: Option<AllocationVariant>,
    pub ranked_candidates: Vec<RankedCandidate>,
    pub excluded_candidates: Vec<ExcludedCandidate>,
    pub policy: AllocationPolicy,
    pub requirements: CandidateRequirements,
    pub weights: ProfileWeights,
    pub tie_break: TieBreakReason,
}
