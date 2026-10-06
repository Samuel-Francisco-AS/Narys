use super::*;
use crate::{
    agents::{
        backend::MockAgentBackend,
        registry::AgentRegistry,
        types::{AgentCapabilities, AgentConfig},
    },
    cognition::{
        mock::{MockProvider, MockScenario},
        policy::ThinkingLevel,
        rate::{
            ClockReading, ConstraintSource, DailyBudgetPolicy, FixedWindow, LocalRateLimit,
            RateClock, RateLimitManager, RatePolicy,
        },
        registry::ProviderRegistry,
        telemetry::{
            Fact, Provenance, QuotaDimension, QuotaScope, TelemetryStore, Timing, UsageDimension,
            MAX_FACT_VALUE,
        },
        types::{ProviderCapabilities, ProviderConfig},
    },
};
use std::sync::Arc;

fn known<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, Some(42)).unwrap()
}
fn configured<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(
        value,
        CatalogProvenance::Operational(Provenance::UserConfiguration),
        None,
    )
    .unwrap()
}
fn identity(
    id: &str,
    class: ResourceClass,
    family: &str,
    path: &str,
    domain: &str,
) -> ResourceIdentity {
    ResourceIdentity {
        id: ResourceId::new(id).unwrap(),
        class,
        family: ProviderFamily::new(family).unwrap(),
        access_path: AccessPath::new(path).unwrap(),
        billing_domain: BillingDomain {
            id: BillingDomainId::new(domain).unwrap(),
        },
    }
}
fn model(id: &str, efforts: &[&str], available: bool, vision: bool) -> ModelProfile {
    let mut model = ModelProfile::unknown(ModelId::new(id).unwrap());
    model.availability = known(if available {
        Availability::Available
    } else {
        Availability::Unavailable
    });
    model
        .capabilities
        .0
        .insert(CognitiveCapability::TextGeneration, known(true));
    model
        .capabilities
        .0
        .insert(CognitiveCapability::Vision, known(vision));
    model.supported_efforts = known(
        efforts
            .iter()
            .map(|id| EffortProfile {
                id: EffortId::new(*id).unwrap(),
                availability: known(Availability::Available),
                facts: ExecutionFacts::default(),
            })
            .collect(),
    );
    model
}
fn provider(id: &str) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        enabled: true,
        priority: 1,
        capabilities: ProviderCapabilities::with_structured_output(),
    }
}
fn included() -> CognitiveResource {
    let mut resource = CognitiveResource::from_provider_config(
        identity(
            "included",
            ResourceClass::CognitiveProvider,
            "family-a",
            "included",
            "allowance-A",
        ),
        &provider("runtime-included"),
    )
    .unwrap();
    resource.economics.billing_kind = configured(BillingKind::IncludedAllowance);
    resource.availability = known(Availability::Available);
    let mut cheap = model("Cheap", &["low", "medium"], true, false);
    cheap.facts.context_tokens = known(4096);
    cheap.facts.execution.latency_ms = CatalogFact::from(&Fact::Known {
        value: 15,
        provenance: Provenance::LocalRuntime,
        observed_at_unix_ms: Some(43),
    });
    cheap.facts.execution.monetary_cost = known(MonetaryAmount::new("USD", 0).unwrap());
    cheap.facts.execution.allowance_cost = configured(AllowanceConsumption {
        unit: AllowanceUnit::Requests,
        amount: 1,
    });
    cheap.facts.quality = configured(QualityLabel::new("task-qualified").unwrap());
    resource.models = known(vec![
        cheap,
        model("Strong", &["medium", "high", "xhigh"], true, true),
        model("Unavailable", &["high"], false, false),
    ]);
    resource
}
fn fixture() -> ResourceCatalog {
    let mut catalog = ResourceCatalog::default();
    catalog.register(included()).unwrap();
    for (id, family, path, domain, kind, name) in [
        (
            "direct",
            "family-a",
            "direct-api",
            "prepaid-B",
            BillingKind::PrepaidCredits,
            "API-Model",
        ),
        (
            "free",
            "family-b",
            "free-api",
            "free-C",
            BillingKind::FreeTier,
            "Alternative",
        ),
    ] {
        let mut resource = CognitiveResource::from_provider_config(
            identity(id, ResourceClass::CognitiveProvider, family, path, domain),
            &provider(&format!("runtime-{id}")),
        )
        .unwrap();
        resource.economics.billing_kind = configured(kind);
        resource.models = known(vec![ModelProfile::unknown(ModelId::new(name).unwrap())]);
        catalog.register(resource).unwrap();
    }
    let agent = AgentConfig {
        id: "runtime-agent".into(),
        enabled: true,
        priority: 1,
        capabilities: AgentCapabilities {
            planning: true,
            structured_output: true,
            ..Default::default()
        },
    };
    catalog
        .register(
            CognitiveResource::from_agent_config(
                identity(
                    "specialist",
                    ResourceClass::SpecialistAgent,
                    "family-a",
                    "agent-bridge",
                    "agent-domain",
                ),
                &agent,
            )
            .unwrap(),
        )
        .unwrap();
    catalog
        .register(CognitiveResource {
            identity: identity(
                "local",
                ResourceClass::LocalSupport,
                "local-family",
                "local-process",
                "local-domain",
            ),
            origin: ResourceOrigin::Local,
            economics: EconomicFacts::default(),
            enabled: configured(true),
            availability: CatalogFact::Unknown,
            capabilities: CapabilitySet::default(),
            models: CatalogFact::Unknown,
        })
        .unwrap();
    catalog
}
fn resource<'a>(catalog: &'a ResourceCatalog, id: &str) -> &'a CognitiveResource {
    catalog.resource(&ResourceId::new(id).unwrap()).unwrap()
}

