//! B3 synthetic/integration gates. No commercial provider or real credential.
use super::*;
use crate::cognition::{
    admission::{AdmissionConfig, TrafficClass},
    policy::ThinkingLevel,
    provider::{Provider, ProviderFuture},
    rate::{ClockReading, FixedWindow, LocalRateLimit, RateClock, RatePolicy},
    scheduler::{Scheduler, SchedulerEvent},
    telemetry::{Fact, InvocationObservation, QuotaDimension, QuotaScope, UsageDimension},
    types::*,
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

// These fixtures keep B3 scenarios unchanged while passing their configured
// policy explicitly into each real Scheduler request/ranking and bridge call.
struct FixtureAllocator {
    engine: ProviderAutoAllocator,
    snapshot: AllocationRuntimePolicy,
}
impl FixtureAllocator {
    fn new(
        catalog: ResourceCatalog,
        policy: AllocationPolicy,
        floor: Option<QualityFloor>,
    ) -> Self {
        Self {
            engine: ProviderAutoAllocator::new(catalog),
            snapshot: AllocationRuntimePolicy::new(policy, floor),
        }
    }
    fn production(registry: &ProviderRegistry) -> Result<Self, ProviderBridgeError> {
        Ok(Self {
            engine: ProviderAutoAllocator::production(registry)?,
            snapshot: AllocationRuntimePolicy::new(provider_allocation_default(), None),
        })
    }
    fn catalog(&self) -> &ResourceCatalog {
        self.engine.catalog()
    }
    fn plan(
        &self,
        registry: &ProviderRegistry,
        targets: &[ProviderTarget],
        required: ProviderCapabilities,
        mode: &InvocationMode,
        affinity: Option<&str>,
        bytes: usize,
        telemetry: &[crate::cognition::telemetry::ProviderTelemetrySnapshot],
        rate: &[crate::cognition::rate::RateSnapshot],
    ) -> Result<AutoRoutePlan, ProviderBridgeError> {
        self.engine.plan(
            registry,
            &self.snapshot,
            targets,
            required,
            mode,
            affinity,
            bytes,
            telemetry,
            rate,
        )
    }
}
struct FixtureScheduler {
    engine: Scheduler,
    snapshot: AllocationRuntimePolicy,
}
impl std::ops::Deref for FixtureScheduler {
    type Target = Scheduler;
    fn deref(&self) -> &Scheduler {
        &self.engine
    }
}
impl FixtureScheduler {
    fn new(registry: ProviderRegistry) -> Self {
        Self {
            engine: Scheduler::new(registry),
            snapshot: AllocationRuntimePolicy::new(provider_allocation_default(), None),
        }
    }
    fn with_rate_config(
        registry: ProviderRegistry,
        admission: AdmissionConfig,
        clock: Arc<dyn RateClock>,
        storage: Option<crate::persistence::database::Database>,
    ) -> Result<Self, SchedulerError> {
        Ok(Self {
            engine: Scheduler::with_rate_config(registry, admission, clock, storage)?,
            snapshot: AllocationRuntimePolicy::new(provider_allocation_default(), None),
        })
    }
    fn with_auto_allocator(mut self, fixture: FixtureAllocator) -> Self {
        self.engine = self.engine.with_auto_allocator(fixture.engine);
        self.snapshot = fixture.snapshot;
        self
    }
    fn ranked_provider_ids(
        &self,
        selection: &ProviderSelection,
        targets: &[ProviderTarget],
        required: &ProviderCapabilities,
        mode: &InvocationMode,
    ) -> Result<Vec<String>, SchedulerError> {
        self.engine
            .ranked_provider_ids(selection, targets, required, mode, Some(&self.snapshot))
    }
    fn ranked_provider_targets(
        &self,
        selection: &ProviderSelection,
        targets: &[ProviderTarget],
        required: &ProviderCapabilities,
        mode: &InvocationMode,
    ) -> Result<Vec<ProviderTarget>, SchedulerError> {
        self.engine.ranked_provider_targets(
            selection,
            targets,
            required,
            mode,
            Some(&self.snapshot),
        )
    }
    async fn run(
        &self,
        mut request: ProviderTaskRequest,
        budget: TaskBudget,
        cancelled: &AtomicBool,
        events: &mut (dyn FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send),
    ) -> Result<TaskResult, SchedulerError> {
        request.allocation_policy = Some(self.snapshot.clone());
        self.engine.run(request, budget, cancelled, events).await
    }
    async fn run_with_retry(
        &self,
        mut request: ProviderTaskRequest,
        budget: TaskBudget,
        retry: RetryPolicy,
        cancelled: &AtomicBool,
        events: &mut (dyn FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send),
    ) -> Result<TaskResult, SchedulerError> {
        request.allocation_policy = Some(self.snapshot.clone());
        self.engine
            .run_with_retry(request, budget, retry, cancelled, events)
            .await
    }
}

struct Fake {
    seen: Mutex<Vec<ProviderTarget>>,
    supports: AtomicUsize,
    allowed: Option<Vec<(String, Option<ThinkingLevel>)>>,
    actions: Mutex<VecDeque<ProviderError>>,
    partial: bool,
    endpoint: Option<String>,
}
impl Fake {
    fn new() -> Self {
        Self {
            seen: Mutex::new(vec![]),
            supports: AtomicUsize::new(0),
            allowed: None,
            actions: Mutex::new(VecDeque::new()),
            partial: false,
            endpoint: None,
        }
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}
impl Provider for Fake {
    fn supports_invocation(
        &self,
        invocation: &ProviderInvocationConfig,
        mode: &InvocationMode,
    ) -> bool {
        self.supports.fetch_add(1, Ordering::SeqCst);
        invocation.valid()
            && mode.valid()
            && mode.text_stream()
            && self
                .allowed
                .as_ref()
                .is_none_or(|v| v.contains(&(invocation.model.clone(), invocation.thinking_level)))
    }
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async { panic!("must cross observed boundary") })
    }
    fn execute_observed<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        observation: &'a InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if !observation.started_unless_cancelled(cancelled) {
                return Err(ProviderError::Cancelled);
            }
            self.seen.lock().unwrap().push(request.target.clone());
            if let Some(endpoint) = &self.endpoint {
                reqwest::Client::new()
                    .post(endpoint)
                    .body("synthetic")
                    .send()
                    .await
                    .map_err(|_| ProviderError::Fatal)?;
            }
            if self.partial {
                chunk(ProviderChunk {
                    text: "private-output".into(),
                })?;
            }
            if let Some(error) = self.actions.lock().unwrap().pop_front() {
                return Err(error);
            }
            let usage = ProviderUsage {
                calls: 1,
                output_tokens: 1,
                total_tokens: Some(1),
                output_tokens_measured: true,
                ..Default::default()
            };
            observation.final_usage(usage);
            Ok(ProviderResponse {
                text: "ok".into(),
                usage,
            })
        })
    }
}
fn config(id: &str) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        enabled: true,
        priority: 0,
        capabilities: ProviderCapabilities::text_stream(),
    }
}
fn registry(providers: &[(&str, Arc<Fake>)]) -> ProviderRegistry {
    let mut registry = ProviderRegistry::default();
    for (id, provider) in providers {
        registry.register(config(id), provider.clone()).unwrap();
    }
    registry
}
fn known<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, Some(42)).unwrap()
}
fn resource(id: &str) -> CognitiveResource {
    CognitiveResource::from_provider_config(
        ResourceIdentity {
            id: ResourceId::new(id).unwrap(),
            class: ResourceClass::CognitiveProvider,
            family: ProviderFamily::new(id).unwrap(),
            access_path: AccessPath::new("provider_runtime").unwrap(),
            billing_domain: BillingDomain {
                id: BillingDomainId::new(id).unwrap(),
            },
        },
        &config(id),
    )
    .unwrap()
}
fn model(id: &str, cost: u16) -> ModelProfile {
    let mut model = ModelProfile::unknown(ModelId::new(id).unwrap());
    model.facts.execution.relative_cost = known(RelativeCostTier::new(cost).unwrap());
    model
}
fn effort(id: &str, cost: u16) -> EffortProfile {
    EffortProfile {
        id: EffortId::new(id).unwrap(),
        availability: CatalogFact::Unknown,
        facts: ExecutionFacts {
            relative_cost: known(RelativeCostTier::new(cost).unwrap()),
            ..Default::default()
        },
    }
}
fn catalog(resources: Vec<CognitiveResource>) -> ResourceCatalog {
    let mut catalog = ResourceCatalog::default();
    for resource in resources {
        catalog.register(resource).unwrap();
    }
    catalog
}
fn allocator(resources: Vec<CognitiveResource>, profile: AllocationProfile) -> FixtureAllocator {
    FixtureAllocator::new(
        catalog(resources),
        AllocationPolicy {
            profile,
            ..provider_allocation_default()
        },
        None,
    )
}
fn target(id: &str) -> ProviderTarget {
    ProviderTarget {
        provider_id: id.into(),
        invocation: ProviderInvocationConfig {
            model: "model-A".into(),
            thinking_level: None,
            timeouts: Some(ProviderTimeouts {
                request_timeout_ms: 1234,
                stream_idle_timeout_ms: 5678,
            }),
        },
    }
}
fn plan(
    allocator: &FixtureAllocator,
    registry: &ProviderRegistry,
    targets: &[ProviderTarget],
) -> Result<AutoRoutePlan, ProviderBridgeError> {
    allocator.plan(
        registry,
        targets,
        ProviderCapabilities::text_stream(),
        &InvocationMode::default(),
        None,
        0,
        &[],
        &[],
    )
}
fn request(ids: &[&str], selection: ProviderSelection) -> ProviderTaskRequest {
    ProviderTaskRequest {
        allocation_policy: None,
        traffic_class: TrafficClass::ForegroundInteractive,
        mode: InvocationMode::default(),
        input: "private-prompt".into(),
        internal_system_instruction: Some("private-instruction".into()),
        history: vec![ProviderMessage {
            role: ProviderRole::User,
            content: "private-history".into(),
        }],
        context: Arc::new(crate::cognition::orchestrator::technical_context()),
        max_output_tokens: None,
        selection,
        targets: ids.iter().map(|id| target(id)).collect(),
        affinity_key: None,
        estimated_context_bytes: 0,
        required_capabilities: ProviderCapabilities::text_stream(),
    }
}
async fn run(
    scheduler: &FixtureScheduler,
    request: ProviderTaskRequest,
    calls: u32,
    retries: u32,
) -> (Result<TaskResult, SchedulerError>, Vec<SchedulerEvent>) {
    let mut events = vec![];
    let result = scheduler
        .run_with_retry(
            request,
            TaskBudget {
                max_provider_calls: calls,
                max_output_tokens: None,
            },
            RetryPolicy {
                enabled: retries > 0,
                max_retries: retries,
                initial_backoff_ms: 0,
            },
            &AtomicBool::new(false),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .await;
    (result, events)
}
fn selected(events: &[SchedulerEvent]) -> Vec<(String, String, Option<i64>)> {
    events
        .iter()
        .filter_map(|e| {
            if let SchedulerEvent::Selected {
                provider_id,
                model,
                score,
                ..
            } = e
            {
                Some((provider_id.clone(), model.clone(), *score))
            } else {
                None
            }
        })
        .collect()
}
fn allowance(resource: &mut CognitiveResource, remaining: u64) {
    let mut m = model("model-A", 0);
    m.facts
        .execution
        .allowance_costs
        .push(AllowanceConsumption {
            dimension_id: AllowanceDimensionId::new("weekly").unwrap(),
            unit: AllowanceUnit::Requests,
            amount: known(1),
        });
    resource.models = known(vec![m]);
    resource.economics.allowances.push(AllowanceState {
        id: AllowanceDimensionId::new("weekly").unwrap(),
        unit: AllowanceUnit::Requests,
        limit: known(100),
        remaining: known(remaining),
        reset: CatalogFact::Unknown,
    });
}
fn pressure(scheduler: &FixtureScheduler, provider: &str, scope: QuotaScope) {
    scheduler
        .rate
        .set_policy(
            provider,
            RatePolicy {
                limits: vec![LocalRateLimit {
                    scope,
                    dimension: QuotaDimension::RequestsPerMinute,
                    capacity: 0,
                    window: FixedWindow {
                        period_ms: 60_000,
                        anchor_unix_ms: 0,
                    },
                }],
                daily_budget: None,
            },
        )
        .unwrap();
}

#[test]
fn b3_defaults_are_explicit_conservative_without_thresholds() {
    assert_eq!(
        provider_allocation_default(),
        AllocationPolicy {
            profile: AllocationProfile::Balanced,
            variant_selection_mode: VariantSelectionMode::Auto,
            paid_use: PaidUsePolicy::Deny,
            reserve: None
        }
    );
}
#[test]
fn b3_production_catalog_is_identity_only_unknown_economics_and_models() {
    let r = registry(&[("a", Arc::new(Fake::new())), ("b", Arc::new(Fake::new()))]);
    let allocator = FixtureAllocator::production(&r).unwrap();
    assert_eq!(allocator.catalog().resources().count(), 2);
    for descriptor in allocator.catalog().resources() {
        assert_eq!(descriptor.models, CatalogFact::Unknown);
        assert_eq!(descriptor.economics, EconomicFacts::default());
        assert_eq!(
            descriptor.identity.id.as_str(),
            descriptor.identity.family.as_str()
        );
        assert_eq!(
            descriptor.identity.id.as_str(),
            descriptor.identity.billing_domain.id.as_str()
        );
        assert_eq!(descriptor.identity.access_path.as_str(), "provider_runtime");
    }
}
#[test]
fn b3_unknown_exact_invocation_scores_only_real_signals_without_fabrication() {
    let r = resource("a");
    let original = r.clone();
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let allocator = allocator(vec![r], AllocationProfile::Balanced);
    let p = plan(&allocator, &registry, &[target("a")]).unwrap();
    assert_eq!(p.entries()[0].score, 1533);
    let r = allocator
        .catalog()
        .resource(&ResourceId::new("a").unwrap())
        .unwrap();
    assert_eq!(r, &original);
    let candidate = AllocationCandidate::new(r, ModelId::new("model-A").unwrap(), None).unwrap();
    let facts = ResolvedExecutionFacts::for_provider_candidate(&candidate).unwrap();
    assert_eq!(facts.relative_cost.fact, CatalogFact::Unknown);
    assert_eq!(facts.latency_ms.fact, CatalogFact::Unknown);
    assert_eq!(facts.monetary_cost.fact, CatalogFact::Unknown);
    assert_eq!(facts.cognitive_tier.fact, CatalogFact::Unknown);
    assert!(facts.allowance_costs.is_empty());
    // Generic B2 remains fail closed on this same B1 Unresolved request.
    let b1 = AllocationRequest::new(
        provider_requirements(ProviderCapabilities::text_stream(), None),
        provider_allocation_default(),
        vec![candidate.clone()],
    )
    .unwrap();
    assert_eq!(
        AllocationScoringRequest::new(
            &b1,
            vec![ScoringCandidate {
                variant: candidate.variant(),
                signals: CandidateSignals::default()
            }],
            vec![]
        )
        .err(),
        Some(ScoringError::B1Unresolved)
    );
}
#[test]
fn b3_unknown_model_with_explicit_effort_is_exactly_proven_not_expanded() {
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let allocator = allocator(vec![resource("a")], AllocationProfile::Balanced);
    let mut t = target("a");
    t.invocation.thinking_level = Some(ThinkingLevel::High);
    let p = plan(&allocator, &registry, &[t.clone()]).unwrap();
    assert_eq!(p.entries()[0].target, t);
    assert!(p.exclusions().is_empty());
}
#[test]
fn b3_adapter_false_and_unknown_quality_floor_cannot_be_promoted() {
    let fake = Arc::new(Fake {
        allowed: Some(vec![]),
        ..Fake::new()
    });
    let registry = registry(&[("a", fake)]);
    assert_eq!(
        plan(
            &allocator(vec![resource("a")], AllocationProfile::Balanced),
            &registry,
            &[target("a")]
        ),
        Err(ProviderBridgeError::NoEligibleCandidates)
    );
    let registry = super::tests::registry(&[("a", Arc::new(Fake::new()))]);
    let allocator = FixtureAllocator::new(
        catalog(vec![resource("a")]),
        provider_allocation_default(),
        Some(CognitiveTier::new(1).unwrap()),
    );
    assert_eq!(
        plan(&allocator, &registry, &[target("a")]),
        Err(ProviderBridgeError::NoEligibleCandidates)
    );
}
#[test]
fn b3_registry_only_provider_never_enters_authorized_universe() {
    let registry = registry(&[("a", Arc::new(Fake::new())), ("b", Arc::new(Fake::new()))]);
    let allocator = FixtureAllocator::production(&registry).unwrap();
    let p = plan(&allocator, &registry, &[target("a")]).unwrap();
    assert_eq!(p.entries().len(), 1);
    assert_eq!(p.entries()[0].target.provider_id, "a");
}
#[test]
fn b3_known_model_contradiction_wins_operational_support() {
    let mut r = resource("a");
    r.models = known(vec![model("model-B", 50)]);
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let p = plan(
        &allocator(vec![r], AllocationProfile::Balanced),
        &registry,
        &[target("a")],
    )
    .unwrap();
    assert_eq!(p.entries()[0].target.invocation.model, "model-B");
    assert!(p.exclusions().iter().any(|e| matches!(&e.reason, ProviderVariantExclusionReason::HardEligibility { reasons } if reasons.contains(&EligibilityReason::ModelNotSupported))));
}
#[test]
fn b3_known_effort_absent_is_not_reintroduced() {
    let mut m = model("model-A", 1);
    m.supported_efforts = known(vec![effort("low", 10)]);
    let mut r = resource("a");
    r.models = known(vec![m]);
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let mut t = target("a");
    t.invocation.thinking_level = Some(ThinkingLevel::High);
    let p = plan(
        &allocator(vec![r], AllocationProfile::Balanced),
        &registry,
        &[t],
    )
    .unwrap();
    assert_ne!(
        p.entries()[0].target.invocation.thinking_level,
        Some(ThinkingLevel::High)
    );
    assert!(p.exclusions().iter().any(|e| matches!(&e.reason, ProviderVariantExclusionReason::HardEligibility { reasons } if reasons.contains(&EligibilityReason::EffortNotSupported))));
}
#[test]
fn b3_known_unavailable_resource_model_and_effort_remain_excluded() {
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    for layer in [
        EvidenceLayer::Resource,
        EvidenceLayer::Model,
        EvidenceLayer::Effort,
    ] {
        let mut r = resource("a");
        let mut m = model("model-A", 1);
        let mut e = effort("high", 0);
        match layer {
            EvidenceLayer::Resource => r.availability = known(Availability::Unavailable),
            EvidenceLayer::Model => m.availability = known(Availability::Unavailable),
            EvidenceLayer::Effort => e.availability = known(Availability::Unavailable),
        }
        m.supported_efforts = known(vec![e]);
        r.models = known(vec![m]);
        let mut policy = provider_allocation_default();
        policy.variant_selection_mode = VariantSelectionMode::Explicit;
        let a = FixtureAllocator::new(catalog(vec![r]), policy, None);
        let mut t = target("a");
        t.invocation.thinking_level = Some(ThinkingLevel::High);
        assert_eq!(
            plan(&a, &registry, &[t]),
            Err(ProviderBridgeError::NoEligibleCandidates)
        );
    }
}
#[test]
fn b3_capabilities_are_runtime_scoped_and_never_copied_to_models() {
    let required = ProviderCapabilities::with_structured_output();
    let requirements = provider_requirements(required, None);
    assert!(requirements
        .required_capabilities()
        .iter()
        .all(|r| r.scope == CapabilityScope::Runtime));
    assert_eq!(model("model-A", 1).capabilities, CapabilitySet::default());
    // Default synthetic adapter cannot execute structured even with broad config.
    let mut registry = ProviderRegistry::default();
    let mut config = config("a");
    config.capabilities = required;
    registry
        .register(config.clone(), Arc::new(Fake::new()))
        .unwrap();
    let mut r = resource("a");
    r.capabilities = CapabilitySet::from_provider(required);
    let a = allocator(vec![r], AllocationProfile::Balanced);
    let mode = InvocationMode {
        output: OutputContract::JsonSchema {
            name: "result".into(),
            schema: serde_json::json!({"type":"object"}),
            max_bytes: 1024,
        },
        transport: TransportMode::NonStreaming,
    };
    assert_eq!(
        a.plan(
            &registry,
            &[target("a")],
            ProviderCapabilities::structured(),
            &mode,
            None,
            0,
            &[],
            &[]
        ),
        Err(ProviderBridgeError::NoEligibleCandidates)
    );
}
#[test]
fn b3_economy_expansion_selects_effort_and_preserves_original_timeouts() {
    let mut m = model("model-1", 150);
    m.supported_efforts = known(vec![effort("low", 100), effort("high", 20)]);
    let mut r = resource("a");
    r.models = known(vec![m, model("model-2", 50)]);
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let p = plan(
        &allocator(vec![r], AllocationProfile::Economy),
        &registry,
        &[target("a")],
    )
    .unwrap();
    assert_eq!(p.entries().len(), 1);
    assert_eq!(p.entries()[0].target.invocation.model, "model-1");
    assert_eq!(
        p.entries()[0].target.invocation.thinking_level,
        Some(ThinkingLevel::High)
    );
    assert_eq!(
        p.entries()[0].target.invocation.timeouts,
        target("a").invocation.timeouts
    );
}
#[test]
fn b3_collapse_multiple_providers_variants_uses_global_best_order() {
    let resources = [
        ("a", vec![90, 80, 70]),
        ("b", vec![50, 40]),
        ("c", vec![10]),
    ]
    .into_iter()
    .map(|(id, costs)| {
        let mut r = resource(id);
        r.models = known(
            costs
                .into_iter()
                .enumerate()
                .map(|(n, c)| model(&format!("model-{n}"), c))
                .collect(),
        );
        r
    })
    .collect();
    let registry = registry(&[
        ("a", Arc::new(Fake::new())),
        ("b", Arc::new(Fake::new())),
        ("c", Arc::new(Fake::new())),
    ]);
    let p = plan(
        &allocator(resources, AllocationProfile::Economy),
        &registry,
        &[target("a"), target("b"), target("c")],
    )
    .unwrap();
    assert_eq!(
        p.entries()
            .iter()
            .map(|e| e.target.provider_id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "b", "a"]
    );
    assert_eq!(p.entries().len(), 3);
    assert!(p.entries().windows(2).all(|p| p[0].score > p[1].score));
}
#[test]
fn b3_xhigh_ultra_reasoning4_are_typed_bridge_exclusions_without_downgrade() {
    let mut m = model("model-A", 100);
    m.supported_efforts = known(vec![
        effort("low", 50),
        effort("xhigh", 0),
        effort("ultra", 0),
        effort("reasoning-4", 0),
    ]);
    let mut r = resource("a");
    r.models = known(vec![m]);
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let p = plan(
        &allocator(vec![r], AllocationProfile::Economy),
        &registry,
        &[target("a")],
    )
    .unwrap();
    assert_eq!(
        p.entries()[0].target.invocation.thinking_level,
        Some(ThinkingLevel::Low)
    );
    assert_eq!(
        p.exclusions()
            .iter()
            .filter(|e| e.reason == ProviderVariantExclusionReason::EffortBridgeUnsupported)
            .count(),
        3
    );
}
#[test]
fn b3_expansion_overflow_fails_closed_without_truncation_or_adapter_checks() {
    let fake = Arc::new(Fake::new());
    let registry = registry(&[("a", fake.clone())]);
    let mut r = resource("a");
    r.models = known(
        (0..128)
            .map(|n| {
                let mut m = model(&format!("m{n}"), 1);
                m.supported_efforts = known(vec![effort("low", 1), effort("high", 1)]);
                m
            })
            .collect(),
    );
    assert_eq!(
        plan(
            &allocator(vec![r], AllocationProfile::Balanced),
            &registry,
            &[target("a")]
        ),
        Err(ProviderBridgeError::ExpansionOverflow)
    );
    assert_eq!(fake.supports.load(Ordering::SeqCst), 0);
    assert_eq!(fake.calls(), 0);
}
#[test]
fn b3_mismatched_binding_class_and_runtime_facts_fail_closed() {
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    for case in 0..4 {
        let mut r = resource("a");
        match case {
            0 => r.origin = ResourceOrigin::Provider(RuntimeId::new("b").unwrap()),
            1 => {
                r.identity.class = ResourceClass::LocalSupport;
                r.origin = ResourceOrigin::Local;
            }
            2 => r.enabled = known(false),
            _ => r
                .capabilities
                .0
                .insert(CognitiveCapability::Streaming, known(false))
                .map(|_| ())
                .unwrap_or(()),
        }
        assert_eq!(
            plan(
                &allocator(vec![r], AllocationProfile::Balanced),
                &registry,
                &[target("a")]
            ),
            Err(ProviderBridgeError::CatalogRuntimeMismatch)
        );
    }
}
#[test]
fn b3_operational_proof_cannot_be_reused_for_sibling_or_other_effort() {
    let r = resource("a");
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let candidate = AllocationCandidate::new(&r, ModelId::new("model-A").unwrap(), None).unwrap();
    let requirements = provider_requirements(ProviderCapabilities::text_stream(), None);
    let b1 = AllocationRequest::new(
        requirements,
        provider_allocation_default(),
        vec![candidate.clone()],
    )
    .unwrap()
    .evaluate();
    let proof = OperationalVariantProof::prove(
        registry.get("a").unwrap(),
        &candidate,
        &target("a"),
        ProviderCapabilities::text_stream(),
        &InvocationMode::default(),
    )
    .unwrap();
    for (name, effort) in [
        ("model-B", None),
        ("model-A", Some(EffortId::new("high").unwrap())),
    ] {
        let other = AllocationCandidate::new(&r, ModelId::new(name).unwrap(), effort).unwrap();
        assert!(matches!(
            ResolvedProviderCandidate::resolve(
                other,
                b1.candidates()[0].clone(),
                proof.clone(),
                CandidateSignals::default(),
                &target("a"),
                &InvocationMode::default(),
            ),
            Err(ProviderBridgeError::InvalidOperationalProof)
        ));
    }
}
#[test]
fn b3_context_signal_bounded_monotonic_zero_and_no_overflow() {
    assert_eq!(context_switch_signal(0), 0);
    assert_eq!(context_switch_signal(1), 1);
    assert_eq!(context_switch_signal(usize::MAX), 100);
    let mut prev = 0;
    for bytes in 0..=150_000 {
        let signal = context_switch_signal(bytes);
        assert!(signal >= prev && signal <= 100);
        prev = signal;
    }
}
#[test]
fn b3_candidate_signals_keep_ordinal_clamp_priority_and_neutral_unknown_affinity() {
    let s = provider_candidate_signals(7, u16::MAX, "a", None, usize::MAX).unwrap();
    assert_eq!(s.preference_ordinal(), Some(7));
    assert_eq!(s.registry_priority(), Some(32));
    assert_eq!((s.continuity(), s.switching_cost()), (None, None));
    let a = provider_candidate_signals(0, 0, "a", Some("a"), 4096).unwrap();
    let b = provider_candidate_signals(1, 0, "b", Some("a"), 4096).unwrap();
    assert_eq!((a.continuity(), a.switching_cost()), (Some(4), Some(0)));
    assert_eq!((b.continuity(), b.switching_cost()), (Some(0), Some(4)));
    let zero = provider_candidate_signals(0, 0, "a", Some("a"), 0).unwrap();
    assert_eq!((zero.continuity(), zero.switching_cost()), (None, None));
}
#[test]
fn b3_determinism_independent_of_registry_and_catalog_order() {
    let fakes = [("a", Arc::new(Fake::new())), ("b", Arc::new(Fake::new()))];
    let reversed = [fakes[1].clone(), fakes[0].clone()];
    let r1 = registry(&fakes);
    let r2 = registry(&reversed);
    let a1 = allocator(
        vec![resource("a"), resource("b")],
        AllocationProfile::Balanced,
    );
    let a2 = allocator(
        vec![resource("b"), resource("a")],
        AllocationProfile::Balanced,
    );
    let targets = [target("b"), target("a")];
    assert_eq!(plan(&a1, &r1, &targets), plan(&a2, &r2, &targets));
}
#[test]
fn b3_plan_is_sanitized_and_ranking_does_not_mutate_catalog_or_dtos() {
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let a = allocator(vec![resource("a")], AllocationProfile::Balanced);
    let before = serde_json::to_value(a.catalog().snapshot(&[], &[]).unwrap()).unwrap();
    for _ in 0..3 {
        let p = plan(&a, &registry, &[target("a")]).unwrap();
        let json = serde_json::to_string(&p).unwrap();
        for forbidden in [
            "private-prompt",
            "private-history",
            "private-instruction",
            "credential",
            "schema",
            "breakdown",
            "accountId",
        ] {
            assert!(!json.contains(forbidden));
        }
    }
    assert_eq!(
        before,
        serde_json::to_value(a.catalog().snapshot(&[], &[]).unwrap()).unwrap()
    );
}

#[tokio::test]
async fn b3_fixed_and_preferred_ignore_paid_exhausted_expensive_catalog() {
    let a = Arc::new(Fake::new());
    let b = Arc::new(Fake::new());
    let mut ra = resource("a");
    allowance(&mut ra, 0);
    ra.economics.billing_kind = known(BillingKind::MeteredBilling);
    if let CatalogFact::Known { value, .. } = &mut ra.models {
        value[0].facts.execution.relative_cost = known(RelativeCostTier::new(255).unwrap());
    }
    let mut rb = resource("b");
    allowance(&mut rb, 80);
    rb.economics.billing_kind = known(BillingKind::FreeTier);
    let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())]))
        .with_auto_allocator(allocator(vec![ra, rb], AllocationProfile::Balanced));
    for selection in [
        ProviderSelection::Fixed("a".into()),
        ProviderSelection::Preferred,
    ] {
        let mut req = request(&["a", "b"], selection);
        req.targets[0].invocation.thinking_level = Some(ThinkingLevel::High);
        let original = req.targets[0].clone();
        let (result, events) = run(&s, req, 2, 0).await;
        assert_eq!(result.unwrap().provider_id, "a");
        assert_eq!(a.seen.lock().unwrap().last(), Some(&original));
        assert!(selected(&events)
            .iter()
            .all(|(_, _, score)| score.is_none()));
    }
    assert_eq!((a.calls(), b.calls()), (2, 0));
}

