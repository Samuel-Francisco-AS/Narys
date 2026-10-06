//! FIX-1 gates: explicit assessments and economic observations, never selection.
use super::*;
use crate::cognition::telemetry::{Provenance, Timing, MAX_FACT_VALUE};

fn catalogued<T>(value: T, at: u64) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, Some(at)).unwrap()
}
fn configured<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(
        value,
        CatalogProvenance::Operational(Provenance::UserConfiguration),
        None,
    )
    .unwrap()
}
fn descriptor(id: &str, domain: &str, class: ResourceClass) -> CognitiveResource {
    CognitiveResource {
        identity: ResourceIdentity {
            id: ResourceId::new(id).unwrap(),
            class,
            family: ProviderFamily::new("synthetic-family").unwrap(),
            access_path: AccessPath::new(id).unwrap(),
            billing_domain: BillingDomain {
                id: BillingDomainId::new(domain).unwrap(),
            },
        },
        origin: match class {
            ResourceClass::SpecialistAgent => ResourceOrigin::Agent(RuntimeId::new(id).unwrap()),
            ResourceClass::CognitiveProvider => {
                ResourceOrigin::Provider(RuntimeId::new(id).unwrap())
            }
            ResourceClass::LocalSupport => ResourceOrigin::Local,
        },
        economics: EconomicFacts::default(),
        enabled: configured(true),
        availability: CatalogFact::Unknown,
        capabilities: CapabilitySet::default(),
        models: CatalogFact::Unknown,
    }
}
fn execution(tier: u16, allowance: u64) -> ExecutionFacts {
    ExecutionFacts {
        cognitive_tier: catalogued(CognitiveTier::new(tier).unwrap(), 1),
        relative_cost: configured(RelativeCostTier::new(tier).unwrap()),
        latency_ms: catalogued(10, 2),
        monetary_cost: CatalogFact::Unknown,
        allowance_costs: vec![AllowanceConsumption {
            dimension_id: AllowanceDimensionId::new("credits-allowance").unwrap(),
            unit: AllowanceUnit::Credits,
            amount: configured(allowance),
        }],
    }
}
fn strong() -> ModelProfile {
    let mut model = ModelProfile::unknown(ModelId::new("Strong").unwrap());
    model.facts.execution = execution(1, 1);
    model.supported_efforts = catalogued(
        [("medium", 2, 1), ("high", 3, 2), ("xhigh", 4, 4)]
            .into_iter()
            .map(|(name, tier, allowance)| EffortProfile {
                id: EffortId::new(name).unwrap(),
                availability: CatalogFact::Unknown,
                facts: execution(tier, allowance),
            })
            .collect(),
        3,
    );
    model
}
fn resource_with_model(id: &str, model: ModelProfile) -> CognitiveResource {
    let mut resource = descriptor(id, id, ResourceClass::CognitiveProvider);
    resource.models = catalogued(vec![model], 4);
    resource
}
fn credits(remaining: u64, at: u64) -> AllowanceState {
    AllowanceState {
        id: AllowanceDimensionId::new("credits-allowance").unwrap(),
        unit: AllowanceUnit::Credits,
        limit: catalogued(100, at),
        remaining: catalogued(remaining, at),
        reset: catalogued(Timing::UnixMs(500), at),
    }
}

