//! FIX-2: dimensions and units are independent; no depletion or selection.
use super::*;
use crate::cognition::telemetry::{Timing, MAX_FACT_VALUE};

fn known<T>(value: T, at: u64) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, Some(at)).unwrap()
}
fn id(value: &str) -> AllowanceDimensionId {
    AllowanceDimensionId::new(value).unwrap()
}
fn agent() -> CognitiveResource {
    CognitiveResource {
        identity: ResourceIdentity {
            id: ResourceId::new("specialist").unwrap(),
            class: ResourceClass::SpecialistAgent,
            family: ProviderFamily::new("synthetic-family").unwrap(),
            access_path: AccessPath::new("included").unwrap(),
            billing_domain: BillingDomain {
                id: BillingDomainId::new("shared-domain").unwrap(),
            },
        },
        origin: ResourceOrigin::Agent(RuntimeId::new("specialist-runtime").unwrap()),
        economics: EconomicFacts {
            billing_kind: known(BillingKind::IncludedAllowance, 1),
            ..Default::default()
        },
        enabled: known(true, 1),
        availability: CatalogFact::Unknown,
        capabilities: CapabilitySet::default(),
        models: CatalogFact::Unknown,
    }
}
fn percent_window(name: &str, remaining: u64, reset: u64) -> AllowanceState {
    AllowanceState {
        id: id(name),
        unit: AllowanceUnit::Percent,
        limit: known(100, 1),
        remaining: known(remaining, 2),
        reset: known(Timing::UnixMs(reset), 3),
    }
}
fn cost(name: &str, amount: CatalogFact<u64>) -> AllowanceConsumption {
    AllowanceConsumption {
        dimension_id: id(name),
        unit: AllowanceUnit::Percent,
        amount,
    }
}
fn with_model(
    mut resource: CognitiveResource,
    model_facts: ExecutionFacts,
    effort_facts: ExecutionFacts,
) -> CognitiveResource {
    let mut model = ModelProfile::unknown(ModelId::new("opaque-model").unwrap());
    model.facts.execution = model_facts;
    model.supported_efforts = known(
        vec![EffortProfile {
            id: EffortId::new("xhigh").unwrap(),
            availability: CatalogFact::Unknown,
            facts: effort_facts,
        }],
        4,
    );
    resource.models = known(vec![model], 5);
    resource
}
fn variant(catalog: &ResourceCatalog) -> ExecutionVariant {
    catalog
        .describe_variant(
            &ResourceId::new("specialist").unwrap(),
            &ModelId::new("opaque-model").unwrap(),
            Some(&EffortId::new("xhigh").unwrap()),
        )
        .unwrap()
}

#[test]
fn fix2_specialist_percent_windows_coexist_without_lr8() {
    let mut resource = agent();
    let windows = vec![
        percent_window("rolling-5h", 60, 500),
        percent_window("weekly", 20, 900),
    ];
    resource.economics.allowances = windows.clone();
    let mut catalog = ResourceCatalog::default();
    catalog.register(resource).unwrap();
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    assert!(snapshot.resources[0].lr8.is_none());
    assert_eq!(
        snapshot.resources[0].descriptor.economics.allowances,
        windows
    );
    assert_eq!(
        catalog
            .domain_economics(&BillingDomainId::new("shared-domain").unwrap())
            .unwrap()
            .allowances,
        windows
    );
    let json = serde_json::to_value(snapshot).unwrap();
    let states = &json["resources"][0]["descriptor"]["economics"]["allowances"];
    assert_eq!(states[0]["id"], "rolling-5h");
    assert_eq!(states[1]["id"], "weekly");
    assert_eq!(states[0]["unit"]["kind"], "percent");
    assert_eq!(states[1]["unit"]["kind"], "percent");
    assert_eq!(states[0]["remaining"]["value"], 60);
    assert_eq!(states[1]["remaining"]["value"], 20);
    assert_eq!(states[1]["reset"]["observedAtUnixMs"], 3);
}

