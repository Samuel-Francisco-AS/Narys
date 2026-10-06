//! Synthetic B1 gate: descriptors only; no backend, IO, clock or commercial data.
use super::*;
use crate::cognition::telemetry::MAX_FACT_VALUE;

fn known<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, Some(42)).unwrap()
}
fn tier(value: u16) -> CognitiveTier {
    CognitiveTier::new(value).unwrap()
}
fn requirement(capability: CognitiveCapability, scope: CapabilityScope) -> CapabilityRequirement {
    CapabilityRequirement { capability, scope }
}
fn requirements(floor: Option<u16>) -> CandidateRequirements {
    CandidateRequirements::new(vec![], floor.map(tier)).unwrap()
}
fn vision() -> CandidateRequirements {
    CandidateRequirements::new(
        vec![requirement(
            CognitiveCapability::Vision,
            CapabilityScope::Model,
        )],
        None,
    )
    .unwrap()
}
fn fixture() -> CognitiveResource {
    let mut model = ModelProfile::unknown(ModelId::new("model-A").unwrap());
    model.availability = known(Availability::Available);
    model
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(true));
    model.facts.execution.cognitive_tier = known(tier(2));
    model.supported_efforts = known(vec![EffortProfile {
        id: EffortId::new("effort-A").unwrap(),
        availability: known(Availability::Available),
        facts: ExecutionFacts::default(),
    }]);
    CognitiveResource {
        identity: ResourceIdentity {
            id: ResourceId::new("resource-A").unwrap(),
            class: ResourceClass::LocalSupport,
            family: ProviderFamily::new("family-A").unwrap(),
            access_path: AccessPath::new("path-A").unwrap(),
            billing_domain: BillingDomain {
                id: BillingDomainId::new("domain-A").unwrap(),
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
fn model_mut(resource: &mut CognitiveResource) -> &mut ModelProfile {
    let CatalogFact::Known { value, .. } = &mut resource.models else {
        panic!("fixture model")
    };
    &mut value[0]
}
fn effort_mut(resource: &mut CognitiveResource) -> &mut EffortProfile {
    let CatalogFact::Known { value, .. } = &mut model_mut(resource).supported_efforts else {
        panic!("fixture effort")
    };
    &mut value[0]
}
fn candidate<'a>(resource: &'a CognitiveResource, effort: Option<&str>) -> AllocationCandidate<'a> {
    AllocationCandidate::new(
        resource,
        ModelId::new("model-A").unwrap(),
        effort.map(|id| EffortId::new(id).unwrap()),
    )
    .unwrap()
}
fn evaluate(
    resource: &CognitiveResource,
    effort: Option<&str>,
    req: CandidateRequirements,
) -> CandidateEligibility {
    AllocationRequest::new(
        req,
        AllocationPolicy::default(),
        vec![candidate(resource, effort)],
    )
    .unwrap()
    .evaluate()
    .candidates()[0]
        .clone()
}

#[test]
fn b1_capability_true_proves_model_gate() {
    let result = evaluate(&fixture(), None, vision());
    assert_eq!(result.status(), EligibilityStatus::Eligible);
    assert!(result.reasons().is_empty());
    assert_eq!(result.capability_evidence()[0].model, known(true));
    assert_eq!(
        result.capability_evidence()[0].resource,
        CatalogFact::Unknown
    );
}
#[test]
fn b1_capability_false_is_explicitly_unsupported() {
    let mut resource = fixture();
    model_mut(&mut resource)
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(false));
    let result = evaluate(&resource, None, vision());
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::CapabilityUnsupported {
            requirement: vision().required_capabilities()[0],
            layer: EvidenceLayer::Model,
        }]
    );
}
#[test]
fn b1_capability_unknown_never_proves_sufficiency() {
    let mut resource = fixture();
    model_mut(&mut resource).capabilities = CapabilitySet::default();
    let result = evaluate(&resource, None, vision());
    assert_eq!(result.status(), EligibilityStatus::Unresolved);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::CapabilityUnknown {
            requirement: vision().required_capabilities()[0],
            layer: EvidenceLayer::Model,
        }]
    );
}
#[test]
fn b1_quality_equal_to_floor_is_sufficient() {
    let result = evaluate(&fixture(), None, requirements(Some(2)));
    assert_eq!(result.status(), EligibilityStatus::Eligible);
    assert_eq!(result.effective_tier().fact, known(tier(2)));
    assert_eq!(result.effective_tier().layer, Some(EvidenceLayer::Model));
}
#[test]
fn b1_quality_below_floor_excludes() {
    let mut resource = fixture();
    model_mut(&mut resource).facts.execution.cognitive_tier = known(tier(1));
    let result = evaluate(&resource, None, requirements(Some(2)));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::CognitiveTierBelowFloor]
    );
}
#[test]
fn b1_quality_unknown_with_floor_is_unresolved() {
    let mut resource = fixture();
    model_mut(&mut resource).facts.execution.cognitive_tier = CatalogFact::Unknown;
    let result = evaluate(&resource, None, requirements(Some(2)));
    assert_eq!(result.status(), EligibilityStatus::Unresolved);
    assert_eq!(result.reasons(), &[EligibilityReason::CognitiveTierUnknown]);
    assert_eq!(result.effective_tier().fact, CatalogFact::Unknown);
    assert_eq!(result.effective_tier().layer, None);
}
#[test]
fn b1_quality_unknown_without_floor_does_not_exclude() {
    let mut resource = fixture();
    model_mut(&mut resource).facts.execution.cognitive_tier = CatalogFact::Unknown;
    let result = evaluate(&resource, None, requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Eligible);
    assert!(result.reasons().is_empty());
}
#[test]
fn b1_effort_specific_tier_is_used_without_addition() {
    let mut resource = fixture();
    effort_mut(&mut resource).facts.cognitive_tier = known(tier(4));
    let result = evaluate(&resource, Some("effort-A"), requirements(Some(4)));
    assert_eq!(result.status(), EligibilityStatus::Eligible);
    assert_eq!(result.effective_tier().fact, known(tier(4)));
    assert_eq!(result.effective_tier().layer, Some(EvidenceLayer::Effort));
    assert_eq!(
        evaluate(&resource, Some("effort-A"), requirements(Some(5))).status(),
        EligibilityStatus::Ineligible
    );
    assert_eq!(
        resource
            .model(&ModelId::new("model-A").unwrap())
            .unwrap()
            .facts
            .execution
            .cognitive_tier,
        known(tier(2))
    );
}
#[test]
fn b1_effort_unknown_tier_uses_model_assessment_without_mutation() {
    let mut resource = fixture();
    model_mut(&mut resource).facts.execution.cognitive_tier = known(tier(3));
    let before = resource.clone();
    let result = evaluate(&resource, Some("effort-A"), requirements(Some(3)));
    assert_eq!(result.status(), EligibilityStatus::Eligible);
    assert_eq!(result.effective_tier().fact, known(tier(3)));
    assert_eq!(result.effective_tier().layer, Some(EvidenceLayer::Model));
    assert_eq!(resource, before);
    assert_eq!(
        effort_mut(&mut resource).facts.cognitive_tier,
        CatalogFact::Unknown
    );
}
#[test]
fn b1_known_lower_effort_tier_cannot_be_masked_by_higher_model() {
    let mut resource = fixture();
    model_mut(&mut resource).facts.execution.cognitive_tier = known(tier(4));
    effort_mut(&mut resource).facts.cognitive_tier = known(tier(1));
    let result = evaluate(&resource, Some("effort-A"), requirements(Some(2)));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(result.effective_tier().fact, known(tier(1)));
}
#[test]
fn b1_resource_unavailable_excludes() {
    let mut resource = fixture();
    resource.availability = known(Availability::Unavailable);
    let result = evaluate(&resource, None, requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::AvailabilityUnavailable {
            layer: EvidenceLayer::Resource
        }]
    );
}
#[test]
fn b1_model_unavailable_excludes() {
    let mut resource = fixture();
    model_mut(&mut resource).availability = known(Availability::Unavailable);
    let result = evaluate(&resource, None, requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::AvailabilityUnavailable {
            layer: EvidenceLayer::Model
        }]
    );
}
#[test]
fn b1_effort_unavailable_excludes_only_when_selected() {
    let mut resource = fixture();
    effort_mut(&mut resource).availability = known(Availability::Unavailable);
    let result = evaluate(&resource, Some("effort-A"), requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::AvailabilityUnavailable {
            layer: EvidenceLayer::Effort
        }]
    );
    let unselected = evaluate(&resource, None, requirements(None));
    assert_eq!(unselected.status(), EligibilityStatus::Eligible);
    assert_eq!(unselected.availability_evidence().effort, None);
}
#[test]
fn b1_availability_unknown_is_preserved_at_each_layer() {
    for layer in [
        EvidenceLayer::Resource,
        EvidenceLayer::Model,
        EvidenceLayer::Effort,
    ] {
        let mut resource = fixture();
        match layer {
            EvidenceLayer::Resource => resource.availability = CatalogFact::Unknown,
            EvidenceLayer::Model => model_mut(&mut resource).availability = CatalogFact::Unknown,
            EvidenceLayer::Effort => effort_mut(&mut resource).availability = CatalogFact::Unknown,
        }
        let result = evaluate(&resource, Some("effort-A"), requirements(None));
        assert_eq!(result.status(), EligibilityStatus::Unresolved);
        assert_eq!(
            result.reasons(),
            &[EligibilityReason::AvailabilityUnknown { layer }]
        );
        let evidence = result.availability_evidence();
        let fact = match layer {
            EvidenceLayer::Resource => &evidence.resource,
            EvidenceLayer::Model => &evidence.model,
            EvidenceLayer::Effort => evidence.effort.as_ref().unwrap(),
        };
        assert_eq!(*fact, CatalogFact::Unknown);
    }
}
#[test]
fn b1_disabled_excludes_without_changing_availability() {
    let mut resource = fixture();
    resource.enabled = known(false);
    let result = evaluate(&resource, None, requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(result.reasons(), &[EligibilityReason::ResourceDisabled]);
    assert_eq!(
        result.availability_evidence().resource,
        known(Availability::Available)
    );
}
#[test]
fn b1_enabled_unknown_is_unresolved() {
    let mut resource = fixture();
    resource.enabled = CatalogFact::Unknown;
    let result = evaluate(&resource, None, requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Unresolved);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::ResourceEnabledUnknown]
    );
}
#[test]
fn b1_absent_effort_is_not_supported_even_when_model_tier_suffices() {
    let result = evaluate(&fixture(), Some("not-declared"), requirements(Some(2)));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(result.reasons(), &[EligibilityReason::EffortNotSupported]);
    let mut resource = fixture();
    model_mut(&mut resource).supported_efforts = known(vec![]);
    assert_eq!(
        evaluate(&resource, Some("effort-A"), requirements(None)).reasons(),
        &[EligibilityReason::EffortNotSupported]
    );
}
#[test]
fn b1_effort_catalog_unknown_does_not_prove_support() {
    let mut resource = fixture();
    model_mut(&mut resource).supported_efforts = CatalogFact::Unknown;
    let result = evaluate(&resource, Some("effort-A"), requirements(Some(2)));
    assert_eq!(result.status(), EligibilityStatus::Unresolved);
    assert_eq!(result.reasons(), &[EligibilityReason::EffortSupportUnknown]);
    assert_eq!(
        result.availability_evidence().effort,
        Some(CatalogFact::Unknown)
    );
    assert_eq!(
        evaluate(&resource, None, requirements(None)).status(),
        EligibilityStatus::Eligible
    );
}
#[test]
fn b1_model_catalog_unknown_and_known_absence_are_distinct() {
    let mut resource = fixture();
    resource.models = CatalogFact::Unknown;
    let result = evaluate(&resource, None, requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Unresolved);
    assert_eq!(result.reasons(), &[EligibilityReason::ModelSupportUnknown]);
    assert_eq!(result.availability_evidence().model, CatalogFact::Unknown);
    resource.models = known(vec![]);
    let result = evaluate(&resource, None, requirements(None));
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(result.reasons(), &[EligibilityReason::ModelNotSupported]);
}
#[test]
fn b1_multiple_failures_have_canonical_order_and_contradiction_precedence() {
    use CognitiveCapability::{TextGeneration, Vision};
    let mut resource = fixture();
    resource.enabled = known(false);
    resource.availability = CatalogFact::Unknown;
    model_mut(&mut resource).availability = known(Availability::Unavailable);
    effort_mut(&mut resource).availability = CatalogFact::Unknown;
    model_mut(&mut resource).capabilities = CapabilitySet::default();
    model_mut(&mut resource)
        .capabilities
        .0
        .insert(Vision, known(false));
    let a = requirement(TextGeneration, CapabilityScope::Model);
    let b = requirement(Vision, CapabilityScope::Model);
    let expected = vec![
        EligibilityReason::ResourceDisabled,
        EligibilityReason::AvailabilityUnknown {
            layer: EvidenceLayer::Resource,
        },
        EligibilityReason::AvailabilityUnavailable {
            layer: EvidenceLayer::Model,
        },
        EligibilityReason::AvailabilityUnknown {
            layer: EvidenceLayer::Effort,
        },
        EligibilityReason::CapabilityUnknown {
            requirement: a,
            layer: EvidenceLayer::Model,
        },
        EligibilityReason::CapabilityUnsupported {
            requirement: b,
            layer: EvidenceLayer::Model,
        },
        EligibilityReason::CognitiveTierBelowFloor,
    ];
    for caps in [vec![b, a], vec![a, b]] {
        let req = CandidateRequirements::new(caps, Some(tier(3))).unwrap();
        let result = evaluate(&resource, Some("effort-A"), req.clone());
        assert_eq!(result.status(), EligibilityStatus::Ineligible);
        assert_eq!(result.reasons(), expected);
        assert_eq!(result, evaluate(&resource, Some("effort-A"), req));
    }
}
#[test]
fn b1_economics_do_not_change_eligibility_or_report() {
    let mut resource = fixture();
    let original = evaluate(&resource, Some("effort-A"), vision());
    // Unknown cost, allowance, latency and relative cost already exist in fixture.
    for execution in [
        &resource
            .model(&ModelId::new("model-A").unwrap())
            .unwrap()
            .facts
            .execution,
        &resource
            .model(&ModelId::new("model-A").unwrap())
            .unwrap()
            .effort(&EffortId::new("effort-A").unwrap())
            .unwrap()
            .facts,
    ] {
        assert_eq!(execution.monetary_cost, CatalogFact::Unknown);
        assert_eq!(execution.latency_ms, CatalogFact::Unknown);
        assert_eq!(execution.relative_cost, CatalogFact::Unknown);
        assert!(execution.allowance_costs.is_empty());
    }
    resource.economics.allowances.push(AllowanceState::unknown(
        AllowanceDimensionId::new("dimension-A").unwrap(),
        AllowanceUnit::Credits,
    ));
    assert_eq!(original, evaluate(&resource, Some("effort-A"), vision()));
    resource.economics.billing_kind = known(BillingKind::MeteredBilling);
    resource.economics.monetary_balance = known(MonetaryAmount::new("USD", 0).unwrap());
    resource.economics.allowances[0].limit = known(100);
    resource.economics.allowances[0].remaining = known(0);
    {
        let execution = &mut model_mut(&mut resource).facts.execution;
        execution.monetary_cost = known(MonetaryAmount::new("USD", 123).unwrap());
        execution.latency_ms = known(MAX_FACT_VALUE);
        execution.relative_cost = known(RelativeCostTier::new(255).unwrap());
        execution.allowance_costs.push(AllowanceConsumption {
            dimension_id: AllowanceDimensionId::new("dimension-A").unwrap(),
            unit: AllowanceUnit::Credits,
            amount: known(100),
        });
    }
    effort_mut(&mut resource).facts.monetary_cost = known(MonetaryAmount::new("BRL", 456).unwrap());
    effort_mut(&mut resource).facts.latency_ms = known(0);
    effort_mut(&mut resource).facts.relative_cost = known(RelativeCostTier::new(0).unwrap());
    assert_eq!(original, evaluate(&resource, Some("effort-A"), vision()));
    // Even with default paid Deny: B1 validates configuration, not spend eligibility.
    assert_eq!(original.status(), EligibilityStatus::Eligible);
}
#[test]
fn b1_resource_true_never_becomes_model_proof() {
    let mut resource = fixture();
    resource
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(true));
    model_mut(&mut resource).capabilities = CapabilitySet::default();
    let result = evaluate(&resource, None, vision());
    assert_eq!(result.status(), EligibilityStatus::Unresolved);
    assert_eq!(result.capability_evidence()[0].resource, known(true));
    assert_eq!(result.capability_evidence()[0].model, CatalogFact::Unknown);
    model_mut(&mut resource)
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(false));
    assert_eq!(
        evaluate(&resource, None, vision()).status(),
        EligibilityStatus::Ineligible
    );
}
#[test]
fn b1_runtime_scope_uses_only_runtime_proof() {
    let mut resource = fixture();
    model_mut(&mut resource)
        .capabilities
        .0
        .insert(CognitiveCapability::CommandExecution, known(false));
    let req = CandidateRequirements::new(
        vec![requirement(
            CognitiveCapability::CommandExecution,
            CapabilityScope::Runtime,
        )],
        None,
    )
    .unwrap();
    for (value, expected) in [
        (CatalogFact::Unknown, EligibilityStatus::Unresolved),
        (known(false), EligibilityStatus::Ineligible),
        (known(true), EligibilityStatus::Eligible),
    ] {
        resource
            .capabilities
            .0
            .insert(CognitiveCapability::CommandExecution, value);
        assert_eq!(evaluate(&resource, None, req.clone()).status(), expected);
    }
}

