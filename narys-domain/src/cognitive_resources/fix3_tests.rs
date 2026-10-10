//! FIX-3: unit integrity within one resource, independent of economic policy.
use super::*;

fn known<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, None).unwrap()
}
fn consumption(dimension: &str, unit: AllowanceUnit) -> AllowanceConsumption {
    AllowanceConsumption {
        dimension_id: AllowanceDimensionId::new(dimension).unwrap(),
        unit,
        amount: known(2),
    }
}
fn resource(
    name: &str,
    model_costs: Vec<AllowanceConsumption>,
    effort_costs: Vec<AllowanceConsumption>,
) -> CognitiveResource {
    let mut model = ModelProfile::unknown(ModelId::new("opaque-model").unwrap());
    model.facts.execution.allowance_costs = model_costs;
    model.supported_efforts = known(vec![EffortProfile {
        id: EffortId::new("xhigh").unwrap(),
        availability: CatalogFact::Unknown,
        facts: ExecutionFacts {
            allowance_costs: effort_costs,
            ..Default::default()
        },
    }]);
    CognitiveResource {
        identity: ResourceIdentity {
            id: ResourceId::new(name).unwrap(),
            class: ResourceClass::SpecialistAgent,
            family: ProviderFamily::new("synthetic-family").unwrap(),
            access_path: AccessPath::new(name).unwrap(),
            billing_domain: BillingDomain {
                id: BillingDomainId::new("shared-domain").unwrap(),
            },
        },
        origin: ResourceOrigin::Agent(RuntimeId::new(name).unwrap()),
        economics: EconomicFacts {
            allowances: vec![AllowanceState {
                id: AllowanceDimensionId::new("weekly").unwrap(),
                unit: AllowanceUnit::Percent,
                limit: known(100),
                remaining: known(20),
                reset: CatalogFact::Unknown,
            }],
            ..Default::default()
        },
        enabled: known(true),
        availability: CatalogFact::Unknown,
        capabilities: CapabilitySet::default(),
        models: known(vec![model]),
    }
}

#[test]
fn fix3_model_consumption_matches_declared_dimension_unit() {
    let resource = resource(
        "valid",
        vec![consumption("weekly", AllowanceUnit::Percent)],
        vec![],
    );
    let mut catalog = ResourceCatalog::default();
    catalog.register(resource.clone()).unwrap();
    assert_eq!(
        catalog.snapshot(&[], &[]).unwrap().resources[0].descriptor,
        resource
    );
}

#[test]
fn fix3_model_mismatch_is_rejected_in_every_model_even_for_unknown_amount() {
    for amount in [known(2), CatalogFact::Unknown] {
        let mut bad_cost = consumption("weekly", AllowanceUnit::Tokens);
        bad_cost.amount = amount;
        let mut bad = resource("bad", vec![bad_cost], vec![]);
        if let CatalogFact::Known { value, .. } = &mut bad.models {
            value.insert(
                0,
                ModelProfile::unknown(ModelId::new("first-valid-model").unwrap()),
            );
        }
        assert_eq!(
            ResourceCatalog::default().register(bad),
            Err(CatalogError::AllowanceUnitMismatch)
        );
    }
}

#[test]
fn fix3_effort_mismatch_is_rejected_in_every_effort_even_for_unknown_amount() {
    for amount in [known(2), CatalogFact::Unknown] {
        let mut bad_cost = consumption("weekly", AllowanceUnit::Tokens);
        bad_cost.amount = amount;
        let mut bad = resource(
            "bad",
            vec![consumption("weekly", AllowanceUnit::Percent)],
            vec![bad_cost],
        );
        if let CatalogFact::Known { value, .. } = &mut bad.models {
            if let CatalogFact::Known { value, .. } = &mut value[0].supported_efforts {
                value.insert(
                    0,
                    EffortProfile {
                        id: EffortId::new("first-valid-effort").unwrap(),
                        availability: CatalogFact::Unknown,
                        facts: ExecutionFacts::default(),
                    },
                );
            }
        }
        assert_eq!(
            ResourceCatalog::default().register(bad),
            Err(CatalogError::AllowanceUnitMismatch)
        );
    }
}