#[test]
fn fix2_requests_share_unit_but_duplicate_dimension_fails_atomically() {
    let mut resource = agent();
    resource.economics.allowances = vec![
        AllowanceState::unknown(id("requests-per-minute"), AllowanceUnit::Requests),
        AllowanceState::unknown(id("requests-per-day"), AllowanceUnit::Requests),
    ];
    let mut catalog = ResourceCatalog::default();
    catalog.register(resource.clone()).unwrap();
    assert_eq!(
        catalog.snapshot(&[], &[]).unwrap().resources[0]
            .descriptor
            .economics
            .allowances,
        resource.economics.allowances
    );
    for unit in [AllowanceUnit::Requests, AllowanceUnit::Tokens] {
        let mut duplicate = resource.clone();
        duplicate.economics.allowances[1].id = id("requests-per-minute");
        duplicate.economics.allowances[1].unit = unit;
        let mut rejected = ResourceCatalog::default();
        assert_eq!(
            rejected.register(duplicate),
            Err(CatalogError::DuplicateAllowanceDimension)
        );
        assert_eq!(rejected.resources().count(), 0);
    }
}

#[test]
fn fix2_multidimensional_cost_survives_model_effort_snapshot_and_variant() {
    let model_facts = ExecutionFacts {
        allowance_costs: vec![cost("rolling-5h", known(4, 6)), cost("weekly", known(2, 7))],
        ..Default::default()
    };
    let effort_facts = ExecutionFacts {
        allowance_costs: vec![cost("rolling-5h", known(2, 8)), cost("weekly", known(1, 9))],
        ..Default::default()
    };
    let mut resource = with_model(agent(), model_facts.clone(), effort_facts.clone());
    resource.economics.allowances = vec![
        percent_window("rolling-5h", 60, 500),
        percent_window("weekly", 20, 900),
    ];
    let mut catalog = ResourceCatalog::default();
    catalog.register(resource).unwrap();
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    let model = &snapshot.resources[0].descriptor.models.value().unwrap()[0];
    assert_eq!(model.facts.execution, model_facts);
    assert_eq!(
        model.supported_efforts.value().unwrap()[0].facts,
        effort_facts
    );
    let described = variant(&catalog);
    assert_eq!(described.model_facts.execution, model_facts);
    assert_eq!(described.effort_facts, Some(effort_facts));
    let json = serde_json::to_value(described).unwrap();
    let costs = &json["effortFacts"]["allowanceCosts"];
    assert_eq!(costs.as_array().unwrap().len(), 2);
    assert_eq!(costs[0]["dimensionId"], "rolling-5h");
    assert_eq!(costs[1]["dimensionId"], "weekly");
    assert_eq!(costs[0]["amount"]["value"], 2);
    assert_eq!(costs[1]["amount"]["value"], 1);
    assert_eq!(
        costs[0]["amount"]["provenance"]["kind"],
        "integration_catalog"
    );
    assert_eq!(costs[1]["amount"]["observedAtUnixMs"], 9);
}

#[test]
fn fix2_unknown_and_unreported_costs_never_create_zero_or_dimensions() {
    let effort_facts = ExecutionFacts {
        allowance_costs: vec![
            cost("rolling-5h", known(2, 1)),
            cost("weekly", CatalogFact::Unknown),
        ],
        ..Default::default()
    };
    let mut resource = with_model(agent(), ExecutionFacts::default(), effort_facts.clone());
    resource.economics.allowances = vec![
        percent_window("rolling-5h", 60, 500),
        percent_window("weekly", 20, 900),
        AllowanceState::unknown(id("monthly"), AllowanceUnit::Requests),
    ];
    let mut catalog = ResourceCatalog::default();
    catalog.register(resource).unwrap();
    let described = variant(&catalog);
    assert!(described.model_facts.execution.allowance_costs.is_empty());
    let facts = described.effort_facts.unwrap();
    assert_eq!(facts, effort_facts);
    assert_eq!(facts.allowance_costs[1].amount, CatalogFact::Unknown);
    assert_ne!(facts.allowance_costs[1].amount, known(0, 1));
    assert!(facts
        .allowance_costs
        .iter()
        .all(|entry| entry.dimension_id != id("monthly")));
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    assert_eq!(
        snapshot.resources[0].descriptor.economics.allowances.len(),
        3
    );
    assert_eq!(
        snapshot.resources[0].descriptor.economics.allowances[2].remaining,
        CatalogFact::Unknown
    );
    let json = serde_json::to_value(facts).unwrap();
    assert_eq!(json["allowanceCosts"][1]["amount"]["state"], "unknown");
}