#[test]
fn fix1_effort_specific_economics_survive_snapshot_and_variant_without_name_ranking() {
    let original = resource_with_model("included", strong());
    let mut catalog = ResourceCatalog::default();
    catalog.register(original.clone()).unwrap();
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    let model = snapshot.resources[0]
        .descriptor
        .model(&ModelId::new("Strong").unwrap())
        .unwrap();
    for (name, tier, cost) in [("medium", 2, 1), ("high", 3, 2), ("xhigh", 4, 4)] {
        let effort = model.effort(&EffortId::new(name).unwrap()).unwrap();
        assert_eq!(effort.facts, execution(tier, cost));
        assert_eq!(
            effort.facts.cognitive_tier.value().unwrap().value(),
            tier as u8
        );
        assert_eq!(effort.facts.allowance_costs[0].amount.value(), Some(&cost));
        assert_eq!(effort.facts.allowance_costs[0].unit, AllowanceUnit::Credits);
        let variant = catalog
            .describe_variant(&original.identity.id, &model.id, Some(&effort.id))
            .unwrap();
        assert_eq!(variant.model_facts, model.facts);
        assert_eq!(variant.effort_facts, Some(execution(tier, cost)));
    }
    let json = serde_json::to_value(&snapshot).unwrap();
    let efforts =
        &json["resources"][0]["descriptor"]["models"]["value"][0]["supportedEfforts"]["value"];
    assert_eq!(efforts[2]["facts"]["cognitiveTier"]["value"], 4);
    assert_eq!(
        efforts[2]["facts"]["cognitiveTier"]["provenance"]["kind"],
        "integration_catalog"
    );
    assert_eq!(
        efforts[2]["facts"]["allowanceCosts"][0]["amount"]["provenance"]["source"],
        "user_configuration"
    );
    assert_eq!(efforts[2]["facts"]["latencyMs"]["observedAtUnixMs"], 2);
    assert_eq!(efforts[2]["facts"]["monetaryCost"]["state"], "unknown");
}

#[test]
fn fix1_same_effort_name_can_have_different_facts_on_different_resources() {
    let first = resource_with_model("a", strong());
    let mut second_model = strong();
    if let CatalogFact::Known { value, .. } = &mut second_model.supported_efforts {
        value[1].facts = execution(8, 9);
        value[1].facts.monetary_cost = catalogued(MonetaryAmount::new("USD", 7).unwrap(), 8);
        value[1].facts.latency_ms = configured(99);
    }
    let second = resource_with_model("b", second_model);
    let mut catalog = ResourceCatalog::default();
    catalog.register(first).unwrap();
    catalog.register(second).unwrap();
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    let effort = |index: usize| {
        snapshot.resources[index]
            .descriptor
            .model(&ModelId::new("Strong").unwrap())
            .unwrap()
            .effort(&EffortId::new("high").unwrap())
            .unwrap()
    };
    assert_eq!(effort(0).id, effort(1).id);
    assert_eq!(
        effort(0).facts.cognitive_tier,
        execution(3, 2).cognitive_tier
    );
    assert_eq!(
        effort(1).facts.cognitive_tier,
        execution(8, 9).cognitive_tier
    );
    assert_ne!(
        effort(0).facts.allowance_costs,
        effort(1).facts.allowance_costs
    );
    assert_ne!(effort(0).facts.monetary_cost, effort(1).facts.monetary_cost);
    assert_ne!(effort(0).facts.latency_ms, effort(1).facts.latency_ms);
}

#[test]
fn fix1_model_cognitive_tiers_are_explicit_and_names_do_not_define_capacity() {
    let mut a = ModelProfile::unknown(ModelId::new("z-opaque").unwrap());
    a.facts.execution.cognitive_tier = configured(CognitiveTier::new(2).unwrap());
    let mut b = ModelProfile::unknown(ModelId::new("a-opaque").unwrap());
    b.facts.execution.cognitive_tier = catalogued(CognitiveTier::new(5).unwrap(), 9);
    let mut resource = descriptor("models", "domain", ResourceClass::CognitiveProvider);
    resource.models = catalogued(vec![a.clone(), b.clone()], 10);
    let mut catalog = ResourceCatalog::default();
    catalog.register(resource).unwrap();
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    let model = |id: &ModelId| snapshot.resources[0].descriptor.model(id).unwrap();
    assert_eq!(model(&a.id).facts, a.facts);
    assert_eq!(model(&b.id).facts, b.facts);
    // Test the ordinal value contract, never which candidate should execute.
    assert!(CognitiveTier::new(2).unwrap() < CognitiveTier::new(5).unwrap());
    assert_eq!(model(&a.id).supported_efforts, CatalogFact::Unknown);
    assert_eq!(
        model(&b.id).facts.execution.allowance_costs,
        Vec::<AllowanceConsumption>::new()
    );
}