#[test]
fn synthetic_heterogeneous_resources_access_paths_and_billing_domains_are_independent() {
    let catalog = fixture();
    assert_eq!(catalog.resources().count(), 5);
    let included = resource(&catalog, "included");
    let direct = resource(&catalog, "direct");
    assert_eq!(included.identity.family, direct.identity.family);
    assert_ne!(included.identity.access_path, direct.identity.access_path);
    assert_ne!(
        included.identity.billing_domain.id,
        direct.identity.billing_domain.id
    );
    assert_eq!(
        included.economics.billing_kind,
        configured(BillingKind::IncludedAllowance)
    );
    assert_eq!(
        direct.economics.billing_kind,
        configured(BillingKind::PrepaidCredits)
    );
    assert_eq!(
        resource(&catalog, "free").economics.billing_kind,
        configured(BillingKind::FreeTier)
    );
    assert_ne!(
        included.identity.family,
        resource(&catalog, "free").identity.family
    );
    assert_eq!(
        resource(&catalog, "specialist").identity.class,
        ResourceClass::SpecialistAgent
    );
    assert_eq!(
        resource(&catalog, "local").identity.class,
        ResourceClass::LocalSupport
    );
    assert_eq!(included.models.value().unwrap().len(), 3);
}

#[test]
fn synthetic_model_specific_efforts_xhigh_and_legacy_bridge_do_not_approximate() {
    let catalog = fixture();
    let included = resource(&catalog, "included");
    let cheap = included.model(&ModelId::new("Cheap").unwrap()).unwrap();
    let strong = included.model(&ModelId::new("Strong").unwrap()).unwrap();
    assert_eq!(
        cheap
            .supported_efforts
            .value()
            .unwrap()
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["low", "medium"]
    );
    assert_eq!(
        strong
            .supported_efforts
            .value()
            .unwrap()
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["medium", "high", "xhigh"]
    );
    let xhigh = EffortId::new("xhigh").unwrap();
    assert_eq!(cheap.effort(&xhigh), Err(CatalogError::EffortNotSupported));
    assert_eq!(strong.effort(&xhigh).unwrap().id, xhigh);
    assert_eq!(
        xhigh.try_thinking_level(),
        Err(CatalogError::LegacyEffortNotRepresentable)
    );
    let variant = catalog
        .describe_variant(&included.identity.id, &strong.id, Some(&xhigh))
        .unwrap();
    assert_eq!(variant.effort, Some(xhigh));
    assert_eq!(variant.access_path, included.identity.access_path);
    assert_eq!(
        variant.billing_domain_id,
        included.identity.billing_domain.id
    );
    for level in [
        ThinkingLevel::Low,
        ThinkingLevel::Medium,
        ThinkingLevel::High,
    ] {
        assert_eq!(
            EffortId::from_thinking_level(level).try_thinking_level(),
            Ok(level)
        );
    }
    assert!(serde_json::from_str::<ThinkingLevel>("\"xhigh\"").is_err());
    assert_eq!(
        EffortId::new("future-provider-effort")
            .unwrap()
            .try_thinking_level(),
        Err(CatalogError::LegacyEffortNotRepresentable)
    );
}

