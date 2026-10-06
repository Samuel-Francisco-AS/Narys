//! Synthetic B2 gate: no credentials, provider calls, IO, managers or clock.
use super::*;
use crate::cognition::{
    rate::{ConstraintSource, RateConstraintSnapshot, RatePolicy, RateSnapshot},
    telemetry::{
        Fact, Provenance, ProviderTelemetrySnapshot, QuotaDimension, QuotaScope, QuotaSnapshot,
        ScopedQuotas, Timing, MAX_FACT_VALUE,
    },
};
use std::collections::BTreeMap;

fn known<T>(v: T) -> CatalogFact<T> {
    CatalogFact::known(v, CatalogProvenance::IntegrationCatalog, Some(42)).unwrap()
}
fn factual<T>(v: T) -> Fact<T> {
    Fact::Known {
        value: v,
        provenance: Provenance::ProviderHeader,
        observed_at_unix_ms: Some(42),
    }
}
fn money(currency: &str, n: u64) -> MonetaryAmount {
    MonetaryAmount::new(currency, n).unwrap()
}
fn resource(id: &str) -> CognitiveResource {
    let mut model = ModelProfile::unknown(ModelId::new("model-A").unwrap());
    model.availability = known(Availability::Available);
    model.facts.execution.cognitive_tier = known(CognitiveTier::new(2).unwrap());
    model.supported_efforts = known(vec![EffortProfile {
        id: EffortId::new("effort-A").unwrap(),
        availability: known(Availability::Available),
        facts: ExecutionFacts::default(),
    }]);
    CognitiveResource {
        identity: ResourceIdentity {
            id: ResourceId::new(id).unwrap(),
            class: ResourceClass::LocalSupport,
            family: ProviderFamily::new("family").unwrap(),
            access_path: AccessPath::new("path").unwrap(),
            billing_domain: BillingDomain {
                id: BillingDomainId::new(id).unwrap(),
            },
        },
        origin: ResourceOrigin::Local,
        economics: EconomicFacts::default(),
        enabled: known(true),
        availability: known(Availability::Available),
        capabilities: CapabilitySet::default(),
        models: known(vec![model]),
    }
}
fn provider(id: &str) -> CognitiveResource {
    let mut r = resource(id);
    r.identity.class = ResourceClass::CognitiveProvider;
    r.origin = ResourceOrigin::Provider(RuntimeId::new(id).unwrap());
    r
}
fn models_mut(r: &mut CognitiveResource) -> &mut Vec<ModelProfile> {
    let CatalogFact::Known { value, .. } = &mut r.models else {
        panic!("fixture models")
    };
    value
}
fn execution(r: &mut CognitiveResource) -> &mut ExecutionFacts {
    &mut models_mut(r)[0].facts.execution
}
fn effort(r: &mut CognitiveResource) -> &mut ExecutionFacts {
    let CatalogFact::Known { value, .. } = &mut models_mut(r)[0].supported_efforts else {
        panic!("fixture efforts")
    };
    &mut value[0].facts
}
fn consumption(id: &str, unit: AllowanceUnit, amount: CatalogFact<u64>) -> AllowanceConsumption {
    AllowanceConsumption {
        dimension_id: AllowanceDimensionId::new(id).unwrap(),
        unit,
        amount,
    }
}
fn allowance(
    r: &mut CognitiveResource,
    id: &str,
    unit: AllowanceUnit,
    amount: CatalogFact<u64>,
    limit: CatalogFact<u64>,
    remaining: CatalogFact<u64>,
) {
    execution(r)
        .allowance_costs
        .push(consumption(id, unit.clone(), amount));
    r.economics.allowances.push(AllowanceState {
        id: AllowanceDimensionId::new(id).unwrap(),
        unit,
        limit,
        remaining,
        reset: known(Timing::DelayMs(1000)),
    });
}
fn reserve(r: &mut CognitiveResource) {
    allowance(
        r,
        "weekly",
        AllowanceUnit::Requests,
        known(1),
        known(100),
        known(5),
    );
}
fn comfortable(r: &mut CognitiveResource) {
    allowance(
        r,
        "weekly",
        AllowanceUnit::Requests,
        known(1),
        known(100),
        known(80),
    );
}
fn policy(profile: AllocationProfile) -> AllocationPolicy {
    AllocationPolicy {
        profile,
        reserve: Some(ReservePolicy::new(40, 10).unwrap()),
        variant_selection_mode: VariantSelectionMode::Auto,
        ..AllocationPolicy::default()
    }
}
fn signals(
    ordinal: Option<u16>,
    priority: Option<u16>,
    continuity: Option<u16>,
    switching: Option<u16>,
) -> CandidateSignals {
    CandidateSignals::new(ordinal, priority, continuity, switching).unwrap()
}
fn candidate(r: &CognitiveResource, e: bool) -> AllocationCandidate<'_> {
    AllocationCandidate::new(
        r,
        ModelId::new("model-A").unwrap(),
        e.then(|| EffortId::new("effort-A").unwrap()),
    )
    .unwrap()
}
fn b1<'a>(
    resources: &[&'a CognitiveResource],
    p: AllocationPolicy,
    floor: Option<u16>,
) -> AllocationRequest<'a> {
    AllocationRequest::new(
        CandidateRequirements::new(vec![], floor.map(|t| CognitiveTier::new(t).unwrap())).unwrap(),
        p,
        resources.iter().map(|r| candidate(r, false)).collect(),
    )
    .unwrap()
}
fn select(c: &AllocationCandidate<'_>, s: CandidateSignals) -> ScoringCandidate {
    ScoringCandidate {
        variant: c.variant(),
        signals: s,
    }
}
fn score_request(b: &AllocationRequest<'_>, contexts: Vec<EconomicContext>) -> AllocationDecision {
    AllocationScoringRequest::new(
        b,
        b.candidates()
            .iter()
            .map(|c| select(c, CandidateSignals::default()))
            .collect(),
        contexts,
    )
    .unwrap()
    .decide()
}
fn decide(resources: &[&CognitiveResource], profile: AllocationProfile) -> AllocationDecision {
    score_request(&b1(resources, policy(profile), Some(2)), vec![])
}
fn winner(d: &AllocationDecision) -> &str {
    d.winner.as_ref().unwrap().resource_id.as_str()
}
fn exclusion(d: &AllocationDecision, reason: EconomicExclusion) {
    assert!(d.ranked_candidates.is_empty());
    assert!(d.winner.is_none());
    assert!(
        matches!(&d.excluded_candidates[0].evidence.economic_eligibility, EconomicEligibility::Excluded(reasons) if reasons.contains(&reason))
    );
}
fn paid(kind: BillingKind, cost: CatalogFact<MonetaryAmount>) -> CognitiveResource {
    let mut r = resource("paid");
    r.economics.billing_kind = known(kind);
    execution(&mut r).monetary_cost = cost;
    r
}
fn paid_decide(r: &CognitiveResource, currency: &str, budget: u64) -> AllocationDecision {
    let mut p = policy(AllocationProfile::Economy);
    p.paid_use = PaidUsePolicy::AllowKnownCostWithinBudget {
        budget: money(currency, budget),
    };
    score_request(&b1(&[r], p, Some(2)), vec![])
}
fn rate(id: &str, constraints: Vec<RateConstraintSnapshot>) -> RateSnapshot {
    RateSnapshot {
        provider_id: id.into(),
        captured_at_unix_ms: Some(43),
        context_generation: 0,
        policy: RatePolicy::default(),
        constraints,
        pending_reservations: 0,
        local_blocks: 0,
        saturated: false,
        persistence_failed: false,
    }
}
fn constraint(
    scope: QuotaScope,
    dim: QuotaDimension,
    remaining: Option<u64>,
) -> RateConstraintSnapshot {
    RateConstraintSnapshot {
        scope,
        dimension: dim,
        source: ConstraintSource::ExternalFact,
        provenance: Some(Provenance::ProviderHeader),
        external: None,
        capacity: Some(100),
        consumed: 0,
        reserved: 0,
        effective_remaining: remaining,
        reset_unix_ms: Some(1042),
        reset_in_ms: Some(999),
        saturated: false,
        unaccounted_token_calls: 0,
    }
}
fn quota(remaining: Fact<u64>) -> QuotaSnapshot {
    QuotaSnapshot {
        limit: factual(100),
        remaining,
        reset: factual(Timing::DelayMs(1000)),
    }
}
fn telemetry(id: &str, quotas: Vec<ScopedQuotas>) -> ProviderTelemetrySnapshot {
    ProviderTelemetrySnapshot {
        provider_id: id.into(),
        captured_at_unix_ms: Some(44),
        context_generation: 0,
        updated_age_ms: None,
        usage: BTreeMap::new(),
        quotas,
        retry_hint: Fact::Unknown,
        last_outcome: Fact::Unknown,
    }
}
fn scoped(scope: QuotaScope, dimension: QuotaDimension, q: QuotaSnapshot) -> ScopedQuotas {
    ScopedQuotas {
        scope,
        dimensions: BTreeMap::from([(dimension, q)]),
    }
}