#[test]
fn b1_opaque_quality_and_effort_labels_never_prove_a_tier() {
    let mut resource = fixture();
    model_mut(&mut resource).facts.execution.cognitive_tier = CatalogFact::Unknown;
    model_mut(&mut resource).facts.quality = known(QualityLabel::new("high").unwrap());
    effort_mut(&mut resource).id = EffortId::new("xhigh").unwrap();
    let result = evaluate(&resource, Some("xhigh"), requirements(Some(4)));
    assert_eq!(result.status(), EligibilityStatus::Unresolved);
    assert_eq!(result.reasons(), &[EligibilityReason::CognitiveTierUnknown]);
    assert_eq!(result.effective_tier().fact, CatalogFact::Unknown);
}

#[test]
fn b1_resource_classes_do_not_imply_support_or_authority() {
    for (class, origin) in [
        (ResourceClass::LocalSupport, ResourceOrigin::Local),
        (
            ResourceClass::CognitiveProvider,
            ResourceOrigin::Provider(RuntimeId::new("runtime-A").unwrap()),
        ),
        (
            ResourceClass::SpecialistAgent,
            ResourceOrigin::Agent(RuntimeId::new("runtime-A").unwrap()),
        ),
    ] {
        let mut resource = fixture();
        resource.identity.class = class;
        resource.origin = origin;
        assert_eq!(
            evaluate(&resource, None, vision()).status(),
            EligibilityStatus::Eligible
        );
        model_mut(&mut resource).capabilities = CapabilitySet::default();
        assert_eq!(
            evaluate(&resource, None, vision()).status(),
            EligibilityStatus::Unresolved
        );
    }
}
#[test]
fn b1_runtime_false_vetoes_model_true_and_both_layers_can_be_required() {
    let mut resource = fixture();
    resource
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(false));
    let result = evaluate(&resource, None, vision());
    assert_eq!(result.status(), EligibilityStatus::Ineligible);
    assert_eq!(
        result.reasons(),
        &[EligibilityReason::CapabilityUnsupported {
            requirement: vision().required_capabilities()[0],
            layer: EvidenceLayer::Resource
        }]
    );
    resource.capabilities = CapabilitySet::default();
    let req = CandidateRequirements::new(
        vec![
            requirement(CognitiveCapability::Vision, CapabilityScope::Model),
            requirement(CognitiveCapability::Vision, CapabilityScope::Runtime),
        ],
        None,
    )
    .unwrap();
    assert_eq!(
        evaluate(&resource, None, req.clone()).status(),
        EligibilityStatus::Unresolved
    );
    resource
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(true));
    assert_eq!(
        evaluate(&resource, None, req).status(),
        EligibilityStatus::Eligible
    );
}
#[test]
fn b1_requirements_are_bounded_unique_and_quality_tiers_stay_bounded() {
    let all = [
        CognitiveCapability::TextGeneration,
        CognitiveCapability::Streaming,
        CognitiveCapability::Vision,
        CognitiveCapability::ToolCalling,
        CognitiveCapability::StructuredOutput,
        CognitiveCapability::Planning,
        CognitiveCapability::RepositoryRead,
        CognitiveCapability::FileWrite,
        CognitiveCapability::CommandExecution,
        CognitiveCapability::ToolUse,
    ];
    let reqs: Vec<_> = all
        .into_iter()
        .flat_map(|cap| {
            [
                requirement(cap, CapabilityScope::Runtime),
                requirement(cap, CapabilityScope::Model),
            ]
        })
        .collect();
    assert_eq!(reqs.len(), MAX_CAPABILITY_REQUIREMENTS);
    assert!(CandidateRequirements::new(reqs.clone(), Some(tier(255))).is_ok());
    let mut excess = reqs.clone();
    excess.push(reqs[0]);
    assert_eq!(
        CandidateRequirements::new(excess, None),
        Err(AllocationError::TooManyRequirements)
    );
    assert_eq!(
        CandidateRequirements::new(vec![reqs[0], reqs[0]], None),
        Err(AllocationError::DuplicateRequirement)
    );
    assert!(CognitiveTier::new(256).is_err());
    assert!(serde_json::from_str::<CognitiveTier>("256").is_err());
}
#[test]
fn b1_policy_contracts_validate_reserve_budget_and_conservative_defaults() {
    for (reduced, reserve, valid) in [
        (0, 0, true),
        (100, 100, true),
        (50, 20, true),
        (20, 50, false),
        (101, 0, false),
        (255, 255, false),
    ] {
        let result = ReservePolicy::new(reduced, reserve);
        assert_eq!(result.is_ok(), valid);
        if let Ok(policy) = result {
            assert_eq!(policy.reduced_below_percent(), reduced);
            assert_eq!(policy.reserve_below_percent(), reserve);
        }
    }
    for currency in ["", "US", "usd", "USDD", "U$D"] {
        assert!(PaidBudget::new(currency, 0).is_err());
    }
    assert!(PaidBudget::new("USD", MAX_FACT_VALUE + 1).is_err());
    let budget = PaidBudget::new("BRL", MAX_FACT_VALUE).unwrap();
    assert_eq!(budget.currency(), "BRL");
    assert_eq!(budget.micros(), MAX_FACT_VALUE);
    assert_eq!(PaidUsePolicy::default(), PaidUsePolicy::Deny);
    assert_eq!(AllocationPolicy::default().paid_use, PaidUsePolicy::Deny);
    assert_eq!(
        AllocationPolicy::default().variant_selection_mode,
        VariantSelectionMode::Explicit
    );
    for state in [
        ScarcityState::Comfortable,
        ScarcityState::Reduced,
        ScarcityState::Reserve,
        ScarcityState::Exhausted,
        ScarcityState::Unknown,
    ] {
        assert!(serde_json::to_string(&state).is_ok());
    }
}
#[test]
fn b1_policy_profiles_modes_and_spend_contracts_do_not_affect_gates() {
    let resource = fixture();
    let expected = evaluate(&resource, None, vision());
    for profile in [
        AllocationProfile::Economy,
        AllocationProfile::Balanced,
        AllocationProfile::Fast,
    ] {
        for variant_selection_mode in [VariantSelectionMode::Explicit, VariantSelectionMode::Auto] {
            for paid_use in [
                PaidUsePolicy::Deny,
                PaidUsePolicy::AllowKnownCostWithinBudget {
                    budget: PaidBudget::new("USD", 0).unwrap(),
                },
            ] {
                let policy = AllocationPolicy {
                    profile,
                    variant_selection_mode,
                    paid_use,
                    reserve: Some(ReservePolicy::new(100, 100).unwrap()),
                };
                let report =
                    AllocationRequest::new(vision(), policy, vec![candidate(&resource, None)])
                        .unwrap()
                        .evaluate();
                assert_eq!(report.candidates(), &[expected.clone()]);
            }
        }
    }
}
#[test]
fn b1_candidate_rejects_invalid_descriptors_without_echoing_payload() {
    let mut resource = fixture();
    resource.enabled = CatalogFact::Known {
        value: true,
        provenance: CatalogProvenance::RuntimeContract,
        observed_at_unix_ms: Some(MAX_FACT_VALUE + 1),
    };
    assert_eq!(
        AllocationCandidate::new(&resource, ModelId::new("model-A").unwrap(), None).unwrap_err(),
        AllocationError::InvalidCandidate
    );
    resource = fixture();
    let model = model_mut(&mut resource).clone();
    let CatalogFact::Known { value, .. } = &mut resource.models else {
        unreachable!()
    };
    value.push(model);
    assert_eq!(
        AllocationCandidate::new(&resource, ModelId::new("model-A").unwrap(), None).unwrap_err(),
        AllocationError::InvalidCandidate
    );
    assert_eq!(
        AllocationError::InvalidCandidate.to_string(),
        "InvalidCandidate"
    );
}
#[test]
fn b1_request_bounds_duplicates_and_empty_authorized_universe() {
    let resource = fixture();
    let c = candidate(&resource, None);
    assert_eq!(
        AllocationRequest::new(
            requirements(None),
            AllocationPolicy::default(),
            vec![c.clone(), c]
        )
        .unwrap_err(),
        AllocationError::DuplicateCandidate
    );
    let candidates: Vec<_> = (0..MAX_ALLOCATION_CANDIDATES)
        .map(|i| {
            AllocationCandidate::new(&resource, ModelId::new(format!("model-{i}")).unwrap(), None)
                .unwrap()
        })
        .collect();
    assert_eq!(
        AllocationRequest::new(
            requirements(None),
            AllocationPolicy::default(),
            candidates.clone()
        )
        .unwrap()
        .evaluate()
        .candidates()
        .len(),
        MAX_ALLOCATION_CANDIDATES
    );
    let mut excess = candidates;
    excess.push(candidate(&resource, None));
    assert_eq!(
        AllocationRequest::new(requirements(None), AllocationPolicy::default(), excess)
            .unwrap_err(),
        AllocationError::TooManyCandidates
    );
    assert!(
        AllocationRequest::new(requirements(None), AllocationPolicy::default(), vec![])
            .unwrap()
            .evaluate()
            .candidates()
            .is_empty()
    );
}
#[test]
fn b1_no_expansion_no_winner_and_caller_order_is_preserved() {
    let resource = fixture();
    let policy = AllocationPolicy {
        variant_selection_mode: VariantSelectionMode::Auto,
        ..AllocationPolicy::default()
    };
    let report = AllocationRequest::new(
        requirements(None),
        policy.clone(),
        vec![
            candidate(&resource, Some("effort-A")),
            candidate(&resource, None),
        ],
    )
    .unwrap()
    .evaluate();
    assert_eq!(report.candidates().len(), 2);
    assert_eq!(
        report.candidates()[0].variant().effort,
        Some(EffortId::new("effort-A").unwrap())
    );
    assert_eq!(report.candidates()[1].variant().effort, None);
    let only_one =
        AllocationRequest::new(requirements(None), policy, vec![candidate(&resource, None)])
            .unwrap()
            .evaluate();
    assert_eq!(only_one.candidates().len(), 1);
    let json = serde_json::to_value(report).unwrap();
    assert!(json.get("winner").is_none());
    assert!(json.get("score").is_none());
    assert!(json.get("ranked").is_none());
}
#[test]
fn b1_report_serialization_is_bounded_and_contains_only_safe_evidence() {
    let mut resource = fixture();
    model_mut(&mut resource).facts.quality =
        known(QualityLabel::new("private-quality-marker").unwrap());
    let private_markers = [
        "synthetic-secret",
        "remote-account-marker",
        "backend-handle-marker",
        "private-prompt-marker",
        "task-input-marker",
    ];
    // The authorized descriptor is the only input; private backend/task material
    // in the caller's context has no field/path into candidates or reports.
    let before = resource.clone();
    let report = AllocationRequest::new(
        vision(),
        AllocationPolicy::default(),
        vec![candidate(&resource, Some("effort-A"))],
    )
    .unwrap()
    .evaluate();
    let json = serde_json::to_value(&report).unwrap();
    let serialized = serde_json::to_string(&report).unwrap();
    for marker in private_markers
        .into_iter()
        .chain(["private-quality-marker"])
    {
        assert!(!serialized.contains(marker));
    }
    assert_eq!(
        json["candidates"][0]["variant"],
        serde_json::json!({"resourceId": "resource-A", "accessPath": "path-A", "billingDomainId": "domain-A", "modelId": "model-A", "effort": "effort-A"})
    );
    assert_eq!(
        json["candidates"][0]["effectiveTier"]["fact"]["observedAtUnixMs"],
        42
    );
    assert_eq!(
        json["candidates"][0]["capabilityEvidence"][0]["model"]["provenance"]["kind"],
        "integration_catalog"
    );
    assert_eq!(
        json["candidates"][0]["availabilityEvidence"]["resource"]["value"],
        "available"
    );
    let entry = json["candidates"][0].as_object().unwrap();
    let keys: Vec<_> = entry.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec![
            "availabilityEvidence",
            "capabilityEvidence",
            "effectiveTier",
            "effortSupport",
            "enabledEvidence",
            "modelSupport",
            "reasons",
            "resourceClass",
            "status",
            "variant"
        ]
    );
    assert_eq!(resource, before);
    assert_eq!(
        serialized,
        serde_json::to_string(
            &AllocationRequest::new(
                vision(),
                AllocationPolicy::default(),
                vec![candidate(&resource, Some("effort-A"))]
            )
            .unwrap()
            .evaluate()
        )
        .unwrap()
    );
}