#[test]
fn fix1_unknown_effort_facts_do_not_inherit_model_assessments_or_id_semantics() {
    let mut model = strong();
    if let CatalogFact::Known { value, .. } = &mut model.supported_efforts {
        value[2].facts = ExecutionFacts::default();
    }
    let mut catalog = ResourceCatalog::default();
    let resource = resource_with_model("unknown", model);
    let id = resource.identity.id.clone();
    catalog.register(resource).unwrap();
    let variant = catalog
        .describe_variant(
            &id,
            &ModelId::new("Strong").unwrap(),
            Some(&EffortId::new("xhigh").unwrap()),
        )
        .unwrap();
    assert_eq!(variant.effort_facts, Some(ExecutionFacts::default()));
    assert_eq!(variant.model_facts.execution, execution(1, 1));
    assert_eq!(
        catalog
            .describe_variant(&id, &variant.model_id, None)
            .unwrap()
            .effort_facts,
        None
    );
    assert_eq!(
        EffortId::new("xhigh").unwrap().try_thinking_level(),
        Err(CatalogError::LegacyEffortNotRepresentable)
    );
}

#[test]
fn fix1_generic_specialist_allowance_needs_no_provider_or_rate_authority() {
    // Direct descriptors and catalog only: no registries, RateLimitManager,
    // QuotaScope, HTTP, filesystem, credentials or migration in this gate.
    let mut agent = descriptor(
        "specialist",
        "included-domain",
        ResourceClass::SpecialistAgent,
    );
    agent.economics.billing_kind = configured(BillingKind::IncludedAllowance);
    agent.economics.allowances = vec![credits(5, 17)];
    let mut catalog = ResourceCatalog::default();
    catalog.register(agent).unwrap();
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    let economics = &snapshot.resources[0].descriptor.economics;
    assert_eq!(
        economics.billing_kind,
        configured(BillingKind::IncludedAllowance)
    );
    assert_eq!(economics.allowances[0], credits(5, 17));
    assert_eq!(economics.monetary_balance, CatalogFact::Unknown);
    assert!(snapshot.resources[0].lr8.is_none());
    let json = serde_json::to_value(&snapshot).unwrap();
    let allowance = &json["resources"][0]["descriptor"]["economics"]["allowances"][0];
    assert_eq!(allowance["unit"]["kind"], "credits");
    assert_eq!(allowance["limit"]["value"], 100);
    assert_eq!(allowance["remaining"]["value"], 5);
    assert_eq!(allowance["reset"]["value"]["kind"], "unix_ms");
    assert_eq!(allowance["reset"]["observedAtUnixMs"], 17);
}

#[test]
fn fix1_unknown_allowance_fields_stay_independent_for_every_resource_class() {
    let mut catalog = ResourceCatalog::default();
    for (id, class) in [
        ("agent", ResourceClass::SpecialistAgent),
        ("provider", ResourceClass::CognitiveProvider),
        ("local", ResourceClass::LocalSupport),
    ] {
        let mut resource = descriptor(id, id, class);
        resource.economics.allowances = vec![AllowanceState::unknown(
            AllowanceDimensionId::new("credits-allowance").unwrap(),
            AllowanceUnit::Credits,
        )];
        catalog.register(resource).unwrap();
    }
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    for resource in &snapshot.resources {
        let allowance = &resource.descriptor.economics.allowances[0];
        assert_eq!(allowance.limit, CatalogFact::Unknown);
        assert_eq!(allowance.remaining, CatalogFact::Unknown);
        assert_eq!(allowance.reset, CatalogFact::Unknown);
        assert_ne!(allowance.remaining, configured(0));
        assert_eq!(
            resource.descriptor.economics.billing_kind,
            CatalogFact::Unknown
        );
        assert_eq!(resource.descriptor.availability, CatalogFact::Unknown);
        assert!(resource.lr8.is_none());
    }
    let mut partial = descriptor("partial", "partial", ResourceClass::SpecialistAgent);
    let mut observation = AllowanceState::unknown(
        AllowanceDimensionId::new("percent-allowance").unwrap(),
        AllowanceUnit::Percent,
    );
    observation.remaining = catalogued(5, 3);
    partial.economics.allowances = vec![observation.clone()];
    catalog.register(partial).unwrap();
    assert_eq!(
        catalog
            .resource(&ResourceId::new("partial").unwrap())
            .unwrap()
            .economics
            .allowances[0],
        observation
    );
    assert_eq!(observation.limit, CatalogFact::Unknown); // no guessed denominator
    assert_eq!(observation.reset, CatalogFact::Unknown);
}