#[test]
fn fix3_consumption_without_state_remains_valid_and_does_not_create_state() {
    let original = resource(
        "valid",
        vec![consumption("unknown-dimension", AllowanceUnit::Credits)],
        vec![consumption("unknown-dimension", AllowanceUnit::Credits)],
    );
    let mut catalog = ResourceCatalog::default();
    catalog.register(original.clone()).unwrap();
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    assert_eq!(snapshot.resources[0].descriptor, original);
    assert_eq!(
        snapshot.resources[0].descriptor.economics.allowances.len(),
        1
    );
    assert_eq!(
        snapshot.resources[0].descriptor.economics.allowances[0]
            .id
            .as_str(),
        "weekly"
    );
}

#[test]
fn fix3_model_and_effort_share_dimension_without_unit_or_cost_merging() {
    let mut effort_cost = consumption("weekly", AllowanceUnit::Percent);
    effort_cost.amount = known(4);
    let original = resource(
        "valid",
        vec![consumption("weekly", AllowanceUnit::Percent)],
        vec![effort_cost],
    );
    let mut catalog = ResourceCatalog::default();
    catalog.register(original.clone()).unwrap();
    let variant = catalog
        .describe_variant(
            &original.identity.id,
            &ModelId::new("opaque-model").unwrap(),
            Some(&EffortId::new("xhigh").unwrap()),
        )
        .unwrap();
    assert_eq!(
        variant.model_facts.execution.allowance_costs[0].amount,
        known(2)
    );
    assert_eq!(
        variant.effort_facts.unwrap().allowance_costs[0].amount,
        known(4)
    );
}

#[test]
fn fix3_rejected_registration_is_atomic_with_existing_resources() {
    let mut catalog = ResourceCatalog::default();
    let valid = resource(
        "existing",
        vec![consumption("weekly", AllowanceUnit::Percent)],
        vec![],
    );
    catalog.register(valid.clone()).unwrap();
    let before = serde_json::to_value(catalog.snapshot(&[], &[]).unwrap()).unwrap();
    let bad = resource(
        "rejected",
        vec![],
        vec![consumption("weekly", AllowanceUnit::Tokens)],
    );
    assert_eq!(
        catalog.register(bad),
        Err(CatalogError::AllowanceUnitMismatch)
    );
    assert!(catalog
        .resource(&ResourceId::new("rejected").unwrap())
        .is_none());
    assert_eq!(
        serde_json::to_value(catalog.snapshot(&[], &[]).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        catalog.domain_economics(&valid.identity.billing_domain.id),
        Ok(&valid.economics)
    );
}

#[test]
fn fix3_shared_billing_domain_does_not_reconcile_other_resource_units() {
    let mut catalog = ResourceCatalog::default();
    let with_state = resource(
        "with-state",
        vec![consumption("weekly", AllowanceUnit::Percent)],
        vec![],
    );
    let mut without_state = resource(
        "without-state",
        vec![consumption("weekly", AllowanceUnit::Tokens)],
        vec![],
    );
    without_state.economics.allowances.clear();
    assert_eq!(
        with_state.identity.billing_domain,
        without_state.identity.billing_domain
    );
    catalog.register(with_state.clone()).unwrap();
    catalog.register(without_state.clone()).unwrap();
    assert_eq!(catalog.resources().count(), 2);
    assert_eq!(
        catalog.domain_economics(&with_state.identity.billing_domain.id),
        Err(CatalogError::ConflictingEconomicFacts)
    );
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    assert_eq!(snapshot.resources[0].descriptor, with_state);
    assert_eq!(snapshot.resources[1].descriptor, without_state);
}