#[test]
fn b2_least_sufficient_relative_cost_and_b1_higher_floor() {
    let mut a = resource("opaque-z");
    let mut b = resource("opaque-a");
    execution(&mut a).relative_cost = known(RelativeCostTier::new(20).unwrap());
    execution(&mut b).relative_cost = known(RelativeCostTier::new(100).unwrap());
    execution(&mut b).cognitive_tier = known(CognitiveTier::new(4).unwrap());
    assert_eq!(
        winner(&decide(&[&a, &b], AllocationProfile::Economy)),
        "opaque-z"
    );
    let request = b1(&[&a, &b], policy(AllocationProfile::Economy), Some(4));
    assert_eq!(
        request.evaluate().candidates()[0].status(),
        EligibilityStatus::Ineligible
    );
    let result = AllocationScoringRequest::new(
        &request,
        vec![select(
            &request.candidates()[1],
            CandidateSignals::default(),
        )],
        vec![],
    )
    .unwrap()
    .decide();
    assert_eq!(winner(&result), "opaque-a");
}
#[test]
fn b2_higher_tier_has_no_free_bonus() {
    let a = resource("a");
    let mut b = resource("b");
    execution(&mut b).cognitive_tier = known(CognitiveTier::new(5).unwrap());
    let d = decide(&[&b, &a], AllocationProfile::Economy);
    assert_eq!(winner(&d), "a");
    assert_eq!(
        d.ranked_candidates[0].score_breakdown,
        d.ranked_candidates[1].score_breakdown
    );
    assert_eq!(d.tie_break, TieBreakReason::CanonicalVariant);
}
#[test]
fn b2_reserve_loses_to_comfortable_economy_and_balanced() {
    let mut a = resource("a");
    let mut b = resource("b");
    reserve(&mut a);
    comfortable(&mut b);
    for p in [AllocationProfile::Economy, AllocationProfile::Balanced] {
        assert_eq!(winner(&decide(&[&a, &b], p)), "b");
    }
}
#[test]
fn b2_reserve_only_capable_candidate_still_wins() {
    let mut a = resource("a");
    let mut b = resource("b");
    reserve(&mut a);
    execution(&mut b).cognitive_tier = known(CognitiveTier::new(1).unwrap());
    let request = b1(&[&a, &b], policy(AllocationProfile::Balanced), Some(2));
    assert_eq!(
        request.evaluate().candidates()[1].status(),
        EligibilityStatus::Ineligible
    );
    let d = AllocationScoringRequest::new(
        &request,
        vec![select(
            &request.candidates()[0],
            CandidateSignals::default(),
        )],
        vec![],
    )
    .unwrap()
    .decide();
    assert_eq!(winner(&d), "a");
}
#[test]
fn b2_exhausted_remaining_zero() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "requests",
        AllowanceUnit::Requests,
        known(1),
        CatalogFact::Unknown,
        known(0),
    );
    exclusion(
        &decide(&[&r], AllocationProfile::Economy),
        EconomicExclusion::AllowanceExhausted,
    );
}
#[test]
fn b2_exhausted_cannot_cover_next_execution() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "tokens",
        AllowanceUnit::Tokens,
        known(31),
        known(100),
        known(30),
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    exclusion(&d, EconomicExclusion::AllowanceExhausted);
    assert_eq!(
        d.excluded_candidates[0].evidence.scarcity.dimensions[0].reason,
        ScarcityReason::CannotCoverExecution
    );
}
#[test]
fn b2_unknown_remaining_is_neutral() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Credits,
        known(1),
        known(100),
        CatalogFact::Unknown,
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    let c = &d.ranked_candidates[0];
    assert_eq!(
        c.evidence.scarcity.summary,
        ScarcitySummary {
            known_worst: None,
            has_unknown: true
        }
    );
    assert_eq!(
        c.evidence.scarcity.dimensions[0].state,
        ScarcityState::Unknown
    );
    assert_eq!(c.score_breakdown.scarcity.value, 0);
}
#[test]
fn b2_unknown_limit_does_not_turn_remaining_into_percent() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Requests,
        known(1),
        CatalogFact::Unknown,
        known(30),
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    let a = &d.ranked_candidates[0].evidence.scarcity.dimensions[0];
    assert_eq!(a.state, ScarcityState::Unknown);
    assert_eq!(a.remaining_percent, None);
}
#[test]
fn b2_multidimensional_preserves_worst_known_and_unknown() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "rolling",
        AllowanceUnit::Requests,
        known(1),
        known(100),
        known(80),
    );
    allowance(
        &mut r,
        "weekly",
        AllowanceUnit::Tokens,
        known(2),
        known(100),
        known(5),
    );
    allowance(
        &mut r,
        "another",
        AllowanceUnit::Credits,
        CatalogFact::Unknown,
        known(100),
        CatalogFact::Unknown,
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    let e = &d.ranked_candidates[0].evidence.scarcity;
    assert_eq!(e.dimensions.len(), 3);
    assert_eq!(
        e.summary,
        ScarcitySummary {
            known_worst: Some(ScarcityState::Reserve),
            has_unknown: true
        }
    );
    assert_eq!(
        d.ranked_candidates[0].score_breakdown.scarcity.value,
        -ECONOMY_WEIGHTS.reserve
    );
}
#[test]
fn b2_model_effort_dimension_override_without_mutation() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Credits,
        known(4),
        known(100),
        known(3),
    );
    effort(&mut r)
        .allowance_costs
        .push(consumption("x", AllowanceUnit::Credits, known(2)));
    let before = r.clone();
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Economy),
        vec![candidate(&r, true)],
    )
    .unwrap();
    let d = score_request(&request, vec![]);
    let e = &d.ranked_candidates[0].evidence;
    assert_eq!(
        e.resolved_facts.as_ref().unwrap().allowance_costs[0]
            .consumption
            .amount,
        known(2)
    );
    assert_eq!(
        e.scarcity.dimensions[0].consumption.layer,
        EvidenceLayer::Effort
    );
    assert_eq!(r, before);
}
#[test]
fn b2_effort_only_dimension_added_model_only_preserved() {
    let mut r = resource("a");
    execution(&mut r)
        .allowance_costs
        .push(consumption("model", AllowanceUnit::Requests, known(4)));
    effort(&mut r)
        .allowance_costs
        .push(consumption("effort", AllowanceUnit::Tokens, known(2)));
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Economy),
        vec![candidate(&r, true)],
    )
    .unwrap();
    let d = score_request(&request, vec![]);
    let costs = &d.ranked_candidates[0]
        .evidence
        .resolved_facts
        .as_ref()
        .unwrap()
        .allowance_costs;
    assert_eq!(costs.len(), 2);
    assert_eq!(costs[0].layer, EvidenceLayer::Effort);
    assert_eq!(costs[1].layer, EvidenceLayer::Model);
}
#[test]
fn b2_effort_unknown_amount_overrides_model_known_amount() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Credits,
        known(4),
        known(100),
        known(3),
    );
    effort(&mut r).allowance_costs.push(consumption(
        "x",
        AllowanceUnit::Credits,
        CatalogFact::Unknown,
    ));
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Economy),
        vec![candidate(&r, true)],
    )
    .unwrap();
    let d = score_request(&request, vec![]);
    assert!(d.ranked_candidates[0]
        .evidence
        .resolved_facts
        .as_ref()
        .unwrap()
        .allowance_costs[0]
        .consumption
        .amount
        .value()
        .is_none());
    assert_ne!(
        d.ranked_candidates[0].evidence.scarcity.summary.known_worst,
        Some(ScarcityState::Exhausted)
    );
}
#[test]
fn b2_model_effort_unit_conflict_without_state_fails_closed() {
    let mut r = resource("a");
    execution(&mut r)
        .allowance_costs
        .push(consumption("x", AllowanceUnit::Credits, known(4)));
    effort(&mut r)
        .allowance_costs
        .push(consumption("x", AllowanceUnit::Tokens, known(2)));
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Economy),
        vec![candidate(&r, true)],
    )
    .unwrap();
    exclusion(
        &score_request(&request, vec![]),
        EconomicExclusion::EconomicEvidenceConflict,
    );
}
#[test]
fn b2_zero_consumption_never_creates_scarcity() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Requests,
        known(0),
        known(100),
        known(0),
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    let a = &d.ranked_candidates[0].evidence.scarcity;
    assert_eq!(a.summary, ScarcitySummary::default());
    assert!(!a.dimensions[0].consumed);
    assert_eq!(
        a.dimensions[0].reason,
        ScarcityReason::ExplicitZeroConsumption
    );
}
#[test]
fn b2_unreferenced_exhausted_allowance_is_ignored() {
    let mut r = resource("a");
    reserve(&mut r);
    r.economics.allowances[0].remaining = known(0);
    execution(&mut r).allowance_costs.clear();
    let d = decide(&[&r], AllocationProfile::Economy);
    assert_eq!(winner(&d), "a");
    assert!(d.ranked_candidates[0]
        .evidence
        .scarcity
        .dimensions
        .is_empty());
}
#[test]
fn b2_unknown_amount_is_relevant_and_zero_remaining_proves_exhaustion() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Requests,
        CatalogFact::Unknown,
        known(100),
        known(0),
    );
    exclusion(
        &decide(&[&r], AllocationProfile::Economy),
        EconomicExclusion::AllowanceExhausted,
    );
}
#[test]
fn b2_thresholds_strict_below_and_floor_percent() {
    for (remaining, state) in [
        (0, ScarcityState::Exhausted),
        (9, ScarcityState::Reserve),
        (10, ScarcityState::Reduced),
        (39, ScarcityState::Reduced),
        (40, ScarcityState::Comfortable),
    ] {
        let mut r = resource("a");
        allowance(
            &mut r,
            "x",
            AllowanceUnit::Requests,
            known(1),
            known(100),
            known(remaining),
        );
        let d = decide(&[&r], AllocationProfile::Economy);
        let e = if d.ranked_candidates.is_empty() {
            &d.excluded_candidates[0].evidence
        } else {
            &d.ranked_candidates[0].evidence
        };
        assert_eq!(e.scarcity.dimensions[0].state, state);
    }
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Requests,
        known(1),
        known(101),
        known(10),
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    assert_eq!(
        d.ranked_candidates[0].evidence.scarcity.dimensions[0].remaining_percent,
        Some(9)
    );
}
#[test]
fn b2_percentage_overflow_safe_at_maximum() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Tokens,
        known(1),
        known(MAX_FACT_VALUE),
        known(MAX_FACT_VALUE),
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    assert_eq!(
        d.ranked_candidates[0].evidence.scarcity.dimensions[0].remaining_percent,
        Some(100)
    );
}
#[test]
fn b2_no_reserve_policy_and_zero_limit_preserve_unknown() {
    let mut r = resource("a");
    reserve(&mut r);
    let mut p = policy(AllocationProfile::Economy);
    p.reserve = None;
    let d = score_request(&b1(&[&r], p, Some(2)), vec![]);
    assert_eq!(
        d.ranked_candidates[0].evidence.scarcity.dimensions[0].state,
        ScarcityState::Unknown
    );
    let mut zero = resource("zero");
    allowance(
        &mut zero,
        "x",
        AllowanceUnit::Requests,
        known(1),
        known(0),
        known(0),
    );
    exclusion(
        &decide(&[&zero], AllocationProfile::Economy),
        EconomicExclusion::AllowanceExhausted,
    );
}