#[test]
fn synthetic_unavailable_unknown_and_model_capability_divergence_survive_snapshot() {
    let snapshot = fixture().snapshot(&[], &[]).unwrap();
    let included = &snapshot
        .resources
        .iter()
        .find(|r| r.descriptor.identity.id.as_str() == "included")
        .unwrap()
        .descriptor;
    let cheap = included.model(&ModelId::new("Cheap").unwrap()).unwrap();
    let strong = included.model(&ModelId::new("Strong").unwrap()).unwrap();
    let unavailable = included
        .model(&ModelId::new("Unavailable").unwrap())
        .unwrap();
    assert_eq!(
        cheap.capabilities.get(CognitiveCapability::Vision),
        known(false)
    );
    assert_eq!(
        strong.capabilities.get(CognitiveCapability::Vision),
        known(true)
    );
    assert_eq!(unavailable.availability, known(Availability::Unavailable));
    assert_eq!(
        cheap
            .facts
            .execution
            .monetary_cost
            .value()
            .unwrap()
            .micros(),
        0
    );
    assert_eq!(strong.facts.execution.monetary_cost, CatalogFact::Unknown);
    assert_eq!(cheap.facts.context_tokens, known(4096));
    assert_eq!(
        cheap.facts.execution.latency_ms,
        CatalogFact::known(
            15,
            CatalogProvenance::Operational(Provenance::LocalRuntime),
            Some(43)
        )
        .unwrap()
    );
    assert_eq!(
        cheap.facts.quality,
        configured(QualityLabel::new("task-qualified").unwrap())
    );
    for id in ["direct", "free", "specialist", "local"] {
        let resource = &snapshot
            .resources
            .iter()
            .find(|r| r.descriptor.identity.id.as_str() == id)
            .unwrap()
            .descriptor;
        assert_eq!(resource.availability, CatalogFact::Unknown);
        assert_eq!(resource.economics.monetary_balance, CatalogFact::Unknown);
    }
    let json = serde_json::to_value(snapshot).unwrap();
    assert!(json.to_string().contains("integration_catalog"));
    assert!(json.to_string().contains("user_configuration"));
    assert!(json.to_string().contains("local_runtime"));
}

#[test]
fn missing_model_catalog_and_efforts_are_unknown_even_for_legacy_named_models() {
    let mut descriptor = CognitiveResource::from_provider_config(
        identity(
            "new",
            ResourceClass::CognitiveProvider,
            "new-family",
            "new-path",
            "new-domain",
        ),
        &provider("new-runtime"),
    )
    .unwrap();
    assert_eq!(descriptor.models, CatalogFact::Unknown);
    let id = ModelId::new("new-model").unwrap();
    assert_eq!(
        descriptor.model(&id),
        Err(CatalogError::ModelCatalogUnknown)
    );
    let model = ModelProfile::unknown(id.clone());
    assert_eq!(
        model.effort(&EffortId::from_thinking_level(ThinkingLevel::Low)),
        Err(CatalogError::EffortSupportUnknown)
    );
    assert_eq!(
        model
            .capabilities
            .get(CognitiveCapability::StructuredOutput),
        CatalogFact::Unknown
    );
    assert_eq!(
        descriptor
            .capabilities
            .get(CognitiveCapability::StructuredOutput)
            .value(),
        Some(&true)
    );
    descriptor.models = configured(vec![model]);
    let mut catalog = ResourceCatalog::default();
    catalog.register(descriptor).unwrap();
    assert_eq!(
        catalog.describe_variant(
            &ResourceId::new("new").unwrap(),
            &id,
            Some(&EffortId::new("low").unwrap())
        ),
        Err(CatalogError::EffortSupportUnknown)
    );
}

struct FixedClock;
impl RateClock for FixedClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            monotonic_ms: 0,
            unix_ms: Some(100),
        }
    }
}
fn authorities() -> (TelemetryStore, Arc<RateLimitManager>) {
    let rate = RateLimitManager::new(
        ["runtime-included".into(), "runtime-direct".into()],
        Arc::new(FixedClock),
        None,
    )
    .unwrap();
    let store = TelemetryStore::with_rate(
        ["runtime-included".into(), "runtime-direct".into()],
        rate.clone(),
    );
    (store, rate)
}