#[test]
fn fix2_execution_duplicate_dimensions_and_bounds_fail_closed() {
    let mut other_unit = cost("weekly", CatalogFact::Unknown);
    other_unit.unit = AllowanceUnit::Tokens;
    let invalid = [
        (
            vec![cost("weekly", known(1, 1)), cost("weekly", known(2, 2))],
            CatalogError::DuplicateAllowanceDimension,
        ),
        (
            vec![cost("weekly", known(1, 1)), other_unit],
            CatalogError::DuplicateAllowanceDimension,
        ),
        (
            (0..=MAX_ALLOWANCES)
                .map(|n| cost(&format!("window-{n}"), CatalogFact::Unknown))
                .collect(),
            CatalogError::CapacityExceeded,
        ),
        (
            vec![cost("weekly", known(101, 1))],
            CatalogError::InvalidFact,
        ),
        (
            vec![cost(
                "weekly",
                CatalogFact::Known {
                    value: 1,
                    provenance: CatalogProvenance::IntegrationCatalog,
                    observed_at_unix_ms: Some(MAX_FACT_VALUE + 1),
                },
            )],
            CatalogError::InvalidFact,
        ),
    ];
    for (costs, expected) in invalid {
        let facts = ExecutionFacts {
            allowance_costs: costs,
            ..Default::default()
        };
        for resource in [
            with_model(agent(), facts.clone(), ExecutionFacts::default()),
            with_model(agent(), ExecutionFacts::default(), facts.clone()),
        ] {
            let mut catalog = ResourceCatalog::default();
            assert_eq!(catalog.register(resource), Err(expected));
            assert_eq!(catalog.resources().count(), 0);
        }
    }
}

#[test]
fn fix2_dimension_ids_are_bounded_and_all_units_remain_distinct() {
    assert!(AllowanceDimensionId::new("x".repeat(64)).is_ok());
    for invalid in [
        String::new(),
        "x".repeat(65),
        "bad window".into(),
        "weekly\n".into(),
        "não".into(),
    ] {
        assert!(AllowanceDimensionId::new(&invalid).is_err());
        assert!(
            serde_json::from_value::<AllowanceDimensionId>(serde_json::json!(invalid)).is_err()
        );
    }
    assert_eq!(
        serde_json::from_value::<AllowanceDimensionId>(serde_json::json!("rolling-5h")).unwrap(),
        id("rolling-5h")
    );
    let units = [
        AllowanceUnit::Requests,
        AllowanceUnit::Tokens,
        AllowanceUnit::Credits,
        AllowanceUnit::Percent,
        AllowanceUnit::Custom(AllowanceUnitId::new("compute-units").unwrap()),
    ];
    let mut resource = agent();
    resource.economics.allowances = units
        .iter()
        .enumerate()
        .map(|(n, unit)| AllowanceState::unknown(id(&format!("dimension-{n}")), unit.clone()))
        .collect();
    let facts = ExecutionFacts {
        allowance_costs: units
            .iter()
            .enumerate()
            .map(|(n, unit)| AllowanceConsumption {
                dimension_id: id(&format!("dimension-{n}")),
                unit: unit.clone(),
                amount: known(2, n as u64),
            })
            .collect(),
        ..Default::default()
    };
    let mut catalog = ResourceCatalog::default();
    catalog
        .register(with_model(
            resource,
            facts.clone(),
            ExecutionFacts::default(),
        ))
        .unwrap();
    assert_eq!(variant(&catalog).model_facts.execution, facts);
    let snapshot = catalog.snapshot(&[], &[]).unwrap();
    assert_eq!(
        snapshot.resources[0]
            .descriptor
            .economics
            .allowances
            .iter()
            .map(|state| state.unit.clone())
            .collect::<Vec<_>>(),
        units
    );
    let mut bad_percent = agent();
    bad_percent.economics.allowances = vec![percent_window("weekly", 101, 900)];
    assert_eq!(
        ResourceCatalog::default().register(bad_percent),
        Err(CatalogError::InvalidFact)
    );
}