#[test]
fn fix1_billing_identity_survives_separate_capture_and_domain_reads_fail_closed() {
    let mut a = descriptor("a", "shared", ResourceClass::SpecialistAgent);
    a.economics.billing_kind = configured(BillingKind::IncludedAllowance);
    a.economics.allowances = vec![credits(5, 1)];
    let mut b = descriptor("b", "shared", ResourceClass::LocalSupport);
    b.economics = a.economics.clone();
    b.economics.allowances = vec![credits(4, 2)];
    let mut catalog = ResourceCatalog::default();
    let domain = a.identity.billing_domain.id.clone();
    let identity = a.identity.billing_domain.clone();
    catalog.register(a.clone()).unwrap();
    assert_eq!(catalog.domain_economics(&domain), Ok(&a.economics));
    catalog.register(b.clone()).unwrap(); // identity never depends on equal facts
    assert_eq!(a.identity.billing_domain, b.identity.billing_domain);
    assert_eq!(
        catalog.domain_economics(&domain),
        Err(CatalogError::ConflictingEconomicFacts)
    );
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    assert_eq!(
        snapshot.resources[0].descriptor.identity.billing_domain,
        identity
    );
    assert_eq!(
        snapshot.resources[1].descriptor.identity.billing_domain,
        identity
    );
    assert_eq!(snapshot.resources[0].descriptor.economics, a.economics);
    assert_eq!(snapshot.resources[1].descriptor.economics, b.economics);
    let mut other = descriptor("c", "independent", ResourceClass::LocalSupport);
    other.economics = b.economics.clone();
    let other_domain = other.identity.billing_domain.id.clone();
    catalog.register(other).unwrap();
    assert_eq!(catalog.domain_economics(&other_domain), Ok(&b.economics));
    assert_eq!(
        catalog.domain_economics(&BillingDomainId::new("missing").unwrap()),
        Err(CatalogError::BillingDomainNotFound)
    );
    let mut separately_captured = ResourceCatalog::default();
    a.economics.allowances = vec![credits(5, 1)];
    b.economics.allowances = vec![credits(5, 2)]; // even equal counts don't merge provenance/time
    separately_captured.register(a).unwrap();
    separately_captured.register(b).unwrap();
    assert_eq!(
        separately_captured.domain_economics(&domain),
        Err(CatalogError::ConflictingEconomicFacts)
    );
}

#[test]
fn fix1_same_domain_identical_evidence_and_distinct_units_are_explicit() {
    let mut a = descriptor("a", "shared", ResourceClass::LocalSupport);
    a.economics.monetary_balance = configured(MonetaryAmount::new("USD", 7).unwrap());
    a.economics.allowances = vec![
        credits(5, 1),
        AllowanceState::unknown(
            AllowanceDimensionId::new("compute-allowance").unwrap(),
            AllowanceUnit::Custom(AllowanceUnitId::new("compute-units").unwrap()),
        ),
    ];
    let mut b = descriptor("b", "shared", ResourceClass::SpecialistAgent);
    b.economics = a.economics.clone();
    let mut catalog = ResourceCatalog::default();
    catalog.register(a.clone()).unwrap();
    catalog.register(b).unwrap();
    assert_eq!(
        catalog.domain_economics(&a.identity.billing_domain.id),
        Ok(&a.economics)
    );
    assert_eq!(a.economics.monetary_balance.value().unwrap().micros(), 7);
    assert_eq!(a.economics.allowances[0].remaining.value(), Some(&5));
    // Credits and money coexist, with no conversion or summation.
    assert_eq!(a.economics.allowances[1].remaining, CatalogFact::Unknown);
}