#[test]
fn lr8_exact_model_scope_and_runtime_binding_prevent_quota_leaks() {
    let catalog = fixture();
    let (store, rate) = authorities();
    store.observe_quota(
        "runtime-included",
        QuotaScope::Model {
            model: "Cheap".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(10),
        Some(0),
        Some(Timing::UnixMs(500)),
        Provenance::ProviderHeader,
    );
    store.observe_quota(
        "runtime-included",
        QuotaScope::Provider,
        QuotaDimension::TokensPerMinute,
        Some(100),
        None,
        None,
        Provenance::ProviderResponse,
    );
    // Undescribed/miscased model and unrelated runtime must never be assigned.
    store.observe_quota(
        "runtime-included",
        QuotaScope::Model {
            model: "cheap".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(20),
        Some(20),
        None,
        Provenance::ProviderHeader,
    );
    store.observe_quota(
        "runtime-direct",
        QuotaScope::Model {
            model: "API-Model".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(3),
        Some(3),
        None,
        Provenance::ProviderResponse,
    );
    let telemetry = store.snapshots();
    let rates = rate.read_only_snapshots();
    let before = serde_json::to_value((&telemetry, &rates)).unwrap();
    let snapshot = catalog.snapshot(&telemetry, &rates).unwrap();
    let included = snapshot
        .resources
        .iter()
        .find(|r| r.descriptor.identity.id.as_str() == "included")
        .unwrap()
        .lr8
        .as_ref()
        .unwrap();
    let t = included.telemetry.as_ref().unwrap();
    let r = included.rate.as_ref().unwrap();
    assert_eq!(t.model_quotas.len(), 1);
    assert_eq!(r.model_constraints.len(), 1);
    assert!(!t
        .model_quotas
        .contains_key(&ModelId::new("Strong").unwrap()));
    assert!(!r
        .model_constraints
        .contains_key(&ModelId::new("Strong").unwrap()));
    let quota = &t.model_quotas[&ModelId::new("Cheap").unwrap()][&QuotaDimension::RequestsPerDay];
    assert_eq!(
        quota,
        &telemetry
            .iter()
            .find(|s| s.provider_id == "runtime-included")
            .unwrap()
            .quotas
            .iter()
            .find(|q| matches!(&q.scope, QuotaScope::Model { model } if model == "Cheap"))
            .unwrap()
            .dimensions[&QuotaDimension::RequestsPerDay]
    );
    assert!(matches!(
        quota.remaining,
        Fact::Known {
            value: 0,
            provenance: Provenance::ProviderHeader,
            ..
        }
    ));
    assert_eq!(
        t.provider_quotas[&QuotaDimension::TokensPerMinute].remaining,
        Fact::Unknown
    );
    assert_eq!(
        t.usage[&UsageDimension::TotalTokens].observed,
        Fact::Unknown
    );
    assert_eq!(
        r.model_constraints[&ModelId::new("Cheap").unwrap()][0].effective_remaining,
        Some(0)
    );
    let direct = snapshot
        .resources
        .iter()
        .find(|r| r.descriptor.identity.id.as_str() == "direct")
        .unwrap()
        .lr8
        .as_ref()
        .unwrap();
    assert_eq!(direct.telemetry.as_ref().unwrap().model_quotas.len(), 1);
    for id in ["specialist", "local", "free"] {
        assert!(snapshot
            .resources
            .iter()
            .find(|r| r.descriptor.identity.id.as_str() == id)
            .unwrap()
            .lr8
            .is_none());
    }
    assert_eq!(serde_json::to_value((&telemetry, &rates)).unwrap(), before);
    for _ in 0..3 {
        catalog
            .snapshot(&store.snapshots(), &rate.read_only_snapshots())
            .unwrap();
    }
    assert_eq!(
        serde_json::to_value(rate.read_only_snapshots()).unwrap(),
        serde_json::to_value(rates).unwrap()
    );
}

#[test]
fn lr8_local_policy_and_external_sources_remain_separate() {
    let (store, rate) = authorities();
    rate.set_policy(
        "runtime-included",
        RatePolicy {
            limits: vec![LocalRateLimit {
                scope: QuotaScope::Model {
                    model: "Strong".into(),
                },
                dimension: QuotaDimension::RequestsPerMinute,
                capacity: 2,
                window: FixedWindow {
                    period_ms: 1000,
                    anchor_unix_ms: 100,
                },
            }],
            daily_budget: Some(DailyBudgetPolicy {
                anchor_unix_ms: 100,
                max_requests: Some(5),
                max_accounted_tokens: None,
            }),
        },
    )
    .unwrap();
    store.observe_quota(
        "runtime-included",
        QuotaScope::Model {
            model: "Cheap".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(10),
        Some(4),
        None,
        Provenance::ProviderHeader,
    );
    let snapshot = fixture()
        .snapshot(&store.snapshots(), &rate.read_only_snapshots())
        .unwrap();
    let lr8 = snapshot
        .resources
        .iter()
        .find(|r| r.descriptor.identity.id.as_str() == "included")
        .unwrap()
        .lr8
        .as_ref()
        .unwrap();
    let r = lr8.rate.as_ref().unwrap();
    assert_eq!(r.provider_constraints.len(), 1);
    assert_eq!(
        r.provider_constraints[0].source,
        ConstraintSource::DailyBudget
    );
    assert_eq!(
        r.provider_constraints[0].provenance,
        Some(Provenance::UserConfiguration)
    );
    assert_eq!(
        r.model_constraints[&ModelId::new("Strong").unwrap()][0].source,
        ConstraintSource::LocalPolicy
    );
    assert_eq!(
        r.model_constraints[&ModelId::new("Cheap").unwrap()][0].source,
        ConstraintSource::ExternalFact
    );
    assert_eq!(
        r.model_constraints[&ModelId::new("Cheap").unwrap()][0].provenance,
        Some(Provenance::ProviderHeader)
    );
    assert_eq!(lr8.telemetry.as_ref().unwrap().model_quotas.len(), 1);
}

#[test]
fn lr8_generation_mismatch_and_duplicate_captures_fail_closed() {
    let catalog = fixture();
    let (store, rate) = authorities();
    let t = store.snapshots();
    let mut r = rate.read_only_snapshots();
    r.iter_mut()
        .find(|s| s.provider_id == "runtime-included")
        .unwrap()
        .context_generation = 1;
    assert!(matches!(
        catalog.snapshot(&t, &r),
        Err(CatalogError::ContextMismatch)
    ));
    let mut duplicates = t.clone();
    duplicates.push(t[0].clone());
    assert!(matches!(
        catalog.snapshot(&duplicates, &[]),
        Err(CatalogError::DuplicateSnapshot)
    ));
    let mut duplicates = t;
    let included = duplicates
        .iter_mut()
        .find(|s| s.provider_id == "runtime-included")
        .unwrap();
    included.quotas.push(included.quotas[0].clone());
    assert!(matches!(
        catalog.snapshot(&duplicates, &[]),
        Err(CatalogError::DuplicateSnapshot)
    ));
    // Rotation discards quotas in the authority. Catalog retains no runtime cache.
    store.observe_quota(
        "runtime-included",
        QuotaScope::Model {
            model: "Cheap".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(10),
        Some(0),
        None,
        Provenance::ProviderHeader,
    );
    store.invalidate_provider_quotas("runtime-included");
    let snapshot = catalog
        .snapshot(&store.snapshots(), &rate.read_only_snapshots())
        .unwrap();
    let lr8 = snapshot
        .resources
        .iter()
        .find(|s| s.descriptor.identity.id.as_str() == "included")
        .unwrap()
        .lr8
        .as_ref()
        .unwrap();
    assert!(lr8.telemetry.as_ref().unwrap().model_quotas.is_empty());
    // LR-8 intentionally retains bucket indices after rotation, with unknown
    // external facts. The descriptive bridge must preserve that contract.
    let constraints =
        &lr8.rate.as_ref().unwrap().model_constraints[&ModelId::new("Cheap").unwrap()];
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].capacity, None);
    assert_eq!(constraints[0].effective_remaining, None);
    assert_eq!(
        constraints[0].external,
        Some(crate::cognition::telemetry::QuotaSnapshot::default())
    );
    assert_eq!(constraints[0].provenance, None);
    assert_eq!(lr8.telemetry.as_ref().unwrap().context_generation, 1);
}

#[test]
fn typed_identifiers_validate_bounds_and_deserialization_without_silent_normalization() {
    for invalid in [
        "",
        " has-space",
        "has-space ",
        "line\n",
        "a/b",
        "a:b",
        "email@example.org",
        "https://private",
        "á",
        &"a".repeat(65),
    ] {
        assert!(ResourceId::new(invalid).is_err());
        assert!(ProviderFamily::new(invalid).is_err());
        assert!(AccessPath::new(invalid).is_err());
        assert!(BillingDomainId::new(invalid).is_err());
        assert!(RuntimeId::new(invalid).is_err());
        assert!(EffortId::new(invalid).is_err());
        assert!(serde_json::from_value::<ResourceId>(serde_json::json!(invalid)).is_err());
    }
    assert!(ResourceId::new("a".repeat(64)).is_ok());
    assert!(ModelId::new("a".repeat(128)).is_ok());
    assert!(ModelId::new("a".repeat(129)).is_err());
    assert!(ModelId::new("@namespace/model:version").is_ok());
    assert_ne!(
        ModelId::new("Model").unwrap(),
        ModelId::new("model").unwrap()
    );
    let id = ModelId::new("@namespace/model:version").unwrap();
    assert_eq!(
        serde_json::from_value::<ModelId>(serde_json::to_value(&id).unwrap()).unwrap(),
        id
    );
}

#[test]
fn catalog_registration_rejects_duplicates_and_invalid_origins_atomically() {
    let mut catalog = fixture();
    let before = serde_json::to_value(catalog.snapshot(&[], &[]).unwrap()).unwrap();
    assert_eq!(
        catalog.register(included()),
        Err(CatalogError::DuplicateResource)
    );
    let mut resource = included();
    resource.identity.id = ResourceId::new("duplicate-runtime").unwrap();
    resource.identity.billing_domain.id = BillingDomainId::new("other-domain").unwrap();
    assert_eq!(
        catalog.register(resource),
        Err(CatalogError::DuplicateRuntimeBinding)
    );
    let mut resource = included();
    resource.identity.class = ResourceClass::SpecialistAgent;
    assert_eq!(catalog.register(resource), Err(CatalogError::InvalidOrigin));
    let mut resource = included();
    resource.models = known(vec![
        model("duplicate", &[], true, false),
        model("duplicate", &[], true, false),
    ]);
    assert_eq!(
        catalog.register(resource),
        Err(CatalogError::DuplicateModel)
    );
    let mut resource = included();
    resource.models = known(vec![model(
        "duplicate-effort",
        &["low", "low"],
        true,
        false,
    )]);
    assert_eq!(
        catalog.register(resource),
        Err(CatalogError::DuplicateEffort)
    );
    assert_eq!(
        serde_json::to_value(catalog.snapshot(&[], &[]).unwrap()).unwrap(),
        before
    );
}

#[test]
fn numeric_facts_and_cardinality_are_bounded() {
    assert!(CatalogFact::known(
        true,
        CatalogProvenance::IntegrationCatalog,
        Some(MAX_FACT_VALUE + 1)
    )
    .is_err());
    assert!(MonetaryAmount::new("USD", MAX_FACT_VALUE).is_ok());
    assert!(MonetaryAmount::new("USD", MAX_FACT_VALUE + 1).is_err());
    for currency in ["", "usd", "US", "USDD", "U$D"] {
        assert!(MonetaryAmount::new(currency, 0).is_err());
    }
    let mut catalog = ResourceCatalog::default();
    let mut bad = included();
    bad.models = known(vec![model("bad", &[], true, false)]);
    if let CatalogFact::Known { value, .. } = &mut bad.models {
        value[0].facts.execution.latency_ms = known(MAX_FACT_VALUE + 1);
    }
    assert_eq!(catalog.register(bad), Err(CatalogError::InvalidFact));
    let mut bad = included();
    bad.availability = CatalogFact::Known {
        value: Availability::Available,
        provenance: CatalogProvenance::IntegrationCatalog,
        observed_at_unix_ms: Some(MAX_FACT_VALUE + 1),
    };
    assert_eq!(catalog.register(bad), Err(CatalogError::InvalidFact));
    let mut bad = included();
    bad.models = known(
        (0..=MAX_MODELS)
            .map(|n| ModelProfile::unknown(ModelId::new(format!("model-{n}")).unwrap()))
            .collect(),
    );
    assert_eq!(catalog.register(bad), Err(CatalogError::CapacityExceeded));
    let mut bad = included();
    let names: Vec<_> = (0..=MAX_EFFORTS).map(|n| format!("effort-{n}")).collect();
    bad.models = known(vec![model(
        "many",
        &names.iter().map(String::as_str).collect::<Vec<_>>(),
        true,
        false,
    )]);
    assert_eq!(catalog.register(bad), Err(CatalogError::CapacityExceeded));
    assert_eq!(catalog.resources().count(), 0);
    for n in 0..MAX_RESOURCES {
        let mut local = resource(&fixture(), "local").clone();
        local.identity.id = ResourceId::new(format!("local-{n}")).unwrap();
        catalog.register(local).unwrap();
    }
    assert_eq!(
        catalog.register(resource(&fixture(), "local").clone()),
        Err(CatalogError::CapacityExceeded)
    );
}

#[test]
fn variants_preserve_unavailable_and_unknown_efforts_without_default_invention() {
    let mut catalog = fixture();
    let id = ResourceId::new("included").unwrap();
    let unavailable = catalog
        .describe_variant(
            &id,
            &ModelId::new("Unavailable").unwrap(),
            Some(&EffortId::new("high").unwrap()),
        )
        .unwrap();
    assert_eq!(
        unavailable.model_availability,
        known(Availability::Unavailable)
    );
    let default = catalog
        .describe_variant(&id, &ModelId::new("Cheap").unwrap(), None)
        .unwrap();
    assert_eq!(default.effort, None);
    assert_eq!(default.effort_availability, CatalogFact::Unknown);
    let mut extra = included();
    extra.identity.id = ResourceId::new("extra").unwrap();
    extra.origin = ResourceOrigin::Provider(RuntimeId::new("extra-runtime").unwrap());
    let mut m = model("explicit", &["low", "high"], true, false);
    if let CatalogFact::Known { value, .. } = &mut m.supported_efforts {
        value[0].availability = CatalogFact::Unknown;
        value[1].availability = known(Availability::Unavailable);
    }
    extra.models = known(vec![m, model("no-efforts", &[], true, false)]);
    catalog.register(extra).unwrap();
    for (effort, expected) in [
        ("low", CatalogFact::Unknown),
        ("high", known(Availability::Unavailable)),
    ] {
        assert_eq!(
            catalog
                .describe_variant(
                    &ResourceId::new("extra").unwrap(),
                    &ModelId::new("explicit").unwrap(),
                    Some(&EffortId::new(effort).unwrap())
                )
                .unwrap()
                .effort_availability,
            expected
        );
    }
    assert_eq!(
        catalog.describe_variant(
            &ResourceId::new("extra").unwrap(),
            &ModelId::new("no-efforts").unwrap(),
            Some(&EffortId::new("low").unwrap())
        ),
        Err(CatalogError::EffortNotSupported)
    );
}

struct PrivateProvider {
    _private: [&'static str; 6],
}
impl crate::cognition::provider::Provider for PrivateProvider {
    fn execute<'a>(
        &'a self,
        _: &'a crate::cognition::types::ProviderRequest,
        _: &'a std::sync::atomic::AtomicBool,
        _: &'a mut (dyn FnMut(
            crate::cognition::types::ProviderChunk,
        ) -> Result<(), crate::cognition::types::ProviderError>
                     + Send),
    ) -> crate::cognition::provider::ProviderFuture<'a> {
        panic!("catalog must not execute providers")
    }
}
#[test]
fn independent_registries_and_safe_config_bridges_exclude_all_private_material() {
    let markers = [
        "sk-private-key-marker",
        "Bearer private-token-marker",
        "stronghold-private-marker",
        "remote-account-private-marker",
        "payment-card-private-marker",
        "private-prompt-marker",
    ];
    let mut providers = ProviderRegistry::default();
    let mut agents = AgentRegistry::default();
    providers
        .register(
            provider("same-id"),
            Arc::new(PrivateProvider { _private: markers }),
        )
        .unwrap();
    agents
        .register(
            AgentConfig {
                id: "same-id".into(),
                enabled: true,
                priority: 1,
                capabilities: AgentCapabilities {
                    planning: true,
                    structured_output: true,
                    ..Default::default()
                },
            },
            MockAgentBackend::new(markers.join(" ")),
        )
        .unwrap();
    let provider_config = &providers.get("same-id").unwrap().config;
    let agent_config = &agents.get("same-id").unwrap().config;
    let mut catalog = ResourceCatalog::default();
    catalog
        .register(
            CognitiveResource::from_provider_config(
                identity(
                    "provider",
                    ResourceClass::CognitiveProvider,
                    "generic-family",
                    "generic-path",
                    "provider-domain",
                ),
                provider_config,
            )
            .unwrap(),
        )
        .unwrap();
    catalog
        .register(
            CognitiveResource::from_agent_config(
                identity(
                    "agent",
                    ResourceClass::SpecialistAgent,
                    "generic-family",
                    "generic-path",
                    "agent-domain",
                ),
                agent_config,
            )
            .unwrap(),
        )
        .unwrap();
    let agent = resource(&catalog, "agent");
    let provider = resource(&catalog, "provider");
    assert_eq!(
        agent
            .capabilities
            .get(CognitiveCapability::Planning)
            .value(),
        Some(&true)
    );
    assert_eq!(
        agent
            .capabilities
            .get(CognitiveCapability::RepositoryRead)
            .value(),
        Some(&false)
    );
    assert_eq!(
        agent.capabilities.get(CognitiveCapability::ToolUse).value(),
        Some(&false)
    );
    assert_eq!(
        agent.capabilities.get(CognitiveCapability::TextGeneration),
        CatalogFact::Unknown
    );
    assert_eq!(
        provider.capabilities.get(CognitiveCapability::Planning),
        CatalogFact::Unknown
    );
    assert_eq!(
        provider
            .capabilities
            .get(CognitiveCapability::ToolCalling)
            .value(),
        Some(&false)
    );
    let json = serde_json::to_string(&catalog.snapshot(&[], &[]).unwrap()).unwrap();
    for marker in markers {
        assert!(!json.contains(marker));
    }
    for field in [
        "apiKey",
        "bearerToken",
        "stronghold",
        "accountId",
        "payment",
        "priority",
    ] {
        assert!(!json.contains(field));
    }
    assert_eq!(providers.configs().len(), 1);
    assert_eq!(agents.configs().len(), 1);
    providers
        .register(
            provider_config.clone(),
            Arc::new(MockProvider::new(MockScenario::Normal)),
        )
        .unwrap_err();
    assert_eq!(agents.configs().len(), 1);
}