// B1 FIX-1: the authorized universe must retain LR-8.5A identity integrity.
fn coherent_request(
    candidates: Vec<AllocationCandidate<'_>>,
) -> Result<AllocationRequest<'_>, AllocationError> {
    AllocationRequest::new(requirements(None), AllocationPolicy::default(), candidates)
}
fn bound_resource(id: &str, class: ResourceClass, runtime: &str) -> CognitiveResource {
    let mut resource = fixture();
    resource.identity.id = ResourceId::new(id).unwrap();
    resource.identity.class = class;
    resource.origin = match class {
        ResourceClass::CognitiveProvider => {
            ResourceOrigin::Provider(RuntimeId::new(runtime).unwrap())
        }
        ResourceClass::SpecialistAgent => ResourceOrigin::Agent(RuntimeId::new(runtime).unwrap()),
        ResourceClass::LocalSupport => ResourceOrigin::Local,
    };
    resource
}

#[test]
fn b1_fix1_multiple_model_effort_variants_share_one_coherent_resource() {
    let mut resource = fixture();
    let first_model = model_mut(&mut resource);
    first_model.supported_efforts = known(vec![
        EffortProfile {
            id: EffortId::new("medium").unwrap(),
            availability: known(Availability::Available),
            facts: ExecutionFacts::default(),
        },
        EffortProfile {
            id: EffortId::new("high").unwrap(),
            availability: known(Availability::Available),
            facts: ExecutionFacts::default(),
        },
    ]);
    let mut second_model = first_model.clone();
    second_model.id = ModelId::new("model-B").unwrap();
    let CatalogFact::Known { value, .. } = &mut resource.models else {
        unreachable!()
    };
    value.push(second_model);
    let candidates = [
        ("model-A", "medium"),
        ("model-A", "high"),
        ("model-B", "medium"),
    ]
    .into_iter()
    .map(|(model, effort)| {
        AllocationCandidate::new(
            &resource,
            ModelId::new(model).unwrap(),
            Some(EffortId::new(effort).unwrap()),
        )
        .unwrap()
    })
    .collect();
    let report = coherent_request(candidates).unwrap().evaluate();
    assert_eq!(report.candidates().len(), 3);
    assert!(report
        .candidates()
        .iter()
        .all(|entry| entry.status() == EligibilityStatus::Eligible));
    assert_eq!(
        report.candidates()[2].variant().model_id,
        ModelId::new("model-B").unwrap()
    );
}
#[test]
fn b1_fix1_equal_cloned_snapshots_are_accepted_without_pointer_identity() {
    // Repeated runtime binding is valid after reducing equal snapshots by ID.
    let resource = bound_resource("resource-A", ResourceClass::CognitiveProvider, "runtime-A");
    let cloned = resource.clone();
    assert!(!std::ptr::eq(&resource, &cloned));
    let report = coherent_request(vec![
        candidate(&resource, None),
        candidate(&cloned, Some("effort-A")),
    ])
    .unwrap()
    .evaluate();
    let same_descriptor = coherent_request(vec![
        candidate(&resource, None),
        candidate(&resource, Some("effort-A")),
    ])
    .unwrap()
    .evaluate();
    assert_eq!(report, same_descriptor);
}
#[test]
fn b1_fix1_same_resource_id_requires_equal_complete_identity_and_origin() {
    let resource = bound_resource("resource-A", ResourceClass::CognitiveProvider, "runtime-A");
    let mutations: [fn(&mut CognitiveResource); 5] = [
        |r| r.identity.access_path = AccessPath::new("different-path").unwrap(),
        |r| r.identity.billing_domain.id = BillingDomainId::new("different-domain").unwrap(),
        |r| r.identity.family = ProviderFamily::new("different-family").unwrap(),
        |r| {
            r.identity.class = ResourceClass::SpecialistAgent;
            r.origin = ResourceOrigin::Agent(RuntimeId::new("runtime-A").unwrap());
        },
        |r| r.origin = ResourceOrigin::Provider(RuntimeId::new("different-runtime").unwrap()),
    ];
    for mutate in mutations {
        let mut conflicting = resource.clone();
        mutate(&mut conflicting);
        // Both individual descriptors remain valid; request coherence must reject.
        for (first, second) in [(&resource, &conflicting), (&conflicting, &resource)] {
            assert_eq!(
                coherent_request(vec![
                    candidate(first, None),
                    candidate(second, Some("effort-A"))
                ])
                .unwrap_err(),
                AllocationError::ConflictingResourceSnapshot
            );
        }
    }
}
#[test]
fn b1_fix1_same_identity_requires_equal_facts_provenance_and_timestamps() {
    let resource = fixture();
    let mutations: [fn(&mut CognitiveResource); 10] = [
        |r| r.enabled = known(false),
        |r| r.availability = known(Availability::Unavailable),
        |r| {
            r.capabilities
                .0
                .insert(CognitiveCapability::Vision, known(true));
        },
        |r| {
            model_mut(r)
                .capabilities
                .0
                .insert(CognitiveCapability::Vision, known(false));
        },
        |r| model_mut(r).facts.execution.cognitive_tier = known(tier(3)),
        |r| effort_mut(r).availability = known(Availability::Unavailable),
        |r| r.economics.billing_kind = known(BillingKind::MeteredBilling),
        |r| {
            r.enabled =
                CatalogFact::known(true, CatalogProvenance::RuntimeContract, Some(42)).unwrap()
        },
        |r| {
            r.enabled =
                CatalogFact::known(true, CatalogProvenance::IntegrationCatalog, Some(43)).unwrap()
        },
        |r| r.models = CatalogFact::Unknown,
    ];
    for mutate in mutations {
        let mut conflicting = resource.clone();
        mutate(&mut conflicting);
        assert_eq!(resource.identity, conflicting.identity);
        assert_ne!(resource, conflicting);
        for (first, second) in [(&resource, &conflicting), (&conflicting, &resource)] {
            assert_eq!(
                coherent_request(vec![
                    candidate(first, None),
                    candidate(second, Some("effort-A"))
                ])
                .unwrap_err(),
                AllocationError::ConflictingResourceSnapshot
            );
        }
    }
}
#[test]
fn b1_fix1_same_provider_runtime_cannot_bind_different_resource_ids() {
    let a = bound_resource("resource-A", ResourceClass::CognitiveProvider, "runtime-X");
    let b = bound_resource("resource-B", ResourceClass::CognitiveProvider, "runtime-X");
    for (first, second) in [(&a, &b), (&b, &a)] {
        assert_eq!(
            coherent_request(vec![candidate(first, None), candidate(second, None)]).unwrap_err(),
            AllocationError::ConflictingRuntimeBinding
        );
    }
}
#[test]
fn b1_fix1_same_agent_runtime_cannot_bind_different_resource_ids() {
    let a = bound_resource("resource-A", ResourceClass::SpecialistAgent, "runtime-X");
    let b = bound_resource("resource-B", ResourceClass::SpecialistAgent, "runtime-X");
    for (first, second) in [(&a, &b), (&b, &a)] {
        assert_eq!(
            coherent_request(vec![candidate(first, None), candidate(second, None)]).unwrap_err(),
            AllocationError::ConflictingRuntimeBinding
        );
    }
}
#[test]
fn b1_fix1_distinct_local_resources_are_exempt_from_runtime_uniqueness() {
    let a = fixture();
    let mut b = a.clone();
    b.identity.id = ResourceId::new("resource-B").unwrap();
    let report = coherent_request(vec![candidate(&b, None), candidate(&a, None)])
        .unwrap()
        .evaluate();
    assert_eq!(report.candidates().len(), 2);
    assert_eq!(report.candidates()[0].variant().resource_id, b.identity.id);
    assert!(report
        .candidates()
        .iter()
        .all(|entry| entry.status() == EligibilityStatus::Eligible));
}
#[test]
fn b1_fix1_different_runtime_ids_are_valid_in_both_namespaces() {
    for class in [
        ResourceClass::CognitiveProvider,
        ResourceClass::SpecialistAgent,
    ] {
        let a = bound_resource("resource-A", class, "runtime-A");
        let b = bound_resource("resource-B", class, "runtime-B");
        assert_eq!(
            coherent_request(vec![candidate(&a, None), candidate(&b, None)])
                .unwrap()
                .evaluate()
                .candidates()
                .len(),
            2
        );
    }
}
#[test]
fn b1_fix1_provider_and_agent_runtime_namespaces_remain_independent() {
    let provider = bound_resource("resource-A", ResourceClass::CognitiveProvider, "same-label");
    let agent = bound_resource("resource-B", ResourceClass::SpecialistAgent, "same-label");
    assert_eq!(
        coherent_request(vec![candidate(&provider, None), candidate(&agent, None)])
            .unwrap()
            .evaluate()
            .candidates()
            .len(),
        2
    );
}
#[test]
fn b1_fix1_snapshot_conflict_precedes_binding_and_duplicate_conflicts_globally() {
    let a = bound_resource("resource-A", ResourceClass::CognitiveProvider, "runtime-X");
    let b = bound_resource("resource-B", ResourceClass::CognitiveProvider, "runtime-X");
    let mut conflicting = a.clone();
    conflicting.enabled = known(false);
    // Same variant identity, different descriptor: not merely DuplicateCandidate.
    let error =
        coherent_request(vec![candidate(&a, None), candidate(&conflicting, None)]).unwrap_err();
    assert_eq!(error, AllocationError::ConflictingResourceSnapshot);
    // A prior duplicate/binding conflict cannot mask a later snapshot conflict.
    for resources in [
        vec![&a, &a, &b, &conflicting],
        vec![&conflicting, &b, &a, &a],
    ] {
        assert_eq!(
            coherent_request(resources.into_iter().map(|r| candidate(r, None)).collect())
                .unwrap_err(),
            AllocationError::ConflictingResourceSnapshot
        );
    }
    assert_eq!(error.to_string(), "ConflictingResourceSnapshot");
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        "\"conflicting_resource_snapshot\""
    );
}
#[test]
fn b1_fix1_bounds_precede_all_universe_conflicts() {
    let a = fixture();
    let mut conflicting = a.clone();
    conflicting.enabled = known(false);
    let mut candidates = vec![candidate(&a, None); MAX_ALLOCATION_CANDIDATES];
    candidates.push(candidate(&conflicting, None));
    assert_eq!(
        coherent_request(candidates).unwrap_err(),
        AllocationError::TooManyCandidates
    );
}
#[test]
fn b1_fix1_binding_conflict_precedes_duplicate_variant() {
    let a = bound_resource("resource-A", ResourceClass::SpecialistAgent, "runtime-X");
    let b = bound_resource("resource-B", ResourceClass::SpecialistAgent, "runtime-X");
    let error = coherent_request(vec![
        candidate(&a, None),
        candidate(&a, None),
        candidate(&b, None),
    ])
    .unwrap_err();
    assert_eq!(error, AllocationError::ConflictingRuntimeBinding);
    assert_eq!(error.to_string(), "ConflictingRuntimeBinding");
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        "\"conflicting_runtime_binding\""
    );
    assert_eq!(
        coherent_request(vec![candidate(&a, None), candidate(&a, None)]).unwrap_err(),
        AllocationError::DuplicateCandidate
    );
}
#[test]
fn b1_fix1_different_resources_may_share_domain_with_divergent_economics() {
    let a = bound_resource("resource-A", ResourceClass::CognitiveProvider, "runtime-A");
    let mut b = bound_resource("resource-B", ResourceClass::CognitiveProvider, "runtime-B");
    b.economics.billing_kind = known(BillingKind::MeteredBilling);
    b.economics.monetary_balance = known(MonetaryAmount::new("USD", 7).unwrap());
    assert_eq!(a.identity.billing_domain, b.identity.billing_domain);
    let before_a = a.clone();
    let before_b = b.clone();
    let report = coherent_request(vec![candidate(&a, None), candidate(&b, None)])
        .unwrap()
        .evaluate();
    assert!(report
        .candidates()
        .iter()
        .all(|entry| entry.status() == EligibilityStatus::Eligible));
    assert_eq!(a, before_a);
    assert_eq!(b, before_b);
}