#[test]
fn b2_paid_deny_includes_zero_cost_paid_paths() {
    for kind in [BillingKind::MeteredBilling, BillingKind::PrepaidCredits] {
        for cost in [
            CatalogFact::Unknown,
            known(money("USD", 0)),
            known(money("USD", 10)),
        ] {
            let r = paid(kind, cost);
            exclusion(
                &decide(&[&r], AllocationProfile::Fast),
                EconomicExclusion::PaidUseDenied,
            );
        }
    }
}
#[test]
fn b2_positive_monetary_cost_alone_proves_paid_evidence() {
    for kind in [
        BillingKind::Unknown,
        BillingKind::FreeTier,
        BillingKind::IncludedAllowance,
    ] {
        let r = paid(kind, known(money("USD", 1)));
        exclusion(
            &decide(&[&r], AllocationProfile::Economy),
            EconomicExclusion::PaidUseDenied,
        );
    }
}
#[test]
fn b2_paid_allowed_known_cost_at_budget() {
    let r = paid(BillingKind::MeteredBilling, known(money("USD", 100)));
    let d = paid_decide(&r, "USD", 100);
    assert_eq!(winner(&d), "paid");
    assert_eq!(
        d.ranked_candidates[0]
            .evidence
            .spend
            .as_ref()
            .unwrap()
            .outcome,
        SpendOutcome::AllowedWithinBudget
    );
}
#[test]
fn b2_paid_unknown_cost_excluded() {
    let r = paid(BillingKind::MeteredBilling, CatalogFact::Unknown);
    exclusion(
        &paid_decide(&r, "USD", 100),
        EconomicExclusion::PaidCostUnknown,
    );
}
#[test]
fn b2_currency_mismatch_without_fx() {
    let r = paid(BillingKind::MeteredBilling, known(money("USD", 100)));
    exclusion(
        &paid_decide(&r, "BRL", 100),
        EconomicExclusion::CurrencyMismatch,
    );
}
#[test]
fn b2_budget_exceeded() {
    let r = paid(BillingKind::MeteredBilling, known(money("USD", 101)));
    exclusion(
        &paid_decide(&r, "USD", 100),
        EconomicExclusion::PaidBudgetExceeded,
    );
}
#[test]
fn b2_prepaid_sufficient_balance() {
    let mut r = paid(BillingKind::PrepaidCredits, known(money("USD", 100)));
    r.economics.monetary_balance = known(money("USD", 100));
    assert_eq!(winner(&paid_decide(&r, "USD", 100)), "paid");
}
#[test]
fn b2_prepaid_insufficient_balance() {
    let mut r = paid(BillingKind::PrepaidCredits, known(money("USD", 100)));
    r.economics.monetary_balance = known(money("USD", 99));
    exclusion(
        &paid_decide(&r, "USD", 100),
        EconomicExclusion::PrepaidBalanceInsufficient,
    );
}
#[test]
fn b2_prepaid_unknown_balance() {
    let r = paid(BillingKind::PrepaidCredits, known(money("USD", 100)));
    exclusion(
        &paid_decide(&r, "USD", 100),
        EconomicExclusion::PrepaidBalanceUnknown,
    );
}
#[test]
fn b2_prepaid_balance_currency_mismatch() {
    let mut r = paid(BillingKind::PrepaidCredits, known(money("USD", 100)));
    r.economics.monetary_balance = known(money("BRL", 100));
    exclusion(
        &paid_decide(&r, "USD", 100),
        EconomicExclusion::PrepaidBalanceCurrencyMismatch,
    );
}
#[test]
fn b2_metered_does_not_require_prepaid_balance() {
    let mut r = paid(BillingKind::MeteredBilling, known(money("USD", 100)));
    for balance in [
        CatalogFact::Unknown,
        known(money("USD", 0)),
        known(money("BRL", 0)),
    ] {
        r.economics.monetary_balance = balance;
        assert_eq!(winner(&paid_decide(&r, "USD", 100)), "paid");
    }
}
#[test]
fn b2_unknown_billing_and_unknown_cost_neither_free_nor_paid() {
    for kind in [CatalogFact::Unknown, known(BillingKind::Unknown)] {
        let mut r = resource("a");
        r.economics.billing_kind = kind.clone();
        let d = decide(&[&r], AllocationProfile::Economy);
        let s = d.ranked_candidates[0].evidence.spend.as_ref().unwrap();
        assert_eq!(s.billing_kind, kind);
        assert!(!s.paid_path_evidence);
        assert!(!s.positive_cost_evidence);
        assert_eq!(s.outcome, SpendOutcome::NoPositivePaidEvidence);
        assert_eq!(
            d.ranked_candidates[0].score_breakdown.monetary_cost.value,
            0
        );
    }
}
#[test]
fn b2_zero_monetary_cost_zero_budget() {
    let r = paid(BillingKind::MeteredBilling, known(money("USD", 0)));
    let d = paid_decide(&r, "USD", 0);
    assert_eq!(winner(&d), "paid");
    assert_eq!(
        d.ranked_candidates[0]
            .score_breakdown
            .monetary_cost
            .evidence
            .budget_percent,
        Some(0)
    );
}
#[test]
fn b2_prepaid_zero_still_requires_known_balance_under_allow() {
    let r = paid(BillingKind::PrepaidCredits, known(money("USD", 0)));
    exclusion(
        &paid_decide(&r, "USD", 0),
        EconomicExclusion::PrepaidBalanceUnknown,
    );
}
#[test]
fn b2_shared_billing_domain_conflict_fails_before_ranking() {
    let a = resource("a");
    let mut b = resource("b");
    b.identity.billing_domain = a.identity.billing_domain.clone();
    b.economics.billing_kind = known(BillingKind::FreeTier);
    let request = b1(&[&a, &b], policy(AllocationProfile::Economy), Some(2));
    let selected = request
        .candidates()
        .iter()
        .map(|c| select(c, CandidateSignals::default()))
        .collect();
    assert!(matches!(
        AllocationScoringRequest::new(&request, selected, vec![]),
        Err(ScoringError::ConflictingBillingDomainFacts)
    ));
}
#[test]
fn b2_shared_billing_domain_equal_facts_are_valid() {
    let mut a = resource("a");
    reserve(&mut a);
    let mut b = resource("b");
    b.identity.billing_domain = a.identity.billing_domain.clone();
    b.economics = a.economics.clone();
    let d = decide(&[&a, &b], AllocationProfile::Economy);
    assert_eq!(d.ranked_candidates.len(), 2);
}
#[test]
fn b2_shared_billing_domain_provenance_and_timestamp_conflicts() {
    let mut a = resource("a");
    a.economics.billing_kind = known(BillingKind::FreeTier);
    for altered in [
        CatalogFact::known(
            BillingKind::FreeTier,
            CatalogProvenance::RuntimeContract,
            Some(42),
        )
        .unwrap(),
        CatalogFact::known(
            BillingKind::FreeTier,
            CatalogProvenance::IntegrationCatalog,
            Some(43),
        )
        .unwrap(),
    ] {
        let mut b = resource("b");
        b.identity.billing_domain = a.identity.billing_domain.clone();
        b.economics.billing_kind = altered;
        let request = b1(&[&a, &b], policy(AllocationProfile::Economy), Some(2));
        assert!(matches!(
            AllocationScoringRequest::new(
                &request,
                request
                    .candidates()
                    .iter()
                    .map(|c| select(c, CandidateSignals::default()))
                    .collect(),
                vec![]
            ),
            Err(ScoringError::ConflictingBillingDomainFacts)
        ));
    }
}
#[test]
fn b2_small_affinity_does_not_overcome_reserve() {
    let mut a = resource("a");
    let mut b = resource("b");
    reserve(&mut a);
    comfortable(&mut b);
    for profile in [AllocationProfile::Economy, AllocationProfile::Balanced] {
        let request = b1(&[&a, &b], policy(profile), Some(2));
        let d = AllocationScoringRequest::new(
            &request,
            vec![
                select(&request.candidates()[0], signals(None, None, Some(5), None)),
                select(&request.candidates()[1], CandidateSignals::default()),
            ],
            vec![],
        )
        .unwrap()
        .decide();
        assert_eq!(winner(&d), "b");
    }
}
#[test]
fn b2_fast_large_factual_latency_continuity_can_overcome_reserve() {
    let mut a = resource("a");
    let mut b = resource("b");
    reserve(&mut a);
    comfortable(&mut b);
    execution(&mut a).latency_ms = known(100);
    execution(&mut b).latency_ms = known(10000);
    let request = b1(&[&a, &b], policy(AllocationProfile::Fast), Some(2));
    let d = AllocationScoringRequest::new(
        &request,
        vec![
            select(
                &request.candidates()[0],
                signals(None, None, Some(100), Some(0)),
            ),
            select(
                &request.candidates()[1],
                signals(None, None, Some(0), Some(100)),
            ),
        ],
        vec![],
    )
    .unwrap()
    .decide();
    assert_eq!(winner(&d), "a");
    assert_eq!(d.ranked_candidates[0].score_breakdown.total, 2370);
    assert_eq!(d.ranked_candidates[1].score_breakdown.total, -2700);
}
#[test]
fn b2_fast_never_selects_exhausted_even_with_maximum_advantages() {
    let mut a = resource("a");
    reserve(&mut a);
    a.economics.allowances[0].remaining = known(0);
    execution(&mut a).latency_ms = known(0);
    let b = resource("b");
    let request = b1(&[&a, &b], policy(AllocationProfile::Fast), Some(2));
    let d = AllocationScoringRequest::new(
        &request,
        vec![
            select(
                &request.candidates()[0],
                signals(Some(0), Some(0), Some(100), Some(0)),
            ),
            select(
                &request.candidates()[1],
                signals(Some(255), Some(32), Some(0), Some(100)),
            ),
        ],
        vec![],
    )
    .unwrap()
    .decide();
    assert_eq!(winner(&d), "b");
    assert_eq!(d.excluded_candidates.len(), 1);
}
#[test]
fn b2_unknown_latency_neutral_between_factual_fast_and_slow() {
    let mut fast = resource("fast");
    let unknown = resource("unknown");
    let mut slow = resource("slow");
    execution(&mut fast).latency_ms = known(100);
    execution(&mut slow).latency_ms = known(10000);
    let d = decide(&[&unknown, &slow, &fast], AllocationProfile::Fast);
    assert_eq!(winner(&d), "fast");
    assert_eq!(
        d.ranked_candidates[1].variant.resource_id.as_str(),
        "unknown"
    );
    assert_eq!(d.ranked_candidates[1].score_breakdown.latency.value, 0);
    assert_eq!(
        d.ranked_candidates[1].score_breakdown.latency.evidence.fact,
        CatalogFact::Unknown
    );
}
#[test]
fn b2_scalar_effort_known_override_unknown_fallback_matches_b1() {
    let mut r = resource("a");
    execution(&mut r).relative_cost = known(RelativeCostTier::new(100).unwrap());
    execution(&mut r).latency_ms = known(800);
    execution(&mut r).monetary_cost = known(money("USD", 100));
    effort(&mut r).cognitive_tier = known(CognitiveTier::new(4).unwrap());
    effort(&mut r).relative_cost = known(RelativeCostTier::new(20).unwrap());
    effort(&mut r).monetary_cost = known(money("USD", 0));
    let mut p = policy(AllocationProfile::Economy);
    p.paid_use = PaidUsePolicy::AllowKnownCostWithinBudget {
        budget: money("USD", 100),
    };
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        p,
        vec![candidate(&r, true)],
    )
    .unwrap();
    let d = score_request(&request, vec![]);
    let f = d.ranked_candidates[0]
        .evidence
        .resolved_facts
        .as_ref()
        .unwrap();
    assert_eq!(f.cognitive_tier.fact, known(CognitiveTier::new(4).unwrap()));
    assert_eq!(f.cognitive_tier.layer, Some(EvidenceLayer::Effort));
    assert_eq!(
        f.relative_cost.fact,
        known(RelativeCostTier::new(20).unwrap())
    );
    assert_eq!(f.relative_cost.layer, Some(EvidenceLayer::Effort));
    assert_eq!(f.latency_ms.fact, known(800));
    assert_eq!(f.latency_ms.layer, Some(EvidenceLayer::Model));
    assert_eq!(f.monetary_cost.fact, known(money("USD", 0)));
    assert_eq!(f.monetary_cost.layer, Some(EvidenceLayer::Effort));
    assert_eq!(
        f.cognitive_tier.fact,
        d.ranked_candidates[0].evidence.b1.effective_tier().fact
    );
}
#[test]
fn b2_unknown_scalar_components_have_zero_influence() {
    let r = resource("a");
    let d = decide(&[&r], AllocationProfile::Balanced);
    let s = &d.ranked_candidates[0].score_breakdown;
    assert_eq!(s.total, 0);
    assert_eq!(s.relative_cost.evidence.layer, None);
    assert_eq!(s.latency.evidence.layer, None);
    assert!(!s.monetary_cost.evidence.comparable);
}
#[test]
fn b2_relative_cost_zero_is_not_monetary_zero() {
    let mut r = paid(BillingKind::MeteredBilling, CatalogFact::Unknown);
    execution(&mut r).relative_cost = known(RelativeCostTier::new(0).unwrap());
    exclusion(
        &paid_decide(&r, "USD", 100),
        EconomicExclusion::PaidCostUnknown,
    );
}
#[test]
fn b2_monetary_score_comparable_only_after_guard() {
    let mut a = paid(BillingKind::MeteredBilling, known(money("USD", 20)));
    a.identity.id = ResourceId::new("a").unwrap();
    let mut b = resource("b");
    execution(&mut b).monetary_cost = known(money("USD", 80));
    let mut p = policy(AllocationProfile::Economy);
    p.paid_use = PaidUsePolicy::AllowKnownCostWithinBudget {
        budget: money("USD", 100),
    };
    let d = score_request(&b1(&[&a, &b], p, Some(2)), vec![]);
    assert_eq!(winner(&d), "a");
    assert_eq!(
        d.ranked_candidates[0].score_breakdown.monetary_cost.value,
        -200
    );
    assert_eq!(
        d.ranked_candidates[1].score_breakdown.monetary_cost.value,
        -800
    );
}
#[test]
fn b2_different_zero_cost_currency_has_no_monetary_comparison() {
    let r = paid(BillingKind::FreeTier, known(money("USD", 0)));
    let d = paid_decide(&r, "BRL", 100);
    assert_eq!(
        d.ranked_candidates[0].score_breakdown.monetary_cost.value,
        0
    );
    assert!(
        !d.ranked_candidates[0]
            .score_breakdown
            .monetary_cost
            .evidence
            .comparable
    );
}