#[test]
fn fix1_bounded_tiers_and_economic_values_fail_closed() {
    for value in [0, 255] {
        assert!(CognitiveTier::new(value).is_ok());
        assert!(RelativeCostTier::new(value).is_ok());
    }
    for value in [256, u16::MAX] {
        assert!(CognitiveTier::new(value).is_err());
        assert!(RelativeCostTier::new(value).is_err());
        assert!(serde_json::from_value::<CognitiveTier>(serde_json::json!(value)).is_err());
        assert!(serde_json::from_value::<RelativeCostTier>(serde_json::json!(value)).is_err());
    }
    let tier = CognitiveTier::new(3).unwrap();
    assert_eq!(
        serde_json::from_value::<CognitiveTier>(serde_json::to_value(tier).unwrap()).unwrap(),
        tier
    );
    assert!(AllowanceUnitId::new("secret\n").is_err());
    assert!(AllowanceUnitId::new("x".repeat(65)).is_err());
    let mut catalog = ResourceCatalog::default();
    for (limit, remaining, reset) in [
        (Some(MAX_FACT_VALUE + 1), None, None),
        (None, Some(MAX_FACT_VALUE + 1), None),
        (Some(2), Some(3), None),
        (None, None, Some(Timing::DelayMs(MAX_FACT_VALUE + 1))),
        (None, None, Some(Timing::UnixMs(MAX_FACT_VALUE + 1))),
    ] {
        let mut bad = descriptor("bad", "bad", ResourceClass::SpecialistAgent);
        bad.economics.allowances = vec![AllowanceState {
            id: AllowanceDimensionId::new("credits-allowance").unwrap(),
            unit: AllowanceUnit::Credits,
            limit: limit.map_or(CatalogFact::Unknown, configured),
            remaining: remaining.map_or(CatalogFact::Unknown, configured),
            reset: reset.map_or(CatalogFact::Unknown, configured),
        }];
        assert_eq!(catalog.register(bad), Err(CatalogError::InvalidFact));
    }
    for field in [0, 1, 2, 3] {
        let mut model = strong();
        if let CatalogFact::Known { value, .. } = &mut model.supported_efforts {
            match field {
                0 => value[0].facts.latency_ms = configured(MAX_FACT_VALUE + 1),
                1 => {
                    value[0].facts.allowance_costs = vec![AllowanceConsumption {
                        dimension_id: AllowanceDimensionId::new("credits-allowance").unwrap(),
                        unit: AllowanceUnit::Credits,
                        amount: configured(MAX_FACT_VALUE + 1),
                    }]
                }
                2 => {
                    value[0].facts.allowance_costs = vec![AllowanceConsumption {
                        dimension_id: AllowanceDimensionId::new("percent-allowance").unwrap(),
                        unit: AllowanceUnit::Percent,
                        amount: configured(101),
                    }]
                }
                _ => {
                    value[0].facts.cognitive_tier = CatalogFact::Known {
                        value: tier,
                        provenance: CatalogProvenance::IntegrationCatalog,
                        observed_at_unix_ms: Some(MAX_FACT_VALUE + 1),
                    }
                }
            }
        }
        assert_eq!(
            catalog.register(resource_with_model("bad", model)),
            Err(CatalogError::InvalidFact)
        );
    }
    let mut bad = descriptor("bad", "bad", ResourceClass::LocalSupport);
    bad.economics.allowances = vec![credits(5, 1), credits(4, 2)];
    assert_eq!(
        catalog.register(bad),
        Err(CatalogError::DuplicateAllowanceDimension)
    );
    let mut bad = descriptor("bad", "bad", ResourceClass::LocalSupport);
    bad.economics.allowances = (0..=MAX_ALLOWANCES)
        .map(|n| {
            AllowanceState::unknown(
                AllowanceDimensionId::new(format!("dimension-{n}")).unwrap(),
                AllowanceUnit::Requests,
            )
        })
        .collect();
    assert_eq!(catalog.register(bad), Err(CatalogError::CapacityExceeded));
    assert_eq!(catalog.resources().count(), 0);
    let mut boundary = descriptor("boundary", "boundary", ResourceClass::SpecialistAgent);
    boundary.economics.allowances = vec![AllowanceState {
        id: AllowanceDimensionId::new("credits-allowance").unwrap(),
        unit: AllowanceUnit::Credits,
        limit: configured(MAX_FACT_VALUE),
        remaining: configured(0),
        reset: configured(Timing::DelayMs(0)),
    }];
    catalog.register(boundary).unwrap();
}