#[test]
fn snapshot_normalization_preserves_every_lr8_provenance_and_unknown() {
    for provenance in [
        Provenance::ProviderHeader,
        Provenance::ProviderResponse,
        Provenance::UserConfiguration,
        Provenance::LocalRuntime,
    ] {
        let fact = Fact::Known {
            value: 0_u64,
            provenance,
            observed_at_unix_ms: Some(17),
        };
        let normalized = CatalogFact::from(&fact);
        assert_eq!(
            normalized,
            CatalogFact::known(0, CatalogProvenance::Operational(provenance), Some(17)).unwrap()
        );
        let json = serde_json::to_value(normalized).unwrap();
        assert_eq!(json["observedAtUnixMs"], 17);
        assert_eq!(json["value"], 0);
        assert_eq!(
            json["provenance"]["source"],
            serde_json::to_value(provenance).unwrap()
        );
    }
    assert_eq!(
        CatalogFact::<u64>::from(&Fact::Unknown),
        CatalogFact::Unknown
    );
    assert_ne!(CatalogFact::<u64>::Unknown, known(0));
    assert_ne!(
        CatalogFact::<BillingKind>::Unknown,
        configured(BillingKind::FreeTier)
    );
    assert_ne!(
        CatalogFact::<BillingKind>::Unknown,
        configured(BillingKind::MeteredBilling)
    );
    assert_ne!(
        CatalogFact::<Availability>::Unknown,
        known(Availability::Unavailable)
    );
}