#[test]
fn b2_lr8_sibling_model_constraint_and_global_saturation_do_not_leak() {
    let mut r = provider("a");
    let mut sibling = models_mut(&mut r)[0].clone();
    sibling.id = ModelId::new("model-B").unwrap();
    models_mut(&mut r).push(sibling);
    let mut c = constraint(
        QuotaScope::Model {
            model: "model-B".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(0),
    );
    c.saturated = true;
    let mut snapshot = rate("a", vec![c]);
    snapshot.saturated = true;
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&r, None, Some(&snapshot)).unwrap()],
    );
    assert_eq!(winner(&d), "a");
    assert!(d.ranked_candidates[0]
        .evidence
        .operational_pressure
        .constraints
        .is_empty());
    assert_eq!(d.ranked_candidates[0].score_breakdown.scarcity.value, 0);
}
#[test]
fn b2_lr8_exact_model_saturation_excludes_only_exact_candidate() {
    let mut r = provider("a");
    let mut sibling = models_mut(&mut r)[0].clone();
    sibling.id = ModelId::new("model-B").unwrap();
    models_mut(&mut r).push(sibling);
    let c = constraint(
        QuotaScope::Model {
            model: "model-B".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(0),
    );
    let snapshot = rate("a", vec![c]);
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Fast),
        vec![
            candidate(&r, false),
            AllocationCandidate::new(&r, ModelId::new("model-B").unwrap(), None).unwrap(),
        ],
    )
    .unwrap();
    let d = score_request(
        &request,
        vec![EconomicContext::capture(&r, None, Some(&snapshot)).unwrap()],
    );
    assert_eq!(d.winner.unwrap().model_id.as_str(), "model-A");
    assert_eq!(d.excluded_candidates.len(), 1);
    assert_eq!(
        d.excluded_candidates[0].variant.model_id.as_str(),
        "model-B"
    );
}
#[test]
fn b2_lr8_provider_scope_applies_to_both_models() {
    let mut r = provider("a");
    let mut sibling = models_mut(&mut r)[0].clone();
    sibling.id = ModelId::new("model-B").unwrap();
    models_mut(&mut r).push(sibling);
    let snapshot = rate(
        "a",
        vec![constraint(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            Some(0),
        )],
    );
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Fast),
        vec![
            candidate(&r, false),
            AllocationCandidate::new(&r, ModelId::new("model-B").unwrap(), None).unwrap(),
        ],
    )
    .unwrap();
    let d = score_request(
        &request,
        vec![EconomicContext::capture(&r, None, Some(&snapshot)).unwrap()],
    );
    assert!(d.winner.is_none());
    assert_eq!(d.excluded_candidates.len(), 2);
}
#[test]
fn b2_applicable_saturated_constraint_excluded_even_without_remaining() {
    let r = provider("a");
    let mut c = constraint(QuotaScope::Provider, QuotaDimension::TokensPerMinute, None);
    c.saturated = true;
    let snapshot = rate("a", vec![c]);
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Fast), Some(2)),
        vec![EconomicContext::capture(&r, None, Some(&snapshot)).unwrap()],
    );
    exclusion(&d, EconomicExclusion::Lr8ConstraintSaturated);
}
#[test]
fn b2_lr8_telemetry_fallback_when_corresponding_rate_absent() {
    let r = provider("a");
    let t = telemetry(
        "a",
        vec![scoped(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            quota(factual(5)),
        )],
    );
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&r, Some(&t), None).unwrap()],
    );
    let pressure = &d.ranked_candidates[0].evidence.operational_pressure;
    assert_eq!(pressure.constraints.len(), 1);
    assert_eq!(
        pressure.constraints[0].source,
        PressureSource::TelemetryFallback
    );
    assert_eq!(pressure.summary.known_worst, Some(ScarcityState::Reserve));
}
#[test]
fn b2_lr8_rate_and_telemetry_no_double_counting() {
    let r = provider("a");
    let t = telemetry(
        "a",
        vec![scoped(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            quota(factual(5)),
        )],
    );
    let snapshot = rate(
        "a",
        vec![constraint(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            Some(5),
        )],
    );
    let request = b1(&[&r], policy(AllocationProfile::Economy), Some(2));
    let rate_only = score_request(
        &request,
        vec![EconomicContext::capture(&r, None, Some(&snapshot)).unwrap()],
    );
    let both = score_request(
        &request,
        vec![EconomicContext::capture(&r, Some(&t), Some(&snapshot)).unwrap()],
    );
    assert_eq!(
        rate_only.ranked_candidates[0].score_breakdown,
        both.ranked_candidates[0].score_breakdown
    );
    assert_eq!(
        both.ranked_candidates[0]
            .evidence
            .operational_pressure
            .constraints
            .len(),
        1
    );
}
#[test]
fn b2_lr8_retained_unknown_rate_cannot_be_overridden_by_telemetry() {
    let r = provider("a");
    let t = telemetry(
        "a",
        vec![scoped(
            QuotaScope::Provider,
            QuotaDimension::TokensPerMinute,
            quota(factual(0)),
        )],
    );
    let snapshot = rate(
        "a",
        vec![constraint(
            QuotaScope::Provider,
            QuotaDimension::TokensPerMinute,
            None,
        )],
    );
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&r, Some(&t), Some(&snapshot)).unwrap()],
    );
    assert_eq!(winner(&d), "a");
    assert_eq!(
        d.ranked_candidates[0].evidence.operational_pressure.summary,
        ScarcitySummary {
            known_worst: None,
            has_unknown: true
        }
    );
}
#[test]
fn b2_lr8_telemetry_provider_quota_not_sibling_model_quota() {
    let mut r = provider("a");
    let mut sibling = models_mut(&mut r)[0].clone();
    sibling.id = ModelId::new("model-B").unwrap();
    models_mut(&mut r).push(sibling);
    let t = telemetry(
        "a",
        vec![
            scoped(
                QuotaScope::Model {
                    model: "model-B".into(),
                },
                QuotaDimension::RequestsPerDay,
                quota(factual(0)),
            ),
            scoped(
                QuotaScope::Provider,
                QuotaDimension::RequestsPerMinute,
                quota(factual(80)),
            ),
        ],
    );
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&r, Some(&t), None).unwrap()],
    );
    assert_eq!(winner(&d), "a");
    let p = &d.ranked_candidates[0].evidence.operational_pressure;
    assert_eq!(p.constraints.len(), 1);
    assert_eq!(p.constraints[0].scope, PressureScope::Provider);
    assert_eq!(p.summary.known_worst, Some(ScarcityState::Comfortable));
}
#[test]
fn b2_lr8_sources_and_reset_evidence_preserved_without_refill() {
    let r = provider("a");
    let mut c = constraint(
        QuotaScope::Provider,
        QuotaDimension::RequestsPerMinute,
        Some(5),
    );
    c.source = ConstraintSource::LocalPolicy;
    let mut daily = constraint(
        QuotaScope::Provider,
        QuotaDimension::RequestsPerDay,
        Some(80),
    );
    daily.source = ConstraintSource::DailyBudget;
    let snapshot = rate("a", vec![daily, c]);
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&r, None, Some(&snapshot)).unwrap()],
    );
    let p = &d.ranked_candidates[0].evidence.operational_pressure;
    assert_eq!(p.constraints[0].source, PressureSource::RateLocalPolicy);
    assert_eq!(p.constraints[1].source, PressureSource::RateDailyBudget);
    assert_eq!(p.constraints[0].reset_in_ms, Some(999));
    assert_eq!(p.constraints[0].reset_unix_ms, Some(1042));
}
#[test]
fn b2_429_retry_hint_and_scarcity_never_authorize_spend() {
    let mut r = provider("paid");
    r.economics.billing_kind = known(BillingKind::MeteredBilling);
    execution(&mut r).monetary_cost = known(money("USD", 1));
    let mut t = telemetry("paid", vec![]);
    t.retry_hint = factual(Timing::DelayMs(429));
    t.last_outcome = factual(crate::cognition::telemetry::Outcome::Failed {
        code: "rate_limited",
    });
    for remaining in [Some(0), Some(5), Some(80), None] {
        let snapshot = rate(
            "paid",
            vec![constraint(
                QuotaScope::Provider,
                QuotaDimension::RequestsPerDay,
                remaining,
            )],
        );
        let d = score_request(
            &b1(&[&r], policy(AllocationProfile::Fast), Some(2)),
            vec![EconomicContext::capture(&r, Some(&t), Some(&snapshot)).unwrap()],
        );
        exclusion(&d, EconomicExclusion::PaidUseDenied);
        assert_eq!(d.policy.paid_use, PaidUsePolicy::Deny);
    }
}
#[test]
fn b2_unresolved_b1_rejected_by_request() {
    let mut r = resource("a");
    r.availability = CatalogFact::Unknown;
    let request = b1(&[&r], policy(AllocationProfile::Economy), Some(2));
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![select(
                &request.candidates()[0],
                CandidateSignals::default()
            )],
            vec![]
        ),
        Err(ScoringError::B1Unresolved)
    ));
}
#[test]
fn b2_ineligible_b1_rejected_by_request_even_when_cheap() {
    let mut r = resource("a");
    r.enabled = known(false);
    execution(&mut r).relative_cost = known(RelativeCostTier::new(0).unwrap());
    let request = b1(&[&r], policy(AllocationProfile::Economy), Some(2));
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![select(
                &request.candidates()[0],
                CandidateSignals::default()
            )],
            vec![]
        ),
        Err(ScoringError::B1Ineligible)
    ));
}
#[test]
fn b2_variant_outside_b1_universe_rejected() {
    let a = resource("a");
    let b = resource("b");
    let request = b1(&[&a], policy(AllocationProfile::Economy), Some(2));
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![select(&candidate(&b, false), CandidateSignals::default())],
            vec![]
        ),
        Err(ScoringError::CandidateNotInB1Universe)
    ));
}
#[test]
fn b2_duplicate_selected_variant_rejected() {
    let a = resource("a");
    let request = b1(&[&a], policy(AllocationProfile::Economy), Some(2));
    let s = select(&request.candidates()[0], CandidateSignals::default());
    assert!(matches!(
        AllocationScoringRequest::new(&request, vec![s.clone(), s], vec![]),
        Err(ScoringError::DuplicateCandidate)
    ));
}
#[test]
fn b2_wrong_provider_context_rejected_at_capture() {
    let r = provider("a");
    let snapshot = rate("b", vec![]);
    let t = telemetry("b", vec![]);
    assert!(matches!(
        EconomicContext::capture(&r, None, Some(&snapshot)),
        Err(ScoringError::Lr8ResourceMismatch)
    ));
    assert!(matches!(
        EconomicContext::capture(&r, Some(&t), None),
        Err(ScoringError::Lr8ResourceMismatch)
    ));
}
#[test]
fn b2_provider_context_cannot_attach_to_local_or_agent() {
    let snapshot = rate("a", vec![]);
    let mut r = resource("a");
    assert!(matches!(
        EconomicContext::capture(&r, None, Some(&snapshot)),
        Err(ScoringError::Lr8ResourceMismatch)
    ));
    r.identity.class = ResourceClass::SpecialistAgent;
    r.origin = ResourceOrigin::Agent(RuntimeId::new("a").unwrap());
    assert!(matches!(
        EconomicContext::capture(&r, None, Some(&snapshot)),
        Err(ScoringError::Lr8ResourceMismatch)
    ));
}
#[test]
fn b2_same_resource_changed_economic_snapshot_rejected() {
    let a = resource("a");
    let mut changed = a.clone();
    changed.economics.billing_kind = known(BillingKind::FreeTier);
    let request = b1(&[&a], policy(AllocationProfile::Economy), Some(2));
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![select(
                &request.candidates()[0],
                CandidateSignals::default()
            )],
            vec![EconomicContext::capture(&changed, None, None).unwrap()]
        ),
        Err(ScoringError::ConflictingResourceSnapshot)
    ));
}
#[test]
fn b2_same_resource_changed_noneconomic_snapshot_also_rejected() {
    let a = resource("a");
    let mut changed = a.clone();
    execution(&mut changed).latency_ms = known(100);
    let request = b1(&[&a], policy(AllocationProfile::Economy), Some(2));
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![select(
                &request.candidates()[0],
                CandidateSignals::default()
            )],
            vec![EconomicContext::capture(&changed, None, None).unwrap()]
        ),
        Err(ScoringError::ConflictingResourceSnapshot)
    ));
}
#[test]
fn b2_equal_cloned_descriptor_context_valid() {
    let a = resource("a");
    let clone = a.clone();
    let d = score_request(
        &b1(&[&a], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&clone, None, None).unwrap()],
    );
    assert_eq!(winner(&d), "a");
}
#[test]
fn b2_duplicate_context_rejected() {
    let a = resource("a");
    let request = b1(&[&a], policy(AllocationProfile::Economy), Some(2));
    let c = EconomicContext::capture(&a, None, None).unwrap();
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![select(
                &request.candidates()[0],
                CandidateSignals::default()
            )],
            vec![c.clone(), c]
        ),
        Err(ScoringError::DuplicateEconomicContext)
    ));
}
#[test]
fn b2_context_not_in_selected_universe_rejected() {
    let a = resource("a");
    let b = resource("b");
    let request = b1(&[&a], policy(AllocationProfile::Economy), Some(2));
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![select(
                &request.candidates()[0],
                CandidateSignals::default()
            )],
            vec![EconomicContext::capture(&b, None, None).unwrap()]
        ),
        Err(ScoringError::UnexpectedEconomicContext)
    ));
}
#[test]
fn b2_lr8_generation_mismatch_rejected() {
    let a = provider("a");
    let mut t = telemetry("a", vec![]);
    t.context_generation = 1;
    let snapshot = rate("a", vec![]);
    assert!(matches!(
        EconomicContext::capture(&a, Some(&t), Some(&snapshot)),
        Err(ScoringError::Lr8ContextMismatch)
    ));
}
#[test]
fn b2_duplicate_raw_constraints_and_telemetry_scopes_rejected() {
    let a = provider("a");
    let c = constraint(
        QuotaScope::Provider,
        QuotaDimension::RequestsPerDay,
        Some(5),
    );
    let snapshot = rate("a", vec![c.clone(), c]);
    assert!(matches!(
        EconomicContext::capture(&a, None, Some(&snapshot)),
        Err(ScoringError::InvalidLr8Evidence)
    ));
    let q = scoped(
        QuotaScope::Provider,
        QuotaDimension::RequestsPerDay,
        quota(factual(5)),
    );
    let t = telemetry("a", vec![q.clone(), q]);
    assert!(matches!(
        EconomicContext::capture(&a, Some(&t), None),
        Err(ScoringError::InvalidLr8Evidence)
    ));
}
#[test]
fn b2_raw_lr8_bounds_and_impossible_pairs_rejected() {
    let a = provider("a");
    for remaining in [Some(101), Some(MAX_FACT_VALUE + 1)] {
        let snapshot = rate(
            "a",
            vec![constraint(
                QuotaScope::Provider,
                QuotaDimension::RequestsPerDay,
                remaining,
            )],
        );
        assert!(matches!(
            EconomicContext::capture(&a, None, Some(&snapshot)),
            Err(ScoringError::InvalidLr8Evidence)
        ));
    }
    let t = telemetry(
        "a",
        vec![scoped(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            quota(factual(101)),
        )],
    );
    assert!(matches!(
        EconomicContext::capture(&a, Some(&t), None),
        Err(ScoringError::InvalidLr8Evidence)
    ));
}