#[tokio::test]
async fn b3_explicit_ranking_and_execution_preserve_original_operational_distinction() {
    for case in 0..3 {
        let mut fake = Fake::new();
        if case == 2 {
            fake.allowed = Some(vec![]);
        }
        let a = Arc::new(fake);
        let b = Arc::new(Fake::new());
        let mut a_config = config("a");
        if case == 0 {
            a_config.enabled = false;
        } else if case == 1 {
            a_config.capabilities.streaming = false;
        }
        let mut registry = ProviderRegistry::default();
        registry.register(a_config, a.clone()).unwrap();
        registry.register(config("b"), b.clone()).unwrap();
        let s = FixtureScheduler::new(registry);
        let req = request(&["a", "b"], ProviderSelection::Preferred);
        let ids = s
            .ranked_provider_ids(
                &req.selection,
                &req.targets,
                &req.required_capabilities,
                &req.mode,
            )
            .unwrap();
        assert_eq!(ids, if case == 2 { vec!["a", "b"] } else { vec!["b"] });
        // Ranking never attempted an exact invocation for explicit policy.
        assert_eq!(a.supports.load(Ordering::SeqCst), 0);
        assert_eq!(
            run(&s, req, 2, 0).await.0.unwrap_err(),
            SchedulerError::NoProvider
        );
        assert_eq!(
            run(
                &s,
                request(&["a", "b"], ProviderSelection::Fixed("a".into())),
                2,
                0
            )
            .await
            .0
            .unwrap_err(),
            SchedulerError::NoProvider
        );
        assert_eq!(
            run(
                &s,
                request(&["a", "b"], ProviderSelection::Fixed("b".into())),
                2,
                0
            )
            .await
            .0
            .unwrap()
            .provider_id,
            "b"
        );
        assert_eq!((a.calls(), b.calls()), (0, 1));
    }
}
#[tokio::test]
async fn b3_paid_deny_excludes_before_reservation_and_real_loopback_http() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(Mutex::new(vec![]));
    let received = paths.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                if bytes.len() >= end + 4 + 9 {
                    break;
                }
            }
        }
        received.lock().unwrap().push(
            String::from_utf8(bytes)
                .unwrap()
                .lines()
                .next()
                .unwrap()
                .to_owned(),
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let a = Arc::new(Fake {
        endpoint: Some(format!("http://{address}/a")),
        ..Fake::new()
    });
    let b = Arc::new(Fake {
        endpoint: Some(format!("http://{address}/b")),
        ..Fake::new()
    });
    let mut ra = resource("a");
    ra.economics.billing_kind = known(BillingKind::MeteredBilling);
    let mut m = model("model-A", 0);
    m.facts.execution.latency_ms = known(1);
    ra.models = known(vec![m]);
    let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())]))
        .with_auto_allocator(allocator(vec![ra, resource("b")], AllocationProfile::Fast));
    let (result, events) = run(&s, request(&["a", "b"], ProviderSelection::Auto), 2, 0).await;
    assert_eq!(result.unwrap().provider_id, "b");
    server.join().unwrap();
    assert_eq!(&*paths.lock().unwrap(), &["POST /b HTTP/1.1"]);
    assert_eq!((a.calls(), b.calls()), (0, 1));
    assert!(events
        .iter()
        .all(|e| !matches!(e,SchedulerEvent::Fallback { to,.. } if to=="a")));
    let rate = s.rate.read_only_snapshots();
    let a = rate.iter().find(|s| s.provider_id == "a").unwrap();
    assert_eq!(a.pending_reservations, 0);
    assert_eq!(a.local_blocks, 0);
    assert_eq!(
        s.admission_snapshot()
            .iter()
            .find(|s| s.provider_id == "a")
            .unwrap()
            .total_admissions,
        0
    );
    let t = s
        .telemetry_snapshot()
        .into_iter()
        .find(|s| s.provider_id == "a")
        .unwrap();
    assert!(matches!(
        t.usage[&UsageDimension::Requests].observed,
        Fact::Known { value: 0, .. }
    ));
}
#[tokio::test]
async fn b3_reserve_reorders_but_single_reserve_remains_usable_exhausted_excludes() {
    for (remaining, only_a, winner) in [(5, false, "b"), (5, true, "a"), (0, false, "b")] {
        let a = Arc::new(Fake::new());
        let b = Arc::new(Fake::new());
        let mut ra = resource("a");
        allowance(&mut ra, remaining);
        let mut rb = resource("b");
        allowance(&mut rb, 80);
        let allocation = FixtureAllocator::new(
            catalog(vec![ra, rb]),
            AllocationPolicy {
                reserve: Some(ReservePolicy::new(40, 10).unwrap()),
                ..provider_allocation_default()
            },
            None,
        );
        let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())]))
            .with_auto_allocator(allocation);
        let (result, _) = run(
            &s,
            request(
                if only_a { &["a"] } else { &["a", "b"] },
                ProviderSelection::Auto,
            ),
            2,
            0,
        )
        .await;
        assert_eq!(result.unwrap().provider_id, winner);
        if winner == "b" {
            assert_eq!(a.calls(), 0);
        }
    }
}
#[tokio::test]
async fn b3_lr8_provider_exact_model_exclude_and_sibling_does_not_interfere_unknown_catalog() {
    for (scope, winner) in [
        (QuotaScope::Provider, "b"),
        (
            QuotaScope::Model {
                model: "model-A".into(),
            },
            "b",
        ),
        (
            QuotaScope::Model {
                model: "sibling".into(),
            },
            "a",
        ),
    ] {
        let a = Arc::new(Fake::new());
        let b = Arc::new(Fake::new());
        let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())]));
        pressure(&s, "a", scope);
        let req = request(&["a", "b"], ProviderSelection::Auto);
        assert_eq!(
            s.ranked_provider_ids(
                &req.selection,
                &req.targets,
                &req.required_capabilities,
                &req.mode
            )
            .unwrap()[0],
            winner
        );
        let (result, events) = run(&s, req, 2, 0).await;
        assert_eq!(result.unwrap().provider_id, winner);
        assert_eq!(selected(&events)[0].0, winner);
        if winner == "b" {
            assert_eq!(a.calls(), 0);
        }
    }
}
#[tokio::test]
async fn b3_operational_rate_gate_revalidates_after_frozen_plan_without_rescore() {
    let a = Arc::new(Fake::new());
    let b = Arc::new(Fake::new());
    let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())]));
    let result = s
        .run(
            request(&["a", "b"], ProviderSelection::Auto),
            TaskBudget {
                max_provider_calls: 2,
                max_output_tokens: None,
            },
            &AtomicBool::new(false),
            &mut |e| {
                if matches!(e, SchedulerEvent::Selected { .. }) {
                    pressure(&s, "a", QuotaScope::Provider);
                }
                Ok(())
            },
        )
        .await;
    assert_eq!(result.unwrap_err(), SchedulerError::RateCapacityExceeded);
    assert_eq!((a.calls(), b.calls()), (0, 0));
}
#[tokio::test]
async fn b3_affinity_real_success_to_b2_zero_context_and_restart() {
    let a = Arc::new(Fake::new());
    let b = Arc::new(Fake::new());
    let providers = [("a", a.clone()), ("b", b.clone())];
    let s = FixtureScheduler::new(registry(&providers));
    let mut req = request(&["b"], ProviderSelection::Fixed("b".into()));
    req.affinity_key = Some("session".into());
    run(&s, req, 1, 0).await.0.unwrap();
    for (scheduler, bytes, winner) in [
        (&s, 4096, "b"),
        (&s, 0, "a"),
        (
            &FixtureScheduler::new(registry(&providers)),
            usize::MAX,
            "a",
        ),
    ] {
        let mut req = request(&["a", "b"], ProviderSelection::Auto);
        req.affinity_key = Some("session".into());
        req.estimated_context_bytes = bytes;
        let (result, events) = run(scheduler, req, 2, 0).await;
        assert_eq!(result.unwrap().provider_id, winner);
        assert!(matches!(
            events[0],
            SchedulerEvent::Selected {
                routing_reason: "auto_allocator",
                score: Some(_),
                ..
            }
        ));
    }
}
#[tokio::test]
async fn b3_run_and_ranked_ids_and_taskgraph_targets_share_variant_engine() {
    let a = Arc::new(Fake::new());
    let b = Arc::new(Fake::new());
    let mut ra = resource("a");
    ra.models = known(vec![model("better", 0)]);
    let mut rb = resource("b");
    rb.models = known(vec![model("model-A", 200)]);
    let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())]))
        .with_auto_allocator(allocator(vec![ra, rb], AllocationProfile::Economy));
    let req = request(&["b", "a"], ProviderSelection::Auto);
    let ids = s
        .ranked_provider_ids(
            &req.selection,
            &req.targets,
            &req.required_capabilities,
            &req.mode,
        )
        .unwrap();
    let chosen = s
        .ranked_provider_targets(
            &req.selection,
            &req.targets,
            &req.required_capabilities,
            &req.mode,
        )
        .unwrap();
    assert_eq!(chosen[0].invocation.model, "better");
    assert_eq!(ids, vec!["a", "b"]);
    let (result, events) = run(&s, req, 2, 0).await;
    assert_eq!(result.unwrap().provider_id, ids[0]);
    assert_eq!(selected(&events)[0].1, "better");
    assert_eq!(a.seen.lock().unwrap()[0], chosen[0]);
}
#[tokio::test]
async fn b3_negative_b2_score_serializes_through_selected_event_without_private_payload() {
    let a = Arc::new(Fake::new());
    let mut ra = resource("a");
    ra.models = known(vec![model("model-A", 255)]);
    let s = FixtureScheduler::new(registry(&[("a", a)]))
        .with_auto_allocator(allocator(vec![ra], AllocationProfile::Economy));
    let (result, events) = run(&s, request(&["a"], ProviderSelection::Auto), 1, 0).await;
    result.unwrap();
    let SchedulerEvent::Selected {
        provider_id,
        model,
        attempt,
        routing_reason,
        score,
    } = &events[0]
    else {
        panic!()
    };
    assert!(score.unwrap() < 0);
    let event = crate::luna::task::TaskEventKind::ProviderSelected {
        provider_id: provider_id.clone(),
        model: model.clone(),
        attempt: *attempt,
        routing_reason: (*routing_reason).into(),
        score: *score,
    };
    let value = serde_json::to_value(event).unwrap();
    assert_eq!(value["score"].as_i64(), *score);
    assert_eq!(value["routing_reason"], "auto_allocator");
    let json = value.to_string();
    for forbidden in [
        "private-prompt",
        "private-history",
        "private-instruction",
        "private-output",
        "schema",
        "context",
    ] {
        assert!(!json.contains(forbidden));
    }
}
#[tokio::test]
async fn b3_retry_fallback_freezes_variants_scores_order_and_call_budget() {
    let a = Arc::new(Fake {
        actions: Mutex::new([ProviderError::Timeout, ProviderError::Timeout].into()),
        ..Fake::new()
    });
    let b = Arc::new(Fake::new());
    let mut ra = resource("a");
    ra.models = known(vec![model("winner", 0), model("second", 50)]);
    let s =
        FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())])).with_auto_allocator(
            allocator(vec![ra, resource("b")], AllocationProfile::Economy),
        );
    let (result, events) = run(&s, request(&["a", "b"], ProviderSelection::Auto), 3, 1).await;
    let result = result.unwrap();
    assert_eq!(result.provider_id, "b");
    assert_eq!(
        (
            result.usage.provider_calls,
            result.usage.retries,
            result.usage.fallbacks
        ),
        (3, 1, 1)
    );
    let chosen = selected(&events);
    assert_eq!(
        chosen.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
        vec!["a", "a", "b"]
    );
    assert_eq!(chosen[0], chosen[1]);
    let seen = a.seen.lock().unwrap().clone();
    assert_eq!(seen[0], seen[1]);
    assert_eq!(a.supports.load(Ordering::SeqCst), 3); // exactly expansion once
}
#[tokio::test]
async fn b3_429_never_reintroduces_paid_candidate_or_replans() {
    let a = Arc::new(Fake {
        actions: Mutex::new(
            [ProviderError::RateLimited {
                retry_after_ms: Some(1),
            }]
            .into(),
        ),
        ..Fake::new()
    });
    let b = Arc::new(Fake::new());
    let mut rb = resource("b");
    rb.economics.billing_kind = known(BillingKind::MeteredBilling);
    let s =
        FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())])).with_auto_allocator(
            allocator(vec![resource("a"), rb], AllocationProfile::Balanced),
        );
    let (result, events) = run(&s, request(&["a", "b"], ProviderSelection::Auto), 4, 3).await;
    assert!(matches!(
        result,
        Err(SchedulerError::Provider(ProviderError::RateLimited { .. }))
    ));
    assert_eq!((a.calls(), b.calls()), (1, 0));
    assert_eq!(a.supports.load(Ordering::SeqCst), 1);
    assert_eq!(b.supports.load(Ordering::SeqCst), 1);
    assert!(!events
        .iter()
        .any(|e| matches!(e, SchedulerEvent::Fallback { .. })));
}
#[tokio::test]
async fn b3_partial_output_still_prevents_retry_and_fallback() {
    let a = Arc::new(Fake {
        actions: Mutex::new([ProviderError::Timeout].into()),
        partial: true,
        ..Fake::new()
    });
    let b = Arc::new(Fake::new());
    let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("b", b.clone())]));
    let (result, events) = run(&s, request(&["a", "b"], ProviderSelection::Auto), 4, 3).await;
    assert_eq!(
        result.unwrap_err(),
        SchedulerError::Provider(ProviderError::Timeout)
    );
    assert_eq!((a.calls(), b.calls()), (1, 0));
    assert!(!events.iter().any(|e| matches!(
        e,
        SchedulerEvent::Retry { .. } | SchedulerEvent::Fallback { .. }
    )));
}
#[tokio::test]
async fn b3_cancellation_before_and_after_selection_and_call_budget_still_gate() {
    let a = Arc::new(Fake::new());
    let s = FixtureScheduler::new(registry(&[("a", a.clone())]));
    for before in [true, false] {
        let cancelled = AtomicBool::new(before);
        let result = s
            .run(
                request(&["a"], ProviderSelection::Auto),
                TaskBudget {
                    max_provider_calls: 1,
                    max_output_tokens: None,
                },
                &cancelled,
                &mut |_| {
                    cancelled.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await;
        assert_eq!(result.unwrap_err(), SchedulerError::Cancelled);
    }
    let (result, _) = run(&s, request(&["a"], ProviderSelection::Auto), 0, 0).await;
    assert_eq!(result.unwrap_err(), SchedulerError::BudgetExceeded);
    assert_eq!(a.calls(), 0);
}
struct Clock(AtomicU64);
impl RateClock for Clock {
    fn now(&self) -> ClockReading {
        let n = self.0.load(Ordering::SeqCst);
        ClockReading {
            monotonic_ms: n,
            unix_ms: Some(1000 + n),
        }
    }
}
#[tokio::test]
async fn b3_readonly_ranking_keeps_usage_reservations_blocks_and_boundary_projection() {
    let clock = Arc::new(Clock(AtomicU64::new(0)));
    let a = Arc::new(Fake::new());
    let s = FixtureScheduler::with_rate_config(
        registry(&[("a", a)]),
        AdmissionConfig::default(),
        clock.clone(),
        None,
    )
    .unwrap();
    s.rate
        .set_policy(
            "a",
            RatePolicy {
                limits: vec![LocalRateLimit {
                    scope: QuotaScope::Provider,
                    dimension: QuotaDimension::RequestsPerMinute,
                    capacity: 1,
                    window: FixedWindow {
                        period_ms: 100,
                        anchor_unix_ms: 1000,
                    },
                }],
                daily_budget: None,
            },
        )
        .unwrap();
    run(
        &s,
        request(&["a"], ProviderSelection::Fixed("a".into())),
        1,
        0,
    )
    .await
    .0
    .unwrap();
    clock.0.store(100, Ordering::SeqCst);
    let ledger = |scheduler: &Scheduler| {
        let mut value = serde_json::to_value(scheduler.telemetry_snapshot()).unwrap();
        for provider in value.as_array_mut().unwrap() {
            provider.as_object_mut().unwrap().remove("capturedAtUnixMs");
            provider.as_object_mut().unwrap().remove("updatedAgeMs");
        }
        value
    };
    let before = ledger(&s);
    let rate = serde_json::to_value(s.rate.read_only_snapshots()).unwrap();
    let req = request(&["a"], ProviderSelection::Auto);
    for _ in 0..10 {
        assert_eq!(
            s.ranked_provider_ids(
                &req.selection,
                &req.targets,
                &req.required_capabilities,
                &req.mode
            )
            .unwrap(),
            vec!["a"]
        );
    }
    assert_eq!(before, ledger(&s));
    assert_eq!(
        rate,
        serde_json::to_value(s.rate.read_only_snapshots()).unwrap()
    );
    // Rewind the injected clock only to inspect the unrefreshed live epoch via
    // the same read-only DTO: a mutating snapshot would have erased consumed=1.
    clock.0.store(0, Ordering::SeqCst);
    let old = s.rate.read_only_snapshots();
    assert_eq!(old[0].constraints[0].consumed, 1);
    assert_eq!(old[0].constraints[0].effective_remaining, Some(0));
}

#[test]
fn b3_quality_floor_below_and_unknown_remain_inviolable_with_runtime_proof() {
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    for tier in [None, Some(1)] {
        let mut r = resource("a");
        let mut m = model("model-A", 0);
        m.facts.execution.cognitive_tier = tier.map_or(CatalogFact::Unknown, |n| {
            known(CognitiveTier::new(n).unwrap())
        });
        r.models = known(vec![m]);
        let a = FixtureAllocator::new(
            catalog(vec![r]),
            provider_allocation_default(),
            Some(CognitiveTier::new(2).unwrap()),
        );
        assert_eq!(
            plan(&a, &registry, &[target("a")]),
            Err(ProviderBridgeError::NoEligibleCandidates)
        );
    }
}
#[test]
fn b3_unknown_enabled_runtime_capability_and_availability_resolve_without_known_fabrication() {
    let mut r = resource("a");
    r.enabled = CatalogFact::Unknown;
    r.capabilities = CapabilitySet::default();
    let original = r.clone();
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let a = allocator(vec![r], AllocationProfile::Balanced);
    assert_eq!(
        plan(&a, &registry, &[target("a")]).unwrap().entries()[0].score,
        1533
    );
    assert_eq!(
        a.catalog()
            .resource(&ResourceId::new("a").unwrap())
            .unwrap(),
        &original
    );
}
#[test]
fn b3_unknown_effort_support_keeps_described_model_economics_and_b1_evidence() {
    let mut r = resource("a");
    r.models = known(vec![model("model-A", 50)]);
    let original = r.clone();
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let mut policy = provider_allocation_default();
    policy.variant_selection_mode = VariantSelectionMode::Explicit;
    let a = FixtureAllocator::new(catalog(vec![r]), policy, None);
    let mut t = target("a");
    t.invocation.thinking_level = Some(ThinkingLevel::High);
    let p = plan(&a, &registry, &[t.clone()]).unwrap();
    assert_eq!(p.entries()[0].target, t);
    assert_eq!(p.entries()[0].score, 1233);
    assert_eq!(
        a.catalog()
            .resource(&ResourceId::new("a").unwrap())
            .unwrap(),
        &original
    );
}
#[test]
fn b3_lr8_dtos_are_immutable_and_generation_mismatch_is_typed() {
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let a = FixtureAllocator::production(&registry).unwrap();
    let scheduler = FixtureScheduler::new(super::tests::registry(&[("a", Arc::new(Fake::new()))]));
    let telemetry = scheduler.telemetry_snapshot();
    let mut rate = scheduler.rate.read_only_snapshots();
    let before_t = serde_json::to_value(&telemetry).unwrap();
    let before_r = serde_json::to_value(&rate).unwrap();
    a.plan(
        &registry,
        &[target("a")],
        ProviderCapabilities::text_stream(),
        &InvocationMode::default(),
        None,
        0,
        &telemetry,
        &rate,
    )
    .unwrap();
    assert_eq!(before_t, serde_json::to_value(&telemetry).unwrap());
    assert_eq!(before_r, serde_json::to_value(&rate).unwrap());
    rate[0].context_generation += 1;
    assert_eq!(
        a.plan(
            &registry,
            &[target("a")],
            ProviderCapabilities::text_stream(),
            &InvocationMode::default(),
            None,
            0,
            &telemetry,
            &rate
        ),
        Err(ProviderBridgeError::Lr8ContextMismatch)
    );
}
#[tokio::test]
async fn b3_affinity_profile_weights_can_trade_context_against_reserve() {
    for (profile, expected) in [
        (AllocationProfile::Balanced, "a"),
        (AllocationProfile::Fast, "b"),
    ] {
        let providers = [("a", Arc::new(Fake::new())), ("b", Arc::new(Fake::new()))];
        let mut ra = resource("a");
        allowance(&mut ra, 80);
        let mut rb = resource("b");
        allowance(&mut rb, 5);
        let a = FixtureAllocator::new(
            catalog(vec![ra, rb]),
            AllocationPolicy {
                profile,
                reserve: Some(ReservePolicy::new(40, 10).unwrap()),
                ..provider_allocation_default()
            },
            None,
        );
        let s = FixtureScheduler::new(registry(&providers)).with_auto_allocator(a);
        let mut req = request(&["b"], ProviderSelection::Fixed("b".into()));
        req.affinity_key = Some("session".into());
        run(&s, req, 1, 0).await.0.unwrap();
        let mut req = request(&["a", "b"], ProviderSelection::Auto);
        req.affinity_key = Some("session".into());
        req.estimated_context_bytes = usize::MAX;
        assert_eq!(run(&s, req, 2, 0).await.0.unwrap().provider_id, expected);
    }
}

#[test]
fn b3_proof_is_bound_to_exact_timeouts_and_invocation_mode() {
    let r = resource("a");
    let registry = registry(&[("a", Arc::new(Fake::new()))]);
    let c = AllocationCandidate::new(&r, ModelId::new("model-A").unwrap(), None).unwrap();
    let b1 = AllocationRequest::new(
        provider_requirements(ProviderCapabilities::text_stream(), None),
        provider_allocation_default(),
        vec![c.clone()],
    )
    .unwrap()
    .evaluate();
    let target = target("a");
    let mode = InvocationMode::default();
    let proof = OperationalVariantProof::prove(
        registry.get("a").unwrap(),
        &c,
        &target,
        ProviderCapabilities::text_stream(),
        &mode,
    )
    .unwrap();
    let mut changed_target = target.clone();
    changed_target.invocation.timeouts = None;
    let changed_mode = InvocationMode {
        transport: TransportMode::NonStreaming,
        ..mode.clone()
    };
    for (target, mode) in [(&changed_target, &mode), (&target, &changed_mode)] {
        assert!(matches!(
            ResolvedProviderCandidate::resolve(
                c.clone(),
                b1.candidates()[0].clone(),
                proof.clone(),
                CandidateSignals::default(),
                target,
                mode
            ),
            Err(ProviderBridgeError::InvalidOperationalProof)
        ));
    }
}
#[tokio::test]
async fn b3_allocation_initialization_error_cannot_block_explicit_modes() {
    let a = Arc::new(Fake::new());
    let bad = Arc::new(Fake::new());
    let s = FixtureScheduler::new(registry(&[("a", a.clone()), ("invalid/id", bad.clone())]));
    for mode in [
        ProviderSelection::Fixed("a".into()),
        ProviderSelection::Preferred,
    ] {
        assert_eq!(
            run(&s, request(&["a"], mode), 1, 0)
                .await
                .0
                .unwrap()
                .provider_id,
            "a"
        );
    }
    assert_eq!(
        run(&s, request(&["a"], ProviderSelection::Auto), 1, 0)
            .await
            .0
            .unwrap_err(),
        SchedulerError::InvalidTargetConfig
    );
    assert_eq!((a.calls(), bad.calls()), (2, 0));
}