#[test]
fn capability_bridges_preserve_all_declared_flags_in_their_own_domains() {
    use CognitiveCapability::*;
    for enabled in [false, true] {
        let provider = CapabilitySet::from_provider(ProviderCapabilities {
            text_generation: enabled,
            streaming: enabled,
            vision: enabled,
            tool_calling: enabled,
            structured_output: enabled,
        });
        for capability in [
            TextGeneration,
            Streaming,
            Vision,
            ToolCalling,
            StructuredOutput,
        ] {
            assert_eq!(
                provider.get(capability),
                CatalogFact::known(enabled, CatalogProvenance::RuntimeContract, None).unwrap()
            );
        }
        let agent = CapabilitySet::from_agent(AgentCapabilities {
            planning: enabled,
            repository_read: enabled,
            file_write: enabled,
            command_execution: enabled,
            tool_use: enabled,
            structured_output: enabled,
        });
        for capability in [
            Planning,
            RepositoryRead,
            FileWrite,
            CommandExecution,
            ToolUse,
            StructuredOutput,
        ] {
            assert_eq!(
                agent.get(capability),
                CatalogFact::known(enabled, CatalogProvenance::RuntimeContract, None).unwrap()
            );
        }
        assert_eq!(provider.get(ToolUse), CatalogFact::Unknown);
        assert_eq!(agent.get(ToolCalling), CatalogFact::Unknown);
    }
}