#[test]
fn b2_identical_evidence_and_signals_repeated_decisions_are_identical() {
    let mut a = resource("a");
    let mut b = resource("b");
    reserve(&mut a);
    comfortable(&mut b);
    let request = b1(&[&a, &b], policy(AllocationProfile::Balanced), Some(2));
    let scoring = AllocationScoringRequest::new(
        &request,
        vec![
            select(
                &request.candidates()[0],
                signals(Some(1), Some(3), Some(5), Some(20)),
            ),
            select(
                &request.candidates()[1],
                signals(Some(0), Some(10), Some(0), Some(30)),
            ),
        ],
        vec![],
    )
    .unwrap();
    let expected = scoring.decide();
    for _ in 0..100 {
        assert_eq!(scoring.decide(), expected);
    }
}
#[test]
fn b2_incidental_candidate_context_and_lr8_constraint_order_independent() {
    let a = provider("a");
    let b = provider("b");
    let mut snapshots = [
        rate(
            "a",
            vec![
                constraint(
                    QuotaScope::Provider,
                    QuotaDimension::RequestsPerDay,
                    Some(5),
                ),
                constraint(
                    QuotaScope::Model {
                        model: "model-A".into(),
                    },
                    QuotaDimension::RequestsPerMinute,
                    Some(80),
                ),
            ],
        ),
        rate("b", vec![]),
    ];
    let d1 = score_request(
        &b1(&[&a, &b], policy(AllocationProfile::Economy), Some(2)),
        vec![
            EconomicContext::capture(&a, None, Some(&snapshots[0])).unwrap(),
            EconomicContext::capture(&b, None, Some(&snapshots[1])).unwrap(),
        ],
    );
    snapshots[0].constraints.reverse();
    let d2 = score_request(
        &b1(&[&b, &a], policy(AllocationProfile::Economy), Some(2)),
        vec![
            EconomicContext::capture(&b, None, Some(&snapshots[1])).unwrap(),
            EconomicContext::capture(&a, None, Some(&snapshots[0])).unwrap(),
        ],
    );
    assert_eq!(d1, d2);
}
#[test]
fn b2_allowance_fact_vec_and_capability_map_order_independent() {
    let mut a = resource("a");
    reserve(&mut a);
    allowance(
        &mut a,
        "rolling",
        AllowanceUnit::Tokens,
        known(1),
        known(100),
        known(80),
    );
    models_mut(&mut a)[0]
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(true));
    models_mut(&mut a)[0]
        .capabilities
        .0
        .insert(CognitiveCapability::Streaming, known(true));
    let first = decide(&[&a], AllocationProfile::Economy);
    a.economics.allowances.reverse();
    execution(&mut a).allowance_costs.reverse();
    let old = models_mut(&mut a)[0].capabilities.0.clone();
    models_mut(&mut a)[0].capabilities.0.clear();
    for (k, v) in old.into_iter().rev() {
        models_mut(&mut a)[0].capabilities.0.insert(k, v);
    }
    assert_eq!(decide(&[&a], AllocationProfile::Economy), first);
}
#[test]
fn b2_policy_ordinal_is_explicit_and_can_change_winner() {
    let a = resource("a");
    let b = resource("b");
    let request = b1(&[&a, &b], policy(AllocationProfile::Balanced), Some(2));
    for preferred in [0, 1] {
        let d = AllocationScoringRequest::new(
            &request,
            request
                .candidates()
                .iter()
                .enumerate()
                .map(|(n, c)| {
                    select(
                        c,
                        signals(Some(u16::from(n != preferred)), None, None, None),
                    )
                })
                .collect(),
            vec![],
        )
        .unwrap()
        .decide();
        assert_eq!(winner(&d), if preferred == 0 { "a" } else { "b" });
    }
}
#[test]
fn b2_policy_ordinal_tie_break_and_missing_ordinal_last() {
    let a = resource("a");
    let b = resource("b");
    let request = b1(&[&a, &b], policy(AllocationProfile::Economy), Some(2));
    // A ordinal 1 costs 4, continuity 2 supplies 4; totals tie at zero.
    let d = AllocationScoringRequest::new(
        &request,
        vec![
            select(
                &request.candidates()[0],
                signals(Some(1), None, Some(2), None),
            ),
            select(&request.candidates()[1], signals(Some(0), None, None, None)),
        ],
        vec![],
    )
    .unwrap()
    .decide();
    assert_eq!(winner(&d), "b");
    assert_eq!(d.tie_break, TieBreakReason::PolicyOrdinal);
    let d = AllocationScoringRequest::new(
        &request,
        vec![
            select(&request.candidates()[0], CandidateSignals::default()),
            select(&request.candidates()[1], signals(Some(0), None, None, None)),
        ],
        vec![],
    )
    .unwrap()
    .decide();
    assert_eq!(winner(&d), "b");
    assert_eq!(d.tie_break, TieBreakReason::PolicyOrdinal);
}
#[test]
fn b2_registry_priority_continuity_and_switching_are_independent_components() {
    let a = resource("a");
    let request = b1(&[&a], policy(AllocationProfile::Balanced), Some(2));
    let d = AllocationScoringRequest::new(
        &request,
        vec![select(
            &request.candidates()[0],
            signals(Some(3), Some(10), Some(5), Some(8)),
        )],
        vec![],
    )
    .unwrap()
    .decide();
    let s = &d.ranked_candidates[0].score_breakdown;
    assert_eq!(s.policy_preference.value, -18);
    assert_eq!(s.registry_preference.value, 2);
    assert_eq!(s.continuity.value, 20);
    assert_eq!(s.switching.value, -32);
    assert_eq!(s.total, -28);
}
#[test]
fn b2_score_formula_auditable_with_maximum_inputs_each_profile() {
    let mut a = resource("a");
    reserve(&mut a);
    execution(&mut a).relative_cost = known(RelativeCostTier::new(255).unwrap());
    execution(&mut a).latency_ms = known(MAX_FACT_VALUE);
    execution(&mut a).monetary_cost = known(money("USD", MAX_FACT_VALUE));
    for profile in [
        AllocationProfile::Economy,
        AllocationProfile::Balanced,
        AllocationProfile::Fast,
    ] {
        let mut p = policy(profile);
        p.paid_use = PaidUsePolicy::AllowKnownCostWithinBudget {
            budget: money("USD", MAX_FACT_VALUE),
        };
        let request = b1(&[&a], p, Some(2));
        let d = AllocationScoringRequest::new(
            &request,
            vec![select(
                &request.candidates()[0],
                signals(Some(255), Some(32), Some(100), Some(100)),
            )],
            vec![],
        )
        .unwrap()
        .decide();
        let s = &d.ranked_candidates[0].score_breakdown;
        let w = profile.scoring_weights();
        let expected = -255 * w.policy_preference + 100 * w.continuity
            - 100 * w.switching
            - 255 * w.relative_cost
            - w.reserve
            - 100 * w.monetary_cost
            - 100 * w.latency;
        assert_eq!(s.total, expected);
        assert_eq!(s.component_sum(), expected);
        let checked = [
            s.policy_preference.value,
            s.registry_preference.value,
            s.continuity.value,
            s.switching.value,
            s.relative_cost.value,
            s.scarcity.value,
            s.monetary_cost.value,
            s.latency.value,
        ]
        .into_iter()
        .try_fold(0i64, i64::checked_add)
        .unwrap();
        assert_eq!(checked, s.total);
        assert!(s.total.abs() < 20000);
    }
}
#[test]
fn b2_maximum_latency_clamp_does_not_overflow_or_reverse_ordering() {
    let mut a = resource("a");
    let mut b = resource("b");
    execution(&mut a).latency_ms = known(0);
    execution(&mut b).latency_ms = known(MAX_FACT_VALUE);
    let d = decide(&[&b, &a], AllocationProfile::Fast);
    assert_eq!(winner(&d), "a");
    assert_eq!(d.ranked_candidates[0].score_breakdown.latency.value, 1500);
    assert_eq!(d.ranked_candidates[1].score_breakdown.latency.value, -1500);
}
#[test]
fn b2_signal_bounds_fail_closed_and_none_is_possible() {
    assert_eq!(CandidateSignals::default(), signals(None, None, None, None));
    for args in [
        (Some(256), None, None, None),
        (None, Some(33), None, None),
        (None, None, Some(101), None),
        (None, None, None, Some(101)),
        (
            Some(u16::MAX),
            Some(u16::MAX),
            Some(u16::MAX),
            Some(u16::MAX),
        ),
    ] {
        assert_eq!(
            CandidateSignals::new(args.0, args.1, args.2, args.3),
            Err(ScoringError::InvalidSignals)
        );
    }
}
#[test]
fn b2_multiple_variants_same_resource_effort_selection() {
    let mut r = resource("a");
    execution(&mut r).relative_cost = known(RelativeCostTier::new(100).unwrap());
    effort(&mut r).relative_cost = known(RelativeCostTier::new(20).unwrap());
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Economy),
        vec![candidate(&r, false), candidate(&r, true)],
    )
    .unwrap();
    let d = score_request(&request, vec![]);
    assert_eq!(d.ranked_candidates.len(), 2);
    assert_eq!(d.winner.unwrap().effort.unwrap().as_str(), "effort-A");
}
#[test]
fn b2_all_inputs_remain_equal_after_decision_no_debit() {
    let mut r = provider("a");
    reserve(&mut r);
    r.economics.billing_kind = known(BillingKind::PrepaidCredits);
    r.economics.monetary_balance = known(money("USD", 100));
    execution(&mut r).monetary_cost = known(money("USD", 10));
    let snapshot = rate(
        "a",
        vec![constraint(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            Some(80),
        )],
    );
    let t = telemetry(
        "a",
        vec![scoped(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            quota(factual(80)),
        )],
    );
    let mut p = policy(AllocationProfile::Economy);
    p.paid_use = PaidUsePolicy::AllowKnownCostWithinBudget {
        budget: money("USD", 100),
    };
    let request = b1(&[&r], p, Some(2));
    let context = EconomicContext::capture(&r, Some(&t), Some(&snapshot)).unwrap();
    let before_r = r.clone();
    let before_b1 = request.evaluate();
    let before_lr8 = serde_json::to_value(context.lr8()).unwrap();
    let before_snap = serde_json::to_value(&snapshot).unwrap();
    let before_t = serde_json::to_value(&t).unwrap();
    let scoring = AllocationScoringRequest::new(
        &request,
        vec![select(
            &request.candidates()[0],
            CandidateSignals::default(),
        )],
        vec![context.clone()],
    )
    .unwrap();
    for _ in 0..10 {
        assert_eq!(winner(&scoring.decide()), "a");
    }
    assert_eq!(r, before_r);
    assert_eq!(request.evaluate(), before_b1);
    assert_eq!(serde_json::to_value(context.lr8()).unwrap(), before_lr8);
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), before_snap);
    assert_eq!(serde_json::to_value(&t).unwrap(), before_t);
    assert_eq!(r.economics.monetary_balance, known(money("USD", 100)));
    assert_eq!(r.economics.allowances[0].remaining, known(5));
}
#[test]
fn b2_empty_selection_returns_no_winner() {
    let d = score_request(&b1(&[], policy(AllocationProfile::Economy), None), vec![]);
    assert!(d.winner.is_none());
    assert!(d.ranked_candidates.is_empty());
    assert!(d.excluded_candidates.is_empty());
    assert_eq!(d.tie_break, TieBreakReason::NoEconomicCandidate);
}
#[test]
fn b2_request_cardinality_bounds_checked_before_processing() {
    let a = resource("a");
    let request = b1(&[&a], policy(AllocationProfile::Economy), Some(2));
    let selection = select(&request.candidates()[0], CandidateSignals::default());
    assert!(matches!(
        AllocationScoringRequest::new(
            &request,
            vec![selection.clone(); MAX_ALLOCATION_CANDIDATES + 1],
            vec![]
        ),
        Err(ScoringError::TooManyCandidates)
    ));
    let context = EconomicContext::capture(&a, None, None).unwrap();
    assert!(matches!(
        AllocationScoringRequest::new(&request, vec![selection], vec![context; MAX_RESOURCES + 1]),
        Err(ScoringError::TooManyContexts)
    ));
}
#[test]
fn b2_full_bounded_universe_ranked_deterministically() {
    let resources: Vec<_> = (0..MAX_ALLOCATION_CANDIDATES)
        .map(|n| resource(&format!("r{n:03}")))
        .collect();
    let refs: Vec<_> = resources.iter().rev().collect();
    let d = decide(&refs, AllocationProfile::Economy);
    assert_eq!(d.ranked_candidates.len(), MAX_ALLOCATION_CANDIDATES);
    assert_eq!(winner(&d), "r000");
    assert!(d
        .ranked_candidates
        .windows(2)
        .all(|p| p[0].variant < p[1].variant));
}
#[test]
fn b2_max_model_effort_allowance_union_keeps_32_dimensions_bounded() {
    let mut r = resource("a");
    for n in 0..MAX_ALLOWANCES {
        execution(&mut r).allowance_costs.push(consumption(
            &format!("m{n}"),
            AllowanceUnit::Requests,
            known(1),
        ));
        effort(&mut r).allowance_costs.push(consumption(
            &format!("e{n}"),
            AllowanceUnit::Tokens,
            CatalogFact::Unknown,
        ));
    }
    let request = AllocationRequest::new(
        CandidateRequirements::default(),
        policy(AllocationProfile::Economy),
        vec![candidate(&r, true)],
    )
    .unwrap();
    let d = score_request(&request, vec![]);
    assert_eq!(
        d.ranked_candidates[0].evidence.scarcity.dimensions.len(),
        2 * MAX_ALLOWANCES
    );
    assert!(d.ranked_candidates[0].evidence.scarcity.summary.has_unknown);
}
#[test]
fn b2_decision_json_contains_typed_evidence_not_opaque_quality_or_usage() {
    let mut r = provider("a");
    models_mut(&mut r)[0].facts.quality = known(QualityLabel::new("private-marker").unwrap());
    let mut t = telemetry("a", vec![]);
    t.last_outcome = factual(crate::cognition::telemetry::Outcome::Failed {
        code: "private-marker",
    });
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&r, Some(&t), None).unwrap()],
    );
    let json = serde_json::to_string(&d).unwrap();
    assert!(!json.contains("private-marker"));
    assert!(json.contains("scoreBreakdown"));
    assert!(json.contains("tieBreak"));
    assert!(json.contains("economicEligibility"));
}