#[test]
fn config_enabled_never_proves_remote_availability_and_disabled_is_explicit() {
    let mut config = provider("runtime");
    let identity = identity(
        "config",
        ResourceClass::CognitiveProvider,
        "family",
        "path",
        "domain",
    );
    let enabled = CognitiveResource::from_provider_config(identity.clone(), &config).unwrap();
    assert_eq!(
        enabled.enabled,
        CatalogFact::known(true, CatalogProvenance::RuntimeContract, None).unwrap()
    );
    assert_eq!(enabled.availability, CatalogFact::Unknown);
    config.enabled = false;
    let disabled = CognitiveResource::from_provider_config(identity, &config).unwrap();
    assert_eq!(
        disabled.enabled,
        CatalogFact::known(false, CatalogProvenance::RuntimeContract, None).unwrap()
    );
    assert_eq!(
        disabled.availability,
        CatalogFact::known(
            Availability::Unavailable,
            CatalogProvenance::RuntimeContract,
            None
        )
        .unwrap()
    );
    assert_eq!(disabled.models, CatalogFact::Unknown);
}

#[test]
fn lr8_measured_usage_stays_provider_scoped_and_preserves_provenance() {
    let (store, rate) = authorities();
    let observation = store.attempt("runtime-included");
    assert!(observation.started());
    observation.observed_usage([
        (UsageDimension::InputTokens, Some(2)),
        (UsageDimension::OutputTokens, Some(3)),
    ]);
    let snapshot = fixture()
        .snapshot(&store.snapshots(), &rate.read_only_snapshots())
        .unwrap();
    let lr8 = snapshot
        .resources
        .iter()
        .find(|r| r.descriptor.identity.id.as_str() == "included")
        .unwrap()
        .lr8
        .as_ref()
        .unwrap();
    let t = lr8.telemetry.as_ref().unwrap();
    assert!(matches!(
        t.usage[&UsageDimension::Requests].observed,
        Fact::Known {
            value: 1,
            provenance: Provenance::LocalRuntime,
            ..
        }
    ));
    assert!(matches!(
        t.usage[&UsageDimension::InputTokens].observed,
        Fact::Known {
            value: 2,
            provenance: Provenance::ProviderResponse,
            ..
        }
    ));
    assert_eq!(t.usage[&UsageDimension::OutputTokens].reporting_requests, 1);
    assert_eq!(
        t.usage[&UsageDimension::TotalTokens].observed,
        Fact::Unknown
    );
    assert!(t.model_quotas.is_empty());
}