#[test]
fn b2_known_exhaustion_dominates_reserve_and_retains_unknown_flag() {
    let mut r = resource("a");
    reserve(&mut r);
    allowance(
        &mut r,
        "rolling",
        AllowanceUnit::Percent,
        known(1),
        known(100),
        known(0),
    );
    allowance(
        &mut r,
        "another",
        AllowanceUnit::Custom(AllowanceUnitId::new("custom").unwrap()),
        CatalogFact::Unknown,
        CatalogFact::Unknown,
        CatalogFact::Unknown,
    );
    let d = decide(&[&r], AllocationProfile::Economy);
    let s = &d.excluded_candidates[0].evidence.scarcity;
    assert_eq!(s.dimensions.len(), 3);
    assert_eq!(
        s.summary,
        ScarcitySummary {
            known_worst: Some(ScarcityState::Exhausted),
            has_unknown: true
        }
    );
}
#[test]
fn b2_fraction_floor_zero_is_not_remaining_zero_and_threshold_extremes() {
    let mut r = resource("a");
    allowance(
        &mut r,
        "x",
        AllowanceUnit::Requests,
        known(1),
        known(1000),
        known(1),
    );
    for (policy_reserve, expected) in [
        (
            ReservePolicy::new(0, 0).unwrap(),
            ScarcityState::Comfortable,
        ),
        (
            ReservePolicy::new(100, 100).unwrap(),
            ScarcityState::Reserve,
        ),
    ] {
        let mut p = policy(AllocationProfile::Economy);
        p.reserve = Some(policy_reserve);
        let d = score_request(&b1(&[&r], p, Some(2)), vec![]);
        assert_eq!(
            d.ranked_candidates[0].evidence.scarcity.dimensions[0].remaining_percent,
            Some(0)
        );
        assert_eq!(
            d.ranked_candidates[0].evidence.scarcity.dimensions[0].state,
            expected
        );
    }
}
#[test]
fn b2_telemetry_reset_is_historical_evidence_not_countdown() {
    let r = provider("a");
    let t = telemetry(
        "a",
        vec![scoped(
            QuotaScope::Provider,
            QuotaDimension::RequestsPerDay,
            quota(factual(80)),
        )],
    );
    let d = score_request(
        &b1(&[&r], policy(AllocationProfile::Economy), Some(2)),
        vec![EconomicContext::capture(&r, Some(&t), None).unwrap()],
    );
    let c = &d.ranked_candidates[0]
        .evidence
        .operational_pressure
        .constraints[0];
    assert_eq!(c.reset_in_ms, None);
    assert_eq!(c.reset_unix_ms, None);
    assert_eq!(
        c.external_evidence.as_ref().unwrap().reset,
        factual(Timing::DelayMs(1000))
    );
}

#[test]
fn b2_registry_is_secondary_to_one_policy_ordinal_step() {
    let a = resource("a");
    let b = resource("b");
    for profile in [
        AllocationProfile::Economy,
        AllocationProfile::Balanced,
        AllocationProfile::Fast,
    ] {
        let request = b1(&[&a, &b], policy(profile), Some(2));
        let d = AllocationScoringRequest::new(
            &request,
            vec![
                select(
                    &request.candidates()[0],
                    signals(Some(0), Some(32), None, None),
                ),
                select(
                    &request.candidates()[1],
                    signals(Some(1), Some(0), None, None),
                ),
            ],
            vec![],
        )
        .unwrap()
        .decide();
        assert_eq!(winner(&d), "a");
        assert!(
            d.ranked_candidates[1]
                .score_breakdown
                .registry_preference
                .value
                < profile.scoring_weights().policy_preference
        );
    }
}
